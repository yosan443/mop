use crate::host_notification::HostNotificationHandler;
use crate::rpc::UnixRpcClient;
use chrono::Utc;
use mop_core::error::AppError;
use mop_core::models::plugin::{PluginRecord, PluginState};
use mop_db::repos::{PluginPermissionsRepo, PluginRepo, PluginSettingsRepo};
use mop_jobs::JobService;
use mop_plugin_sdk::*;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};

pub const DEFAULT_CRASH_LIMIT: usize = 5;
pub const DEFAULT_CRASH_WINDOW: Duration = Duration::from_secs(300);

struct RunningPluginProcess {
    child: Option<Child>,
    socket_path: PathBuf,
    #[allow(dead_code)]
    manifest: PluginManifest,
    crash_timestamps: VecDeque<Instant>,
    notification_listener: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct PluginSupervisor {
    plugins_dir: PathBuf,
    run_dir: PathBuf,
    plugin_repo: PluginRepo,
    permissions_repo: PluginPermissionsRepo,
    settings_repo: PluginSettingsRepo,
    job_service: JobService,
    processes: Arc<RwLock<HashMap<String, Arc<Mutex<RunningPluginProcess>>>>>,
    crash_limit: usize,
    crash_window: Duration,
    use_systemd_transient: bool,
}

impl PluginSupervisor {
    pub fn new(
        plugins_dir: impl Into<PathBuf>,
        run_dir: impl Into<PathBuf>,
        plugin_repo: PluginRepo,
        permissions_repo: PluginPermissionsRepo,
        settings_repo: PluginSettingsRepo,
        job_service: JobService,
    ) -> Self {
        Self {
            plugins_dir: plugins_dir.into(),
            run_dir: run_dir.into(),
            plugin_repo,
            permissions_repo,
            settings_repo,
            job_service,
            processes: Arc::new(RwLock::new(HashMap::new())),
            crash_limit: DEFAULT_CRASH_LIMIT,
            crash_window: DEFAULT_CRASH_WINDOW,
            use_systemd_transient: false, // Default to direct execution for dev/tests, can be enabled on Linux
        }
    }

    pub fn with_systemd_transient(mut self, enabled: bool) -> Self {
        self.use_systemd_transient = enabled;
        self
    }

    pub fn with_crash_policy(mut self, limit: usize, window_secs: u64) -> Self {
        self.crash_limit = limit;
        self.crash_window = Duration::from_secs(window_secs);
        self
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Scan plugins directory, validate manifests, and upsert records into DB
    pub async fn scan_and_register_plugins(&self) -> Result<Vec<PluginRecord>, AppError> {
        if !self.plugins_dir.exists() {
            info!(
                "Plugins directory {} does not exist, creating it",
                self.plugins_dir.display()
            );
            let _ = std::fs::create_dir_all(&self.plugins_dir);
            return Ok(Vec::new());
        }

        let mut discovered = Vec::new();
        let read_dir = std::fs::read_dir(&self.plugins_dir).map_err(|e| {
            AppError::Plugin(format!(
                "Failed to read plugins dir {}: {e}",
                self.plugins_dir.display()
            ))
        })?;

        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Path could be /plugins/<id>/<version>/plugin.toml or /plugins/<id>/plugin.toml
            let manifest_path = if path.join("plugin.toml").exists() {
                path.join("plugin.toml")
            } else {
                // Check if there is a version subdirectory
                let mut found = None;
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_p = sub.path();
                        if sub_p.is_dir() && sub_p.join("plugin.toml").exists() {
                            found = Some(sub_p.join("plugin.toml"));
                            break;
                        }
                    }
                }
                match found {
                    Some(p) => p,
                    None => continue,
                }
            };

