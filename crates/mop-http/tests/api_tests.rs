use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use mop_core::config::{Config, RegistrationMode};
use mop_core::models::{AuditResult, Resource, Role, UserResponse};
use mop_db::{create_sqlite_pool, repos::AuditRepo, repos::UserRepo, run_migrations};
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

    // 5. Get resource logs
    let logs_req = Request::builder()
        .uri("/api/v1/resources/systemd:caddy.service/logs?tail=10")
        .header(header::COOKIE, admin_cookie.clone())
        .body(Body::empty())
        .unwrap();
    let logs_res = app.clone().oneshot(logs_req).await.unwrap();
    assert_eq!(logs_res.status(), StatusCode::OK);

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
