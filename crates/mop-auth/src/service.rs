use crate::password::hash_password;
use chrono::Utc;
use mop_core::config::{Config, RegistrationMode};
use mop_core::error::AppError;
use mop_core::models::{Role, User};
use mop_db::repos::UserRepo;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMetaResponse {
    pub needs_setup: bool,
    pub registration: RegistrationMode,
    pub min_password_len: usize,
}

pub struct AuthService;

impl AuthService {
    pub async fn get_meta(
        pool: &SqlitePool,
        config: &Config,
    ) -> Result<AuthMetaResponse, AppError> {
        let user_count = UserRepo::count(pool).await?;
        let needs_setup = user_count == 0;

        Ok(AuthMetaResponse {
            needs_setup,
            registration: config.auth.registration,
            min_password_len: config.auth.min_password_len,
        })
    }

    pub async fn register(
        pool: &SqlitePool,
        config: &Config,
        username: &str,
        password: &str,
    ) -> Result<User, AppError> {
        let trimmed_username = username.trim();
        if trimmed_username.is_empty() {
            return Err(AppError::Validation("Username cannot be empty".into()));
        }
        if password.len() < config.auth.min_password_len {
            return Err(AppError::InvalidPassword(format!(
                "Password must be at least {} characters",
                config.auth.min_password_len
            )));
        }

        let user_count = UserRepo::count(pool).await?;
        let is_first_user = user_count == 0;

        let role = if is_first_user {
            // First user is always admin
            Role::Admin
        } else {
            // Check registration mode
            match config.auth.registration {
                RegistrationMode::FirstUser => {
                    return Err(AppError::RegistrationClosed);
                }
                RegistrationMode::Closed => {
                    return Err(AppError::RegistrationClosed);
                }
                RegistrationMode::Open => {
                    // Open registration defaults to viewer
                    Role::Viewer
                }
            }
        };

        let password_hash = hash_password(password)?;
        let now = Utc::now();
        let user = User {
            id: Ulid::new().to_string(),
            username: trimmed_username.to_string(),
            password_hash,
            role,
            disabled: false,
            created_at: now,
            updated_at: now,
        };

        UserRepo::create(pool, &user).await?;
        Ok(user)
    }

    pub async fn create_user_by_admin(
        pool: &SqlitePool,
        config: &Config,
        username: &str,
        password: &str,
        role: Role,
    ) -> Result<User, AppError> {
        let trimmed_username = username.trim();
        if trimmed_username.is_empty() {
            return Err(AppError::Validation("Username cannot be empty".into()));
        }
        if password.len() < config.auth.min_password_len {
            return Err(AppError::InvalidPassword(format!(
                "Password must be at least {} characters",
                config.auth.min_password_len
            )));
        }

        let password_hash = hash_password(password)?;
        let now = Utc::now();
        let user = User {
            id: Ulid::new().to_string(),
            username: trimmed_username.to_string(),
            password_hash,
            role,
            disabled: false,
            created_at: now,
            updated_at: now,
        };

        UserRepo::create(pool, &user).await?;
        Ok(user)
    }
}
