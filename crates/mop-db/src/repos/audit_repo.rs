use mop_core::error::AppError;
use mop_core::models::AuditEvent;
use sqlx::SqlitePool;

pub struct AuditRepo;

impl AuditRepo {
    pub async fn append(pool: &SqlitePool, event: &AuditEvent) -> Result<(), AppError> {
        let ts_str = event.ts.to_rfc3339();
        let result_str = event.result.as_str();

        sqlx::query(
            "INSERT INTO audit_events (id, ts, user_id, username, action, resource_kind, resource_id, detail_json, result)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&event.id)
        .bind(ts_str)
        .bind(&event.user_id)
        .bind(&event.username)
        .bind(&event.action)
        .bind(&event.resource_kind)
        .bind(&event.resource_id)
        .bind(&event.detail_json)
        .bind(result_str)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to append audit event: {e}")))?;

        Ok(())
    }
}
