use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationMode {
    #[default]
    FirstUser,
    Open,
    Closed,
}

impl std::fmt::Display for RegistrationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistrationMode::FirstUser => write!(f, "first_user"),
            RegistrationMode::Open => write!(f, "open"),
            RegistrationMode::Closed => write!(f, "closed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    pub public_url: Option<String>,
}

fn default_bind() -> String {
    "127.0.0.1:8787".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            public_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: PathBuf,
}

fn default_db_path() -> PathBuf {
    PathBuf::from("/var/lib/mop/mop.db")
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub registration: RegistrationMode,
    #[serde(default = "default_min_password_len")]
    pub min_password_len: usize,
    #[serde(default = "default_session_hours")]
    pub session_hours: u64,
}

fn default_min_password_len() -> usize {
    10
}

fn default_session_hours() -> u64 {
    12
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            registration: RegistrationMode::FirstUser,
            min_password_len: default_min_password_len(),
            session_hours: default_session_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdResourcesConfig {
    #[serde(default)]
    pub units: Vec<String>,
    #[serde(default = "default_allow_actions")]
    pub allow_actions: Vec<String>,
}

fn default_allow_actions() -> Vec<String> {
    vec![
        "start".to_string(),
        "stop".to_string(),
        "restart".to_string(),
    ]
}

impl Default for SystemdResourcesConfig {
    fn default() -> Self {
        Self {
            units: Vec::new(),
            allow_actions: default_allow_actions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerResourcesConfig {
    #[serde(default)]
    pub containers: Vec<String>,
    #[serde(default = "default_label_selector")]
    pub label_selector: String,
    #[serde(default = "default_allow_actions")]
    pub allow_actions: Vec<String>,
}

fn default_label_selector() -> String {
    "mop.managed=true".to_string()
}

impl Default for DockerResourcesConfig {
    fn default() -> Self {
        Self {
            containers: Vec::new(),
            label_selector: default_label_selector(),
            allow_actions: default_allow_actions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourcesConfig {
    #[serde(default)]
    pub fake: bool,
    #[serde(default)]
    pub systemd: SystemdResourcesConfig,
    #[serde(default)]
    pub docker: DockerResourcesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsLimitsConfig {
    #[serde(default = "default_history_lines")]
    pub history_lines_per_resource: usize,
    #[serde(default = "default_ring_buffer_lines")]
    pub ring_buffer_lines_per_resource: usize,
    #[serde(default = "default_max_line_bytes")]
    pub max_line_bytes: usize,
    #[serde(default = "default_max_streams_user")]
    pub max_streams_per_user: usize,
    #[serde(default = "default_max_streams_instance")]
    pub max_streams_per_instance: usize,
}

fn default_history_lines() -> usize {
    500
}
fn default_ring_buffer_lines() -> usize {
    5000
}
fn default_max_line_bytes() -> usize {
    65536
}
fn default_max_streams_user() -> usize {
    5
}
fn default_max_streams_instance() -> usize {
    50
}

impl Default for LogsLimitsConfig {
    fn default() -> Self {
        Self {
            history_lines_per_resource: default_history_lines(),
            ring_buffer_lines_per_resource: default_ring_buffer_lines(),
            max_line_bytes: default_max_line_bytes(),
            max_streams_per_user: default_max_streams_user(),
            max_streams_per_instance: default_max_streams_instance(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsLimitsConfig {
    #[serde(default = "default_action_rate_limit")]
    pub rate_limit_per_user_per_minute: u32,
}

fn default_action_rate_limit() -> u32 {
    10
}

impl Default for ActionsLimitsConfig {
    fn default() -> Self {
        Self {
            rate_limit_per_user_per_minute: default_action_rate_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LimitsConfig {
    #[serde(default)]
    pub logs: LogsLimitsConfig,
    #[serde(default)]
    pub actions: ActionsLimitsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default = "default_plugins_dir")]
    pub dir: PathBuf,
    #[serde(default = "default_crash_limit")]
    pub crash_limit: u32,
    #[serde(default = "default_crash_window_secs")]
    pub crash_window_secs: u64,
}

fn default_plugins_dir() -> PathBuf {
    PathBuf::from("/var/lib/mop/plugins")
}
fn default_crash_limit() -> u32 {
    5
}
fn default_crash_window_secs() -> u64 {
    300
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            dir: default_plugins_dir(),
            crash_limit: default_crash_limit(),
            crash_window_secs: default_crash_window_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    #[serde(default = "default_backup_dir")]
    pub dir: PathBuf,
}

fn default_backup_dir() -> PathBuf {
    PathBuf::from("/var/lib/mop/backups")
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            dir: default_backup_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub resources: ResourcesConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub backup: BackupConfig,
}

impl Config {
    pub fn load_from_file_or_default(path: Option<&Path>) -> Result<Self, AppError> {
        let mut cfg = if let Some(p) = path {
            if p.exists() {
                let content = std::fs::read_to_string(p).map_err(|e| {
                    AppError::Config(format!("Failed to read config from {}: {e}", p.display()))
                })?;
                toml::from_str::<Config>(&content)
                    .map_err(|e| AppError::Config(format!("Failed to parse config file: {e}")))?
            } else {
                Config::default()
            }
        } else {
            let default_path = Path::new("/etc/mop/config.toml");
            if default_path.exists() {
                let content = std::fs::read_to_string(default_path)
                    .map_err(|e| AppError::Config(format!("Failed to read default config: {e}")))?;
                toml::from_str::<Config>(&content)
                    .map_err(|e| AppError::Config(format!("Failed to parse default config: {e}")))?
            } else {
                Config::default()
            }
        };

        cfg.apply_env_overrides();
        Ok(cfg)
    }

    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("MOP_SERVER_BIND") {
            self.server.bind = val;
        }
        if let Ok(val) = std::env::var("MOP_SERVER_PUBLIC_URL") {
            self.server.public_url = Some(val);
        }
        if let Ok(val) = std::env::var("MOP_DATABASE_PATH") {
            self.database.path = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("MOP_AUTH_REGISTRATION") {
            match val.to_lowercase().as_str() {
                "first_user" => self.auth.registration = RegistrationMode::FirstUser,
                "open" => self.auth.registration = RegistrationMode::Open,
                "closed" => self.auth.registration = RegistrationMode::Closed,
                _ => {}
            }
        }
        if let Ok(val) = std::env::var("MOP_RESOURCES_FAKE") {
            if val == "1" || val.eq_ignore_ascii_case("true") {
                self.resources.fake = true;
            }
        }
    }
}
