use crate::handlers::auth::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use mop_auth::{hash_password, service::AuthService, RequireAdmin};
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::{AuditResult, Role, UserResponse};
use mop_db::repos::UserRepo;
use mop_jobs::{AuditLogger, AuditParams};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Role,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub role: Option<Role>,
    pub disabled: Option<bool>,
    pub password: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
) -> Result<Json<Vec<UserResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let users = UserRepo::list(&state.pool).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(e)),
        )
    })?;

    let responses = users.into_iter().map(UserResponse::from).collect();
    Ok(Json(responses))
}

pub async fn create_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Json(payload): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), (StatusCode, Json<ErrorResponse>)> {
    let user = AuthService::create_user_by_admin(
        &state.pool,
        &state.config,
        &payload.username,
        &payload.password,
        payload.role,
    )
    .await
    .map_err(|e| {
        let status = match e {
            AppError::UserExists(_) => StatusCode::CONFLICT,
            AppError::Validation(_) | AppError::InvalidPassword(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(ErrorResponse::from(e)))
    })?;

    let detail = format!(
        r#"{{"target_user_id":"{}","role":"{}"}}"#,
        user.id, user.role
    );
    let _ = AuditLogger::log(
        &state.pool,
        AuditParams {
            user_id: Some(&admin.id),
            username: Some(&admin.username),
            action: "user.create",
            resource_kind: None,
            resource_id: Some(&user.id),
            detail_json: Some(&detail),
            result: AuditResult::Success,
        },
    )
    .await;

    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

pub async fn update_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<String>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Prevent admin from disabling themselves or removing their admin role
    if admin.id == id {
        if let Some(disabled) = payload.disabled {
            if disabled {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::from(AppError::BadRequest(
                        "Cannot disable your own admin account".into(),
                    ))),
                ));
            }
        }
        if let Some(role) = payload.role {
            if role != Role::Admin {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::from(AppError::BadRequest(
                        "Cannot remove admin role from your own account".into(),
                    ))),
                ));
            }
        }
    }

    let password_hash = if let Some(pw) = &payload.password {
        if pw.len() < state.config.auth.min_password_len {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse::from(AppError::InvalidPassword(format!(
                    "Password must be at least {} characters",
                    state.config.auth.min_password_len
                )))),
            ));
        }
        Some(hash_password(pw).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?)
    } else {
        None
    };

    let user = UserRepo::update(
        &state.pool,
        &id,
        payload.role,
        payload.disabled,
        password_hash.as_deref(),
    )
    .await
    .map_err(|e| {
        let status = match e {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(ErrorResponse::from(e)))
    })?;

    let detail = format!(r#"{{"role":"{}","disabled":{}}}"#, user.role, user.disabled);
    let _ = AuditLogger::log(
        &state.pool,
        AuditParams {
            user_id: Some(&admin.id),
            username: Some(&admin.username),
            action: "user.update",
            resource_kind: None,
            resource_id: Some(&user.id),
            detail_json: Some(&detail),
            result: AuditResult::Success,
        },
    )
    .await;

    Ok(Json(UserResponse::from(user)))
}
