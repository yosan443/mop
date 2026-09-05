use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use mop_core::config::{Config, RegistrationMode};
use mop_core::models::{AuditResult, Resource, Role, UserResponse};
use mop_db::{
    create_sqlite_pool, repos::AuditRepo, repos::PluginPermissionsRepo, repos::UserRepo,
    run_migrations,
};
use mop_http::create_app;
use mop_watch::FakeResourceCollector;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tower::ServiceExt;

async fn setup_test_app() -> (axum::Router, sqlx::SqlitePool, NamedTempFile) {
    let tmp_file = NamedTempFile::new().unwrap();
    let pool = create_sqlite_pool(tmp_file.path()).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let config = Config::default();
    let collector = Arc::new(FakeResourceCollector::new());
    let app = create_app(pool.clone(), config, collector);

    (app, pool, tmp_file)
}

async fn setup_custom_app(config: Config) -> (axum::Router, sqlx::SqlitePool, NamedTempFile) {
    let tmp_file = NamedTempFile::new().unwrap();
    let pool = create_sqlite_pool(tmp_file.path()).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let collector = Arc::new(FakeResourceCollector::new());
    let app = create_app(pool.clone(), config, collector);

    (app, pool, tmp_file)
}

#[tokio::test]
async fn test_health_check() {
    let (app, _, _tmp) = setup_test_app().await;

    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_auth_meta_and_setup() {
    let (app, _pool, _tmp) = setup_test_app().await;

    // 1. Check meta initially (needs_setup: true)
    let meta_req = Request::builder()
        .uri("/api/v1/auth/meta")
        .body(Body::empty())
        .unwrap();
    let meta_res = app.clone().oneshot(meta_req).await.unwrap();
    assert_eq!(meta_res.status(), StatusCode::OK);

    let body = meta_res.into_body().collect().await.unwrap().to_bytes();
    let meta: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(meta["needs_setup"], true);
    assert_eq!(meta["registration"], "first_user");

    // 2. Register first user (should become admin)
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "InitialAdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();

    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    assert_eq!(reg_res.status(), StatusCode::CREATED);

    let cookie_header = reg_res
        .headers()
        .get(header::SET_COOKIE)
        .cloned()
        .expect("Session cookie should be set");

    let body = reg_res.into_body().collect().await.unwrap().to_bytes();
    let user: UserResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(user.username, "admin");
    assert_eq!(user.role, Role::Admin);

    // 3. /api/v1/auth/me using session cookie
    let me_req = Request::builder()
        .uri("/api/v1/auth/me")
        .header(header::COOKIE, cookie_header.clone())
        .body(Body::empty())
        .unwrap();

    let me_res = app.clone().oneshot(me_req).await.unwrap();
    assert_eq!(me_res.status(), StatusCode::OK);
    let body = me_res.into_body().collect().await.unwrap().to_bytes();
    let me: UserResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(me.username, "admin");
}

#[tokio::test]
async fn test_auth_rate_limiting_429() {
    let (app, _pool, _tmp) = setup_test_app().await;

    // First user setup
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .header("x-forwarded-for", "192.0.2.1")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "Password12345"
            }))
            .unwrap(),
        ))
        .unwrap();
    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    assert_eq!(reg_res.status(), StatusCode::CREATED);

    // Send 5 login attempts from the same IP (allowed by limiter, 5 req/min)
    for i in 1..=5 {
        let login_req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost")
            .header("x-forwarded-for", "192.0.2.1")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "username": "admin",
                    "password": format!("WrongPassword_{i}")
                }))
                .unwrap(),
            ))
            .unwrap();

        let res = app.clone().oneshot(login_req).await.unwrap();
        // The first 4 login attempts (after 1 register) exceed the 5 req limit on the 5th attempt
        if i < 5 {
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        } else {
            // 1 register + 4 attempts = 5 requests. The 5th login attempt is the 6th request from this IP!
            assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
            let body = res.into_body().collect().await.unwrap().to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["error"]["code"], "RATE_LIMIT_EXCEEDED");
        }
    }

    // A different IP address should still be allowed
    let diff_ip_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .header("x-forwarded-for", "198.51.100.2")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "Password12345"
            }))
            .unwrap(),
        ))
        .unwrap();

    let diff_ip_res = app.clone().oneshot(diff_ip_req).await.unwrap();
    assert_eq!(diff_ip_res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_audit_events_recorded() {
    let (app, pool, _tmp) = setup_test_app().await;

    // 1. Register admin
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "audit_admin",
                "password": "AuditAdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    assert_eq!(reg_res.status(), StatusCode::CREATED);

    // 2. Failed login attempt
    let bad_login = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "audit_admin",
                "password": "WrongPassword!"
            }))
            .unwrap(),
        ))
        .unwrap();
    let bad_res = app.clone().oneshot(bad_login).await.unwrap();
    assert_eq!(bad_res.status(), StatusCode::UNAUTHORIZED);

    // 3. Successful login
    let good_login = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "audit_admin",
                "password": "AuditAdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let good_res = app.clone().oneshot(good_login).await.unwrap();
    assert_eq!(good_res.status(), StatusCode::OK);

    // 4. Verify audit_events in DB
    let events = AuditRepo::list(&pool).await.unwrap();
    assert!(events.len() >= 3);

    let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
    assert!(actions.contains(&"auth.register"));
    assert!(actions.contains(&"auth.login"));

    let denied_events: Vec<_> = events
        .iter()
        .filter(|e| e.result == AuditResult::Denied)
        .collect();
    assert!(!denied_events.is_empty());
}

