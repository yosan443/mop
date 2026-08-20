use chrono::Utc;
use mop_core::error::AppError;
use mop_core::models::{Role, User};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

pub struct UserRepo;

impl UserRepo {
    pub async fn count(pool: &SqlitePool) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::Database(format!("Failed to count users: {e}")))?;
        Ok(row.0)
    }

    pub async fn create(pool: &SqlitePool, user: &User) -> Result<(), AppError> {
        let created_at = user.created_at.to_rfc3339();
        let updated_at = user.updated_at.to_rfc3339();
        let role_str = user.role.as_str();

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, disabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&user.id)
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(role_str)
        .bind(if user.disabled { 1 } else { 0 })
        .bind(created_at)
        .bind(updated_at)
        .execute(pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(db_err) = &e {
                if db_err.is_unique_violation() {
                    return AppError::UserExists(user.username.clone());
                }
            }
            AppError::Database(format!("Failed to insert user: {e}"))
        })?;

        Ok(())
    }

    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, role, disabled, created_at, updated_at
             FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to find user by id: {e}")))?;

        match row {
            Some(row) => Ok(Some(Self::row_to_user(row)?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_username(
        pool: &SqlitePool,
        username: &str,
    ) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, role, disabled, created_at, updated_at
             FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to find user by username: {e}")))?;

        match row {
            Some(row) => Ok(Some(Self::row_to_user(row)?)),
            None => Ok(None),
        }
    }

    pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, AppError> {
        let rows = sqlx::query(
            "SELECT id, username, password_hash, role, disabled, created_at, updated_at
             FROM users ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to list users: {e}")))?;

        let mut users = Vec::with_capacity(rows.len());
        for row in rows {
            users.push(Self::row_to_user(row)?);
        }
        Ok(users)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: &str,
        role: Option<Role>,
        disabled: Option<bool>,
        password_hash: Option<&str>,
    ) -> Result<User, AppError> {
        let mut user = Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("User '{id}' not found")))?;

        if let Some(r) = role {
            user.role = r;
        }
        if let Some(d) = disabled {
            user.disabled = d;
        }
        if let Some(pw) = password_hash {
            user.password_hash = pw.to_string();
        }
        user.updated_at = Utc::now();

        let updated_at = user.updated_at.to_rfc3339();
        let role_str = user.role.as_str();

        sqlx::query(
            "UPDATE users SET role = ?, disabled = ?, password_hash = ?, updated_at = ? WHERE id = ?"
        )
        .bind(role_str)
        .bind(if user.disabled { 1 } else { 0 })
        .bind(&user.password_hash)
        .bind(updated_at)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to update user: {e}")))?;

        Ok(user)
    }

    fn row_to_user(row: sqlx::sqlite::SqliteRow) -> Result<User, AppError> {
        let id: String = row.get("id");
        let username: String = row.get("username");
        let password_hash: String = row.get("password_hash");
        let role_str: String = row.get("role");
        let disabled_int: i64 = row.get("disabled");
        let created_at_str: String = row.get("created_at");
        let updated_at_str: String = row.get("updated_at");

        let role = Role::from_str(&role_str)
            .map_err(|e| AppError::Database(format!("Invalid role '{role_str}' in DB: {e}")))?;

        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|e| AppError::Database(format!("Invalid created_at timestamp in DB: {e}")))?
            .with_timezone(&Utc);

        let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
            .map_err(|e| AppError::Database(format!("Invalid updated_at timestamp in DB: {e}")))?
            .with_timezone(&Utc);

        Ok(User {
            id,
            username,
            password_hash,
            role,
            disabled: disabled_int != 0,
            created_at,
            updated_at,
        })
    }
}
