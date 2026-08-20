use chrono::Utc;
use mop_core::error::AppError;
use mop_core::models::{Job, JobStatus};
use sqlx::SqlitePool;
use ulid::Ulid;

pub struct JobQueue;

impl JobQueue {
    pub async fn submit(
        pool: &SqlitePool,
        kind: &str,
        plugin_id: Option<&str>,
        params_json: &str,
        created_by: &str,
    ) -> Result<Job, AppError> {
        let now = Utc::now();
        let job = Job {
            id: Ulid::new().to_string(),
            kind: kind.to_string(),
            plugin_id: plugin_id.map(|s| s.to_string()),
            status: JobStatus::Queued,
            params_json: params_json.to_string(),
            created_by: created_by.to_string(),
            created_at: now,
            started_at: None,
            finished_at: None,
            error: None,
        };

        sqlx::query(
            "INSERT INTO jobs (id, kind, plugin_id, status, params_json, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&job.id)
        .bind(&job.kind)
        .bind(&job.plugin_id)
        .bind(job.status.as_str())
        .bind(&job.params_json)
        .bind(&job.created_by)
        .bind(job.created_at.to_rfc3339())
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to submit job: {e}")))?;

        Ok(job)
    }
}
