use mop_core::error::AppError;
use sqlx::SqlitePool;

pub struct AppSettingsRepo;

impl AppSettingsRepo {
    pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, AppError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value_json FROM app_settings WHERE key = ?")
                .bind(key)
                .fetch_optional(pool)
                .await
                .map_err(|e| {
                    AppError::Database(format!("Failed to get app setting '{key}': {e}"))
                })?;

        Ok(row.map(|r| r.0))
    }

    pub async fn set(pool: &SqlitePool, key: &str, value_json: &str) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO app_settings (key, value_json) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        )
        .bind(key)
        .bind(value_json)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to set app setting '{key}': {e}")))?;

        Ok(())
    }
}