#[tokio::test]
async fn test_authorization_matrix() {
    let (app, _pool, _tmp) = setup_test_app().await;

    // 1. Unauthenticated request to /api/v1/users -> 401 Unauthorized
    let unauth_req = Request::builder()
        .uri("/api/v1/users")
        .body(Body::empty())
        .unwrap();
    let unauth_res = app.clone().oneshot(unauth_req).await.unwrap();
    assert_eq!(unauth_res.status(), StatusCode::UNAUTHORIZED);

    // 2. Register first user (admin)
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "super_admin",
                "password": "SuperAdminPass123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    let cookie_header = reg_res.headers().get(header::SET_COOKIE).cloned().unwrap();

    // 3. Admin creates an 'operator' user
    let create_op_req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::COOKIE, cookie_header.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "operator_user",
                "password": "OperatorPass123",
                "role": "operator"
            }))
            .unwrap(),
        ))
        .unwrap();
    let create_op_res = app.clone().oneshot(create_op_req).await.unwrap();
    assert_eq!(create_op_res.status(), StatusCode::CREATED);

    // 4. Admin creates a 'viewer' user
    let create_vi_req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::COOKIE, cookie_header.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "viewer_user",
                "password": "ViewerPass123",
                "role": "viewer"
            }))
            .unwrap(),
        ))
        .unwrap();
    let create_vi_res = app.clone().oneshot(create_vi_req).await.unwrap();
    assert_eq!(create_vi_res.status(), StatusCode::CREATED);

    // 5. Operator login and attempt to access /api/v1/users -> 403 Forbidden
    let op_login_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://localhost")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "username": "operator_user",
                        "password": "OperatorPass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(op_login_res.status(), StatusCode::OK);
    let op_cookie = op_login_res
        .headers()
        .get(header::SET_COOKIE)
        .cloned()
        .unwrap();

    let op_users_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .header(header::COOKIE, op_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(op_users_res.status(), StatusCode::FORBIDDEN);

    // 6. Viewer login and attempt to access /api/v1/users -> 403 Forbidden
    let vi_login_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://localhost")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "username": "viewer_user",
                        "password": "ViewerPass123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vi_login_res.status(), StatusCode::OK);
    let vi_cookie = vi_login_res
        .headers()
        .get(header::SET_COOKIE)
        .cloned()
        .unwrap();

    let vi_users_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .header(header::COOKIE, vi_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vi_users_res.status(), StatusCode::FORBIDDEN);

    // 7. CSRF / Origin mismatch check -> 403 Forbidden
    let evil_req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::COOKIE, cookie_header.clone())
        .header(header::ORIGIN, "https://evil-attacker.com")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "hacked_user",
                "password": "Password12345",
                "role": "admin"
            }))
            .unwrap(),
        ))
        .unwrap();
    let evil_res = app.oneshot(evil_req).await.unwrap();
    assert_eq!(evil_res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_closed_registration_mode() {
    let mut config = Config::default();
    config.auth.registration = RegistrationMode::Closed;

    let (app, pool, _tmp) = setup_custom_app(config).await;

    // Pre-create admin user so it's not first user setup
    let admin_user = mop_core::models::User {
        id: ulid::Ulid::new().to_string(),
        username: "admin".to_string(),
        password_hash: mop_auth::hash_password("AdminPassword123").unwrap(),
        role: Role::Admin,
        disabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    UserRepo::create(&pool, &admin_user).await.unwrap();

    // /api/v1/auth/register in closed mode must return 403 Forbidden
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "newbie",
                "password": "NewbiePassword123"
            }))
            .unwrap(),
        ))
        .unwrap();

    let reg_res = app.oneshot(reg_req).await.unwrap();
    assert_eq!(reg_res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_resources_and_actions_api() {
    let (app, pool, _tmp) = setup_test_app().await;

    // 1. Register admin
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "res_admin",
                "password": "ResAdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    assert_eq!(reg_res.status(), StatusCode::CREATED);
    let admin_cookie = reg_res.headers().get(header::SET_COOKIE).cloned().unwrap();

    // 2. Admin creates a viewer user
    let create_vi_req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "res_viewer",
                "password": "ResViewerPassword123",
                "role": "viewer"
            }))
            .unwrap(),
        ))
        .unwrap();
    let create_vi_res = app.clone().oneshot(create_vi_req).await.unwrap();
    assert_eq!(create_vi_res.status(), StatusCode::CREATED);

    // 3. List resources
    let list_req = Request::builder()
        .uri("/api/v1/resources")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let list_res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);

    let body = list_res.into_body().collect().await.unwrap().to_bytes();
    let resources: Vec<Resource> = serde_json::from_slice(&body).unwrap();
    assert_eq!(resources.len(), 8);
    let names: Vec<&str> = resources.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"caddy.service"));
    assert!(names.contains(&"nginx.service"));
    assert!(names.contains(&"komga"));
    assert!(names.contains(&"media-stack"));
    assert!(names.contains(&"manga-worker"));
    assert!(names.contains(&"db"));

    // 4. Get resource detail
    let detail_req = Request::builder()
        .uri("/api/v1/resources/systemd:caddy.service")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let detail_res = app.clone().oneshot(detail_req).await.unwrap();
    assert_eq!(detail_res.status(), StatusCode::OK);

    // 5. Get resource logs (systemd and compose service)
    let logs_req = Request::builder()
        .uri("/api/v1/resources/systemd:caddy.service/logs?tail=10")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let logs_res = app.clone().oneshot(logs_req).await.unwrap();
    assert_eq!(logs_res.status(), StatusCode::OK);

    let compose_logs_req = Request::builder()
        .uri("/api/v1/resources/compose_service:media-stack:manga-worker/logs?tail=10")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let compose_logs_res = app.clone().oneshot(compose_logs_req).await.unwrap();
    assert_eq!(compose_logs_res.status(), StatusCode::OK);
    let compose_logs_body = compose_logs_res
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let compose_logs: Vec<serde_json::Value> = serde_json::from_slice(&compose_logs_body).unwrap();
    assert!(!compose_logs.is_empty());
    assert!(compose_logs.iter().any(|l| l["line"]
        .as_str()
        .unwrap()
        .contains("[manga-worker|media-stack-manga-worker-1]")));

    // 6. Viewer login and try to execute action -> 403 Forbidden
    let vi_login_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://localhost")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "username": "res_viewer",
                        "password": "ResViewerPassword123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vi_login_res.status(), StatusCode::OK);
    let vi_cookie = vi_login_res
        .headers()
        .get(header::SET_COOKIE)
        .cloned()
        .unwrap();

    let vi_act_req = Request::builder()
        .method("POST")
        .uri("/api/v1/resources/systemd:caddy.service/actions")
        .header(header::COOKIE, vi_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "action": "restart"
            }))
            .unwrap(),
        ))
        .unwrap();
    let vi_act_res = app.clone().oneshot(vi_act_req).await.unwrap();
    assert_eq!(vi_act_res.status(), StatusCode::FORBIDDEN);

    // 7. Admin executes action -> 202 Accepted + job_id
    let act_req = Request::builder()
        .method("POST")
        .uri("/api/v1/resources/systemd:caddy.service/actions")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "action": "restart"
            }))
            .unwrap(),
        ))
        .unwrap();
    let act_res = app.clone().oneshot(act_req).await.unwrap();
    assert_eq!(act_res.status(), StatusCode::ACCEPTED);

    let body = act_res.into_body().collect().await.unwrap().to_bytes();
    let act_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = act_json["job_id"].as_str().unwrap();

    // 7b. Attempt immediate concurrent action on the SAME resource while job is running -> 409 Conflict
    let concurrent_req = Request::builder()
        .method("POST")
        .uri("/api/v1/resources/systemd:caddy.service/actions")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "action": "restart"
            }))
            .unwrap(),
        ))
        .unwrap();
    let concurrent_res = app.clone().oneshot(concurrent_req).await.unwrap();
    assert_eq!(concurrent_res.status(), StatusCode::CONFLICT);

    // 8. Wait for job to execute and verify job details
    tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;

    let job_req = Request::builder()
        .uri(format!("/api/v1/jobs/{job_id}"))
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let job_res = app.clone().oneshot(job_req).await.unwrap();
    assert_eq!(job_res.status(), StatusCode::OK);

    let body = job_res.into_body().collect().await.unwrap().to_bytes();
    let job_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(job_json["job"]["status"], "succeeded");
    assert!(!job_json["events"].as_array().unwrap().is_empty());

    // 8b. Unmanaged compose service action -> 403 Forbidden (SPEC §9.3 & 不変条件 3)
    let unmanaged_req = Request::builder()
        .method("POST")
        .uri("/api/v1/resources/compose_service:media-stack:db/actions")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "action": "restart"
            }))
            .unwrap(),
        ))
        .unwrap();
    let unmanaged_res = app.clone().oneshot(unmanaged_req).await.unwrap();
    assert_eq!(unmanaged_res.status(), StatusCode::FORBIDDEN);

    // 8c. Managed compose service action -> 202 Accepted
    let managed_req = Request::builder()
        .method("POST")
        .uri("/api/v1/resources/compose_service:media-stack:manga-worker/actions")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "action": "restart"
            }))
            .unwrap(),
        ))
        .unwrap();
    let managed_res = app.clone().oneshot(managed_req).await.unwrap();
    assert_eq!(managed_res.status(), StatusCode::ACCEPTED);

    // 9. Verify audit log entry for resource.restart
    let audits = AuditRepo::list(&pool).await.unwrap();
    assert!(audits
        .iter()
        .any(|a| a.action == "resource.restart" && a.result == AuditResult::Success));
}

