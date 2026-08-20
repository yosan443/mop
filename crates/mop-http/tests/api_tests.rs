use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use mop_core::config::{Config, RegistrationMode};
use mop_core::models::{AuditResult, Role};
use mop_db::repos::{AuditRepo, UserRepo};
use mop_db::{create_sqlite_pool, run_migrations};
use mop_http::create_app;
use mop_watch::FakeResourceCollector;
use sqlx::SqlitePool;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tower::ServiceExt;

async fn setup_test_app() -> (axum::Router, SqlitePool, NamedTempFile) {
    let tmp = NamedTempFile::new().unwrap();
    let db_path = tmp.path();

    let pool = create_sqlite_pool(db_path)
        .await
        .expect("Pool creation failed");
    run_migrations(&pool).await.expect("Migration failed");

    let config = Config::default();
    let collector = Arc::new(FakeResourceCollector::new());
    let app = create_app(pool.clone(), config, collector);

    (app, pool, tmp)
}

async fn setup_custom_app(config: Config) -> (axum::Router, SqlitePool, NamedTempFile) {
    let tmp = NamedTempFile::new().unwrap();
    let db_path = tmp.path();

    let pool = create_sqlite_pool(db_path)
        .await
        .expect("Pool creation failed");
    run_migrations(&pool).await.expect("Migration failed");

    let collector = Arc::new(FakeResourceCollector::new());
    let app = create_app(pool.clone(), config, collector);

    (app, pool, tmp)
}

#[tokio::test]
async fn test_health_check() {
    let (app, _pool, _tmp) = setup_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_auth_meta_and_setup() {
    let (app, _pool, _tmp) = setup_test_app().await;

    // 1. Initial meta check: needs_setup must be true
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/meta")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["needs_setup"], true);

    // 2. Register first user (Admin)
    let payload = serde_json::json!({
        "username": "admin",
        "password": "SuperSecretPassword123"
    });

    let register_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let register_res = app.clone().oneshot(register_req).await.unwrap();
    let status = register_res.status();
    let reg_body = register_res.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(status, StatusCode::CREATED);

    let reg_json: serde_json::Value = serde_json::from_slice(&reg_body).unwrap();
    assert_eq!(reg_json["username"], "admin");
    assert_eq!(reg_json["role"], "admin");

    // 3. Subsequent meta check: needs_setup must be false
    let response_after = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/meta")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body_after = response_after
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json_after: serde_json::Value = serde_json::from_slice(&body_after).unwrap();
    assert_eq!(json_after["needs_setup"], false);

    // 4. Duplicate setup / register when first_user mode must fail with 403
    let duplicate_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "second_user",
                "password": "Password12345"
            }))
            .unwrap(),
        ))
        .unwrap();

    let duplicate_res = app.oneshot(duplicate_req).await.unwrap();
    assert_eq!(duplicate_res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_auth_rate_limiting_429() {
    let (app, _pool, _tmp) = setup_test_app().await;

    // First user setup
    let setup_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .header("x-forwarded-for", "192.168.1.100")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "SuperPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let setup_res = app.clone().oneshot(setup_req).await.unwrap();
    assert_eq!(setup_res.status(), StatusCode::CREATED);

    // Send 4 consecutive login attempts (total 5 requests from 192.168.1.100)
    for _ in 0..4 {
        let login_req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost")
            .header("x-forwarded-for", "192.168.1.100")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "username": "admin",
                    "password": "WrongPassword"
                }))
                .unwrap(),
            ))
            .unwrap();
        let _ = app.clone().oneshot(login_req).await.unwrap();
    }

    // 6th attempt from same IP must be rate-limited (HTTP 429)
    let ratelimited_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .header("x-forwarded-for", "192.168.1.100")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "SuperPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();

    let ratelimited_res = app.clone().oneshot(ratelimited_req).await.unwrap();
    assert_eq!(ratelimited_res.status(), StatusCode::TOO_MANY_REQUESTS);

    let body = ratelimited_res
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "RATE_LIMIT_EXCEEDED");

    // Different IP must NOT be rate-limited
    let other_ip_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .header("x-forwarded-for", "192.168.1.101")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "SuperPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();

    let other_ip_res = app.oneshot(other_ip_req).await.unwrap();
    assert_eq!(other_ip_res.status(), StatusCode::OK);
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
                "username": "admin",
                "password": "AdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    assert_eq!(reg_res.status(), StatusCode::CREATED);

    // 2. Failed login
    let failed_login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "BadPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let failed_res = app.clone().oneshot(failed_login_req).await.unwrap();
    assert_eq!(failed_res.status(), StatusCode::UNAUTHORIZED);

    // 3. Successful login
    let success_login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "AdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let success_res = app.clone().oneshot(success_login_req).await.unwrap();
    assert_eq!(success_res.status(), StatusCode::OK);

    // Verify audit_events in database
    let events = AuditRepo::list(&pool)
        .await
        .expect("Failed to list audit events");
    assert!(events
        .iter()
        .any(|e| e.action == "auth.register" && e.result == AuditResult::Success));
    assert!(events
        .iter()
        .any(|e| e.action == "auth.login" && e.result == AuditResult::Denied));
    assert!(events
        .iter()
        .any(|e| e.action == "auth.login" && e.result == AuditResult::Success));
}

#[tokio::test]
async fn test_authorization_matrix() {
    let (app, pool, _tmp) = setup_test_app().await;

    // 1. Register admin
    let reg_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "username": "admin",
                "password": "AdminPassword123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let reg_res = app.clone().oneshot(reg_req).await.unwrap();
    let cookie_header = reg_res.headers().get(header::SET_COOKIE).cloned().unwrap();

    // 2. Admin creates operator and viewer directly
    let op_user = mop_core::models::User {
        id: ulid::Ulid::new().to_string(),
        username: "operator_user".to_string(),
        password_hash: mop_auth::hash_password("OperatorPass123").unwrap(),
        role: Role::Operator,
        disabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    UserRepo::create(&pool, &op_user).await.unwrap();

    let vi_user = mop_core::models::User {
        id: ulid::Ulid::new().to_string(),
        username: "viewer_user".to_string(),
        password_hash: mop_auth::hash_password("ViewerPass123").unwrap(),
        role: Role::Viewer,
        disabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    UserRepo::create(&pool, &vi_user).await.unwrap();

    // 3. Unauthenticated access to /api/v1/users must return 401
    let unauth_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth_res.status(), StatusCode::UNAUTHORIZED);

    // 4. Admin access to /api/v1/users with Cookie must return 200
    let admin_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .header(header::COOKIE, cookie_header.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_res.status(), StatusCode::OK);

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

    let op_create_user_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/users")
                .header(header::COOKIE, op_cookie.clone())
                .header(header::ORIGIN, "http://localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "username": "newbie",
                        "password": "Password12345",
                        "role": "viewer"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(op_create_user_res.status(), StatusCode::FORBIDDEN);

    // 6. Viewer login and attempt to create user -> 403 Forbidden
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
