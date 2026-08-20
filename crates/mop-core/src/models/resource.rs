use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    SystemdUnit,
    DockerContainer,
    ComposeService,
    ComposeProject,
}

impl ResourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceKind::SystemdUnit => "systemd_unit",
            ResourceKind::DockerContainer => "docker_container",
            ResourceKind::ComposeService => "compose_service",
            ResourceKind::ComposeProject => "compose_project",
        }
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    Running,
    Stopped,
    Failed,
    Degraded,
    Restarting,
    Unknown,
}

impl ResourceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceStatus::Running => "running",
            ResourceStatus::Stopped => "stopped",
            ResourceStatus::Failed => "failed",
            ResourceStatus::Degraded => "degraded",
            ResourceStatus::Restarting => "restarting",
            ResourceStatus::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ResourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub kind: ResourceKind,
    pub name: String,
    pub display_name: Option<String>,
    pub group_name: Option<String>,
    pub source: String,
    pub labels_json: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}
