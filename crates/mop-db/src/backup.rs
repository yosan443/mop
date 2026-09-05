use chrono::Utc;
use mop_core::config::Config;
use mop_core::error::AppError;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const BACKUP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupManifest {
    pub version: String,
    pub schema_version: u32,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstalledPluginBackupInfo {
    pub id: String,
    pub version: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestoreReport {
    pub database_restored: PathBuf,
    pub database_backup_created: Option<PathBuf>,
    pub config_restored: Option<PathBuf>,
    pub manifest: BackupManifest,
    pub plugin_notices: Vec<String>,
}

/// Compute SHA-256 hex digest for a file
pub fn compute_sha256(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Mask secret fields from a JSON representation of config
pub fn mask_json_value(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let lower = k.to_lowercase();
                if lower.contains("secret")
                    || lower.contains("password")
                    || lower.contains("token")
                    || lower.contains("key")
                {
                    *v = serde_json::Value::String("***".to_string());
                } else {
                    mask_json_value(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                mask_json_value(item);
            }
        }
        _ => {}
    }
}

/// Generate a sanitized TOML string from Config
pub fn mask_config_secrets(config: &Config) -> String {
    let mut val = serde_json::to_value(config).unwrap_or(serde_json::json!({}));
    mask_json_value(&mut val);
    if let Ok(toml_val) = toml::Value::try_from(val) {
        toml::to_string_pretty(&toml_val).unwrap_or_default()
    } else {
        String::new()
    }
}

/// Perform an SQLite online backup (sqlite3_backup / VACUUM INTO)
/// Safely captures database state without locking writers or copying dirty WAL pages directly.
pub async fn online_backup(pool: &SqlitePool, dest_path: &Path) -> Result<(), AppError> {
    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AppError::Database(format!(
                    "Failed to create backup directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    if dest_path.exists() {
        let _ = tokio::fs::remove_file(dest_path).await;
    }

    let escaped = dest_path.to_string_lossy().replace('\'', "''");
    let query = format!("VACUUM INTO '{escaped}'");
    sqlx::query(&query)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(format!("SQLite online backup failed: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest_path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// Create a full backup archive (.tar.zst) containing database snapshot, config, plugin metadata, and checksums
pub async fn create_backup_archive(
    pool: &SqlitePool,
    config: &Config,
    dest_dir: &Path,
) -> Result<PathBuf, AppError> {
    tokio::fs::create_dir_all(dest_dir).await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to create backup output directory {}: {e}",
            dest_dir.display()
        ))
    })?;

    let now = Utc::now();
    let ts_str = now.format("%Y%m%dT%H%M%SZ").to_string();
    let filename = format!("mop-backup-{ts_str}.tar.zst");
    let archive_path = dest_dir.join(&filename);

    let stage_dir = tempfile::tempdir()
        .map_err(|e| AppError::Internal(format!("Failed to create staging directory: {e}")))?;
    let stage_path = stage_dir.path();

    // 1. manifest.json
    let manifest = BackupManifest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: BACKUP_SCHEMA_VERSION,
        created_at: now.to_rfc3339(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| AppError::Internal(format!("Failed to serialize manifest: {e}")))?;
    tokio::fs::write(stage_path.join("manifest.json"), manifest_bytes)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write manifest.json: {e}")))?;

    // 2. database/mop.db
    let db_stage_dir = stage_path.join("database");
    tokio::fs::create_dir_all(&db_stage_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create database staging dir: {e}")))?;
    let db_dest = db_stage_dir.join("mop.db");
    online_backup(pool, &db_dest).await?;

    // 3. config/config.toml
    let config_stage_dir = stage_path.join("config");
    tokio::fs::create_dir_all(&config_stage_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create config staging dir: {e}")))?;
    let masked_toml = mask_config_secrets(config);
    tokio::fs::write(config_stage_dir.join("config.toml"), masked_toml)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write config.toml: {e}")))?;

    // 4. plugins/
    let plugins_stage_dir = stage_path.join("plugins");
    tokio::fs::create_dir_all(&plugins_stage_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create plugins staging dir: {e}")))?;

    let plugin_repo = crate::repos::PluginRepo::new(pool.clone());
    let settings_repo = crate::repos::PluginSettingsRepo::new(pool.clone());

    let installed_plugins = plugin_repo
        .list_plugins()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| InstalledPluginBackupInfo {
            id: p.id,
            version: p.version,
            enabled: p.enabled,
        })
        .collect::<Vec<_>>();

    let installed_bytes = serde_json::to_vec_pretty(&installed_plugins)
        .map_err(|e| AppError::Internal(format!("Failed to serialize installed plugins: {e}")))?;
    tokio::fs::write(plugins_stage_dir.join("installed.json"), installed_bytes)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write installed.json: {e}")))?;

    for plugin in &installed_plugins {
        let p_dir = plugins_stage_dir.join(&plugin.id);
        tokio::fs::create_dir_all(&p_dir).await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to create plugin dir for {}: {e}",
                plugin.id
            ))
        })?;

        if let Ok(settings) = settings_repo.get_applied_settings(&plugin.id).await {
            let mut settings_val = serde_json::to_value(&settings).unwrap_or(serde_json::json!({}));
            mask_json_value(&mut settings_val);
            let settings_bytes = serde_json::to_vec_pretty(&settings_val).unwrap_or_default();
            let _ = tokio::fs::write(p_dir.join("settings.json"), settings_bytes).await;
        }

        // Copy plugin.toml manifest if found on host
        let host_plugin_dir = config.plugins.dir.join(&plugin.id).join(&plugin.version);
        let host_manifest = host_plugin_dir.join("plugin.toml");
        if host_manifest.exists() {
            let _ = tokio::fs::copy(&host_manifest, p_dir.join("plugin.toml")).await;
        }
    }

    // 5. checksums.sha256
    let mut files_to_hash = Vec::new();
    fn collect_files(base: &Path, current: &Path, list: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_files(base, &path, list);
                } else if path.is_file() {
                    if let Ok(rel) = path.strip_prefix(base) {
                        list.push(rel.to_path_buf());
                    }
                }
            }
        }
    }

    collect_files(stage_path, stage_path, &mut files_to_hash);
    files_to_hash.sort();

    let mut checksum_lines = String::new();
    for rel in files_to_hash {
        let full = stage_path.join(&rel);
        let hash = compute_sha256(&full)
            .map_err(|e| AppError::Internal(format!("Failed to hash {}: {e}", rel.display())))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        checksum_lines.push_str(&format!("{hash}  {rel_str}\n"));
    }

    tokio::fs::write(stage_path.join("checksums.sha256"), checksum_lines)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write checksums.sha256: {e}")))?;

    // 6. Tar and zstd encode
    let out_file = File::create(&archive_path)
        .map_err(|e| AppError::Internal(format!("Failed to create archive file: {e}")))?;
    let zstd_encoder = zstd::stream::write::Encoder::new(out_file, 3)
        .map_err(|e| AppError::Internal(format!("Failed to initialize zstd encoder: {e}")))?
        .auto_finish();
    let mut tar_builder = tar::Builder::new(zstd_encoder);

    // Recursively add stage directory entries into tar
    tar_builder
        .append_dir_all(".", stage_path)
        .map_err(|e| AppError::Internal(format!("Failed to build tar archive: {e}")))?;

    tar_builder
        .into_inner()
        .map_err(|e| AppError::Internal(format!("Failed to finish tar archive: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&archive_path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(archive_path)
}

/// Verify integrity and schema compatibility of a backup archive
pub fn verify_backup_archive(
    archive_path: &Path,
) -> Result<(BackupManifest, tempfile::TempDir), AppError> {
    if !archive_path.exists() {
        return Err(AppError::NotFound(format!(
            "Backup archive not found at {}",
            archive_path.display()
        )));
    }

    let in_file = File::open(archive_path)
        .map_err(|e| AppError::Internal(format!("Failed to open archive file: {e}")))?;
    let zstd_decoder = zstd::stream::read::Decoder::new(in_file)
        .map_err(|e| AppError::Validation(format!("Invalid zstd archive: {e}")))?;
    let mut tar_archive = tar::Archive::new(zstd_decoder);

    let temp_dir = tempfile::tempdir()
        .map_err(|e| AppError::Internal(format!("Failed to create temporary directory: {e}")))?;

    tar_archive
        .unpack(temp_dir.path())
        .map_err(|e| AppError::Validation(format!("Failed to unpack tar archive: {e}")))?;

    let root = temp_dir.path();

    // Verify manifest
    let manifest_path = root.join("manifest.json");
    if !manifest_path.exists() {
        return Err(AppError::Validation(
            "Missing manifest.json in backup archive".to_string(),
        ));
    }
    let manifest_content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| AppError::Validation(format!("Failed to read manifest.json: {e}")))?;
    let manifest: BackupManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| AppError::Validation(format!("Failed to parse manifest.json: {e}")))?;

    if manifest.schema_version != BACKUP_SCHEMA_VERSION {
        return Err(AppError::Validation(format!(
            "Incompatible backup schema version: expected {BACKUP_SCHEMA_VERSION}, found {}",
            manifest.schema_version
        )));
    }

    // Verify checksums
    let checksums_path = root.join("checksums.sha256");
    if !checksums_path.exists() {
        return Err(AppError::Validation(
            "Missing checksums.sha256 in backup archive".to_string(),
        ));
    }

    let checksums_content = std::fs::read_to_string(&checksums_path)
        .map_err(|e| AppError::Validation(format!("Failed to read checksums.sha256: {e}")))?;

    for line in checksums_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let expected_hash = parts[0];
        let rel_path = parts[1].trim_start_matches("./");
        let file_path = root.join(rel_path);

        if !file_path.exists() {
            return Err(AppError::Validation(format!(
                "File '{rel_path}' listed in checksums.sha256 was not found in archive"
            )));
        }

        let actual_hash = compute_sha256(&file_path)
            .map_err(|e| AppError::Validation(format!("Failed to hash '{rel_path}': {e}")))?;

        if !expected_hash.eq_ignore_ascii_case(&actual_hash) {
            return Err(AppError::Validation(format!(
                "Checksum mismatch for '{rel_path}': expected {expected_hash}, computed {actual_hash}"
            )));
        }
    }

    Ok((manifest, temp_dir))
}

