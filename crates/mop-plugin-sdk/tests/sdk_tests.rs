use mop_plugin_sdk::*;

#[test]
fn test_manifest_parse_and_validate_success() {
    let toml = r#"
id = "mop.hello"
name = "Hello Plugin"
version = "0.1.0"
api_version = "1"

[backend]
exec = "bin/mop-plugin-hello"

[ui]
entry = "ui/index.js"
element = "mop-plugin-hello"
routes = ["/hello"]

[ui.nav]
title = "Hello"
icon = "hand"

[capabilities]
filesystem_read = ["/srv/data"]
jobs = ["hello.ping"]
network = false
"#;

    let manifest = PluginManifest::parse_and_validate(toml).unwrap();
    assert_eq!(manifest.id, "mop.hello");
    assert_eq!(manifest.name, "Hello Plugin");
    assert_eq!(manifest.api_version, "1");
    assert_eq!(manifest.backend.unwrap().exec, "bin/mop-plugin-hello");
    assert_eq!(manifest.ui.as_ref().unwrap().element, "mop-plugin-hello");
    assert_eq!(manifest.capabilities.jobs, vec!["hello.ping"]);
    assert_eq!(manifest.capabilities.filesystem_read, vec!["/srv/data"]);
    assert!(!manifest.capabilities.network);
}

#[test]
fn test_manifest_validation_failures() {
    // 1. Path traversal in id
    let bad_id = r#"
id = "../evil"
name = "Evil"
version = "0.1.0"
api_version = "1"
"#;
    assert!(PluginManifest::parse_and_validate(bad_id).is_err());

    // 2. Unsupported API version
    let bad_api = r#"
id = "mop.test"
name = "Test"
version = "0.1.0"
api_version = "2"
"#;
    assert!(PluginManifest::parse_and_validate(bad_api).is_err());

    // 3. Absolute path in backend.exec
    let bad_exec = r#"
id = "mop.test"
name = "Test"
version = "0.1.0"
api_version = "1"

[backend]
exec = "/usr/bin/evil"
"#;
    assert!(PluginManifest::parse_and_validate(bad_exec).is_err());

    // 4. Path traversal in ui.entry
    let bad_ui = r#"
id = "mop.test"
name = "Test"
version = "0.1.0"
api_version = "1"

[ui]
entry = "../../../etc/passwd"
element = "mop-test"
"#;
    assert!(PluginManifest::parse_and_validate(bad_ui).is_err());
}

#[test]
fn test_json_rpc_serialization() {
    let req = RpcRequest::new(
        1,
        "job.submit",
        Some(serde_json::json!({
            "job_id": "01JABCDEF",
            "kind": "hello.ping",
            "params": {}
        })),
    );
    let serialized = serde_json::to_string(&req).unwrap();
    assert!(serialized.contains(r#""jsonrpc":"2.0""#));
    assert!(serialized.contains(r#""method":"job.submit""#));

    let res = RpcResponse::success(
        Some(serde_json::json!(1)),
        serde_json::json!({"status": "ok"}),
    );
    let res_str = serde_json::to_string(&res).unwrap();
    assert!(res_str.contains(r#""result":{"status":"ok"}"#));

    let notif = RpcNotification::new(
        "job.progress",
        Some(serde_json::json!({
            "job_id": "01JABCDEF",
            "percent": 50,
            "message": "working"
        })),
    );
    let notif_str = serde_json::to_string(&notif).unwrap();
    assert!(notif_str.contains(r#""method":"job.progress""#));
}
