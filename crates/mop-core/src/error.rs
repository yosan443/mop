use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InternalError,
    NotFound,
    BadRequest,
    Unauthorized,
    Forbidden,
    InvalidCredentials,
    UserExists,
    UserDisabled,
    SetupAlreadyCompleted,
    SetupRequired,
    RegistrationClosed,
    InvalidPassword,
    ResourceNotFound,
    ActionNotAllowed,
    ResourceLocked,
    RateLimitExceeded,
    CsrfOriginMismatch,
    DatabaseError,
    PluginError,
    ValidationError,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Invalid username or password")]
    InvalidCredentials,

    #[error("User '{0}' already exists")]
    UserExists(String),

    #[error("User '{0}' is disabled")]
    UserDisabled(String),

    #[error("Initial setup has already been completed")]
    SetupAlreadyCompleted,

    #[error("Initial setup is required before using mop")]
    SetupRequired,

    #[error("User registration is closed")]
    RegistrationClosed,

    #[error("Password does not meet minimum requirements: {0}")]
    InvalidPassword(String),

    #[error("Resource '{0}' not found")]
    ResourceNotFound(String),

    #[error("Action '{0}' is not allowed on resource '{1}'")]
    ActionNotAllowed(String, String),

    #[error("Resource '{0}' is locked: another action is currently in progress")]
    ResourceLocked(String),

    #[error("Rate limit exceeded: please try again later")]
    RateLimitExceeded,

    #[error("CSRF protection: Origin or Referer header mismatch")]
    CsrfOriginMismatch,

    #[error("Database error: {0}")]
    Database(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

impl AppError {
    pub fn error_code(&self) -> ErrorCode {
        match self {
            AppError::Internal(_) => ErrorCode::InternalError,
            AppError::NotFound(_) => ErrorCode::NotFound,
            AppError::BadRequest(_) => ErrorCode::BadRequest,
            AppError::Unauthorized(_) => ErrorCode::Unauthorized,
            AppError::Forbidden(_) => ErrorCode::Forbidden,
            AppError::InvalidCredentials => ErrorCode::InvalidCredentials,
            AppError::UserExists(_) => ErrorCode::UserExists,
            AppError::UserDisabled(_) => ErrorCode::UserDisabled,
            AppError::SetupAlreadyCompleted => ErrorCode::SetupAlreadyCompleted,
            AppError::SetupRequired => ErrorCode::SetupRequired,
            AppError::RegistrationClosed => ErrorCode::RegistrationClosed,
            AppError::InvalidPassword(_) => ErrorCode::InvalidPassword,
            AppError::ResourceNotFound(_) => ErrorCode::ResourceNotFound,
            AppError::ActionNotAllowed(_, _) => ErrorCode::ActionNotAllowed,
            AppError::ResourceLocked(_) => ErrorCode::ResourceLocked,
            AppError::RateLimitExceeded => ErrorCode::RateLimitExceeded,
            AppError::CsrfOriginMismatch => ErrorCode::CsrfOriginMismatch,
            AppError::Database(_) => ErrorCode::DatabaseError,
            AppError::Plugin(_) => ErrorCode::PluginError,
            AppError::Config(_) => ErrorCode::InternalError,
            AppError::Validation(_) => ErrorCode::ValidationError,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponsePayload {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorResponsePayload,
}

impl From<&AppError> for ErrorResponse {
    fn from(err: &AppError) -> Self {
        Self {
            error: ErrorResponsePayload {
                code: err.error_code(),
                message: err.to_string(),
            },
        }
    }
}

impl From<AppError> for ErrorResponse {
    fn from(err: AppError) -> Self {
        ErrorResponse::from(&err)
    }
}
