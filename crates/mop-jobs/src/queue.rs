use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::{Job, JobEvent, JobStatus};
use sqlx::{Row, SqlitePool};
use tokio::sync::broadcast;
use ulid::Ulid;

#[derive(Clone)]
pub struct JobService {
    pool: SqlitePool,
    job_tx: broadcast::Sender<Job>,
}

impl JobService {
    pub fn new(pool: SqlitePool) -> Self {
        let (job_tx, _) = broadcast::channel(256);
        Self { pool, job_tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Job> {
        self.job_tx.subscribe()
    }

    pub async fn submit(
        &self,
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
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to submit job: {e}")))?;

        let _ = self.job_tx.send(job.clone());
        Ok(job)
    }

    pub async fn list(&self) -> Result<Vec<Job>, AppError> {
        let rows = sqlx::query(
            "SELECT id, kind, plugin_id, status, params_json, created_by, created_at, started_at, finished_at, error
             FROM jobs ORDER BY created_at DESC LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to list jobs: {e}")))?;

        let mut jobs = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let kind: String = row.get("kind");
            let plugin_id: Option<String> = row.get("plugin_id");
            let status_str: String = row.get("status");
            let params_json: String = row.get("params_json");
            let created_by: String = row.get("created_by");
            let created_at_str: String = row.get("created_at");
            let started_at_str: Option<String> = row.get("started_at");
            let finished_at_str: Option<String> = row.get("finished_at");
            let error: Option<String> = row.get("error");

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let started_at = started_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            let finished_at = finished_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });

            let status = match status_str.as_str() {
                "queued" => JobStatus::Queued,
                "running" => JobStatus::Running,
                "succeeded" => JobStatus::Succeeded,
                "failed" => JobStatus::Failed,
                "canceled" => JobStatus::Canceled,
                _ => JobStatus::Queued,
            };

            jobs.push(Job {
                id,
                kind,
                plugin_id,
                status,
                params_json,
                created_by,
                created_at,
                started_at,
                finished_at,
                error,
            });
        }

        Ok(jobs)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Job>, AppError> {
        let row = sqlx::query(
            "SELECT id, kind, plugin_id, status, params_json, created_by, created_at, started_at, finished_at, error
             FROM jobs WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to get job: {e}")))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let id: String = row.get("id");
        let kind: String = row.get("kind");
        let plugin_id: Option<String> = row.get("plugin_id");
        let status_str: String = row.get("status");
        let params_json: String = row.get("params_json");
        let created_by: String = row.get("created_by");
        let created_at_str: String = row.get("created_at");
        let started_at_str: Option<String> = row.get("started_at");
        let finished_at_str: Option<String> = row.get("finished_at");
        let error: Option<String> = row.get("error");

        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let started_at = started_at_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });
        let finished_at = finished_at_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        let status = match status_str.as_str() {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            "canceled" => JobStatus::Canceled,
            _ => JobStatus::Queued,
        };

        Ok(Some(Job {
            id,
            kind,
            plugin_id,
            status,
            params_json,
            created_by,
            created_at,
            started_at,
            finished_at,
            error,
        }))
    }

    pub async fn get_events(&self, job_id: &str) -> Result<Vec<JobEvent>, AppError> {
        let rows = sqlx::query(
            "SELECT job_id, seq, ts, level, message, data_json
             FROM job_events WHERE job_id = ? ORDER BY seq ASC",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to get job events: {e}")))?;

        let mut events = Vec::new();
        for row in rows {
            let job_id: String = row.get("job_id");
            let seq: i64 = row.get("seq");
            let ts_str: String = row.get("ts");
            let level: String = row.get("level");
            let message: String = row.get("message");
            let data_json: Option<String> = row.get("data_json");

            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            events.push(JobEvent {
                job_id,
                seq,
                ts,
                level,
                message,
                data_json,
            });
        }

        Ok(events)
    }

    pub async fn append_event(
        &self,
        job_id: &str,
        seq: i64,
        level: &str,
        message: &str,
        data_json: Option<&str>,
    ) -> Result<(), AppError> {
        let now_str = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO job_events (job_id, seq, ts, level, message, data_json)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(job_id)
        .bind(seq)
        .bind(now_str)
        .bind(level)
        .bind(message)
        .bind(data_json)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to append job event: {e}")))?;

        Ok(())
    }

    pub async fn update_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error: Option<&str>,
    ) -> Result<Job, AppError> {
        let now_str = Utc::now().to_rfc3339();
        match status {
            JobStatus::Running => {
                sqlx::query("UPDATE jobs SET status = ?, started_at = ? WHERE id = ?")
                    .bind(status.as_str())
                    .bind(&now_str)
                    .bind(job_id)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| AppError::Database(format!("Failed to update job status: {e}")))?;
            }
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Canceled => {
                sqlx::query("UPDATE jobs SET status = ?, finished_at = ?, error = ? WHERE id = ?")
                    .bind(status.as_str())
                    .bind(&now_str)
                    .bind(error)
                    .bind(job_id)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| AppError::Database(format!("Failed to update job status: {e}")))?;
            }
            JobStatus::Queued => {}
        }

        let updated = self
            .get(job_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Job {job_id}")))?;
        let _ = self.job_tx.send(updated.clone());
        Ok(updated)
    }
}
