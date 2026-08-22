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
                group_name: Some("Systemd".to_string()),
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
                group_name: Some("Systemd".to_string()),
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
                group_name: Some("Docker".to_string()),
                source: "label".to_string(),
                labels_json: Some(r#"{"mop.managed":"true","mop.group":"Docker"}"#.to_string()),
                first_seen: now,
                last_seen: now,
            },
            // M3 Compose Resources: media-stack project
            Resource {
                id: "compose_project:media-stack".to_string(),
                kind: ResourceKind::ComposeProject,
                name: "media-stack".to_string(),
                display_name: Some("media-stack".to_string()),
                group_name: Some("Docker Compose".to_string()),
                source: "compose".to_string(),
                labels_json: Some(
                    serde_json::json!({
                        "type": "compose_project",
                        "project": "media-stack",
                        "containers_count": 2,
                        "managed_containers_count": 1,
                        "containers": [
                            {
                                "name": "media-stack-manga-worker-1",
                                "service": "manga-worker",
                                "status": "running",
                                "is_managed": true,
                            },
                            {
                                "name": "media-stack-db-1",
                                "service": "db",
                                "status": "running",
                                "is_managed": false,
                            }
                        ],
                    })
                    .to_string(),
                ),
                first_seen: now,
                last_seen: now,
            },
            Resource {
                id: "compose_service:media-stack:manga-worker".to_string(),
                kind: ResourceKind::ComposeService,
                name: "manga-worker".to_string(),
                display_name: Some("media-stack / manga-worker".to_string()),
                group_name: Some("media-stack".to_string()),
                source: "compose".to_string(),
                labels_json: Some(
                    serde_json::json!({
                        "type": "compose_service",
                        "project": "media-stack",
                        "service": "manga-worker",
                        "depends_on": ["db"],
                        "containers_count": 1,
                        "mop.managed": "true",
                        "containers": [
                            {
                                "name": "media-stack-manga-worker-1",
                                "service": "manga-worker",
                                "status": "running",
                                "is_managed": true,
                            }
                        ],
                    })
                    .to_string(),
                ),
                first_seen: now,
                last_seen: now,
            },
            Resource {
                id: "compose_service:media-stack:db".to_string(),
                kind: ResourceKind::ComposeService,
                name: "db".to_string(),
                display_name: Some("media-stack / db".to_string()),
                group_name: Some("media-stack".to_string()),
                source: "compose".to_string(),
                labels_json: Some(
                    serde_json::json!({
                        "type": "compose_service",
                        "project": "media-stack",
                        "service": "db",
                        "depends_on": [],
                        "containers_count": 1,
                        "mop.managed": "false",
                        "containers": [
                            {
                                "name": "media-stack-db-1",
                                "service": "db",
                                "status": "running",
                                "is_managed": false,
                            }
                        ],
                    })
                    .to_string(),
                ),
                first_seen: now,
                last_seen: now,
            },
            Resource {
                id: "docker:media-stack-manga-worker-1".to_string(),
                kind: ResourceKind::DockerContainer,
                name: "media-stack-manga-worker-1".to_string(),
                display_name: Some("media-stack-manga-worker-1".to_string()),
                group_name: Some("media-stack".to_string()),
                source: "compose".to_string(),
                labels_json: Some(
                    serde_json::json!({
                        "com.docker.compose.project": "media-stack",
                        "com.docker.compose.service": "manga-worker",
                        "com.docker.compose.container-number": "1",
                        "com.docker.compose.depends_on": "db",
                        "mop.managed": "true",
                    })
                    .to_string(),
                ),
                first_seen: now,
                last_seen: now,
            },
            Resource {
                id: "docker:media-stack-db-1".to_string(),
                kind: ResourceKind::DockerContainer,
                name: "media-stack-db-1".to_string(),
                display_name: Some("media-stack-db-1".to_string()),
                group_name: Some("media-stack".to_string()),
                source: "compose".to_string(),
                labels_json: Some(
                    serde_json::json!({
                        "com.docker.compose.project": "media-stack",
                        "com.docker.compose.service": "db",
                        "com.docker.compose.container-number": "1",
                        "mop.managed": "false",
                    })
                    .to_string(),
                ),
                first_seen: now,
                last_seen: now,
            },
        ];

        let mut statuses = HashMap::new();
        statuses.insert("systemd:caddy.service".to_string(), ResourceStatus::Running);
        statuses.insert("systemd:nginx.service".to_string(), ResourceStatus::Stopped);
        statuses.insert("docker:komga".to_string(), ResourceStatus::Running);
        statuses.insert(
            "compose_project:media-stack".to_string(),
            ResourceStatus::Running,
        );
        statuses.insert(
            "compose_service:media-stack:manga-worker".to_string(),
            ResourceStatus::Running,
        );
        statuses.insert(
            "compose_service:media-stack:db".to_string(),
            ResourceStatus::Running,
        );
        statuses.insert(
            "docker:media-stack-manga-worker-1".to_string(),
            ResourceStatus::Running,
        );
        statuses.insert(
            "docker:media-stack-db-1".to_string(),
            ResourceStatus::Running,
        );

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

                let (caddy_buf, _nginx_buf, komga_buf, worker_buf) = {
                    let map = buffers.read().await;
                    (
                        map.get("systemd:caddy.service").cloned(),
                        map.get("systemd:nginx.service").cloned(),
                        map.get("docker:komga").cloned(),
                        map.get("compose_service:media-stack:manga-worker").cloned(),
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
                let is_worker_running = {
                    let st = statuses.read().await;
                    st.get("compose_service:media-stack:manga-worker")
                        == Some(&ResourceStatus::Running)
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

                if is_worker_running {
                    if let Some(buf) = worker_buf {
                        buf.push(LogLine {
                            ts: now,
                            stream: "stdout".to_string(),
                            line: format!(
                                "[manga-worker-1] [INFO] Watching inbox queue for incoming archives (seq={seq})"
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

        if id == "compose_service:media-stack:manga-worker" {
            let mut merged = Vec::new();
            if let Some(buf) = map.get("docker:media-stack-manga-worker-1") {
                let lines = buf.get_snapshot(tail, since).await;
                for line in lines {
                    let prefix = "[manga-worker|media-stack-manga-worker-1]";
                    let line_content = if line.line.starts_with(prefix) {
                        line.line
                    } else {
                        format!("{prefix} {}", line.line)
                    };
                    merged.push(LogLine {
                        ts: line.ts,
                        stream: line.stream,
                        line: line_content,
                    });
                }
            }
            if let Some(buf) = map.get(id) {
                merged.extend(buf.get_snapshot(tail, since).await);
            }
            merged.sort_by_key(|l| l.ts);
            if merged.len() > tail {
                let start = merged.len() - tail;
                merged = merged[start..].to_vec();
            }
            return Ok(merged);
        }

        if id == "compose_project:media-stack" {
            let mut merged = Vec::new();
            let children = [
                (
                    "manga-worker",
                    "docker:media-stack-manga-worker-1",
                    "media-stack-manga-worker-1",
                ),
                ("db", "docker:media-stack-db-1", "media-stack-db-1"),
            ];
            for (svc, cont_id, cont_name) in children {
                if let Some(buf) = map.get(cont_id) {
                    let lines = buf.get_snapshot(tail, since).await;
                    for line in lines {
                        let prefix = format!("[{svc}|{cont_name}]");
                        let line_content = if line.line.starts_with(&prefix) {
                            line.line
                        } else {
                            format!("{prefix} {}", line.line)
                        };
                        merged.push(LogLine {
                            ts: line.ts,
                            stream: line.stream,
                            line: line_content,
                        });
                    }
                }
            }
            if let Some(buf) = map.get(id) {
                merged.extend(buf.get_snapshot(tail, since).await);
            }
            merged.sort_by_key(|l| l.ts);
            if merged.len() > tail {
                let start = merged.len() - tail;
                merged = merged[start..].to_vec();
            }
            return Ok(merged);
        }

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

        if id == "compose_service:media-stack:manga-worker" {
            let (tx, rx) = broadcast::channel(512);
            if let Some(buf) = map.get("docker:media-stack-manga-worker-1") {
                let mut child_rx = buf.subscribe();
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    while let Ok(line) = child_rx.recv().await {
                        let prefix = "[manga-worker|media-stack-manga-worker-1]";
                        let line_content = if line.line.starts_with(prefix) {
                            line.line
                        } else {
                            format!("{prefix} {}", line.line)
                        };
                        let _ = tx_clone.send(LogLine {
                            ts: line.ts,
                            stream: line.stream,
                            line: line_content,
                        });
                    }
                });
            }
            return Ok(rx);
        }

        if id == "compose_project:media-stack" {
            let (tx, rx) = broadcast::channel(512);
            let children = [
                (
                    "manga-worker",
                    "docker:media-stack-manga-worker-1",
                    "media-stack-manga-worker-1",
                ),
                ("db", "docker:media-stack-db-1", "media-stack-db-1"),
            ];
            for (svc, cont_id, cont_name) in children {
                if let Some(buf) = map.get(cont_id) {
                    let mut child_rx = buf.subscribe();
                    let tx_clone = tx.clone();
                    let svc_str = svc.to_string();
                    let cont_str = cont_name.to_string();
                    tokio::spawn(async move {
                        while let Ok(line) = child_rx.recv().await {
                            let prefix = format!("[{svc_str}|{cont_str}]");
                            let line_content = if line.line.starts_with(&prefix) {
                                line.line
                            } else {
                                format!("{prefix} {}", line.line)
                            };
                            let _ = tx_clone.send(LogLine {
                                ts: line.ts,
                                stream: line.stream,
                                line: line_content,
                            });
                        }
                    });
                }
            }
            return Ok(rx);
        }

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

        // Unmanaged service or container protection (SPEC §9.3 & 不変条件 3)
        if id == "compose_service:media-stack:db" || id == "docker:media-stack-db-1" {
            return Err(AppError::ActionNotAllowed(
                action.to_string(),
                format!("{id} has no managed containers (mop.managed=true required)"),
            ));
        }

        let kind = if id.starts_with("systemd:") {
            ResourceKind::SystemdUnit
        } else if id.starts_with("compose_project:") {
            ResourceKind::ComposeProject
        } else if id.starts_with("compose_service:") {
            ResourceKind::ComposeService
        } else {
            ResourceKind::DockerContainer
        };

        // Simulate realistic backend execution time so lock is held during immediate concurrent requests
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        let target_status = match action {
            "start" => ResourceStatus::Running,
            "stop" => ResourceStatus::Stopped,
            "restart" => {
                // Set to restarting first, then simulate return to running
                {
                    let mut write = self.statuses.write().await;
                    write.insert(id.to_string(), ResourceStatus::Restarting);

                    // If restarting compose_project or compose_service, also update managed child containers
                    if id == "compose_project:media-stack" {
                        write.insert(
                            "compose_service:media-stack:manga-worker".to_string(),
                            ResourceStatus::Restarting,
                        );
                        write.insert(
                            "docker:media-stack-manga-worker-1".to_string(),
                            ResourceStatus::Restarting,
                        );
                    } else if id == "compose_service:media-stack:manga-worker" {
                        write.insert(
                            "docker:media-stack-manga-worker-1".to_string(),
                            ResourceStatus::Restarting,
                        );
                    }
                }

                let _ = self.event_tx.send(ResourceEvent {
                    id: id.to_string(),
                    kind,
                    status: ResourceStatus::Restarting,
                    ts: Utc::now(),
                    message: Some(format!("Resource {id} is restarting")),
                });

                if id == "compose_project:media-stack" {
                    let _ = self.event_tx.send(ResourceEvent {
                        id: "compose_service:media-stack:manga-worker".to_string(),
                        kind: ResourceKind::ComposeService,
                        status: ResourceStatus::Restarting,
                        ts: Utc::now(),
                        message: Some("Service manga-worker is restarting".to_string()),
                    });
                    let _ = self.event_tx.send(ResourceEvent {
                        id: "docker:media-stack-manga-worker-1".to_string(),
                        kind: ResourceKind::DockerContainer,
                        status: ResourceStatus::Restarting,
                        ts: Utc::now(),
                        message: Some(
                            "Container media-stack-manga-worker-1 is restarting".to_string(),
                        ),
                    });
                } else if id == "compose_service:media-stack:manga-worker" {
                    let _ = self.event_tx.send(ResourceEvent {
                        id: "docker:media-stack-manga-worker-1".to_string(),
                        kind: ResourceKind::DockerContainer,
                        status: ResourceStatus::Restarting,
                        ts: Utc::now(),
                        message: Some(
                            "Container media-stack-manga-worker-1 is restarting".to_string(),
                        ),
                    });
                }

                // Spawn transition back to running after 400ms
                let statuses = self.statuses.clone();
                let id_clone = id.to_string();
                let event_tx = self.event_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                    {
                        let mut write = statuses.write().await;
                        write.insert(id_clone.clone(), ResourceStatus::Running);
                        if id_clone == "compose_project:media-stack" {
                            write.insert(
                                "compose_service:media-stack:manga-worker".to_string(),
                                ResourceStatus::Running,
                            );
                            write.insert(
                                "docker:media-stack-manga-worker-1".to_string(),
                                ResourceStatus::Running,
                            );
                        } else if id_clone == "compose_service:media-stack:manga-worker" {
                            write.insert(
                                "docker:media-stack-manga-worker-1".to_string(),
                                ResourceStatus::Running,
                            );
                        }
                    }

                    let _ = event_tx.send(ResourceEvent {
                        id: id_clone.clone(),
                        kind,
                        status: ResourceStatus::Running,
                        ts: Utc::now(),
                        message: Some(format!("Resource {id_clone} restarted successfully")),
                    });

                    if id_clone == "compose_project:media-stack" {
                        let _ = event_tx.send(ResourceEvent {
                            id: "compose_service:media-stack:manga-worker".to_string(),
                            kind: ResourceKind::ComposeService,
                            status: ResourceStatus::Running,
                            ts: Utc::now(),
                            message: Some(
                                "Service manga-worker restarted successfully".to_string(),
                            ),
                        });
                        let _ = event_tx.send(ResourceEvent {
                            id: "docker:media-stack-manga-worker-1".to_string(),
                            kind: ResourceKind::DockerContainer,
                            status: ResourceStatus::Running,
                            ts: Utc::now(),
                            message: Some(
                                "Container media-stack-manga-worker-1 restarted successfully"
                                    .to_string(),
                            ),
                        });
                    } else if id_clone == "compose_service:media-stack:manga-worker" {
                        let _ = event_tx.send(ResourceEvent {
                            id: "docker:media-stack-manga-worker-1".to_string(),
                            kind: ResourceKind::DockerContainer,
                            status: ResourceStatus::Running,
                            ts: Utc::now(),
                            message: Some(
                                "Container media-stack-manga-worker-1 restarted successfully"
                                    .to_string(),
                            ),
                        });
                    }
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
