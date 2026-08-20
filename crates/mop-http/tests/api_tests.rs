use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use mop_core::config::Config;
use mop_db::{create_sqlite_pool, run_migrations};
use mop_http::create_app;
use mop_watch::FakeResourceCollector;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tower::ServiceExt;

async fn setup_test_app() -> (axum::Router, NamedTempFile) {
    let tmp = NamedTempFile::new().unwrap();
    let db_path = tmp.path();

    let pool = create_sqlite_pool(db_path)
        .await
        .expect("Pool creation failed");
    run_migrations(&pool).await.expect("Migration failed");

    let config = Config::default();
    let collector = Arc::new(FakeResourceCollector::new());
    let app = create_app(pool, config, collector);

    (app, tmp)
}

#[tokio::test]
async fn test_health_check() {
    let (app, _tmp) = setup_test_app().await;

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
    let (app, _tmp) = setup_test_app().await;

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
async fn test_unauthorized_users_endpoint() {
    let (app, _tmp) = setup_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
