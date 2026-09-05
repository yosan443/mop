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
    pid: Option<u32>,
    is_transient_unit: bool,
    unit_name: Option<String>,
    socket_path: PathBuf,
    #[allow(dead_code)]
    manifest: PluginManifest,
}

#[derive(Clone)]
pub struct PluginSupervisor {
    plugins_dir: PathBuf,
    run_dir: PathBuf,
    plugin_repo: PluginRepo,
    permissions_repo: PluginPermissionsRepo,
    settings_repo: PluginSettingsRepo,
    #[allow(dead_code)]
    job_service: JobService,
    processes: Arc<RwLock<HashMap<String, Arc<Mutex<RunningPluginProcess>>>>>,
    #[allow(dead_code)]
    pid_to_plugin: Arc<RwLock<HashMap<u32, String>>>,
    host_handler: HostNotificationHandler,
    host_listener_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    crash_history: Arc<RwLock<HashMap<String, VecDeque<Instant>>>>,
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
        let plugins_dir = plugins_dir.into();
        let run_dir = run_dir.into();
        let host_socket = run_dir.join("host.sock");
        let pid_to_plugin = Arc::new(RwLock::new(HashMap::new()));
        let host_handler =
            HostNotificationHandler::new(job_service.clone(), &host_socket, pid_to_plugin.clone());

