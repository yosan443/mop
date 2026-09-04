use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::plugin::{
    PluginPermissionRecord, PluginRecord, PluginState, SettingDiffItem, SettingsDiff,
};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct PluginRepo {
    pool: SqlitePool,
}

impl PluginRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_plugins(&self) -> Result<Vec<PluginRecord>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, version, enabled, state, manifest_json, installed_at, enabled_at FROM plugins ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to list plugins: {e}")))?;

        let mut plugins = Vec::new();
        for r in rows {
            let state_str: String = r.get("state");
            let state = match state_str.as_str() {
                "installed" => PluginState::Installed,
                "enabled" => PluginState::Enabled,
                "running" => PluginState::Running,
                "degraded" => PluginState::Degraded,
                "disabled" => PluginState::Disabled,
                _ => PluginState::Installed,
            };

            let installed_at_str: String = r.get("installed_at");
            let installed_at = DateTime::parse_from_rfc3339(&installed_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            let enabled_at_str: Option<String> = r.get("enabled_at");
            let enabled_at = enabled_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            plugins.push(PluginRecord {
                id: r.get("id"),
                name: r.get("name"),
                version: r.get("version"),
                enabled: r.get::<i64, _>("enabled") != 0,
                state,
                manifest_json: r.get("manifest_json"),
                installed_at,
                enabled_at,
            });
        }

        Ok(plugins)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<PluginRecord>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, version, enabled, state, manifest_json, installed_at, enabled_at FROM plugins WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to find plugin {id}: {e}")))?;

        let Some(r) = row else { return Ok(None) };

        let state_str: String = r.get("state");
        let state = match state_str.as_str() {
            "installed" => PluginState::Installed,
            "enabled" => PluginState::Enabled,
            "running" => PluginState::Running,
            "degraded" => PluginState::Degraded,
            "disabled" => PluginState::Disabled,
            _ => PluginState::Installed,
        };

        let installed_at_str: String = r.get("installed_at");
        let installed_at = DateTime::parse_from_rfc3339(&installed_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let enabled_at_str: Option<String> = r.get("enabled_at");
        let enabled_at = enabled_at_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });

        Ok(Some(PluginRecord {
            id: r.get("id"),
            name: r.get("name"),
            version: r.get("version"),
            enabled: r.get::<i64, _>("enabled") != 0,
            state,
            manifest_json: r.get("manifest_json"),
            installed_at,
            enabled_at,
        }))
    }

    pub async fn upsert_plugin(&self, plugin: &PluginRecord) -> Result<(), AppError> {
        let state_str = plugin.state.as_str();
        let installed_at_str = plugin.installed_at.to_rfc3339();
        let enabled_at_str = plugin.enabled_at.map(|dt| dt.to_rfc3339());
        let enabled_int = if plugin.enabled { 1 } else { 0 };

        sqlx::query(
            "INSERT INTO plugins (id, name, version, enabled, state, manifest_json, installed_at, enabled_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                manifest_json = excluded.manifest_json"
        )
        .bind(&plugin.id)
        .bind(&plugin.name)
        .bind(&plugin.version)
        .bind(enabled_int)
        .bind(state_str)
        .bind(&plugin.manifest_json)
        .bind(installed_at_str)
        .bind(enabled_at_str)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to upsert plugin {}: {e}", plugin.id)))?;

        Ok(())
    }

    pub async fn update_state(&self, id: &str, state: PluginState) -> Result<(), AppError> {
        let state_str = state.as_str();
        sqlx::query("UPDATE plugins SET state = ? WHERE id = ?")
            .bind(state_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                AppError::Database(format!("Failed to update state for plugin {id}: {e}"))
            })?;

        Ok(())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), AppError> {
        let enabled_int = if enabled { 1 } else { 0 };
        let now_str = Utc::now().to_rfc3339();
        let state_str = if enabled { "enabled" } else { "disabled" };

        if enabled {
            sqlx::query("UPDATE plugins SET enabled = ?, state = ?, enabled_at = ? WHERE id = ?")
                .bind(enabled_int)
                .bind(state_str)
                .bind(now_str)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Database(format!("Failed to enable plugin {id}: {e}")))?;
        } else {
            sqlx::query("UPDATE plugins SET enabled = ?, state = ? WHERE id = ?")
                .bind(enabled_int)
                .bind(state_str)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Database(format!("Failed to disable plugin {id}: {e}")))?;
        }

        Ok(())
    }

    pub async fn delete_plugin(&self, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM plugins WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(format!("Failed to delete plugin {id}: {e}")))?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct PluginPermissionsRepo {
    pool: SqlitePool,
}

