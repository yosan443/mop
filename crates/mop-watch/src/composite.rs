use crate::docker::DockerCollector;
use crate::systemd::SystemdCollector;
use crate::traits::{LogLine, ResourceCollector, ResourceDetail, ResourceEvent};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mop_core::config::ResourcesConfig;
use mop_core::error::AppError;
use mop_core::models::Resource;
use tokio::sync::broadcast;

pub struct CompositeCollector {
    systemd: SystemdCollector,
    docker: DockerCollector,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl CompositeCollector {
    pub async fn new(config: ResourcesConfig) -> Result<Self, AppError> {
        let (event_tx, _) = broadcast::channel(256);
        let systemd = SystemdCollector::new(config.systemd, event_tx.clone()).await?;
        let docker = DockerCollector::new(config.docker, event_tx.clone()).await?;

        Ok(Self {
            systemd,
            docker,
            event_tx,
        })
    }
}

#[async_trait]
impl ResourceCollector for CompositeCollector {
    async fn list_resources(&self) -> Result<Vec<Resource>, AppError> {
        let mut all = Vec::new();
        if let Ok(sys_res) = self.systemd.list_resources().await {
            all.extend(sys_res);
        }
        if let Ok(dock_res) = self.docker.list_resources().await {
            all.extend(dock_res);
        }
        Ok(all)
    }

    async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        if id.starts_with("systemd:") {
            self.systemd.get_resource_detail(id).await
        } else if id.starts_with("docker:") {
            self.docker.get_resource_detail(id).await
        } else {
            Ok(None)
        }
    }

    async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError> {
        if id.starts_with("systemd:") {
            if let Some(buf) = self.systemd.get_log_buffer(id) {
                return Ok(buf.get_snapshot(tail, since).await);
            }
        } else if id.starts_with("docker:") {
            let buf = self.docker.get_log_buffer(id).await;
            return Ok(buf.get_snapshot(tail, since).await);
        }
        Err(AppError::ResourceNotFound(id.to_string()))
    }

    async fn subscribe_logs(
        &self,
        id: &str,
        _since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError> {
        if id.starts_with("systemd:") {
            if let Some(buf) = self.systemd.get_log_buffer(id) {
                return Ok(buf.subscribe());
            }
        } else if id.starts_with("docker:") {
            let buf = self.docker.get_log_buffer(id).await;
            return Ok(buf.subscribe());
        }
        Err(AppError::ResourceNotFound(id.to_string()))
    }

    async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        if id.starts_with("systemd:") {
            self.systemd.execute_action(id, action).await
        } else if id.starts_with("docker:") {
            self.docker.execute_action(id, action).await
        } else {
            Err(AppError::ResourceNotFound(id.to_string()))
        }
    }

    fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.event_tx.subscribe()
    }
}
