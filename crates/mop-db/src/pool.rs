use mop_core::error::AppError;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

pub async fn create_sqlite_pool(db_path: &Path) -> Result<SqlitePool, AppError> {
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AppError::Database(format!(
                    "Failed to create database directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    let connection_string = format!("sqlite://{}", db_path.to_string_lossy());
    let opts = SqliteConnectOptions::from_str(&connection_string)
        .map_err(|e| AppError::Database(format!("Invalid database connection string: {e}")))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_millis(5000))
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(opts)
        .await
        .map_err(|e| AppError::Database(format!("Failed to connect to SQLite: {e}")))?;

    Ok(pool)
}
