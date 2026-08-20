use crate::ring_buffer::ResourceLogBuffer;
use crate::traits::{LogLine, ResourceCollector, ResourceDetail, ResourceEvent};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct FakeResourceCollector {
    resources: Arc<RwLock<Vec<Resource>>>,
    statuses: Arc<RwLock<HashMap<String, ResourceStatus>>>,
    log_buffers: Arc<RwLock<HashMap<String, ResourceLogBuffer>>>,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl Default for FakeResourceCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeResourceCollector {
    pub fn new() -> Self {
        let now = Utc::now();
        let resources = vec![
            Resource {
                id: "systemd:caddy.service".to_string(),
                kind: ResourceKind::SystemdUnit,
                name: "caddy.service".to_string(),
                display_name: Some("Caddy Web Server".to_string()),
                group_name: Some("Web".to_string()),
                source: "allowlist".to_string(),
                labels_json: None,
                first_seen: now,
                last_seen: now,
            },
            Resource {
                id: "systemd:nginx.service".to_string(),
                kind: ResourceKind::SystemdUnit,
                name: "nginx.service".to_string(),
                display_name: Some("Nginx Reverse Proxy".to_string()),
                group_name: Some("Web".to_string()),
                source: "allowlist".to_string(),
                labels_json: None,
                first_seen: now,
                last_seen: now,
            },
            Resource {
                id: "docker:komga".to_string(),
                kind: ResourceKind::DockerContainer,
                name: "komga".to_string(),
                display_name: Some("Komga Media Server".to_string()),
                group_name: Some("Media".to_string()),
                source: "label".to_string(),
                labels_json: Some(r#"{"mop.managed":"true","mop.group":"Media"}"#.to_string()),
                first_seen: now,
                last_seen: now,
            },
        ];

        let mut statuses = HashMap::new();
        statuses.insert("systemd:caddy.service".to_string(), ResourceStatus::Running);
        statuses.insert("systemd:nginx.service".to_string(), ResourceStatus::Stopped);
        statuses.insert("docker:komga".to_string(), ResourceStatus::Running);

        let mut log_buffers = HashMap::new();
        for r in &resources {
            let buf = ResourceLogBuffer::new(5000, 65536);
            let res_id = r.id.clone();
            tokio::spawn({
                let buf = buf.clone();
                async move {
                    buf.push(LogLine {
                        ts: now - chrono::Duration::seconds(30),
                        stream: "stdout".to_string(),
                        line: format!("[INFO] Service {res_id} initialized successfully"),
                    })
                    .await;
                    buf.push(LogLine {
                        ts: now - chrono::Duration::seconds(15),
                        stream: "stdout".to_string(),
                        line: "[INFO] Accepting incoming connections on port 8080".to_string(),
                    })
                    .await;
                    buf.push(LogLine {
                        ts: now - chrono::Duration::seconds(5),
                        stream: "stdout".to_string(),
                        line: "[INFO] Healthcheck ok (0.42ms)".to_string(),
                    })
                    .await;
                }
            });
            log_buffers.insert(r.id.clone(), buf);
        }

        let (event_tx, _) = broadcast::channel(256);

        let collector = Self {
            resources: Arc::new(RwLock::new(resources)),
            statuses: Arc::new(RwLock::new(statuses)),
            log_buffers: Arc::new(RwLock::new(log_buffers)),
            event_tx,
        };

        // Start background log generator task
        collector.start_background_log_generator();
        collector
    }

    fn start_background_log_generator(&self) {
        let buffers = self.log_buffers.clone();
        let statuses = self.statuses.clone();

        tokio::spawn(async move {
            let mut seq = 0u64;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                seq += 1;
                let now = Utc::now();

                let (caddy_buf, _nginx_buf, komga_buf) = {
                    let map = buffers.read().await;
                    (
                        map.get("systemd:caddy.service").cloned(),
                        map.get("systemd:nginx.service").cloned(),
                        map.get("docker:komga").cloned(),
                    )
                };

                let is_caddy_running = {
                    let st = statuses.read().await;
                    st.get("systemd:caddy.service") == Some(&ResourceStatus::Running)
                };
                let is_komga_running = {
                    let st = statuses.read().await;
                    st.get("docker:komga") == Some(&ResourceStatus::Running)
                };

                if is_caddy_running {
                    if let Some(buf) = caddy_buf {
                        buf.push(LogLine {
                            ts: now,
                            stream: "stdout".to_string(),
                            line: format!(
                                "[INFO] HTTP request GET / status=200 latency=1.{}ms req_id={seq}",
                                seq % 9
                            ),
                        })
                        .await;
                    }
                }

                if is_komga_running {
                    if let Some(buf) = komga_buf {
                        buf.push(LogLine {
                            ts: now,
                            stream: "stdout".to_string(),
                            line: format!(
                                "[INFO] Library scan completed: 42 books verified (seq={seq})"
                            ),
                        })
                        .await;
                    }
                }
            }
        });
    }
}

#[async_trait]
impl ResourceCollector for FakeResourceCollector {
    async fn list_resources(&self) -> Result<Vec<Resource>, AppError> {
        let read = self.resources.read().await;
        Ok(read.clone())
    }

    async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        let res_read = self.resources.read().await;
        let Some(resource) = res_read.iter().find(|r| r.id == id) else {
            return Ok(None);
        };

        let status = {
            let st_read = self.statuses.read().await;
            st_read.get(id).copied().unwrap_or(ResourceStatus::Unknown)
        };

        let active_state = match status {
            ResourceStatus::Running => "active",
            ResourceStatus::Stopped => "inactive",
            ResourceStatus::Failed => "failed",
            ResourceStatus::Degraded => "degraded",
            ResourceStatus::Restarting => "reloading",
            ResourceStatus::Unknown => "unknown",
        };

        let sub_state = match status {
            ResourceStatus::Running => "running",
            ResourceStatus::Stopped => "dead",
            ResourceStatus::Failed => "failed",
            ResourceStatus::Degraded => "degraded",
            ResourceStatus::Restarting => "start-pre",
            ResourceStatus::Unknown => "unknown",
        };

        let (uptime_secs, memory_bytes, cpu_percent) = match status {
            ResourceStatus::Running => (Some(7200), Some(1024 * 1024 * 128), Some(2.4)),
            ResourceStatus::Restarting => (Some(1), Some(1024 * 1024 * 64), Some(15.8)),
            _ => (None, None, None),
        };

        Ok(Some(ResourceDetail {
            resource: resource.clone(),
            status,
            active_state: active_state.to_string(),
            sub_state: Some(sub_state.to_string()),
            uptime_secs,
            memory_bytes,
            cpu_percent,
        }))
    }

    async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError> {
        let map = self.log_buffers.read().await;
        let Some(buf) = map.get(id) else {
            return Err(AppError::ResourceNotFound(id.to_string()));
        };

        Ok(buf.get_snapshot(tail, since).await)
    }

    async fn subscribe_logs(
        &self,
        id: &str,
        _since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError> {
        let map = self.log_buffers.read().await;
        let Some(buf) = map.get(id) else {
            return Err(AppError::ResourceNotFound(id.to_string()));
        };

        Ok(buf.subscribe())
    }

    async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        {
            let res_read = self.resources.read().await;
            if !res_read.iter().any(|r| r.id == id) {
                return Err(AppError::ResourceNotFound(id.to_string()));
            }
        }

        let kind = if id.starts_with("systemd:") {
            ResourceKind::SystemdUnit
        } else {
            ResourceKind::DockerContainer
        };

        let target_status = match action {
            "start" => ResourceStatus::Running,
            "stop" => ResourceStatus::Stopped,
            "restart" => {
                // Set to restarting first, then simulate return to running
                {
                    let mut write = self.statuses.write().await;
                    write.insert(id.to_string(), ResourceStatus::Restarting);
                }
                let _ = self.event_tx.send(ResourceEvent {
                    id: id.to_string(),
                    kind,
                    status: ResourceStatus::Restarting,
                    ts: Utc::now(),
                    message: Some(format!("Resource {id} is restarting")),
                });

                // Spawn transition back to running after 400ms
                let statuses = self.statuses.clone();
                let id_clone = id.to_string();
                let event_tx = self.event_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                    {
                        let mut write = statuses.write().await;
                        write.insert(id_clone.clone(), ResourceStatus::Running);
                    }
                    let _ = event_tx.send(ResourceEvent {
                        id: id_clone.clone(),
                        kind,
                        status: ResourceStatus::Running,
                        ts: Utc::now(),
                        message: Some(format!("Resource {id_clone} restarted successfully")),
                    });
                });

                return Ok(());
            }
            other => {
                return Err(AppError::ActionNotAllowed(
                    other.to_string(),
                    id.to_string(),
                ));
            }
        };

        {
            let mut write = self.statuses.write().await;
            write.insert(id.to_string(), target_status);
        }

        let _ = self.event_tx.send(ResourceEvent {
            id: id.to_string(),
            kind,
            status: target_status,
            ts: Utc::now(),
            message: Some(format!("Resource {id} transitioned to {target_status}")),
        });

        Ok(())
    }

    fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.event_tx.subscribe()
    }
}