#[tokio::test]
async fn test_plugin_api_and_rbac_and_ui_serving() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let plugins_dir = tmp_dir.path().join("plugins");
    let run_dir = tmp_dir.path().join("run");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::create_dir_all(&run_dir).unwrap();

    // Create hello plugin structure
    let hello_dir = plugins_dir.join("mop.hello").join("0.1.0");
    let ui_dir = hello_dir.join("ui");
    std::fs::create_dir_all(&ui_dir).unwrap();

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
    std::fs::write(ui_dir.join("index.js"), "console.log('hello');").unwrap();

    let mut config = Config::default();
    config.plugins.dir = plugins_dir;
    config.plugins.run_dir = run_dir;

    let (app, _pool, _tmp) = setup_custom_app(config).await;

    // 1. Initial setup admin
    let setup_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "Password12345!"
            }))
            .unwrap(),
        ))
        .unwrap();
    let setup_res = app.clone().oneshot(setup_req).await.unwrap();
    assert_eq!(setup_res.status(), StatusCode::CREATED);
    let admin_cookie = setup_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // 2. Create viewer user
    let create_viewer_req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "viewer",
                "password": "Password12345!",
                "role": "viewer"
            }))
            .unwrap(),
        ))
        .unwrap();
    let create_viewer_res = app.clone().oneshot(create_viewer_req).await.unwrap();
    assert_eq!(create_viewer_res.status(), StatusCode::CREATED);

    // Login as viewer
    let viewer_login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "viewer",
                "password": "Password12345!"
            }))
            .unwrap(),
        ))
        .unwrap();
    let viewer_login_res = app.clone().oneshot(viewer_login_req).await.unwrap();
    assert_eq!(viewer_login_res.status(), StatusCode::OK);
    let viewer_cookie = viewer_login_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // 3. GET /api/v1/plugins (Viewer can list plugins)
    let list_req = Request::builder()
        .uri("/api/v1/plugins")
        .header(header::COOKIE, viewer_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let list_res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let body = list_res.into_body().collect().await.unwrap().to_bytes();
    let plugins_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(plugins_json.as_array().unwrap().len(), 1);
    assert_eq!(plugins_json[0]["id"], "mop.hello");

    // 4. PUT /api/v1/plugins/{id}/settings (Admin only, viewer gets 403)
    let viewer_settings_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/plugins/mop.hello/settings")
        .header(header::COOKIE, viewer_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "settings": { "greeting": "Bonjour" }
            }))
            .unwrap(),
        ))
        .unwrap();
    let viewer_settings_res = app.clone().oneshot(viewer_settings_req).await.unwrap();
    assert_eq!(viewer_settings_res.status(), StatusCode::FORBIDDEN);

    let admin_settings_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/plugins/mop.hello/settings")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "settings": { "greeting": "Bonjour" }
            }))
            .unwrap(),
        ))
        .unwrap();
    let admin_settings_res = app.clone().oneshot(admin_settings_req).await.unwrap();
    assert_eq!(admin_settings_res.status(), StatusCode::OK);

    // 5. GET /api/v1/plugins/{id}/settings/diff
    let diff_req = Request::builder()
        .uri("/api/v1/plugins/mop.hello/settings/diff")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let diff_res = app.clone().oneshot(diff_req).await.unwrap();
    assert_eq!(diff_res.status(), StatusCode::OK);
    let diff_body = diff_res.into_body().collect().await.unwrap().to_bytes();
    let diff_json: serde_json::Value = serde_json::from_slice(&diff_body).unwrap();
    assert_eq!(diff_json["items"].as_array().unwrap().len(), 1);
    assert_eq!(diff_json["items"][0]["key"], "greeting");
    assert_eq!(
        diff_json["items"][0]["applied_value"],
        serde_json::Value::Null
    );
    assert_eq!(diff_json["items"][0]["draft_value"], "Bonjour");
    assert_eq!(diff_json["items"][0]["change_type"], "added");

    // 6. POST /api/v1/plugins/{id}/settings/apply
    let apply_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/mop.hello/settings/apply")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let apply_res = app.clone().oneshot(apply_req).await.unwrap();
    assert_eq!(apply_res.status(), StatusCode::OK);

    // Verify diff after apply has empty items because all drafts have been applied
    let diff_after_req = Request::builder()
        .uri("/api/v1/plugins/mop.hello/settings/diff")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let diff_after_res = app.clone().oneshot(diff_after_req).await.unwrap();
    assert_eq!(diff_after_res.status(), StatusCode::OK);
    let diff_after_body = diff_after_res
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let diff_after_json: serde_json::Value = serde_json::from_slice(&diff_after_body).unwrap();
    assert_eq!(diff_after_json["items"].as_array().unwrap().len(), 0);

    // Now save a modified draft setting and verify modified change_type with applied_value and draft_value
    let modify_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/plugins/mop.hello/settings")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "settings": { "greeting": "Hello Revised" }
            }))
            .unwrap(),
        ))
        .unwrap();
    let modify_res = app.clone().oneshot(modify_req).await.unwrap();
    assert_eq!(modify_res.status(), StatusCode::OK);

    let diff_mod_req = Request::builder()
        .uri("/api/v1/plugins/mop.hello/settings/diff")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let diff_mod_res = app.clone().oneshot(diff_mod_req).await.unwrap();
    assert_eq!(diff_mod_res.status(), StatusCode::OK);
    let diff_mod_body = diff_mod_res.into_body().collect().await.unwrap().to_bytes();
    let diff_mod_json: serde_json::Value = serde_json::from_slice(&diff_mod_body).unwrap();
    assert_eq!(diff_mod_json["items"].as_array().unwrap().len(), 1);
    assert_eq!(diff_mod_json["items"][0]["key"], "greeting");
    assert_eq!(diff_mod_json["items"][0]["applied_value"], "Bonjour");
    assert_eq!(diff_mod_json["items"][0]["draft_value"], "Hello Revised");
    assert_eq!(diff_mod_json["items"][0]["change_type"], "modified");

    // Verify /api/v1/plugins/refresh (Admin only, viewer forbidden)
    let viewer_refresh_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/refresh")
        .header(header::COOKIE, viewer_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let viewer_refresh_res = app.clone().oneshot(viewer_refresh_req).await.unwrap();
    assert_eq!(viewer_refresh_res.status(), StatusCode::FORBIDDEN);

    let admin_refresh_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/refresh")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let admin_refresh_res = app.clone().oneshot(admin_refresh_req).await.unwrap();
    assert_eq!(admin_refresh_res.status(), StatusCode::OK);

    // 7. POST /api/v1/plugins/{id}/rpc (RBAC check for job.submit)
    let viewer_rpc_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/mop.hello/rpc")
        .header(header::COOKIE, viewer_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "job.submit",
                "params": { "job_type": "hello.ping" },
                "id": 1
            }))
            .unwrap(),
        ))
        .unwrap();
    let viewer_rpc_res = app.clone().oneshot(viewer_rpc_req).await.unwrap();
    assert_eq!(viewer_rpc_res.status(), StatusCode::FORBIDDEN);

    // Grant capability 'jobs': 'hello.ping' for mop.hello
    let perms_repo = PluginPermissionsRepo::new(_pool.clone());
    perms_repo
        .grant_permission("mop.hello", "jobs", "hello.ping", "admin")
        .await
        .unwrap();

    // Condition 1: job_type other than hello.ping is rejected with 403 (CAPABILITY_REQUIRED)
    let evil_rpc_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/mop.hello/rpc")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "job.submit",
                "params": { "job_type": "evil.hack" },
                "id": 2
            }))
            .unwrap(),
        ))
        .unwrap();
    let evil_rpc_res = app.clone().oneshot(evil_rpc_req).await.unwrap();
    assert_eq!(evil_rpc_res.status(), StatusCode::FORBIDDEN);
    let evil_body = evil_rpc_res.into_body().collect().await.unwrap().to_bytes();
    let evil_json: serde_json::Value = serde_json::from_slice(&evil_body).unwrap();
    assert!(evil_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("CAPABILITY_REQUIRED"));

    // Missing job_type returns 400 BAD_REQUEST
    let missing_kind_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/mop.hello/rpc")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "job.submit",
                "params": {},
                "id": 25
            }))
            .unwrap(),
        ))
        .unwrap();
    let missing_kind_res = app.clone().oneshot(missing_kind_req).await.unwrap();
    assert_eq!(missing_kind_res.status(), StatusCode::BAD_REQUEST);
    let missing_body = missing_kind_res
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let missing_json: serde_json::Value = serde_json::from_slice(&missing_body).unwrap();
    assert!(missing_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing 'job_type'"));

    // Condition 2: allowed job_type "hello.ping" submitted, but process is not running.
    // Job must be failed immediately and NOT left orphaned in Queued state.
    let valid_job_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/mop.hello/rpc")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "job.submit",
                "params": { "job_type": "hello.ping" },
                "id": 3
            }))
            .unwrap(),
        ))
        .unwrap();
    let valid_job_res = app.clone().oneshot(valid_job_req).await.unwrap();
    assert_eq!(valid_job_res.status(), StatusCode::OK);
    let valid_job_body = valid_job_res
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let valid_job_json: serde_json::Value = serde_json::from_slice(&valid_job_body).unwrap();
    assert!(valid_job_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("process is not running"));

    // Verify job in JobService was marked as failed (no queued orphan)
    let list_jobs_req = Request::builder()
        .uri("/api/v1/jobs")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let list_jobs_res = app.clone().oneshot(list_jobs_req).await.unwrap();
    let list_jobs_body = list_jobs_res
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let list_jobs_json: serde_json::Value = serde_json::from_slice(&list_jobs_body).unwrap();
    let job_item = list_jobs_json
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["kind"] == "hello.ping")
        .unwrap();
    assert_eq!(job_item["status"], "failed");

    // 8. GET /api/v1/plugins/{id}/ui/index.js (Static asset serving)
    let ui_req = Request::builder()
        .uri("/api/v1/plugins/mop.hello/ui/index.js")
        .header(header::COOKIE, viewer_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let ui_res = app.clone().oneshot(ui_req).await.unwrap();
    assert_eq!(ui_res.status(), StatusCode::OK);
    let content_type = ui_res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("javascript"));

    // 9. Path traversal protection: GET /api/v1/plugins/{id}/ui/../plugin.toml -> 403 Forbidden
    let traversal_req = Request::builder()
        .uri("/api/v1/plugins/mop.hello/ui/..%2Fplugin.toml")
        .header(header::COOKIE, viewer_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let traversal_res = app.clone().oneshot(traversal_req).await.unwrap();
    assert_eq!(traversal_res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_plugin_settings_apply_validation_blocks_promotion() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let plugins_dir = tmp_dir.path().join("plugins");
    let run_dir = tmp_dir.path().join("run");
    let plugin_sockets_dir = run_dir.join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::create_dir_all(&plugin_sockets_dir).unwrap();

    let plugin_dir = plugins_dir.join("mop.valtest").join("0.1.0");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let manifest_toml = r#"
id = "mop.valtest"
name = "Validation Test Plugin"
version = "0.1.0"
api_version = "1"
"#;
    std::fs::write(plugin_dir.join("plugin.toml"), manifest_toml).unwrap();

    let mut config = Config::default();
    config.plugins.dir = plugins_dir;
    config.plugins.run_dir = run_dir.clone();

    let (app, _pool, _tmp) = setup_custom_app(config).await;

    // 1. Setup admin user
    let setup_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "Password12345!"
            }))
            .unwrap(),
        ))
        .unwrap();
    let setup_res = app.clone().oneshot(setup_req).await.unwrap();
    assert_eq!(setup_res.status(), StatusCode::CREATED);
    let admin_cookie = setup_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // 2. Scan plugin
    let refresh_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/refresh")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let refresh_res = app.clone().oneshot(refresh_req).await.unwrap();
    assert_eq!(refresh_res.status(), StatusCode::OK);

    // 3. Save draft setting
    let save_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/plugins/mop.valtest/settings")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "settings": { "port": 99999 }
            }))
            .unwrap(),
        ))
        .unwrap();
    let save_res = app.clone().oneshot(save_req).await.unwrap();
    assert_eq!(save_res.status(), StatusCode::OK);

    // 4. Mock Unix socket for mop.valtest that returns valid: false for config.validate
    let socket_path = plugin_sockets_dir.join("mop.valtest.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

    let server_task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        if let Ok((stream, _)) = listener.accept().await {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();
            if let Ok(Some(line)) = lines.next_line().await {
                let rpc_req: serde_json::Value = serde_json::from_str(&line).unwrap();
                if rpc_req["method"] == "config.validate" {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": rpc_req["id"],
                        "result": {
                            "valid": false,
                            "message": "Port 99999 is out of allowed range"
                        }
                    });
                    let resp_str = format!("{}\n", resp);
                    let _ = writer.write_all(resp_str.as_bytes()).await;
                    let _ = writer.flush().await;
                }
            }
        }
    });

    // 5. Attempt apply: must be rejected with 400 Bad Request
    let apply_req = Request::builder()
        .method("POST")
        .uri("/api/v1/plugins/mop.valtest/settings/apply")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let apply_res = app.clone().oneshot(apply_req).await.unwrap();
    assert_eq!(apply_res.status(), StatusCode::BAD_REQUEST);

    let _ = server_task.await;

    // 6. Verify that draft was NOT promoted
    let diff_req = Request::builder()
        .uri("/api/v1/plugins/mop.valtest/settings/diff")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let diff_res = app.clone().oneshot(diff_req).await.unwrap();
    assert_eq!(diff_res.status(), StatusCode::OK);
    let diff_body = diff_res.into_body().collect().await.unwrap().to_bytes();
    let diff_json: serde_json::Value = serde_json::from_slice(&diff_body).unwrap();
    assert_eq!(
        diff_json["items"][0]["applied_value"],
        serde_json::Value::Null
    );
    assert_eq!(diff_json["items"][0]["draft_value"], 99999);
    assert_eq!(diff_json["items"][0]["change_type"], "added");
}

