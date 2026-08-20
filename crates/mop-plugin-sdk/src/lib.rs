use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub backend: Option<BackendConfig>,
    pub ui: Option<UiConfig>,
    pub capabilities: PluginCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub exec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub entry: String,
    pub element: String,
    pub routes: Vec<String>,
    pub nav: Option<NavConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavConfig {
    pub title: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub filesystem_read: Vec<String>,
    #[serde(default)]
    pub filesystem_write: Vec<String>,
    #[serde(default)]
    pub jobs: Vec<String>,
    #[serde(default)]
    pub resources_read: Vec<String>,
    #[serde(default)]
    pub resources_action: Vec<String>,
    #[serde(default)]
    pub network: bool,
}
