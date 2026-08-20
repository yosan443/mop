use axum::{extract::State, http::StatusCode, Json};
use mop_auth::{
    backend::{AuthUserRecord, Credentials},
    service::{AuthMetaResponse, AuthService},
    AuthSession, RequireAuth,
};
use mop_core::config::Config;
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::{AuditResult, UserResponse};
use mop_jobs::{AuditLogger, AuditParams};
use serde::Deserialize;
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
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

pub async fn get_auth_meta(
    State(state): State<AppState>,
) -> Result<Json<AuthMetaResponse>, (StatusCode, Json<ErrorResponse>)> {
    let meta = AuthService::get_meta(&state.pool, &state.config)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;
    Ok(Json(meta))
}

pub async fn register(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), (StatusCode, Json<ErrorResponse>)> {
    let user = AuthService::register(
        &state.pool,
        &state.config,
        &payload.username,
        &payload.password,
    )
    .await
    .map_err(|e| {
        let status = match e {
            AppError::RegistrationClosed => StatusCode::FORBIDDEN,
            AppError::Validation(_) | AppError::InvalidPassword(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            AppError::UserExists(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(ErrorResponse::from(e)))
    })?;

    // Auto login registered user
    let record = AuthUserRecord(user.clone());
    if let Err(e) = auth_session.login(&record).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(AppError::Internal(format!(
                "Failed to establish session: {e}"
            )))),
        ));
    }

    // Audit log
    let _ = AuditLogger::log(
        &state.pool,
        AuditParams {
            user_id: Some(&user.id),
            username: Some(&user.username),
            action: "auth.register",
            resource_kind: None,
            resource_id: None,
            detail_json: None,
            result: AuditResult::Success,
        },
    )
    .await;

    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

pub async fn login(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<UserResponse>, (StatusCode, Json<ErrorResponse>)> {
    let creds = Credentials {
        username: payload.username.clone(),
        password: payload.password,
    };

    let user_record = match auth_session.authenticate(creds).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            let _ = AuditLogger::log(
                &state.pool,
                AuditParams {
                    user_id: None,
                    username: Some(&payload.username),
                    action: "auth.login",
                    resource_kind: None,
                    resource_id: None,
                    detail_json: Some(r#"{"reason":"invalid_credentials"}"#),
                    result: AuditResult::Denied,
                },
            )
            .await;

            return Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::from(AppError::InvalidCredentials)),
            ));
        }
        Err(axum_login::Error::Backend(AppError::UserDisabled(username))) => {
            let _ = AuditLogger::log(
                &state.pool,
                AuditParams {
                    user_id: None,
                    username: Some(&username),
                    action: "auth.login",
                    resource_kind: None,
                    resource_id: None,
                    detail_json: Some(r#"{"reason":"user_disabled"}"#),
                    result: AuditResult::Denied,
                },
            )
            .await;

            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::from(AppError::UserDisabled(username))),
            ));
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(AppError::Internal(e.to_string()))),
            ));
        }
    };

    if let Err(e) = auth_session.login(&user_record).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(AppError::Internal(format!(
                "Failed to create session: {e}"
            )))),
        ));
    }

    let user = user_record.into_inner();

    let _ = AuditLogger::log(
        &state.pool,
        AuditParams {
            user_id: Some(&user.id),
            username: Some(&user.username),
            action: "auth.login",
            resource_kind: None,
            resource_id: None,
            detail_json: None,
            result: AuditResult::Success,
        },
    )
    .await;

    Ok(Json(UserResponse::from(user)))
}

pub async fn logout(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    if let Some(user_record) = &auth_session.user {
        let user = &user_record.0;
        let _ = AuditLogger::log(
            &state.pool,
            AuditParams {
                user_id: Some(&user.id),
                username: Some(&user.username),
                action: "auth.logout",
                resource_kind: None,
                resource_id: None,
                detail_json: None,
                result: AuditResult::Success,
            },
        )
        .await;
    }

    if let Err(e) = auth_session.logout().await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(AppError::Internal(format!(
                "Logout failed: {e}"
            )))),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_me(RequireAuth(user): RequireAuth) -> Json<UserResponse> {
    Json(UserResponse::from(user))
}