#[tokio::test]
async fn test_backup_api_rbac_and_execution() {
    let dir = tempfile::tempdir().unwrap();
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).unwrap();

    let mut config = Config::default();
    config.backup.dir = backup_dir.clone();
    let (app, _pool, _tmp) = setup_custom_app(config).await;

    // 1. Unauthenticated request -> 401
    let list_req = Request::builder()
        .uri("/api/v1/backups")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. Setup admin
    let setup_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "Password12345!"
            }))
            .unwrap(),
        ))
        .unwrap();
    let setup_res = app.clone().oneshot(setup_req).await.unwrap();
    assert_eq!(setup_res.status(), StatusCode::CREATED);
    let admin_cookie = setup_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // 3. Create viewer user
    let create_viewer_req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "viewer",
                "password": "Password12345!",
                "role": "viewer"
            }))
            .unwrap(),
        ))
        .unwrap();
    let create_viewer_res = app.clone().oneshot(create_viewer_req).await.unwrap();
    assert_eq!(create_viewer_res.status(), StatusCode::CREATED);

    // Login as viewer
    let viewer_login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "viewer",
                "password": "Password12345!"
            }))
            .unwrap(),
        ))
        .unwrap();
    let viewer_login_res = app.clone().oneshot(viewer_login_req).await.unwrap();
    assert_eq!(viewer_login_res.status(), StatusCode::OK);
    let viewer_cookie = viewer_login_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // 4. Viewer cannot list or create backups -> 403 Forbidden
    let viewer_list_req = Request::builder()
        .uri("/api/v1/backups")
        .header(header::COOKIE, viewer_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(viewer_list_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let viewer_create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/backup")
        .header(header::COOKIE, viewer_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(viewer_create_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 5. Admin can list backups -> 200 OK (initially empty)
    let admin_list_req = Request::builder()
        .uri("/api/v1/backups")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(admin_list_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let list_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(list_json["backups"].as_array().unwrap().len(), 0);

    // 6. Admin triggers backup -> 202 Accepted
    let admin_create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/backup")
        .header(header::COOKIE, admin_cookie.clone())
        .header(header::ORIGIN, "http://localhost")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(admin_create_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let create_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = create_json["job_id"].as_str().unwrap().to_string();
    assert!(!job_id.is_empty());

    // Give background task a moment to finish backup
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 7. Verify backup archive was created and appears in list
    let admin_list_req2 = Request::builder()
        .uri("/api/v1/backups")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let res2 = app.clone().oneshot(admin_list_req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let body2 = res2.into_body().collect().await.unwrap().to_bytes();
    let list_json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    let backups = list_json2["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 1);
    assert!(backups[0]["filename"]
        .as_str()
        .unwrap()
        .starts_with("mop-backup-"));
    assert!(backups[0]["filename"]
        .as_str()
        .unwrap()
        .ends_with(".tar.zst"));
}
