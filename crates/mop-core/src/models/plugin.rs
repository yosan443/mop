use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    Installed,
    Enabled,
    Running,
    Degraded,
    Disabled,
}

impl PluginState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginState::Installed => "installed",
            PluginState::Enabled => "enabled",
            PluginState::Running => "running",
            PluginState::Degraded => "degraded",
            PluginState::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub state: PluginState,
    pub manifest_json: String,
    pub installed_at: DateTime<Utc>,
    pub enabled_at: Option<DateTime<Utc>>,
}
