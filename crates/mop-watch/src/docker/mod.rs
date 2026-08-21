use crate::ring_buffer::ResourceLogBuffer;
use crate::traits::{LogLine, ResourceDetail, ResourceEvent};
use bollard::container::{
    ListContainersOptions, LogsOptions, RestartContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::system::EventsOptions;
use bollard::Docker;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use mop_core::config::DockerResourcesConfig;
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub struct DockerCollector {
    config: DockerResourcesConfig,
    docker: Option<Docker>,
    log_buffers: Arc<RwLock<HashMap<String, ResourceLogBuffer>>>,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl DockerCollector {
    pub async fn new(
        config: DockerResourcesConfig,
        event_tx: broadcast::Sender<ResourceEvent>,
    ) -> Result<Self, AppError> {
        let docker = match Docker::connect_with_local_defaults() {
            Ok(d) => Some(d),
            Err(e) => {
                tracing::warn!("Failed to connect to Docker daemon: {e}");
                None
            }
        };

        let collector = Self {
            config,
            docker,
            log_buffers: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        };

        // Start background event listener for Docker container lifecycle events
        collector.start_docker_event_listener();

        // Start background log collectors for managed containers
        collector.start_container_log_tailers();

        Ok(collector)
    }

    pub fn log_buffers(&self) -> Arc<RwLock<HashMap<String, ResourceLogBuffer>>> {
        self.log_buffers.clone()
    }

    fn start_docker_event_listener(&self) {
        let Some(docker) = self.docker.clone() else {
            return;
        };
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut filters = HashMap::new();
            filters.insert("type".to_string(), vec!["container".to_string()]);
            let options = EventsOptions::<String> {
                filters,
                ..Default::default()
            };

            let mut stream = docker.events(Some(options));
            while let Some(event_result) = stream.next().await {
                if let Ok(evt) = event_result {
                    let action = evt.action.unwrap_or_default();
                    let container_name = evt
                        .actor
                        .as_ref()
                        .and_then(|a| a.attributes.as_ref())
                        .and_then(|attr| attr.get("name").cloned())
                        .map(|n| n.trim_start_matches('/').to_string())
                        .or_else(|| evt.actor.as_ref().and_then(|a| a.id.clone()))
                        .unwrap_or_default();
                    let status = match action.as_str() {
                        "start" | "unpause" => ResourceStatus::Running,
                        "stop" | "die" | "kill" | "pause" => ResourceStatus::Stopped,
                        "restart" => ResourceStatus::Restarting,
                        _ => continue,
                    };

                    let _ = event_tx.send(ResourceEvent {
                        id: format!("docker:{container_name}"),
                        kind: ResourceKind::DockerContainer,
                        status,
                        ts: Utc::now(),
                        message: Some(format!("Docker event: {action}")),
                    });
                }
            }
        });
    }

    fn start_container_log_tailers(&self) {
        let Some(docker) = self.docker.clone() else {
            return;
        };
        let log_buffers = self.log_buffers.clone();
        let containers = self.config.containers.clone();

        tokio::spawn(async move {
            for container in containers {
                let id = format!("docker:{container}");
                let mut map = log_buffers.write().await;
                let buf = map
                    .entry(id.clone())
                    .or_insert_with(|| ResourceLogBuffer::new(5000, 65536))
                    .clone();
                drop(map);

                let d = docker.clone();
                let c_name = container.clone();
                tokio::spawn(async move {
                    let options = LogsOptions::<String> {
                        follow: true,
                        stdout: true,
                        stderr: true,
                        tail: "500".to_string(),
                        ..Default::default()
                    };

                    let mut stream = d.logs(&c_name, Some(options));
                    while let Some(log_item) = stream.next().await {
                        if let Ok(output) = log_item {
                            let line_str = output.to_string();
                            buf.push(LogLine {
                                ts: Utc::now(),
                                stream: "stdout".to_string(),
                                line: line_str.trim_end().to_string(),
                            })
                            .await;
                        }
                    }
                });
            }
        });
    }

    pub async fn list_resources(&self) -> Result<Vec<Resource>, AppError> {
        let Some(docker) = &self.docker else {
            return Ok(Vec::new());
        };

        let mut filters = HashMap::new();
        if !self.config.label_selector.is_empty() {
            filters.insert(
                "label".to_string(),
                vec![self.config.label_selector.clone()],
            );
        }

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = docker
            .list_containers(Some(options))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list docker containers: {e}")))?;

        let now = Utc::now();
        let mut resources = Vec::new();

        for c in containers {
            let names = c.names.unwrap_or_default();
            let raw_name = names
                .first()
                .map(|n| n.trim_start_matches('/'))
                .unwrap_or("unknown");
            let id = format!("docker:{raw_name}");

            // Filter against allowlist if containers list is set
            if !self.config.containers.is_empty()
                && !self.config.containers.iter().any(|name| name == raw_name)
            {
                continue;
            }

            let labels = c.labels.unwrap_or_default();
            let display_name = labels
                .get("mop.display-name")
                .cloned()
                .or_else(|| Some(raw_name.to_string()));
            let group_name = labels
                .get("mop.group")
                .cloned()
                .or_else(|| Some("Docker".to_string()));
            let labels_json = serde_json::to_string(&labels).ok();

            resources.push(Resource {
                id,
                kind: ResourceKind::DockerContainer,
                name: raw_name.to_string(),
                display_name,
                group_name,
                source: if labels.contains_key("mop.managed") {
                    "label".to_string()
                } else {
                    "allowlist".to_string()
                },
                labels_json,
                first_seen: now,
                last_seen: now,
            });
        }

        Ok(resources)
    }

    pub async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        let container_name = id.strip_prefix("docker:").unwrap_or(id);
        let Some(docker) = &self.docker else {
            return Ok(None);
        };

        let inspect = match docker.inspect_container(container_name, None).await {
            Ok(res) => res,
            Err(_) => return Ok(None),
        };

        let state = inspect.state.unwrap_or_default();
        let status_str = state
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let status = match status_str.as_str() {
            "running" => ResourceStatus::Running,
            "exited" | "dead" | "created" => ResourceStatus::Stopped,
            "restarting" => ResourceStatus::Restarting,
            _ => ResourceStatus::Unknown,
        };

        let sub_state = state.health.and_then(|h| h.status).map(|s| s.to_string());

        Ok(Some(ResourceDetail {
            resource: Resource {
                id: id.to_string(),
                kind: ResourceKind::DockerContainer,
                name: container_name.to_string(),
                display_name: Some(container_name.to_string()),
                group_name: Some("Docker".to_string()),
                source: "allowlist".to_string(),
                labels_json: inspect
                    .config
                    .and_then(|c| c.labels)
                    .and_then(|l| serde_json::to_string(&l).ok()),
                first_seen: Utc::now(),
                last_seen: Utc::now(),
            },
            status,
            active_state: status_str,
            sub_state,
            uptime_secs: None,
            memory_bytes: None,
            cpu_percent: None,
        }))
    }

    pub async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        let container_name = id.strip_prefix("docker:").unwrap_or(id);
        let Some(docker) = &self.docker else {
            return Err(AppError::Internal(
                "Docker daemon is not available".to_string(),
            ));
        };

        if !self.config.allow_actions.iter().any(|a| a == action) {
            return Err(AppError::ActionNotAllowed(
                action.to_string(),
                id.to_string(),
            ));
        }

        match action {
            "start" => {
                docker
                    .start_container(container_name, None::<StartContainerOptions<String>>)
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to start container {container_name}: {e}"
                        ))
                    })?;
            }
            "stop" => {
                docker
                    .stop_container(container_name, None::<StopContainerOptions>)
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to stop container {container_name}: {e}"
                        ))
                    })?;
            }
            "restart" => {
                docker
                    .restart_container(container_name, None::<RestartContainerOptions>)
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to restart container {container_name}: {e}"
                        ))
                    })?;
            }
            other => {
                return Err(AppError::ActionNotAllowed(
                    other.to_string(),
                    id.to_string(),
                ))
            }
        }

        Ok(())
    }

    pub async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError> {
        let mut map = self.log_buffers.write().await;
        let buf = map
            .entry(id.to_string())
            .or_insert_with(|| ResourceLogBuffer::new(5000, 65536));
        Ok(buf.get_snapshot(tail, since).await)
    }

    pub async fn subscribe_logs(
        &self,
        id: &str,
        _since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError> {
        let mut map = self.log_buffers.write().await;
        let buf = map
            .entry(id.to_string())
            .or_insert_with(|| ResourceLogBuffer::new(5000, 65536));
        Ok(buf.subscribe())
    }
}
