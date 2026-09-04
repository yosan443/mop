use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginSdkError {
    #[error("Manifest parse error: {0}")]
    ManifestParse(String),
    #[error("Manifest validation error: {0}")]
    ManifestValidation(String),
    #[error("JSON-RPC error [{code}]: {message}")]
    Rpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Timeout error: {0}")]
    Timeout(String),
}

// -----------------------------------------------------------------------------
// Plugin Manifest (SPEC.md §13.2)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub backend: Option<BackendConfig>,
    pub ui: Option<UiConfig>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub exec: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    pub entry: String,
    pub element: String,
    #[serde(default)]
    pub routes: Vec<String>,
    pub nav: Option<NavConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavConfig {
    pub title: String,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
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

impl PluginManifest {
    pub fn parse_and_validate(content: &str) -> Result<Self, PluginSdkError> {
        let manifest: PluginManifest =
            toml::from_str(content).map_err(|e| PluginSdkError::ManifestParse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), PluginSdkError> {
        // 1. Validate ID (alphanumeric, dot, underscore, hyphen; no path traversal characters)
        if self.id.is_empty() {
            return Err(PluginSdkError::ManifestValidation(
                "Plugin id cannot be empty".to_string(),
            ));
        }
        if !self
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
            || self.id.contains("..")
        {
            return Err(PluginSdkError::ManifestValidation(format!(
                "Invalid plugin id '{}': must contain only alphanumeric, dot, underscore, or hyphen",
                self.id
            )));
        }

        // 2. Validate API version (must be "1" for SPEC v1)
        if self.api_version != "1" {
            return Err(PluginSdkError::ManifestValidation(format!(
                "Unsupported api_version '{}': expected '1'",
                self.api_version
            )));
        }

        // 3. Validate backend exec path if present (relative path, no traversal)
        if let Some(backend) = &self.backend {
            if backend.exec.is_empty() {
                return Err(PluginSdkError::ManifestValidation(
                    "backend.exec cannot be empty".to_string(),
                ));
            }
            if backend.exec.starts_with('/') || backend.exec.contains("..") {
                return Err(PluginSdkError::ManifestValidation(format!(
                    "backend.exec '{}' must be a relative path without '..'",
                    backend.exec
                )));
            }
        }

        // 4. Validate UI entry path if present (relative path, no traversal)
        if let Some(ui) = &self.ui {
            if ui.entry.is_empty() {
                return Err(PluginSdkError::ManifestValidation(
                    "ui.entry cannot be empty".to_string(),
                ));
            }
            if ui.entry.starts_with('/') || ui.entry.contains("..") {
                return Err(PluginSdkError::ManifestValidation(format!(
                    "ui.entry '{}' must be a relative path without '..'",
                    ui.entry
                )));
            }
            if ui.element.is_empty() {
                return Err(PluginSdkError::ManifestValidation(
                    "ui.element cannot be empty".to_string(),
                ));
            }
        }

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// JSON-RPC 2.0 Protocol (SPEC.md §13.3)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl RpcRequest {
    pub fn new(
        id: impl Into<serde_json::Value>,
        method: impl Into<String>,
        params: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id.into()),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl RpcNotification {
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<serde_json::Value>, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>, data: Option<serde_json::Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    // Standard JSON-RPC 2.0 error codes
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    // Custom mop error codes
    pub const TIMEOUT: i64 = -32000;
    pub const UNAUTHORIZED: i64 = -32001;
    pub const ACTION_NOT_ALLOWED: i64 = -32002;
    pub const CAPABILITY_REQUIRED: i64 = -32003;

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(Self::INTERNAL_ERROR, message, None)
    }

    pub fn method_not_found(message: impl Into<String>) -> Self {
        Self::new(Self::METHOD_NOT_FOUND, message, None)
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(Self::PARSE_ERROR, message, None)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(Self::INVALID_PARAMS, message, None)
    }
}

// -----------------------------------------------------------------------------
// Standard RPC Method Parameter & Result Types (SPEC.md §13.3)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub capabilities: PluginCapabilities,
    pub settings: serde_json::Value,
    pub api_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginJobInfo {
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeResult {
    pub jobs: Vec<PluginJobInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: String, // "ok", "warn", "error"
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorResult {
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSubmitParams {
    pub job_id: String,
    pub kind: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCancelParams {
    pub job_id: String,
}

// Plugin -> Host Notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgressParams {
    pub job_id: String,
    pub percent: u8,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobLogParams {
    pub job_id: String,
    pub level: String, // "info", "warn", "error"
    pub message: String,
    #[serde(default = "Utc::now")]
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFinishedParams {
    pub job_id: String,
    pub status: String, // "completed", "failed", "cancelled"
    pub error: Option<String>,
}
