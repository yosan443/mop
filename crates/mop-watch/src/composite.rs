use crate::compose::ComposeCollector;
use crate::docker::DockerCollector;
use crate::systemd::SystemdCollector;
use crate::traits::{LogLine, ResourceCollector, ResourceDetail, ResourceEvent};
use async_trait::async_trait;
use bollard::Docker;
use chrono::{DateTime, Utc};
use mop_core::config::ResourcesConfig;
use mop_core::error::AppError;
use mop_core::models::Resource;
use tokio::sync::broadcast;

pub struct CompositeCollector {
    systemd: SystemdCollector,
    docker: DockerCollector,
    compose: ComposeCollector,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl CompositeCollector {
    pub async fn new(config: ResourcesConfig) -> Result<Self, AppError> {
        let (event_tx, _) = broadcast::channel(256);
        let systemd = SystemdCollector::new(config.systemd, event_tx.clone()).await?;
        let docker = DockerCollector::new(config.docker, event_tx.clone()).await?;

        let docker_client = Docker::connect_with_local_defaults().ok();
        let compose = ComposeCollector::new(docker_client, event_tx.clone());

        Ok(Self {
            systemd,
            docker,
            compose,
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
        if let Ok(comp_res) = self.compose.list_resources().await {
            all.extend(comp_res);
        }
        Ok(all)
    }

    async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        if id.starts_with("systemd:") {
            self.systemd.get_resource_detail(id).await
        } else if id.starts_with("docker:") {
            self.docker.get_resource_detail(id).await
        } else if id.starts_with("compose_project:") || id.starts_with("compose_service:") {
            self.compose.get_resource_detail(id).await
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
            self.systemd.get_logs(id, tail, since).await
        } else if id.starts_with("docker:") {
            self.docker.get_logs(id, tail, since).await
        } else if id.starts_with("compose_project:") || id.starts_with("compose_service:") {
            self.compose.get_logs(id, tail, since).await
        } else {
            Err(AppError::ResourceNotFound(id.to_string()))
        }
    }

    async fn subscribe_logs(
        &self,
        id: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError> {
        if id.starts_with("systemd:") {
            self.systemd.subscribe_logs(id, since).await
        } else if id.starts_with("docker:") {
            self.docker.subscribe_logs(id, since).await
        } else if id.starts_with("compose_project:") || id.starts_with("compose_service:") {
            self.compose.subscribe_logs(id, since).await
        } else {
            Err(AppError::ResourceNotFound(id.to_string()))
        }
    }

    async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        if id.starts_with("systemd:") {
            self.systemd.execute_action(id, action).await
        } else if id.starts_with("docker:") {
            self.docker.execute_action(id, action).await
        } else if id.starts_with("compose_project:") || id.starts_with("compose_service:") {
            self.compose.execute_action(id, action).await
        } else {
            Err(AppError::ResourceNotFound(id.to_string()))
        }
    }

    fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.event_tx.subscribe()
    }
}
