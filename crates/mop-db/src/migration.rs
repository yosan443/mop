use mop_core::error::AppError;
use sqlx::{Executor, SqlitePool};
use tracing::info;

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), AppError> {
    // Ensure schema_migrations table exists
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL
        );",
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::Database(format!("Failed to create schema_migrations table: {e}")))?;

    // Define migrations
    let migrations: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_initial_schema.sql"))];

    for (version, sql) in migrations {
        let exists: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM schema_migrations WHERE version = ?")
                .bind(version)
                .fetch_one(pool)
                .await
                .map_err(|e| {
                    AppError::Database(format!("Failed to check migration version {version}: {e}"))
                })?;

        if exists.0 == 0 {
            info!("Applying database migration version {version}");
            let mut tx = pool.begin().await.map_err(|e| {
                AppError::Database(format!("Failed to begin migration transaction: {e}"))
            })?;

            tx.execute(*sql).await.map_err(|e| {
                AppError::Database(format!("Failed to execute migration {version}: {e}"))
            })?;

            let now = chrono::Utc::now().to_rfc3339();
            sqlx::query("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)")
                .bind(version)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    AppError::Database(format!("Failed to record migration {version}: {e}"))
                })?;

            tx.commit().await.map_err(|e| {
                AppError::Database(format!(
                    "Failed to commit migration transaction {version}: {e}"
                ))
            })?;
        }
    }

    Ok(())
}
