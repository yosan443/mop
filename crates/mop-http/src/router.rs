use axum::{
    middleware,
    routing::{get, patch, post},
    Router,
};
use axum_login::AuthManagerLayerBuilder;
use mop_auth::{csrf_protection_middleware, IpRateLimiter, KeyRateLimiter, MopAuthBackend};
use mop_core::config::Config;
use mop_jobs::JobService;
use mop_watch::ResourceCollector;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;
use tower_sessions::{
    cookie::{time::Duration, SameSite},
    Expiry, SessionManagerLayer,
};
use tower_sessions_sqlx_store::SqliteStore;

use crate::handlers::{
    auth::{get_auth_meta, get_me, login, logout, register, AppState},
    events::stream_events,
    health::health_check,
    jobs::{get_job, list_jobs, stream_jobs},
    resources::{
        execute_resource_action, get_resource_detail, get_resource_logs, list_resources,
        stream_resource_logs,
    },
    users::{create_user, list_users, update_user},
};
use crate::static_files::static_handler;

pub fn create_app(
    pool: SqlitePool,
    config: Config,
    collector: Arc<dyn ResourceCollector>,
) -> Router {
    let session_store = SqliteStore::new(pool.clone());
    let session_expiry_hours = config.auth.session_hours as i64;
    let is_https = config
        .server
        .public_url
        .as_deref()
        .is_some_and(|u| u.starts_with("https://"));

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(is_https)
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_path("/")
        .with_expiry(Expiry::OnInactivity(Duration::hours(session_expiry_hours)));

    let auth_backend = MopAuthBackend::new(pool.clone());
    let auth_layer = AuthManagerLayerBuilder::new(auth_backend, session_layer).build();

    let auth_limiter = IpRateLimiter::new_auth_limiter();
    let action_limiter = KeyRateLimiter::new_action_limiter();
    let active_resource_locks = Arc::new(Mutex::new(HashSet::new()));
    let job_service = JobService::new(pool.clone());

    let app_state = AppState {
        pool,
        config,
        auth_limiter,
        action_limiter,
        active_resource_locks,
        collector,
        job_service,
    };

    let api_router = Router::new()
        // Auth API
        .route("/auth/meta", get(get_auth_meta))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(get_me))
        // Users API (Admin)
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", patch(update_user))
        // Resources API (Viewer+ / Operator+)
        .route("/resources", get(list_resources))
        .route("/resources/{id}", get(get_resource_detail))
        .route("/resources/{id}/logs", get(get_resource_logs))
        .route("/resources/{id}/logs/stream", get(stream_resource_logs))
        .route("/resources/{id}/actions", post(execute_resource_action))
        // Jobs API (Viewer+)
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}", get(get_job))
        .route("/jobs/stream", get(stream_jobs))
        // Events SSE (Viewer+)
        .route("/events/stream", get(stream_events));

    Router::new()
        .nest("/api/v1", api_router)
        .route("/health", get(health_check))
        .fallback(static_handler)
        .layer(middleware::from_fn(csrf_protection_middleware))
        .layer(auth_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(app_state)
}