impl PluginPermissionsRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_permissions(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginPermissionRecord>, AppError> {
        let rows = sqlx::query(
            "SELECT plugin_id, capability, value_json, granted_by, granted_at FROM plugin_permissions WHERE plugin_id = ?"
        )
        .bind(plugin_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to list permissions for {plugin_id}: {e}")))?;

        let mut perms = Vec::new();
        for r in rows {
            let granted_at_str: String = r.get("granted_at");
            let granted_at = DateTime::parse_from_rfc3339(&granted_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            perms.push(PluginPermissionRecord {
                plugin_id: r.get("plugin_id"),
                capability: r.get("capability"),
                value_json: r.get("value_json"),
                granted_by: r.get("granted_by"),
                granted_at,
            });
        }

        Ok(perms)
    }

    pub async fn grant_permission(
        &self,
        plugin_id: &str,
        capability: &str,
        value_json: &str,
        granted_by: &str,
    ) -> Result<(), AppError> {
        let now_str = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO plugin_permissions (plugin_id, capability, value_json, granted_by, granted_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(plugin_id, capability, value_json) DO UPDATE SET
                granted_by = excluded.granted_by,
                granted_at = excluded.granted_at"
        )
        .bind(plugin_id)
        .bind(capability)
        .bind(value_json)
        .bind(granted_by)
        .bind(now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(format!("Failed to grant permission for {plugin_id}: {e}")))?;

        Ok(())
    }

    pub async fn revoke_all(&self, plugin_id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM plugin_permissions WHERE plugin_id = ?")
            .bind(plugin_id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                AppError::Database(format!("Failed to revoke permissions for {plugin_id}: {e}"))
            })?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct PluginSettingsRepo {
    pool: SqlitePool,
}

impl PluginSettingsRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_applied_settings(
        &self,
        plugin_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let rows = sqlx::query("SELECT key, value_json FROM plugin_settings WHERE plugin_id = ?")
            .bind(plugin_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                AppError::Database(format!("Failed to get settings for {plugin_id}: {e}"))
            })?;

        let mut map = HashMap::new();
        for r in rows {
            let key: String = r.get("key");
            let value_json: String = r.get("value_json");
            let val = serde_json::from_str(&value_json).unwrap_or(serde_json::Value::Null);
            map.insert(key, val);
        }

        Ok(map)
    }

    pub async fn get_draft_settings(
        &self,
        plugin_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let rows =
            sqlx::query("SELECT key, value_json FROM plugin_settings_draft WHERE plugin_id = ?")
                .bind(plugin_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    AppError::Database(format!("Failed to get draft settings for {plugin_id}: {e}"))
                })?;

        let mut map = HashMap::new();
        for r in rows {
            let key: String = r.get("key");
            let value_json: String = r.get("value_json");
            let val = serde_json::from_str(&value_json).unwrap_or(serde_json::Value::Null);
            map.insert(key, val);
        }

        Ok(map)
    }

    pub async fn save_draft_setting(
        &self,
        plugin_id: &str,
        key: &str,
        value: &serde_json::Value,
        updated_by: &str,
    ) -> Result<(), AppError> {
        let now_str = Utc::now().to_rfc3339();
        let value_json = serde_json::to_string(value).unwrap_or_default();

        sqlx::query(
            "INSERT INTO plugin_settings_draft (plugin_id, key, value_json, updated_by, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(plugin_id, key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_by = excluded.updated_by,
                updated_at = excluded.updated_at",
        )
        .bind(plugin_id)
        .bind(key)
        .bind(value_json)
        .bind(updated_by)
        .bind(now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            AppError::Database(format!("Failed to save draft setting for {plugin_id}: {e}"))
        })?;

        Ok(())
    }

    pub async fn save_draft_settings(
        &self,
        plugin_id: &str,
        settings: HashMap<String, serde_json::Value>,
        updated_by: &str,
    ) -> Result<SettingsDiff, AppError> {
        for (k, v) in &settings {
            self.save_draft_setting(plugin_id, k, v, updated_by).await?;
        }
        self.get_settings_diff(plugin_id).await
    }

    pub async fn apply_draft_settings(
        &self,
        plugin_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let drafts = self.get_draft_settings(plugin_id).await?;
        let now_str = Utc::now().to_rfc3339();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        for (key, val) in drafts {
            let value_json = serde_json::to_string(&val).unwrap_or_default();
            sqlx::query(
                "INSERT INTO plugin_settings (plugin_id, key, value_json, updated_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(plugin_id, key) DO UPDATE SET
                    value_json = excluded.value_json,
                    updated_at = excluded.updated_at",
            )
            .bind(plugin_id)
            .bind(&key)
            .bind(value_json)
            .bind(&now_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        // Clear draft table for this plugin
        sqlx::query("DELETE FROM plugin_settings_draft WHERE plugin_id = ?")
            .bind(plugin_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        self.get_applied_settings(plugin_id).await
    }

    pub async fn get_settings_diff(&self, plugin_id: &str) -> Result<SettingsDiff, AppError> {
        let applied = self.get_applied_settings(plugin_id).await?;
        let draft = self.get_draft_settings(plugin_id).await?;

        let mut all_keys: HashSet<String> = applied.keys().cloned().collect();
        all_keys.extend(draft.keys().cloned());

        let mut items = Vec::new();
        for key in all_keys {
            let app_val = applied.get(&key).cloned();
            let drf_val = draft.get(&key).cloned();

            let change_type = match (&app_val, &drf_val) {
                (None, Some(_)) => "added",
                (Some(a), Some(d)) if a != d => "modified",
                (Some(_), None) => "deleted",
                _ => "unchanged",
            };

            items.push(SettingDiffItem {
                key,
                applied_value: app_val,
                draft_value: drf_val,
                change_type: change_type.to_string(),
            });
        }

        items.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(SettingsDiff { items })
    }
}