/// Restore database and configuration from a backup archive (offline mode)
pub async fn restore_backup_archive(
    archive_path: &Path,
    target_db_path: &Path,
    target_config_path: Option<&Path>,
    target_plugins_dir: Option<&Path>,
) -> Result<RestoreReport, AppError> {
    let (manifest, temp_dir) = verify_backup_archive(archive_path)?;
    let root = temp_dir.path();

    // 1. Existing DB backup
    let mut backup_created = None;
    if target_db_path.exists() {
        let now = Utc::now();
        let bak_name = format!(
            "{}.bak.{}",
            target_db_path.display(),
            now.format("%Y%m%dT%H%M%SZ")
        );
        let bak_path = PathBuf::from(&bak_name);
        tokio::fs::copy(target_db_path, &bak_path)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to backup existing database to {}: {e}",
                    bak_path.display()
                ))
            })?;
        backup_created = Some(bak_path);

        // Remove stale wal / shm files
        let wal_path = PathBuf::from(format!("{}-wal", target_db_path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", target_db_path.display()));
        let _ = tokio::fs::remove_file(wal_path).await;
        let _ = tokio::fs::remove_file(shm_path).await;
    }

    // 2. Restore database
    let archived_db = root.join("database").join("mop.db");
    if !archived_db.exists() {
        return Err(AppError::Validation(
            "Archive is missing database/mop.db".to_string(),
        ));
    }

    if let Some(parent) = target_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    tokio::fs::copy(&archived_db, target_db_path)
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to restore database to {}: {e}",
                target_db_path.display()
            ))
        })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target_db_path, std::fs::Permissions::from_mode(0o600));
    }

    // 3. Restore config (optional)
    let mut config_restored = None;
    if let Some(cfg_path) = target_config_path {
        let archived_cfg = root.join("config").join("config.toml");
        if archived_cfg.exists() {
            if cfg_path.exists() {
                let now = Utc::now();
                let bak_cfg = format!(
                    "{}.bak.{}",
                    cfg_path.display(),
                    now.format("%Y%m%dT%H%M%SZ")
                );
                let _ = tokio::fs::copy(cfg_path, &bak_cfg).await;
            }
            if let Some(parent) = cfg_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            tokio::fs::copy(&archived_cfg, cfg_path)
                .await
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to restore config to {}: {e}",
                        cfg_path.display()
                    ))
                })?;
            config_restored = Some(cfg_path.to_path_buf());
        }
    }

    // 4. Compare installed plugin versions
    let mut plugin_notices = Vec::new();
    let installed_json_path = root.join("plugins").join("installed.json");
    if installed_json_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&installed_json_path) {
            if let Ok(plugins) = serde_json::from_str::<Vec<InstalledPluginBackupInfo>>(&content) {
                for p in plugins {
                    if let Some(p_dir) = target_plugins_dir {
                        let candidate_versioned = p_dir.join(&p.id).join(&p.version);
                        let candidate_direct = p_dir.join(&p.id);
                        if !candidate_versioned.exists() && !candidate_direct.exists() {
                            plugin_notices.push(format!(
                                "Plugin '{}' version '{}' in backup is not currently installed in {}. Manual plugin package re-installation required.",
                                p.id, p.version, p_dir.display()
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(RestoreReport {
        database_restored: target_db_path.to_path_buf(),
        database_backup_created: backup_created,
        config_restored,
        manifest,
        plugin_notices,
    })
}
