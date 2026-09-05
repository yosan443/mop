use mop_core::models::plugin::PluginState;
use mop_db::migration::run_migrations;
use mop_db::pool::create_sqlite_pool;
use mop_db::repos::{PluginPermissionsRepo, PluginRepo, PluginSettingsRepo};
use mop_jobs::JobService;
use mop_plugin::host_notification::HostNotificationHandler;
use mop_plugin::rpc::UnixRpcClient;
use mop_plugin::supervisor::PluginSupervisor;
use mop_plugin_sdk::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_unix_rpc_client_and_timeout() {
    let tmp = tempdir().unwrap();
    let sock_path = tmp.path().join("test_rpc.sock");

    // 1. Mock server that responds properly
    let listener = UnixListener::bind(&sock_path).unwrap();
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();
            if let Ok(Some(line)) = lines.next_line().await {
                let req: RpcRequest = serde_json::from_str(&line).unwrap();
                let res = RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }));
                let res_bytes = serde_json::to_vec(&res).unwrap();
                let _ = writer.write_all(&res_bytes).await;
                let _ = writer.write_all(b"\n").await;
            }
        }
    });

    let client = UnixRpcClient::new(&sock_path);
    let res = client.call("describe", None).await.unwrap();
    assert_eq!(res["status"], "ok");

    // 2. Mock server that times out
    let timeout_sock = tmp.path().join("timeout.sock");
    let timeout_listener = UnixListener::bind(&timeout_sock).unwrap();
    tokio::spawn(async move {
        if let Ok((_stream, _)) = timeout_listener.accept().await {
            // Never write response to trigger timeout
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    let timeout_client = UnixRpcClient::with_timeout(&timeout_sock, Duration::from_millis(200));
    let err = timeout_client.call("describe", None).await.unwrap_err();
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_host_notification_anti_spoofing() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let job_service = JobService::new(pool.clone());
    let host_sock = tmp.path().join("host.sock");
    let pid_to_plugin = Arc::new(RwLock::new(HashMap::new()));

    let handler = HostNotificationHandler::new(job_service.clone(), &host_sock, pid_to_plugin);

    // Create a legitimate job owned by "mop.hello"
    let legitimate_job = job_service
        .submit("hello.ping", Some("mop.hello"), "{}", "admin")
        .await
        .unwrap();

    // Create another job owned by "mop.manga"
    let other_job = job_service
        .submit("manga.convert", Some("mop.manga"), "{}", "admin")
        .await
        .unwrap();

    // 1. Valid notification from owner
    let progress_notif = RpcNotification::new(
        "job.progress",
        Some(serde_json::json!({
            "job_id": legitimate_job.id,
            "percent": 50,
            "message": "Processing..."
        })),
    );
    handler
        .handle_notification("mop.hello", progress_notif)
        .await
        .unwrap();

    let events = job_service.get_events(&legitimate_job.id).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "Processing...");

    // 2. Spoofed notification from different plugin -> MUST be rejected (403 / Forbidden)
    let spoofed_notif = RpcNotification::new(
        "job.progress",
        Some(serde_json::json!({
            "job_id": other_job.id,
            "percent": 100,
            "message": "Spoofed!"
        })),
    );
    let err = handler
        .handle_notification("mop.hello", spoofed_notif)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("does not belong to plugin"));
}

#[tokio::test]
async fn test_supervisor_manifest_scan_and_enable() {
    let tmp = tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let run_dir = tmp.path().join("run");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::create_dir_all(&run_dir).unwrap();

    let db_path = tmp.path().join("test.db");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let plugin_repo = PluginRepo::new(pool.clone());
    let perms_repo = PluginPermissionsRepo::new(pool.clone());
    let settings_repo = PluginSettingsRepo::new(pool.clone());
    let job_service = JobService::new(pool.clone());

    // Create dummy plugin structure
    let hello_dir = plugins_dir.join("mop.hello").join("0.1.0");
    std::fs::create_dir_all(&hello_dir).unwrap();
    let manifest_toml = r#"
id = "mop.hello"
name = "Hello Plugin"
version = "0.1.0"
api_version = "1"

[ui]
entry = "ui/index.js"
element = "mop-plugin-hello"

[capabilities]
jobs = ["hello.ping"]
"#;
    std::fs::write(hello_dir.join("plugin.toml"), manifest_toml).unwrap();

    let supervisor = PluginSupervisor::new(
        &plugins_dir,
        &run_dir,
        plugin_repo.clone(),
        perms_repo.clone(),
        settings_repo.clone(),
        job_service,
    );

    // 1. Scan and register
    let registered = supervisor.scan_and_register_plugins().await.unwrap();
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].id, "mop.hello");
    assert_eq!(registered[0].state, PluginState::Installed);

    // 2. Enable plugin (grants capabilities)
    let enabled = supervisor
        .enable_plugin("mop.hello", "admin")
        .await
        .unwrap();
    assert!(enabled.enabled);
    assert_eq!(enabled.state, PluginState::Enabled);

    let perms = perms_repo.list_permissions("mop.hello").await.unwrap();
    assert_eq!(perms.len(), 1);
    assert_eq!(perms[0].capability, "jobs");
    assert_eq!(perms[0].value_json, "hello.ping");

    // 3. Settings Save / Diff / Apply
    settings_repo
        .save_draft_setting(
            "mop.hello",
            "greeting",
            &serde_json::json!("Konnichiwa"),
            "admin",
        )
        .await
        .unwrap();

    let diff = settings_repo.get_settings_diff("mop.hello").await.unwrap();
    assert_eq!(diff.items.len(), 1);
    assert_eq!(diff.items[0].key, "greeting");
    assert_eq!(diff.items[0].change_type, "added");

    settings_repo
        .apply_draft_settings("mop.hello")
        .await
        .unwrap();
    let applied = settings_repo
        .get_applied_settings("mop.hello")
        .await
        .unwrap();
    assert_eq!(applied["greeting"], "Konnichiwa");

    // 4. Disable plugin
    let disabled = supervisor.disable_plugin("mop.hello").await.unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.state, PluginState::Disabled);
}

