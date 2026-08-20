use chrono::Utc;
use mop_core::error::AppError;
use mop_core::models::{AuditEvent, AuditResult};
use mop_db::repos::AuditRepo;
use sqlx::SqlitePool;
use ulid::Ulid;

pub struct AuditParams<'a> {
    pub user_id: Option<&'a str>,
    pub username: Option<&'a str>,
    pub action: &'a str,
    pub resource_kind: Option<&'a str>,
    pub resource_id: Option<&'a str>,
    pub detail_json: Option<&'a str>,
    pub result: AuditResult,
}

pub struct AuditLogger;

impl AuditLogger {
    pub async fn log(pool: &SqlitePool, params: AuditParams<'_>) -> Result<(), AppError> {
        let event = AuditEvent {
            id: Ulid::new().to_string(),
            ts: Utc::now(),
            user_id: params.user_id.map(|s| s.to_string()),
            username: params.username.map(|s| s.to_string()),
            action: params.action.to_string(),
            resource_kind: params.resource_kind.map(|s| s.to_string()),
            resource_id: params.resource_id.map(|s| s.to_string()),
            detail_json: params.detail_json.map(|s| s.to_string()),
            result: params.result,
        };

        AuditRepo::append(pool, &event).await
    }
}
