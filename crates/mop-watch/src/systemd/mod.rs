use crate::ring_buffer::ResourceLogBuffer;
use crate::traits::{LogLine, ResourceDetail, ResourceEvent};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use mop_core::config::SystemdResourcesConfig;
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use zbus::Connection;

pub struct SystemdCollector {
    config: SystemdResourcesConfig,
    log_buffers: Arc<RwLock<HashMap<String, ResourceLogBuffer>>>,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl SystemdCollector {
    pub async fn new(
        config: SystemdResourcesConfig,
        event_tx: broadcast::Sender<ResourceEvent>,
    ) -> Result<Self, AppError> {
        let mut map = HashMap::new();
        for unit in &config.units {
            let id = format!("systemd:{unit}");
            map.insert(id, ResourceLogBuffer::new(5000, 65536));
        }

        let collector = Self {
            config,
            log_buffers: Arc::new(RwLock::new(map)),
            event_tx,
        };

        // Start background event listener for D-Bus / systemd status updates
        collector.start_dbus_event_listener();

        // Start background journald log stream tailer
        collector.start_journal_tailer();

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

            // In zbus 5, create a MessageStream from Connection to receive broadcast signals
            let mut stream = zbus::MessageStream::from(&conn);
            tracing::info!(
                "Subscribed to systemd D-Bus signals for units: {:?}",
                allowed_units
            );

            while let Some(Ok(msg)) = stream.next().await {
                // Check if message is a signal from systemd
                let header = msg.header();
                if let Some(path) = header.path() {
                    let path_str = path.as_str();
                    let member = header.member().map(|m| m.as_str()).unwrap_or_default();
                    if member == "PropertiesChanged"
                        || member == "UnitNew"
                        || member == "JobRemoved"
                    {
                        for unit in &allowed_units {
                            let escaped = unit.replace('.', "_2e").replace('-', "_2d");
                            if path_str.contains(&escaped) || member == "JobRemoved" {
                                let id = format!("systemd:{unit}");
                                if let Ok(status) = Self::query_unit_status(&conn, unit).await {
                                    let _ = event_tx.send(ResourceEvent {
                                        id,
                                        kind: ResourceKind::SystemdUnit,
                                        status,
                                        ts: Utc::now(),
                                        message: Some(format!(
                                            "systemd unit {unit} state changed: {status}"
                                        )),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    async fn query_unit_status(
        conn: &Connection,
        unit_name: &str,
    ) -> Result<ResourceStatus, zbus::Error> {
        let unit_path: zbus::zvariant::OwnedObjectPath = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "GetUnit",
                &(unit_name),
            )
            .await?
            .body()
            .deserialize()?;

        let active_state: String = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                unit_path.as_ref(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.freedesktop.systemd1.Unit", "ActiveState"),
            )
            .await?
            .body()
            .deserialize::<zbus::zvariant::OwnedValue>()?
            .to_string()
            .trim_matches('"')
            .to_string();

        Ok(match active_state.as_str() {
            "active" => ResourceStatus::Running,
            "inactive" => ResourceStatus::Stopped,
            "failed" => ResourceStatus::Failed,
            "degraded" => ResourceStatus::Degraded,
            "reloading" | "activating" | "deactivating" => ResourceStatus::Restarting,
            _ => ResourceStatus::Unknown,
        })
    }

    fn start_journal_tailer(&self) {
        let log_buffers = self.log_buffers.clone();
        let allowed_units = self.config.units.clone();

        tokio::spawn(async move {
            let now = Utc::now();
            let map = log_buffers.read().await;
            for unit in &allowed_units {
                let id = format!("systemd:{unit}");
                if let Some(buf) = map.get(&id) {
                    buf.push(LogLine {
                        ts: now,
                        stream: "journal".to_string(),
                        line: format!("Started journal log collector for systemd unit {unit}"),
                    })
                    .await;
                }
            }
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
                    status: ResourceStatus::Unknown,
                    active_state: "unknown".to_string(),
                    sub_state: None,
                    uptime_secs: None,
                    memory_bytes: None,
                    cpu_percent: None,
                }));
            }
        };

        // Call GetUnit
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
                status: ResourceStatus::Unknown,
                active_state: "unknown".to_string(),
                sub_state: None,
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

    pub async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError> {
        let map = self.log_buffers.read().await;
        let Some(buf) = map.get(id) else {
            return Err(AppError::ResourceNotFound(id.to_string()));
        };
        Ok(buf.get_snapshot(tail, since).await)
    }

    pub async fn subscribe_logs(
        &self,
        id: &str,
        _since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError> {
        let map = self.log_buffers.read().await;
        let Some(buf) = map.get(id) else {
            return Err(AppError::ResourceNotFound(id.to_string()));
        };
        Ok(buf.subscribe())
    }
}
