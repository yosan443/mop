use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDetail {
    pub resource: Resource,
    pub status: ResourceStatus,
    pub active_state: String,
    pub sub_state: Option<String>,
    pub uptime_secs: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub ts: DateTime<Utc>,
    pub stream: String,
    pub line: String,
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
    async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError>;
}
