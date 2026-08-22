use mop_core::models::plugin::PluginState;
use mop_db::migration::run_migrations;
use mop_db::pool::create_sqlite_pool;
use mop_db::repos::{PluginPermissionsRepo, PluginRepo, PluginSettingsRepo};
use mop_jobs::JobService;
use mop_plugin::host_notification::HostNotificationHandler;
use mop_plugin::rpc::UnixRpcClient;
use mop_plugin::supervisor::PluginSupervisor;
use mop_plugin_sdk::*;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

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

    let handler = HostNotificationHandler::new(job_service.clone(), &host_sock);

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
