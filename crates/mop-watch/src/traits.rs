use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogLine {
    pub ts: DateTime<Utc>,
    pub stream: String, // stdout | stderr | journal
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDetail {
    pub resource: Resource,
    pub status: ResourceStatus,
    pub active_state: String,
    pub sub_state: Option<String>,
    pub uptime_secs: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEvent {
    pub id: String,
    pub kind: ResourceKind,
    pub status: ResourceStatus,
    pub ts: DateTime<Utc>,
    pub message: Option<String>,
}

#[async_trait]
pub trait ResourceCollector: Send + Sync {
    async fn list_resources(&self) -> Result<Vec<Resource>, AppError>;
    async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError>;
    async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError>;
    async fn subscribe_logs(
        &self,
        id: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError>;
    async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError>;
    fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent>;
}
