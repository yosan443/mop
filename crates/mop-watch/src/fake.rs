use crate::traits::{LogLine, ResourceCollector, ResourceDetail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct FakeResourceCollector {
    state: Arc<RwLock<FakeState>>,
}

struct FakeState {
    resources: Vec<Resource>,
    statuses: std::collections::HashMap<String, ResourceStatus>,
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

        let mut statuses = std::collections::HashMap::new();
        statuses.insert("systemd:caddy.service".to_string(), ResourceStatus::Running);
        statuses.insert("systemd:nginx.service".to_string(), ResourceStatus::Stopped);
        statuses.insert("docker:komga".to_string(), ResourceStatus::Running);

        Self {
            state: Arc::new(RwLock::new(FakeState {
                resources,
                statuses,
            })),
        }
    }
}

#[async_trait]
impl ResourceCollector for FakeResourceCollector {
    async fn list_resources(&self) -> Result<Vec<Resource>, AppError> {
        let read = self.state.read().await;
        Ok(read.resources.clone())
    }

    async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        let read = self.state.read().await;
        let res = read.resources.iter().find(|r| r.id == id);
        let Some(resource) = res else {
            return Ok(None);
        };

        let status = read
            .statuses
            .get(id)
            .copied()
            .unwrap_or(ResourceStatus::Unknown);
        let active_state = match status {
            ResourceStatus::Running => "active",
            ResourceStatus::Stopped => "inactive",
            ResourceStatus::Failed => "failed",
            ResourceStatus::Degraded => "degraded",
            ResourceStatus::Restarting => "reloading",
            ResourceStatus::Unknown => "unknown",
        };

        Ok(Some(ResourceDetail {
            resource: resource.clone(),
            status,
            active_state: active_state.to_string(),
            sub_state: Some("running".to_string()),
            uptime_secs: Some(3600),
            memory_bytes: Some(1024 * 1024 * 64),
            cpu_percent: Some(1.2),
        }))
    }

    async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        _since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError> {
        let read = self.state.read().await;
        if !read.resources.iter().any(|r| r.id == id) {
            return Err(AppError::ResourceNotFound(id.to_string()));
        }

        let now = Utc::now();
        let sample_lines = [
            LogLine {
                ts: now - chrono::Duration::seconds(30),
                stream: "stdout".to_string(),
                line: format!("[INFO] Service {id} initialized successfully"),
            },
            LogLine {
                ts: now - chrono::Duration::seconds(20),
                stream: "stdout".to_string(),
                line: format!("[INFO] Ready to accept connections for {id}"),
            },
            LogLine {
                ts: now - chrono::Duration::seconds(5),
                stream: "stdout".to_string(),
                line: format!("[INFO] Heartbeat check passed for {id}"),
            },
        ];

        let count = sample_lines.len().min(tail);
        Ok(sample_lines[sample_lines.len() - count..].to_vec())
    }

    async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        let mut write = self.state.write().await;
        if !write.resources.iter().any(|r| r.id == id) {
            return Err(AppError::ResourceNotFound(id.to_string()));
        }

        match action {
            "start" => {
                write
                    .statuses
                    .insert(id.to_string(), ResourceStatus::Running);
            }
            "stop" => {
                write
                    .statuses
                    .insert(id.to_string(), ResourceStatus::Stopped);
            }
            "restart" => {
                write
                    .statuses
                    .insert(id.to_string(), ResourceStatus::Running);
            }
            other => {
                return Err(AppError::ActionNotAllowed(
                    other.to_string(),
                    id.to_string(),
                ));
            }
        }

        Ok(())
    }
}
