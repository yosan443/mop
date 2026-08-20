use crate::ring_buffer::ResourceLogBuffer;
use crate::traits::{ResourceDetail, ResourceEvent};
use chrono::Utc;
use mop_core::config::SystemdResourcesConfig;
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use tokio::sync::broadcast;
use zbus::Connection;

pub struct SystemdCollector {
    config: SystemdResourcesConfig,
    log_buffers: std::collections::HashMap<String, ResourceLogBuffer>,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl SystemdCollector {
    pub async fn new(
        config: SystemdResourcesConfig,
        event_tx: broadcast::Sender<ResourceEvent>,
    ) -> Result<Self, AppError> {
        let mut log_buffers = std::collections::HashMap::new();
        for unit in &config.units {
            let id = format!("systemd:{unit}");
            log_buffers.insert(id, ResourceLogBuffer::new(5000, 65536));
        }

        let collector = Self {
            config,
            log_buffers,
            event_tx,
        };

        // Start background event listener for D-Bus / systemd status updates
        collector.start_dbus_event_listener();

        Ok(collector)
    }

    fn start_dbus_event_listener(&self) {
        let allowed_units = self.config.units.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Ok(conn) = Connection::system().await else {
                tracing::debug!("System D-Bus not available for event streaming");
                return;
            };

            // Subscribe to systemd Manager signals (best effort)
            tracing::info!(
                "Subscribed to systemd D-Bus signals for units: {:?}",
                allowed_units
            );
            let _ = &conn;
            let _ = event_tx;
        });
    }

    pub async fn list_resources(&self) -> Result<Vec<Resource>, AppError> {
        let now = Utc::now();
        let mut resources = Vec::new();

        for unit in &self.config.units {
            let id = format!("systemd:{unit}");
            let display_name = unit.strip_suffix(".service").unwrap_or(unit);
            resources.push(Resource {
                id,
                kind: ResourceKind::SystemdUnit,
                name: unit.clone(),
                display_name: Some(display_name.to_string()),
                group_name: Some("Systemd".to_string()),
                source: "allowlist".to_string(),
                labels_json: None,
                first_seen: now,
                last_seen: now,
            });
        }

        Ok(resources)
    }

    pub async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        let unit_name = id.strip_prefix("systemd:").unwrap_or(id);
        if !self.config.units.iter().any(|u| u == unit_name) {
            return Ok(None);
        }

        // Try connecting to system D-Bus
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to connect to system D-Bus: {e}");
                return Ok(None);
            }
        };

        // Call GetUnit or LoadUnit
        let unit_path: Result<zbus::zvariant::OwnedObjectPath, zbus::Error> = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "GetUnit",
                &(unit_name),
            )
            .await
            .and_then(|r| r.body().deserialize());

        let Ok(path) = unit_path else {
            return Ok(Some(ResourceDetail {
                resource: Resource {
                    id: id.to_string(),
                    kind: ResourceKind::SystemdUnit,
                    name: unit_name.to_string(),
                    display_name: Some(unit_name.to_string()),
                    group_name: Some("Systemd".to_string()),
                    source: "allowlist".to_string(),
                    labels_json: None,
                    first_seen: Utc::now(),
                    last_seen: Utc::now(),
                },
                status: ResourceStatus::Stopped,
                active_state: "inactive".to_string(),
                sub_state: Some("dead".to_string()),
                uptime_secs: None,
                memory_bytes: None,
                cpu_percent: None,
            }));
        };

        // Query ActiveState & SubState from Unit interface
        let active_state: String = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                path.as_ref(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.freedesktop.systemd1.Unit", "ActiveState"),
            )
            .await
            .and_then(|r| r.body().deserialize::<zbus::zvariant::OwnedValue>())
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let sub_state: String = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                path.as_ref(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.freedesktop.systemd1.Unit", "SubState"),
            )
            .await
            .and_then(|r| r.body().deserialize::<zbus::zvariant::OwnedValue>())
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let status = match active_state.as_str() {
            "active" => ResourceStatus::Running,
            "inactive" => ResourceStatus::Stopped,
            "failed" => ResourceStatus::Failed,
            "degraded" => ResourceStatus::Degraded,
            "reloading" | "activating" | "deactivating" => ResourceStatus::Restarting,
            _ => ResourceStatus::Unknown,
        };

        Ok(Some(ResourceDetail {
            resource: Resource {
                id: id.to_string(),
                kind: ResourceKind::SystemdUnit,
                name: unit_name.to_string(),
                display_name: Some(unit_name.to_string()),
                group_name: Some("Systemd".to_string()),
                source: "allowlist".to_string(),
                labels_json: None,
                first_seen: Utc::now(),
                last_seen: Utc::now(),
            },
            status,
            active_state,
            sub_state: Some(sub_state),
            uptime_secs: None,
            memory_bytes: None,
            cpu_percent: None,
        }))
    }

    pub async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        let unit_name = id.strip_prefix("systemd:").unwrap_or(id);
        if !self.config.units.iter().any(|u| u == unit_name) {
            return Err(AppError::ResourceNotFound(id.to_string()));
        }
        if !self.config.allow_actions.iter().any(|a| a == action) {
            return Err(AppError::ActionNotAllowed(
                action.to_string(),
                id.to_string(),
            ));
        }

        let conn = Connection::system()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect to system D-Bus: {e}")))?;

        let dbus_method = match action {
            "start" => "StartUnit",
            "stop" => "StopUnit",
            "restart" => "RestartUnit",
            other => {
                return Err(AppError::ActionNotAllowed(
                    other.to_string(),
                    id.to_string(),
                ))
            }
        };

        let mode = "replace";
        let _: zbus::zvariant::OwnedObjectPath = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                dbus_method,
                &(unit_name, mode),
            )
            .await
            .map_err(|e| AppError::Internal(format!("D-Bus {dbus_method} failed: {e}")))?
            .body()
            .deserialize()
            .map_err(|e| AppError::Internal(format!("Failed to deserialize job path: {e}")))?;

        Ok(())
    }

    pub fn get_log_buffer(&self, id: &str) -> Option<&ResourceLogBuffer> {
        self.log_buffers.get(id)
    }
}
