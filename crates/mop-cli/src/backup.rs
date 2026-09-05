use mop_core::config::Config;
use mop_core::error::AppError;
use mop_db::backup::{create_backup_archive, restore_backup_archive};
use mop_db::pool::create_sqlite_pool;
use std::path::{Path, PathBuf};

/// Check if mop daemon is currently stopped
pub async fn check_mop_service_stopped() -> bool {
    // 1. Check if host.sock is active and responding
    let socket_path = Path::new("/run/mop/host.sock");
    if socket_path.exists() {
        #[cfg(unix)]
        if std::os::unix::net::UnixStream::connect(socket_path).is_ok() {
            return false;
        }
    }

    // 2. Check systemd service status via D-Bus if available
    #[cfg(unix)]
    if let Ok(conn) = zbus::Connection::system().await {
        let unit_path_res: Result<zbus::zvariant::OwnedObjectPath, _> = conn
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "GetUnit",
                &("mop.service",),
            )
            .await
            .and_then(|r| r.body().deserialize());

        if let Ok(unit_path) = unit_path_res {
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
                    if trimmed == "active" || trimmed == "activating" {
                        return false;
                    }
                }
            }
        }
    }

    true
}

pub async fn handle_backup_create(
    config: &Config,
    output_dir: Option<PathBuf>,
) -> Result<(), AppError> {
    println!("Starting mop online backup...");
    let pool = create_sqlite_pool(&config.database.path).await?;
    let dest_dir = output_dir.unwrap_or_else(|| config.backup.dir.clone());

    let archive_path = create_backup_archive(&pool, config, &dest_dir).await?;

    let meta = std::fs::metadata(&archive_path)
        .map_err(|e| AppError::Internal(format!("Failed to inspect backup file: {e}")))?;
    let size_kb = meta.len() as f64 / 1024.0;

    println!("Backup archive created successfully:");
    println!("  Archive: {}", archive_path.display());
    println!("  Size:    {:.1} KB", size_kb);

    Ok(())
}

pub fn handle_backup_list(config: &Config, dir: Option<PathBuf>) -> Result<(), AppError> {
    let target_dir = dir.unwrap_or_else(|| config.backup.dir.clone());
    if !target_dir.exists() {
        println!(
            "No backups found (directory {} does not exist).",
            target_dir.display()
        );
        return Ok(());
    }

    println!("Backups in {}:", target_dir.display());
    let mut entries = Vec::new();

    if let Ok(dir_entries) = std::fs::read_dir(&target_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("mop-backup-") && name.ends_with(".tar.zst") {
                    if let Ok(meta) = entry.metadata() {
                        entries.push((name.to_string(), meta.len(), meta.modified().ok()));
                    }
                }
            }
        }
    }

    if entries.is_empty() {
        println!("  (none)");
        return Ok(());
    }

    entries.sort_by(|a, b| b.0.cmp(&a.0));

    for (name, size, mtime) in entries {
        let size_kb = size as f64 / 1024.0;
        let time_str = mtime
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            })
            .unwrap_or_else(|| "unknown".to_string());

        println!("  - {:<40} {:>8.1} KB  ({})", name, size_kb, time_str);
    }

    Ok(())
}

pub async fn handle_restore(
    config: &Config,
    archive_path: &Path,
    config_file: Option<&Path>,
    force: bool,
) -> Result<(), AppError> {
    if !force && !check_mop_service_stopped().await {
        return Err(AppError::Validation(
            "mop is currently running. Please stop mop.service before restoring (offline restore only, e.g. 'sudo systemctl stop mop.service'). Use --force to override if necessary.".to_string()
        ));
    }

    println!("Verifying and restoring from {}...", archive_path.display());

    let report = restore_backup_archive(
        archive_path,
        &config.database.path,
        config_file,
        Some(&config.plugins.dir),
    )
    .await?;

    println!("Backup restored successfully:");
    println!(
        "  Database restored: {}",
        report.database_restored.display()
    );
    if let Some(bak) = report.database_backup_created {
        println!("  Previous database backed up to: {}", bak.display());
    }
    if let Some(cfg) = report.config_restored {
        println!("  Config restored:   {}", cfg.display());
    }
    println!(
        "  Manifest version:  {} (schema {})",
        report.manifest.version, report.manifest.schema_version
    );
    println!("  Backup timestamp:  {}", report.manifest.created_at);

    if !report.plugin_notices.is_empty() {
        println!("\nPlugin notices:");
        for notice in report.plugin_notices {
            println!("  - {notice}");
        }
    }

    Ok(())
}