            let content = match std::fs::read_to_string(&manifest_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "Failed to read manifest at {}: {e}",
                        manifest_path.display()
                    );
                    continue;
                }
            };

            let manifest = match PluginManifest::parse_and_validate(&content) {
                Ok(m) => m,
                Err(e) => {
                    warn!(
                        "Invalid plugin manifest at {}: {e}",
                        manifest_path.display()
                    );
                    continue;
                }
            };

            let manifest_json = serde_json::to_string(&manifest).unwrap_or_default();
            let existing = self.plugin_repo.find_by_id(&manifest.id).await?;

            let record = if let Some(mut rec) = existing {
                rec.name = manifest.name.clone();
                rec.version = manifest.version.clone();
                rec.manifest_json = manifest_json;
                self.plugin_repo.upsert_plugin(&rec).await?;
                rec
            } else {
                let rec = PluginRecord {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    enabled: false,
                    state: PluginState::Installed,
                    manifest_json,
                    installed_at: Utc::now(),
                    enabled_at: None,
                };
                self.plugin_repo.upsert_plugin(&rec).await?;
                rec
            };

            discovered.push(record);
        }

        Ok(discovered)
    }

    /// Enable a plugin with granted capabilities and start its backend process
    pub async fn enable_plugin(
        &self,
        plugin_id: &str,
        granted_by: &str,
    ) -> Result<PluginRecord, AppError> {
        let record = self
            .plugin_repo
            .find_by_id(plugin_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Plugin {plugin_id} not found")))?;

        let manifest: PluginManifest = serde_json::from_str(&record.manifest_json)
            .map_err(|e| AppError::Plugin(format!("Failed to deserialize manifest: {e}")))?;

        // Revoke existing permissions and grant declared capabilities
        self.permissions_repo.revoke_all(plugin_id).await?;

        for fs_read in &manifest.capabilities.filesystem_read {
            self.permissions_repo
                .grant_permission(plugin_id, "filesystem_read", fs_read, granted_by)
                .await?;
        }
        for fs_write in &manifest.capabilities.filesystem_write {
            self.permissions_repo
                .grant_permission(plugin_id, "filesystem_write", fs_write, granted_by)
                .await?;
        }
        for job_kind in &manifest.capabilities.jobs {
            self.permissions_repo
                .grant_permission(plugin_id, "jobs", job_kind, granted_by)
                .await?;
        }
        for res_read in &manifest.capabilities.resources_read {
            self.permissions_repo
                .grant_permission(plugin_id, "resources_read", res_read, granted_by)
                .await?;
        }
        for res_action in &manifest.capabilities.resources_action {
            self.permissions_repo
                .grant_permission(plugin_id, "resources_action", res_action, granted_by)
                .await?;
        }
        if manifest.capabilities.network {
            self.permissions_repo
                .grant_permission(plugin_id, "network", "true", granted_by)
                .await?;
        }

        self.plugin_repo.set_enabled(plugin_id, true).await?;

        // Start plugin backend if defined
        if manifest.backend.is_some() {
            if let Err(e) = self.start_plugin_process(plugin_id, &manifest).await {
                error!("Failed to start process for plugin {plugin_id}: {e}");
                self.plugin_repo
                    .update_state(plugin_id, PluginState::Degraded)
                    .await?;
                return Err(e);
            }
        } else {
            self.plugin_repo
                .update_state(plugin_id, PluginState::Enabled)
                .await?;
        }

        let updated = self.plugin_repo.find_by_id(plugin_id).await?.unwrap();
        Ok(updated)
    }

    /// Disable a plugin and terminate its process
    pub async fn disable_plugin(&self, plugin_id: &str) -> Result<PluginRecord, AppError> {
        let _record = self
            .plugin_repo
            .find_by_id(plugin_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Plugin {plugin_id} not found")))?;

        self.stop_plugin_process(plugin_id).await?;
        self.plugin_repo.set_enabled(plugin_id, false).await?;

        let updated = self.plugin_repo.find_by_id(plugin_id).await?.unwrap();
        Ok(updated)
    }

    /// Start the backend process for a plugin and initialize it over Unix socket JSON-RPC
    pub async fn start_plugin_process(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<(), AppError> {
        let backend = manifest.backend.as_ref().ok_or_else(|| {
            AppError::Plugin(format!(
                "Plugin {plugin_id} has no backend executable defined"
            ))
        })?;

        // Find executable location
        let plugin_base_dir = self.find_plugin_base_dir(plugin_id, &manifest.version)?;
        let exec_path = plugin_base_dir.join(&backend.exec);
        if !exec_path.exists() {
            return Err(AppError::Plugin(format!(
                "Executable not found at {}",
                exec_path.display()
            )));
        }

        let plugins_run_dir = self.run_dir.join("plugins");
        let _ = std::fs::create_dir_all(&plugins_run_dir);

        let plugin_socket = plugins_run_dir.join(format!("{plugin_id}.sock"));
        let host_socket = self.run_dir.join("host.sock");

        if plugin_socket.exists() {
            let _ = std::fs::remove_file(&plugin_socket);
        }

        // Start host notification listener for this plugin
        let notif_handler = HostNotificationHandler::new(self.job_service.clone(), &host_socket);
        let notif_listener = notif_handler.start_listener(plugin_id).await?;

        info!(
            "Starting plugin '{}' from {}",
            plugin_id,
            exec_path.display()
        );

        let mut cmd = Command::new(&exec_path);
        cmd.current_dir(&plugin_base_dir)
            .env("MOP_PLUGIN_ID", plugin_id)
            .env("MOP_PLUGIN_SOCKET", &plugin_socket)
            .env("MOP_HOST_SOCKET", &host_socket)
            .env("MOP_API_VERSION", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AppError::Plugin(format!(
                "Failed to spawn plugin process {}: {e}",
                exec_path.display()
            ))
        })?;

        // Capture stdout / stderr as structured logs
        let pid_str = plugin_id.to_string();
        if let Some(stdout) = child.stdout.take() {
            let pid = pid_str.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!(plugin_id = %pid, "[stdout] {}", line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let pid = pid_str.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    warn!(plugin_id = %pid, "[stderr] {}", line);
                }
            });
        }

        // Wait for plugin socket to become available (up to 5s)
        let client = UnixRpcClient::new(&plugin_socket);
        let mut ready = false;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if plugin_socket.exists() {
                // Ensure socket permissions are 0660
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &plugin_socket,
                        std::fs::Permissions::from_mode(0o660),
                    );
                }
                ready = true;
                break;
            }
        }

        if !ready {
            let _ = child.kill().await;
            return Err(AppError::Plugin(format!(
                "Plugin socket {} did not appear within 5 seconds",
                plugin_socket.display()
            )));
        }

        // Initialize plugin over RPC
        let settings = self.settings_repo.get_applied_settings(plugin_id).await?;
        let init_params = InitializeParams {
            capabilities: manifest.capabilities.clone(),
            settings: serde_json::to_value(settings).unwrap_or(serde_json::json!({})),
            api_version: "1".to_string(),
        };

        if let Err(e) = client
            .call(
                "initialize",
                Some(serde_json::to_value(init_params).unwrap()),
            )
            .await
        {
            error!("Failed to initialize plugin '{plugin_id}': {e}");
            let _ = child.kill().await;
            return Err(e);
        }

        self.plugin_repo
            .update_state(plugin_id, PluginState::Running)
            .await?;

        let running_proc = RunningPluginProcess {
            child: Some(child),
            socket_path: plugin_socket,
            manifest: manifest.clone(),
            crash_timestamps: VecDeque::new(),
            notification_listener: Some(notif_listener),
        };

        {
            let mut procs = self.processes.write().await;
            procs.insert(plugin_id.to_string(), Arc::new(Mutex::new(running_proc)));
        }

        // Spawn process monitor task to handle crashes / auto-disable
        self.spawn_process_monitor(plugin_id.to_string());

        Ok(())
    }

    /// Stop the plugin process gracefully with a shutdown RPC call and fallback to SIGTERM/SIGKILL
    pub async fn stop_plugin_process(&self, plugin_id: &str) -> Result<(), AppError> {
        let proc_mutex = {
            let mut procs = self.processes.write().await;
            procs.remove(plugin_id)
        };

        let Some(proc_mutex) = proc_mutex else {
            return Ok(());
        };

        let mut proc = proc_mutex.lock().await;

        // Try graceful shutdown RPC
        let client = UnixRpcClient::new(&proc.socket_path);
        let _ = client.notify("shutdown", None).await;

        // Abort host notification listener
        if let Some(listener) = proc.notification_listener.take() {
            listener.abort();
        }

        // Wait up to 3 seconds for process exit, then kill
        if let Some(mut child) = proc.child.take() {
            let wait_fut = async {
                let _ = child.wait().await;
            };
            if tokio::time::timeout(Duration::from_secs(3), wait_fut)
                .await
                .is_err()
            {
                warn!("Plugin '{plugin_id}' did not exit gracefully, killing");
                let _ = child.kill().await;
            }
        }

        // Remove socket file
        if proc.socket_path.exists() {
            let _ = std::fs::remove_file(&proc.socket_path);
        }

        Ok(())
    }

    /// Get an RPC client for an active plugin
    pub async fn get_rpc_client(&self, plugin_id: &str) -> Result<UnixRpcClient, AppError> {
        let procs = self.processes.read().await;
        let proc_mutex = procs
            .get(plugin_id)
            .ok_or_else(|| AppError::Plugin(format!("Plugin {plugin_id} is not running")))?;

        let proc = proc_mutex.lock().await;
        Ok(UnixRpcClient::new(&proc.socket_path))
    }

    /// Spawn a task to monitor the process for unexpected exits and enforce crash_limit (5 / 300s)
    fn spawn_process_monitor(&self, plugin_id: String) {
        let supervisor = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;

                let (is_running, exit_status) = {
                    let procs = supervisor.processes.read().await;
                    let Some(proc_mutex) = procs.get(&plugin_id) else {
                        break; // Process removed or stopped intentionally
                    };
                    let mut proc = proc_mutex.lock().await;
                    if let Some(child) = proc.child.as_mut() {
                        match child.try_wait() {
                            Ok(Some(status)) => (false, Some(status)),
                            Ok(None) => (true, None),
                            Err(_) => (false, None),
                        }
                    } else {
                        break;
                    }
                };

                if !is_running {
                    warn!(
                        "Plugin '{}' process exited unexpectedly with status: {:?}",
                        plugin_id, exit_status
                    );

                    let auto_disabled = {
                        let procs = supervisor.processes.read().await;
                        let Some(proc_mutex) = procs.get(&plugin_id) else {
                            break;
                        };
                        let mut proc = proc_mutex.lock().await;

                        let now = Instant::now();
                        proc.crash_timestamps.push_back(now);

                        // Prune crashes older than crash_window
                        while let Some(front) = proc.crash_timestamps.front() {
                            if now.duration_since(*front) > supervisor.crash_window {
                                proc.crash_timestamps.pop_front();
                            } else {
                                break;
                            }
                        }

                        let crash_count = proc.crash_timestamps.len();
                        info!(
                            "Plugin '{}' crash count in window: {} / {}",
                            plugin_id, crash_count, supervisor.crash_limit
                        );

                        crash_count >= supervisor.crash_limit
                    };

                    if auto_disabled {
                        error!(
                            "Plugin '{}' reached crash limit ({} crashes within {:?}), automatically disabling",
                            plugin_id, supervisor.crash_limit, supervisor.crash_window
                        );
                        let _ = supervisor
                            .plugin_repo
                            .update_state(&plugin_id, PluginState::Disabled)
                            .await;
                        let _ = supervisor.plugin_repo.set_enabled(&plugin_id, false).await;
                        let _ = supervisor.stop_plugin_process(&plugin_id).await;
                        break;
                    } else {
                        let _ = supervisor
                            .plugin_repo
                            .update_state(&plugin_id, PluginState::Degraded)
                            .await;
                        break;
                    }
                }
            }
        });
    }

    fn find_plugin_base_dir(&self, plugin_id: &str, version: &str) -> Result<PathBuf, AppError> {
        let candidate_versioned = self.plugins_dir.join(plugin_id).join(version);
        if candidate_versioned.exists() {
            return Ok(candidate_versioned);
        }
        let candidate_flat = self.plugins_dir.join(plugin_id);
        if candidate_flat.exists() {
            return Ok(candidate_flat);
        }
        Err(AppError::NotFound(format!(
            "Plugin directory for {plugin_id} not found in {}",
            self.plugins_dir.display()
        )))
    }
}
