use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use mop_auth::{
    backend::{AuthUserRecord, Credentials},
    rate_limit::{IpRateLimiter, KeyRateLimiter},
    service::{AuthMetaResponse, AuthService},
    AuthSession, RequireAuth,
};
use mop_core::config::Config;
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::{AuditResult, UserResponse};
use mop_db::repos::UserRepo;
use mop_jobs::{AuditLogger, AuditParams, JobService};
use mop_watch::ResourceCollector;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub auth_limiter: IpRateLimiter,
    pub action_limiter: KeyRateLimiter<String>,
    pub active_resource_locks: Arc<Mutex<HashSet<String>>>,
    pub collector: Arc<dyn ResourceCollector>,
    pub job_service: JobService,
    pub plugin_supervisor: mop_plugin::supervisor::PluginSupervisor,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

fn extract_client_ip(headers: &HeaderMap) -> IpAddr {
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first_ip) = forwarded.split(',').next() {
            if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

pub async fn get_auth_meta(
    State(state): State<AppState>,
) -> Result<Json<AuthMetaResponse>, (StatusCode, Json<ErrorResponse>)> {
    AuthService::get_meta(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| (status_code_for_error(&e), Json(ErrorResponse::from(e))))
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut auth_session: AuthSession,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), (StatusCode, Json<ErrorResponse>)> {
    let client_ip = extract_client_ip(&headers);

    // Rate limit check
    if let Err(e) = state.auth_limiter.check(client_ip).await {
        return Err((status_code_for_error(&e), Json(ErrorResponse::from(e))));
    }

    let user_res = AuthService::register(
        &state.pool,
        &state.config,
        &payload.username,
        &payload.password,
    )
    .await;

    match user_res {
        Ok(user) => {
            // Log successful registration to audit_events
            let audit_params = AuditParams {
                user_id: Some(&user.id),
                username: Some(&user.username),
                action: "auth.register",
                resource_kind: Some("user"),
                resource_id: Some(&user.id),
                detail_json: None,
                result: AuditResult::Success,
            };
            let _ = AuditLogger::log(&state.pool, audit_params).await;

            // Automatically log in newly registered user
            let record = AuthUserRecord(user.clone());
            if let Err(e) = auth_session.login(&record).await {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::from(AppError::Internal(format!(
                        "Failed to create session: {e}"
                    )))),
                ));
            }

            Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
        }
        Err(e) => {
            // Log registration failure if user was attempted
            let detail = format!(r#"{{"error":"{e}"}}"#);
            let audit_params = AuditParams {
                user_id: None,
                username: Some(&payload.username),
                action: "auth.register",
                resource_kind: Some("user"),
                resource_id: None,
                detail_json: Some(&detail),
                result: AuditResult::Denied,
            };
            let _ = AuditLogger::log(&state.pool, audit_params).await;

            Err((status_code_for_error(&e), Json(ErrorResponse::from(e))))
        }
    }
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut auth_session: AuthSession,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<UserResponse>, (StatusCode, Json<ErrorResponse>)> {
    let client_ip = extract_client_ip(&headers);

    // Rate limit check
    if let Err(e) = state.auth_limiter.check(client_ip).await {
        return Err((status_code_for_error(&e), Json(ErrorResponse::from(e))));
    }

    let credentials = Credentials {
        username: payload.username.clone(),
        password: payload.password,
    };

    let user_record = match auth_session.authenticate(credentials).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // Log failed login
            let detail = r#"{"reason":"invalid_credentials"}"#;
            let audit_params = AuditParams {
                user_id: None,
                username: Some(&payload.username),
                action: "auth.login",
                resource_kind: Some("user"),
                resource_id: None,
                detail_json: Some(detail),
                result: AuditResult::Denied,
            };
            let _ = AuditLogger::log(&state.pool, audit_params).await;

            let err = AppError::InvalidCredentials;
            return Err((status_code_for_error(&err), Json(ErrorResponse::from(err))));
        }
        Err(e) => {
            let err = AppError::Internal(format!("Authentication error: {e}"));
            return Err((status_code_for_error(&err), Json(ErrorResponse::from(err))));
        }
    };

    if user_record.0.disabled {
        let err = AppError::UserDisabled(user_record.0.username.clone());
        return Err((status_code_for_error(&err), Json(ErrorResponse::from(err))));
    }

    if let Err(e) = auth_session.login(&user_record).await {
        let err = AppError::Internal(format!("Failed to create login session: {e}"));
        return Err((status_code_for_error(&err), Json(ErrorResponse::from(err))));
    }

    // Log successful login
    let audit_params = AuditParams {
        user_id: Some(&user_record.0.id),
        username: Some(&user_record.0.username),
        action: "auth.login",
        resource_kind: Some("user"),
        resource_id: Some(&user_record.0.id),
        detail_json: None,
        result: AuditResult::Success,
    };
    let _ = AuditLogger::log(&state.pool, audit_params).await;

    // Reset rate limiter for this IP on successful login
    state.auth_limiter.reset_ip(client_ip).await;

    let full_user = UserRepo::find_by_id(&state.pool, &user_record.0.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(AppError::Internal(
                    "User not found".to_string(),
                ))),
            )
        })?;

    Ok(Json(UserResponse::from(full_user)))
}

pub async fn logout(
    _auth: RequireAuth,
    mut auth_session: AuthSession,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    auth_session
        .logout()
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| {
            let err = AppError::Internal(format!("Logout error: {e}"));
            (status_code_for_error(&err), Json(ErrorResponse::from(err)))
        })
}

pub async fn get_me(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<UserResponse>, (StatusCode, Json<ErrorResponse>)> {
    let full_user = UserRepo::find_by_id(&state.pool, &auth.0.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::from(AppError::NotFound(
                    "User not found".to_string(),
                ))),
            )
        })?;

    Ok(Json(UserResponse::from(full_user)))
}

pub fn status_code_for_error(err: &AppError) -> StatusCode {
    match err {
        AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        AppError::NotFound(_) | AppError::ResourceNotFound(_) => StatusCode::NOT_FOUND,
        AppError::BadRequest(_) | AppError::InvalidPassword(_) | AppError::Validation(_) => {
            StatusCode::BAD_REQUEST
        }
        AppError::Unauthorized(_) | AppError::InvalidCredentials => StatusCode::UNAUTHORIZED,
        AppError::Forbidden(_)
        | AppError::SetupAlreadyCompleted
        | AppError::SetupRequired
        | AppError::RegistrationClosed
        | AppError::UserDisabled(_)
        | AppError::CsrfOriginMismatch
        | AppError::ActionNotAllowed(_, _) => StatusCode::FORBIDDEN,
        AppError::UserExists(_) | AppError::ResourceLocked(_) => StatusCode::CONFLICT,
        AppError::RateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
        AppError::Database(_) | AppError::Plugin(_) | AppError::Config(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
