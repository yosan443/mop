use mop_core::error::AppError;
pub use mop_plugin_sdk::*;
use std::path::Path;

pub struct PluginSupervisor;

impl PluginSupervisor {
    pub fn parse_manifest(path: &Path) -> Result<PluginManifest, AppError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            AppError::Plugin(format!(
                "Failed to read plugin.toml at {}: {e}",
                path.display()
            ))
        })?;
        let manifest: PluginManifest = toml::from_str(&content)
            .map_err(|e| AppError::Plugin(format!("Failed to parse plugin manifest: {e}")))?;
        Ok(manifest)
    }
}