#[tokio::test]
async fn test_crash_limit_auto_disable() {
    let tmp = tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let run_dir = tmp.path().join("run");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::create_dir_all(&run_dir).unwrap();

    let db_path = tmp.path().join("test.db");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let plugin_repo = PluginRepo::new(pool.clone());
    let perms_repo = PluginPermissionsRepo::new(pool.clone());
    let settings_repo = PluginSettingsRepo::new(pool.clone());
    let job_service = JobService::new(pool.clone());

    // Create a plugin that starts up, responds to initialize, and then immediately crashes
    let crash_dir = plugins_dir.join("mop.crasher").join("0.1.0");
    std::fs::create_dir_all(&crash_dir).unwrap();

    let script_path = crash_dir.join("crasher.py");
    let script_content = r#"#!/usr/bin/env python3
import os, sys, socket, json, time

sock_path = os.environ.get("MOP_PLUGIN_SOCKET")
if not sock_path:
    sys.exit(1)

if os.path.exists(sock_path):
    try:
        os.remove(sock_path)
    except:
        pass

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(sock_path)
s.listen(5)

while True:
    conn, _ = s.accept()
    f = conn.makefile("rwb", buffering=0)
    line = f.readline()
    if line:
        try:
            req = json.loads(line.decode("utf-8").strip())
            if req.get("method") == "initialize":
                res = {"jsonrpc": "2.0", "result": {"status": "ok"}, "id": req.get("id", 1)}
                f.write((json.dumps(res) + "\n").encode("utf-8"))
                f.flush()
                time.sleep(0.05)
                conn.close()
                s.close()
                sys.exit(1)
            else:
                res = {"jsonrpc": "2.0", "result": {"status": "ok"}, "id": req.get("id", 1)}
                f.write((json.dumps(res) + "\n").encode("utf-8"))
                f.flush()
        except Exception as e:
            pass
    conn.close()
"#;
    std::fs::write(&script_path, script_content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
    }

    let manifest_toml = r#"
id = "mop.crasher"
name = "Crasher Plugin"
version = "0.1.0"
api_version = "1"

[backend]
exec = "crasher.py"

[capabilities]
"#;
    std::fs::write(crash_dir.join("plugin.toml"), manifest_toml).unwrap();

    // Set crash limit to 3 crashes within 10 seconds
    let supervisor = PluginSupervisor::new(
        &plugins_dir,
        &run_dir,
        plugin_repo.clone(),
        perms_repo.clone(),
        settings_repo.clone(),
        job_service,
    )
    .with_crash_policy(3, 10);

    let _ = supervisor.scan_and_register_plugins().await.unwrap();

    // Enable plugin -> process starts, initializes, and crashes repeatedly
    let enabled_res = supervisor.enable_plugin("mop.crasher", "admin").await;
    assert!(enabled_res.is_ok(), "Initial enable should succeed");

    // Wait and verify that supervisor detects repeated crashes and automatically transitions to Disabled
    let mut auto_disabled = false;
    for i in 0..60 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(record) = plugin_repo.find_by_id("mop.crasher").await.unwrap() {
            println!(
                "Check #{i}: state={:?}, enabled={}",
                record.state, record.enabled
            );
            if record.state == PluginState::Disabled && !record.enabled {
                auto_disabled = true;
                break;
            }
        }
    }

    assert!(
        auto_disabled,
        "Plugin should have been automatically disabled after reaching crash limit"
    );
}

#[tokio::test]
async fn test_supervisor_ipc_and_socket_permissions() {
    let tmp = tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let run_dir = tmp.path().join("run");
    let db_path = tmp.path().join("test.db");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let plugin_repo = PluginRepo::new(pool.clone());
    let perms_repo = PluginPermissionsRepo::new(pool.clone());
    let settings_repo = PluginSettingsRepo::new(pool.clone());
    let job_service = JobService::new(pool.clone());

    let supervisor = PluginSupervisor::new(
        &plugins_dir,
        &run_dir,
        plugin_repo,
        perms_repo,
        settings_repo,
        job_service,
    );

    // Start host listener
    supervisor.ensure_host_listener().await.unwrap();

    let host_sock = run_dir.join("host.sock");
    assert!(host_sock.exists(), "host.sock should exist");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&host_sock).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o660);

        let plugins_run_dir = run_dir.join("plugins");
        assert!(plugins_run_dir.exists(), "plugins_run_dir should exist");
        let dir_meta = std::fs::metadata(&plugins_run_dir).unwrap();
        assert_eq!(dir_meta.permissions().mode() & 0o777, 0o770);
    }
}
