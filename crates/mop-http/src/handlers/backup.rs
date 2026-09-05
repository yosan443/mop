use crate::handlers::auth::AppState;
use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, Utc};
use mop_auth::RequireAdmin;
use mop_core::error::ErrorResponse;
use mop_core::models::{AuditResult, JobStatus};
use mop_jobs::{AuditLogger, AuditParams};

pub async fn create_backup_handler(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let job = state
        .job_service
        .submit("backup.create", None, "{}", &admin.username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;

    let pool = state.pool.clone();
    let config = state.config.clone();
    let job_service = state.job_service.clone();
    let job_id = job.id.clone();
    let user_id = admin.id.clone();
    let username = admin.username.clone();

    tokio::spawn(async move {
        let _ = job_service
            .update_status(&job_id, JobStatus::Running, None)
            .await;
        let _ = job_service
            .append_event(
                &job_id,
                1,
                "info",
                "Starting database snapshot and archive creation",
                None,
            )
            .await;

        match mop_db::create_backup_archive(&pool, &config, &config.backup.dir).await {
            Ok(archive_path) => {
                let msg = format!("Backup archive created at {}", archive_path.display());
                let detail = serde_json::json!({
                    "path": archive_path.to_string_lossy()
                })
                .to_string();
                let _ = job_service
                    .append_event(&job_id, 2, "info", &msg, Some(&detail))
                    .await;
                let _ = job_service
                    .update_status(&job_id, JobStatus::Succeeded, None)
                    .await;

                let _ = AuditLogger::log(
                    &pool,
                    AuditParams {
                        user_id: Some(&user_id),
                        username: Some(&username),
                        action: "backup.create",
                        resource_kind: Some("backup"),
                        resource_id: Some(archive_path.to_string_lossy().as_ref()),
                        detail_json: Some(
                            &serde_json::json!({ "archive": archive_path.to_string_lossy() })
                                .to_string(),
                        ),
                        result: AuditResult::Success,
                    },
                )
                .await;
            }
            Err(e) => {
                let err_msg = format!("Backup creation failed: {e}");
                let _ = job_service
                    .append_event(&job_id, 2, "error", &err_msg, None)
                    .await;
                let _ = job_service
                    .update_status(&job_id, JobStatus::Failed, Some(&err_msg))
                    .await;

                let _ = AuditLogger::log(
                    &pool,
                    AuditParams {
                        user_id: Some(&user_id),
                        username: Some(&username),
                        action: "backup.create",
                        resource_kind: Some("backup"),
                        resource_id: None,
                        detail_json: Some(&serde_json::json!({ "error": err_msg }).to_string()),
                        result: AuditResult::Failure,
                    },
                )
                .await;
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "job_id": job.id,
            "status": "queued",
            "message": "Backup job submitted successfully"
        })),
    ))
}

pub async fn list_backups_handler(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let backup_dir = &state.config.backup.dir;
    let mut backups = Vec::new();

    if backup_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.starts_with("mop-backup-") && name.ends_with(".tar.zst") {
                        if let Ok(meta) = entry.metadata() {
                            let mtime: Option<DateTime<Utc>> =
                                meta.modified().ok().map(|t| t.into());
                            backups.push(serde_json::json!({
                                "filename": name,
                                "size_bytes": meta.len(),
                                "created_at": mtime.map(|t| t.to_rfc3339()).unwrap_or_default(),
                            }));
                        }
                    }
                }
            }
        }
    }

    backups.sort_by(|a, b| {
        let b_ts = b["created_at"].as_str().unwrap_or("");
        let a_ts = a["created_at"].as_str().unwrap_or("");
        b_ts.cmp(a_ts)
    });

    Ok(Json(serde_json::json!({ "backups": backups })))
}
