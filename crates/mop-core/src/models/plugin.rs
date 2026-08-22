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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissionRecord {
    pub plugin_id: String,
    pub capability: String,
    pub value_json: String,
    pub granted_by: String,
    pub granted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingDiffItem {
    pub key: String,
    pub applied_value: Option<serde_json::Value>,
    pub draft_value: Option<serde_json::Value>,
    pub change_type: String, // "added", "modified", "deleted", "unchanged"
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SettingsDiff {
    pub items: Vec<SettingDiffItem>,
}
