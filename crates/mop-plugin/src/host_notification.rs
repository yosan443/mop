use mop_core::error::AppError;
use mop_core::models::JobStatus;
use mop_jobs::JobService;
use mop_plugin_sdk::{JobFinishedParams, JobLogParams, JobProgressParams, RpcNotification};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, error, warn};

/// Handler for receiving notifications from plugins (Plugin -> Host via Unix socket)
#[derive(Clone)]
pub struct HostNotificationHandler {
    job_service: JobService,
    socket_path: PathBuf,
    event_seq: Arc<AtomicI64>,
}

impl HostNotificationHandler {
    pub fn new(job_service: JobService, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            job_service,
            socket_path: socket_path.into(),
            event_seq: Arc::new(AtomicI64::new(1)),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Process a single incoming notification with strict job_id/plugin_id verification
    pub async fn handle_notification(
        &self,
        sender_plugin_id: &str,
        notification: RpcNotification,
    ) -> Result<(), AppError> {
        debug!(
            "Received notification from plugin '{}': method={}",
            sender_plugin_id, notification.method
        );

        match notification.method.as_str() {
            "job.progress" => {
                let params_val = notification.params.ok_or_else(|| {
                    AppError::Plugin("job.progress notification missing params".to_string())
                })?;
                let params: JobProgressParams = serde_json::from_value(params_val)
                    .map_err(|e| AppError::Plugin(format!("Invalid job.progress params: {e}")))?;

                self.verify_job_ownership(&params.job_id, sender_plugin_id)
                    .await?;

                let seq = self.event_seq.fetch_add(1, Ordering::SeqCst);
                let data = serde_json::json!({ "percent": params.percent });
                self.job_service
                    .append_event(
                        &params.job_id,
                        seq,
                        "info",
                        &params.message,
                        Some(&data.to_string()),
                    )
                    .await?;

                // Also update job to running if it was queued
                if let Some(job) = self.job_service.get(&params.job_id).await? {
                    if job.status == JobStatus::Queued {
                        self.job_service
                            .update_status(&params.job_id, JobStatus::Running, None)
                            .await?;
                    }
                }
            }

            "job.log" => {
                let params_val = notification.params.ok_or_else(|| {
                    AppError::Plugin("job.log notification missing params".to_string())
                })?;
                let params: JobLogParams = serde_json::from_value(params_val)
                    .map_err(|e| AppError::Plugin(format!("Invalid job.log params: {e}")))?;

                self.verify_job_ownership(&params.job_id, sender_plugin_id)
                    .await?;

                let seq = self.event_seq.fetch_add(1, Ordering::SeqCst);
                self.job_service
                    .append_event(&params.job_id, seq, &params.level, &params.message, None)
                    .await?;
            }

            "job.finished" => {
                let params_val = notification.params.ok_or_else(|| {
                    AppError::Plugin("job.finished notification missing params".to_string())
                })?;
                let params: JobFinishedParams = serde_json::from_value(params_val)
                    .map_err(|e| AppError::Plugin(format!("Invalid job.finished params: {e}")))?;

                self.verify_job_ownership(&params.job_id, sender_plugin_id)
                    .await?;

                let status = match params.status.as_str() {
                    "completed" | "succeeded" => JobStatus::Succeeded,
                    "failed" => JobStatus::Failed,
                    "cancelled" | "canceled" => JobStatus::Canceled,
                    _ => JobStatus::Failed,
                };

                self.job_service
                    .update_status(&params.job_id, status, params.error.as_deref())
                    .await?;

                let seq = self.event_seq.fetch_add(1, Ordering::SeqCst);
                let finish_msg = if let Some(err) = &params.error {
                    format!("Job finished with status '{}': {}", params.status, err)
                } else {
                    format!("Job finished with status '{}'", params.status)
                };
                self.job_service
                    .append_event(
                        &params.job_id,
                        seq,
                        if status == JobStatus::Succeeded {
                            "info"
                        } else {
                            "error"
                        },
                        &finish_msg,
                        None,
                    )
                    .await?;
            }

            other => {
                warn!(
                    "Ignored unknown notification method '{}' from plugin '{}'",
                    other, sender_plugin_id
                );
            }
        }

        Ok(())
    }

    /// Verify that the job belongs to the specified plugin (SPEC requirement: anti-spoofing)
    async fn verify_job_ownership(
        &self,
        job_id: &str,
        sender_plugin_id: &str,
    ) -> Result<(), AppError> {
        let job = self
            .job_service
            .get(job_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Job {job_id} not found")))?;

        match job.plugin_id.as_deref() {
            Some(pid) if pid == sender_plugin_id => Ok(()),
            Some(pid) => {
                error!(
                    "Security violation: plugin '{}' attempted to report notification for job '{}' owned by '{}'",
                    sender_plugin_id, job_id, pid
                );
                Err(AppError::Forbidden(format!(
                    "Job {job_id} does not belong to plugin {sender_plugin_id}"
                )))
            }
            None => {
                error!(
                    "Security violation: plugin '{}' attempted to report notification for non-plugin job '{}'",
                    sender_plugin_id, job_id
                );
                Err(AppError::Forbidden(format!(
                    "Job {job_id} is not a plugin job"
                )))
            }
        }
    }

    /// Start listening on host.sock for plugin notifications
    pub async fn start_listener(
        &self,
        sender_plugin_id: &str,
    ) -> Result<tokio::task::JoinHandle<()>, AppError> {
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        if let Some(parent) = self.socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let listener = UnixListener::bind(&self.socket_path).map_err(|e| {
            AppError::Plugin(format!(
                "Failed to bind host notification socket {}: {e}",
                self.socket_path.display()
            ))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o660));
        }

        let handler = self.clone();
        let plugin_id = sender_plugin_id.to_string();

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let h = handler.clone();
                        let pid = plugin_id.clone();
                        tokio::spawn(async move {
                            let mut lines = BufReader::new(stream).lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                if line.trim().is_empty() {
                                    continue;
                                }
                                if let Ok(notif) = serde_json::from_str::<RpcNotification>(&line) {
                                    if let Err(e) = h.handle_notification(&pid, notif).await {
                                        error!("Error handling notification from {pid}: {e}");
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        debug!("Host notification listener stopped: {e}");
                        break;
                    }
                }
            }
        });

        Ok(handle)
    }
}