        Self {
            plugins_dir,
            run_dir,
            plugin_repo,
            permissions_repo,
            settings_repo,
            job_service,
            processes: Arc::new(RwLock::new(HashMap::new())),
            pid_to_plugin,
            host_handler,
            host_listener_handle: Arc::new(Mutex::new(None)),
            crash_history: Arc::new(RwLock::new(HashMap::new())),
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

    pub fn host_notification_handler(&self) -> &HostNotificationHandler {
        &self.host_handler
    }

    pub fn job_service(&self) -> &JobService {
        &self.job_service
    }

    /// Register a custom PID to plugin_id mapping (useful for testing or external workers)
    pub async fn register_plugin_pid(&self, pid: u32, plugin_id: &str) {
        self.host_handler.register_plugin_pid(pid, plugin_id).await;
    }

    /// Unregister a PID from plugin_id mapping
    pub fn plugin_socket_path(&self, plugin_id: &str) -> PathBuf {
        self.run_dir
            .join("plugins")
            .join(format!("{plugin_id}.sock"))
    }

    pub async fn unregister_plugin_pid(&self, pid: u32) {
        self.host_handler.unregister_plugin_pid(pid).await;
    }

    /// Ensure the single host.sock listener is active and plugin socket directory is prepared
    pub async fn ensure_host_listener(&self) -> Result<(), AppError> {
        let mut handle_guard = self.host_listener_handle.lock().await;
        if handle_guard.is_none() {
            let plugins_run_dir = self.run_dir.join("plugins");
            let _ = std::fs::create_dir_all(&plugins_run_dir);
            #[cfg(unix)]
            crate::ipc::ensure_group_and_permissions(
                &plugins_run_dir,
                crate::ipc::MOP_IPC_GROUP,
                0o2770,
            );

            let handle = self.host_handler.start_host_listener().await?;
            *handle_guard = Some(handle);
        }
        Ok(())
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

    /// Gracefully restart an active plugin process
    pub async fn restart_plugin_process(&self, plugin_id: &str) -> Result<(), AppError> {
        let record = self
            .plugin_repo
            .find_by_id(plugin_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Plugin {plugin_id} not found")))?;

        if !record.enabled {
            return Ok(());
        }

        let manifest: PluginManifest = serde_json::from_str(&record.manifest_json)
            .map_err(|e| AppError::Plugin(format!("Failed to deserialize manifest: {e}")))?;

        if manifest.backend.is_none() {
            return Ok(());
        }

        self.stop_plugin_process(plugin_id).await?;
        self.start_plugin_process(plugin_id, &manifest).await?;
        Ok(())
    }

    /// Start the backend process for a plugin and initialize it over Unix socket JSON-RPC
    pub async fn start_plugin_process(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<(), AppError> {
        self.ensure_host_listener().await?;

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
        #[cfg(unix)]
        crate::ipc::ensure_group_and_permissions(
            &plugins_run_dir,
            crate::ipc::MOP_IPC_GROUP,
            0o2770,
        );

        let plugin_socket = plugins_run_dir.join(format!("{plugin_id}.sock"));
        let host_socket = self.run_dir.join("host.sock");

        if plugin_socket.exists() {
            let _ = std::fs::remove_file(&plugin_socket);
        }

        info!(
            "Starting plugin '{}' from {}",
            plugin_id,
            exec_path.display()
        );

        let mut child_proc: Option<Child> = None;
        let mut spawned_pid: Option<u32> = None;
        let mut is_transient = false;
        let mut transient_unit_name: Option<String> = None;

        if self.use_systemd_transient {
            // Attempt systemd StartTransientUnit via D-Bus (zbus)
            match self
                .start_transient_unit(
                    plugin_id,
                    &exec_path,
                    &plugin_base_dir,
                    &plugin_socket,
                    &host_socket,
                )
                .await
            {
                Ok((unit_name, pid)) => {
                    info!(
                        "Started plugin '{}' via systemd transient unit '{}' (PID: {:?})",
                        plugin_id, unit_name, pid
                    );
                    is_transient = true;
                    transient_unit_name = Some(unit_name);
                    spawned_pid = pid;
                    if let Some(p) = pid {
                        self.register_plugin_pid(p, plugin_id).await;
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to start systemd transient unit for '{}': {e}. Falling back to direct process spawn.",
                        plugin_id
                    );
                }
            }
        }

        if !is_transient {
            // Direct spawn (fallback or testing mode)
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

            if let Some(pid) = child.id() {
                spawned_pid = Some(pid);
                self.register_plugin_pid(pid, plugin_id).await;
            }

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

            child_proc = Some(child);
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
            if let Some(mut child) = child_proc {
                let _ = child.kill().await;
            }
            if let Some(pid) = spawned_pid {
                self.unregister_plugin_pid(pid).await;
            }
            return Err(AppError::Plugin(format!(
                "Plugin socket {} was not ready within 5 seconds",
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
            if let Some(mut child) = child_proc {
                let _ = child.kill().await;
            }
            if let Some(pid) = spawned_pid {
                self.unregister_plugin_pid(pid).await;
            }
            return Err(e);
        }

        self.plugin_repo
            .update_state(plugin_id, PluginState::Running)
            .await?;

        let running_proc = RunningPluginProcess {
            child: child_proc,
            pid: spawned_pid,
            is_transient_unit: is_transient,
            unit_name: transient_unit_name,
            socket_path: plugin_socket,
            manifest: manifest.clone(),
        };

        {
            let mut procs = self.processes.write().await;
            procs.insert(plugin_id.to_string(), Arc::new(Mutex::new(running_proc)));
        }

        // Spawn process monitor task to handle crashes / backoff / auto-disable
        self.spawn_process_monitor(plugin_id.to_string(), manifest.clone());

        Ok(())
    }

    /// Start a transient systemd service unit via D-Bus (zbus)
    async fn start_transient_unit(
        &self,
        plugin_id: &str,
        exec_path: &std::path::Path,
        plugin_base_dir: &std::path::Path,
        plugin_socket: &std::path::Path,
        host_socket: &std::path::Path,
    ) -> Result<(String, Option<u32>), AppError> {
        let unit_name = format!("mop-plugin-{plugin_id}.service");
        let conn = zbus::Connection::system()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect to system D-Bus: {e}")))?;

        let exec_str = exec_path.to_string_lossy().to_string();
        let working_dir_str = plugin_base_dir.to_string_lossy().to_string();
        let socket_env = format!("MOP_PLUGIN_SOCKET={}", plugin_socket.display());
        let host_env = format!("MOP_HOST_SOCKET={}", host_socket.display());
        let id_env = format!("MOP_PLUGIN_ID={plugin_id}");
        let api_env = "MOP_API_VERSION=1".to_string();

        let desc = format!("mop plugin {plugin_id}");
        let user_name = format!("mop-plugin-{plugin_id}");

        let mut properties: Vec<(&str, zbus::zvariant::Value)> = vec![
            ("Description", desc.as_str().into()),
            ("Type", "simple".into()),
            ("WorkingDirectory", working_dir_str.as_str().into()),
            (
                "Environment",
                vec![
                    socket_env.as_str(),
                    host_env.as_str(),
                    id_env.as_str(),
                    api_env.as_str(),
                ]
                .into(),
            ),
            (
                "ExecStart",
                vec![(
                    exec_str.as_str(),
                    vec![exec_str.as_str()],
                    false, // don't ignore failure
                )]
                .into(),
            ),
            ("DynamicUser", true.into()),
            ("User", user_name.as_str().into()),
            ("Restart", "no".into()),
        ];

        // Attach mop-ipc supplementary group if present in system
        if crate::ipc::get_group_gid(crate::ipc::MOP_IPC_GROUP).is_some() {
            properties.push((
                "SupplementaryGroups",
                vec![crate::ipc::MOP_IPC_GROUP].into(),
            ));
        }

        let job: zbus::zvariant::OwnedObjectPath = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "StartTransientUnit",
                &(
                    &unit_name,
                    "replace",
                    properties,
                    Vec::<(&str, Vec<(&str, zbus::zvariant::Value)>)>::new(),
                ),
            )
            .await
            .map_err(|e| AppError::Internal(format!("D-Bus StartTransientUnit call failed: {e}")))?
            .body()
            .deserialize()
            .map_err(|e| AppError::Internal(format!("Failed to deserialize job path: {e}")))?;

        info!(
            "Started transient unit '{}' with job path '{}'",
            unit_name,
            job.as_str()
        );

        // Poll GetUnit / ActiveState / MainPID
        let mut main_pid = None;
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);
        let mut poll_interval = tokio::time::interval(std::time::Duration::from_millis(50));

        while start.elapsed() < timeout {
            poll_interval.tick().await;

            let unit_path_res: Result<zbus::zvariant::OwnedObjectPath, _> = conn
                .call_method(
                    Some("org.freedesktop.systemd1"),
                    "/org/freedesktop/systemd1",
                    Some("org.freedesktop.systemd1.Manager"),
                    "GetUnit",
                    &(&unit_name,),
                )
                .await
                .and_then(|r| r.body().deserialize());

            let Ok(unit_path) = unit_path_res else {
                continue;
            };

            // Check ActiveState
            if let Ok(reply) = conn
                .call_method(
                    Some("org.freedesktop.systemd1"),
                    unit_path.as_ref(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.freedesktop.systemd1.Unit", "ActiveState"),
                )
                .await
            {
                if let Ok(val) = reply.body().deserialize::<zbus::zvariant::OwnedValue>() {
                    let state = val.to_string();
                    let trimmed = state.trim_matches('"');
                    if trimmed == "failed" {
                        return Err(AppError::Plugin(format!(
                            "Transient unit '{unit_name}' entered failed state"
                        )));
                    }
                }
            }

            // Check MainPID
            if let Ok(reply) = conn
                .call_method(
                    Some("org.freedesktop.systemd1"),
                    unit_path.as_ref(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.freedesktop.systemd1.Service", "MainPID"),
                )
                .await
            {
                if let Ok(val) = reply.body().deserialize::<zbus::zvariant::OwnedValue>() {
                    if let Ok(pid) = val.to_string().trim_matches('"').parse::<u32>() {
                        if pid > 0 {
                            info!("Retrieved MainPID {pid} for transient unit '{unit_name}'");
                            main_pid = Some(pid);
                            break;
                        }
                    }
                }
            }
        }

        if main_pid.is_none() {
            warn!("Timed out waiting for MainPID of transient unit '{unit_name}'");
        }

        Ok((unit_name, main_pid))
    }

    /// Stop a running plugin process or systemd transient unit
    pub async fn stop_plugin_process(&self, plugin_id: &str) -> Result<(), AppError> {
        let proc_mutex = {
            let mut procs = self.processes.write().await;
            procs.remove(plugin_id)
        };

        let Some(proc_mutex) = proc_mutex else {
            return Ok(());
        };

        let mut proc = proc_mutex.lock().await;

        // Unregister PID
        if let Some(pid) = proc.pid {
            self.unregister_plugin_pid(pid).await;
        }

        // Try graceful shutdown RPC
        let client = UnixRpcClient::new(&proc.socket_path);
        let _ = client.notify("shutdown", None).await;

        if proc.is_transient_unit {
            if let Some(unit_name) = &proc.unit_name {
                if let Ok(conn) = zbus::Connection::system().await {
                    let _: Result<zbus::zvariant::OwnedObjectPath, _> = conn
                        .call_method(
                            Some("org.freedesktop.systemd1"),
                            "/org/freedesktop/systemd1",
                            Some("org.freedesktop.systemd1.Manager"),
                            "StopUnit",
                            &(unit_name.as_str(), "replace"),
                        )
                        .await
                        .and_then(|r| r.body().deserialize());
                }
            }
        } else if let Some(mut child) = proc.child.take() {
            // Wait up to 3 seconds for process exit, then kill
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

    async fn record_crash(&self, plugin_id: &str) -> usize {
        let mut history = self.crash_history.write().await;
        let deque = history.entry(plugin_id.to_string()).or_default();
        let now = Instant::now();
        deque.push_back(now);
        while let Some(front) = deque.front() {
            if now.duration_since(*front) > self.crash_window {
                deque.pop_front();
            } else {
                break;
            }
        }
        deque.len()
    }

    async fn auto_disable(&self, plugin_id: &str) {
        error!(
            "Plugin '{}' reached crash limit ({} crashes within {:?}), automatically disabling",
            plugin_id, self.crash_limit, self.crash_window
        );
        let _ = self
            .plugin_repo
            .update_state(plugin_id, PluginState::Disabled)
            .await;
        let _ = self.plugin_repo.set_enabled(plugin_id, false).await;
        let _ = self.stop_plugin_process(plugin_id).await;
    }

    /// Spawn a task to monitor the process for unexpected exits, handle backoff auto-restart, and enforce crash_limit (5 / 300s)
    fn spawn_process_monitor(&self, plugin_id: String, manifest: PluginManifest) {
        let supervisor = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;

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
                    } else if proc.is_transient_unit {
                        // For transient units, check active status via D-Bus
                        (true, None)
                    } else {
                        break;
                    }
                };

                if !is_running {
                    warn!(
                        "Plugin '{}' process exited unexpectedly with status: {:?}",
                        plugin_id, exit_status
                    );

                    // Clean up dead child and PID
                    {
                        let procs = supervisor.processes.read().await;
                        if let Some(proc_mutex) = procs.get(&plugin_id) {
                            let mut proc = proc_mutex.lock().await;
                            let _ = proc.child.take();
                            if let Some(pid) = proc.pid.take() {
                                supervisor.unregister_plugin_pid(pid).await;
                            }
                        }
                    }

                    let crash_count = supervisor.record_crash(&plugin_id).await;
                    info!(
                        "Plugin '{}' crash count in window: {} / {}",
                        plugin_id, crash_count, supervisor.crash_limit
                    );

                    if crash_count >= supervisor.crash_limit {
                        supervisor.auto_disable(&plugin_id).await;
                        break;
                    }

                    let _ = supervisor
                        .plugin_repo
                        .update_state(&plugin_id, PluginState::Degraded)
                        .await;

                    // Check if plugin is still enabled before backoff restart
                    let is_enabled = supervisor
                        .plugin_repo
                        .find_by_id(&plugin_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|p| p.enabled)
                        .unwrap_or(false);

                    if !is_enabled {
                        break;
                    }

                    // Backoff before restarting
                    let backoff_ms = std::cmp::min(crash_count as u64 * 50, 500);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                    // Check enabled again after backoff
                    let is_enabled = supervisor
                        .plugin_repo
                        .find_by_id(&plugin_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|p| p.enabled)
                        .unwrap_or(false);

                    if !is_enabled {
                        break;
                    }

                    // Attempt restart
                    if let Err(e) = supervisor.start_plugin_process(&plugin_id, &manifest).await {
                        warn!("Failed to restart plugin '{}': {e}", plugin_id);
                        let fail_count = supervisor.record_crash(&plugin_id).await;
                        if fail_count >= supervisor.crash_limit {
                            supervisor.auto_disable(&plugin_id).await;
                            break;
                        }
                    } else {
                        // Successfully started new process; a new monitor was spawned
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
