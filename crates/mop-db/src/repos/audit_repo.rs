use chrono::DateTime;
use mop_core::error::AppError;
use mop_core::models::{AuditEvent, AuditResult};
use sqlx::{Row, SqlitePool};

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

    pub async fn list(pool: &SqlitePool) -> Result<Vec<AuditEvent>, AppError> {
        let rows = sqlx::query(
            "SELECT id, ts, user_id, username, action, resource_kind, resource_id, detail_json, result
             FROM audit_events ORDER BY ts ASC"
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to list audit events: {e}")))?;

        let mut events = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let ts_str: String = row.get("ts");
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    AppError::Database(format!("Invalid timestamp in audit event: {e}"))
                })?;
            let user_id: Option<String> = row.get("user_id");
            let username: Option<String> = row.get("username");
            let action: String = row.get("action");
            let resource_kind: Option<String> = row.get("resource_kind");
            let resource_id: Option<String> = row.get("resource_id");
            let detail_json: Option<String> = row.get("detail_json");
            let result_str: String = row.get("result");

            let result = match result_str.as_str() {
                "success" => AuditResult::Success,
                "denied" => AuditResult::Denied,
                "failure" => AuditResult::Failure,
                _ => AuditResult::Failure,
            };

            events.push(AuditEvent {
                id,
                ts,
                user_id,
                username,
                action,
                resource_kind,
                resource_id,
                detail_json,
                result,
            });
        }

        Ok(events)
    }
}
