use crate::backend::AuthSession;
use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::User;

/// Extractor that requires an authenticated user with at least `Viewer` role
pub struct RequireAuth(pub User);

impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_session = AuthSession::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::from(AppError::Internal(
                        "Auth session error".into(),
                    ))),
                )
                    .into_response()
            })?;

        match auth_session.user {
            Some(auth_user) => {
                let user = auth_user.into_inner();
                if user.disabled {
                    Err((
                        StatusCode::FORBIDDEN,
                        Json(ErrorResponse::from(AppError::UserDisabled(user.username))),
                    )
                        .into_response())
                } else {
                    Ok(RequireAuth(user))
                }
            }
            None => Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::from(AppError::Unauthorized(
                    "Authentication required".into(),
                ))),
            )
                .into_response()),
        }
    }
}

/// Extractor that requires an authenticated user with at least `Operator` role
pub struct RequireOperator(pub User);

impl<S> FromRequestParts<S> for RequireOperator
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let RequireAuth(user) = RequireAuth::from_request_parts(parts, state).await?;

        if user.role.can_operate() {
            Ok(RequireOperator(user))
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::from(AppError::Forbidden(
                    "Operator role required".into(),
                ))),
            )
                .into_response())
        }
    }
}

/// Extractor that requires an authenticated user with `Admin` role
pub struct RequireAdmin(pub User);

impl<S> FromRequestParts<S> for RequireAdmin
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let RequireAuth(user) = RequireAuth::from_request_parts(parts, state).await?;

        if user.role.can_administer() {
            Ok(RequireAdmin(user))
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::from(AppError::Forbidden(
                    "Admin role required".into(),
                ))),
            )
                .into_response())
        }
    }
}
