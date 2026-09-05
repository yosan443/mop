use chrono::Utc;
use mop_core::config::Config;
use mop_core::models::plugin::{PluginRecord, PluginState};
use mop_db::backup::*;
use mop_db::migration::run_migrations;
use mop_db::pool::create_sqlite_pool;
use mop_db::repos::{PluginRepo, PluginSettingsRepo, UserRepo};
use std::fs::File;
use tempfile::tempdir;

#[tokio::test]
async fn test_wal_online_backup() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("live.db");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    // Insert user and plugin in WAL mode
    let user = mop_core::models::User {
        id: "01JTESTUSER0000000000000001".to_string(),
        username: "admin".to_string(),
        password_hash: "$argon2id$...".to_string(),
        role: mop_core::models::Role::Admin,
        disabled: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    UserRepo::create(&pool, &user).await.unwrap();

    // Insert data directly
    sqlx::query("INSERT INTO audit_events (id, ts, action, result) VALUES (?, ?, ?, ?)")
        .bind("01JTESTAUDIT000000000000001")
        .bind(Utc::now().to_rfc3339())
        .bind("test.action")
        .bind("success")
        .execute(&pool)
        .await
        .unwrap();

    // Perform online backup while WAL is active
    let backup_db_path = tmp.path().join("backup_snapshot.db");
    online_backup(&pool, &backup_db_path).await.unwrap();

    assert!(backup_db_path.exists(), "Backup DB file must exist");

    // Connect to backed up database and verify records
    let backup_pool = create_sqlite_pool(&backup_db_path).await.unwrap();
    let admin = UserRepo::find_by_username(&backup_pool, "admin")
        .await
        .unwrap();
    assert!(admin.is_some(), "Admin user must exist in backed up DB");
    assert_eq!(admin.unwrap().username, "admin");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&backup_pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "Audit event must be present in backed up DB");
}

#[tokio::test]
async fn test_backup_restore_roundtrip() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("original.db");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let user = mop_core::models::User {
        id: "01JTESTUSER0000000000000002".to_string(),
        username: "superadmin".to_string(),
        password_hash: "$argon2id$...".to_string(),
        role: mop_core::models::Role::Admin,
        disabled: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    UserRepo::create(&pool, &user).await.unwrap();

    let plugin_repo = PluginRepo::new(pool.clone());
    let plugin_rec = PluginRecord {
        id: "mop.manga".to_string(),
        name: "Manga Converter".to_string(),
        version: "0.1.0".to_string(),
        enabled: true,
        state: PluginState::Enabled,
        manifest_json: "{}".to_string(),
        installed_at: Utc::now(),
        enabled_at: Some(Utc::now()),
    };
    plugin_repo.upsert_plugin(&plugin_rec).await.unwrap();

    let settings_repo = PluginSettingsRepo::new(pool.clone());
    settings_repo
        .save_draft_setting(
            "mop.manga",
            "output_dir",
            &serde_json::json!("/data/cbz"),
            "admin",
        )
        .await
        .unwrap();
    settings_repo
        .apply_draft_settings("mop.manga")
        .await
        .unwrap();

    let mut config = Config::default();
    config.database.path = db_path.clone();
    let backup_out_dir = tmp.path().join("backups");

    // 1. Create full backup archive
    let archive_path = create_backup_archive(&pool, &config, &backup_out_dir)
        .await
        .unwrap();
    assert!(archive_path.exists());
    assert!(archive_path.to_string_lossy().ends_with(".tar.zst"));

    // 2. Verify archive
    let (manifest, _) = verify_backup_archive(&archive_path).unwrap();
    assert_eq!(manifest.schema_version, BACKUP_SCHEMA_VERSION);

    // 3. Restore into clean locations
    let restored_db_path = tmp.path().join("restored.db");
    let restored_cfg_path = tmp.path().join("restored_config.toml");
    let report = restore_backup_archive(
        &archive_path,
        &restored_db_path,
        Some(&restored_cfg_path),
        None,
    )
    .await
    .unwrap();

    assert_eq!(report.database_restored, restored_db_path);
    assert!(restored_db_path.exists());
    assert!(restored_cfg_path.exists());

    // Verify restored database contents
    let restored_pool = create_sqlite_pool(&restored_db_path).await.unwrap();
    let user = UserRepo::find_by_username(&restored_pool, "superadmin")
        .await
        .unwrap();
    assert!(user.is_some());

    let restored_settings_repo = PluginSettingsRepo::new(restored_pool.clone());
    let restored_settings = restored_settings_repo
        .get_applied_settings("mop.manga")
        .await
        .unwrap();
    assert_eq!(restored_settings["output_dir"], "/data/cbz");
}

#[tokio::test]
async fn test_checksum_tampering_detection() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let config = Config::default();
    let backup_dir = tmp.path().join("backups");
    let archive_path = create_backup_archive(&pool, &config, &backup_dir)
        .await
        .unwrap();

    // Extract archive, tamper with manifest.json, and re-compress
    let extract_dir = tmp.path().join("tamper_extract");
    let in_file = File::open(&archive_path).unwrap();
    let zstd_dec = zstd::stream::read::Decoder::new(in_file).unwrap();
    let mut tar = tar::Archive::new(zstd_dec);
    tar.unpack(&extract_dir).unwrap();

    // Modify manifest.json WITHOUT updating checksums.sha256
    let manifest_file = extract_dir.join("manifest.json");
    let mut manifest_val: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_file).unwrap()).unwrap();
    manifest_val["version"] = serde_json::json!("9.9.9-hacked");
    std::fs::write(
        &manifest_file,
        serde_json::to_string_pretty(&manifest_val).unwrap(),
    )
    .unwrap();

    // Re-pack into tampered archive
    let tampered_archive = tmp.path().join("tampered.tar.zst");
    let out_file = File::create(&tampered_archive).unwrap();
    let zstd_enc = zstd::stream::write::Encoder::new(out_file, 3)
        .unwrap()
        .auto_finish();
    let mut tar_builder = tar::Builder::new(zstd_enc);
    tar_builder.append_dir_all(".", &extract_dir).unwrap();
    tar_builder.into_inner().unwrap();

    // Verify must detect checksum mismatch
    let verify_res = verify_backup_archive(&tampered_archive);
    assert!(verify_res.is_err());
    let err_msg = verify_res.unwrap_err().to_string();
    assert!(
        err_msg.contains("Checksum mismatch") || err_msg.contains("Checksum verification"),
        "Error message should mention checksum mismatch, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_schema_mismatch_rejection() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let pool = create_sqlite_pool(&db_path).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let config = Config::default();
    let backup_dir = tmp.path().join("backups");
    let archive_path = create_backup_archive(&pool, &config, &backup_dir)
        .await
        .unwrap();

    // Extract archive, change schema_version to 999, update checksums to have valid hashes
    let extract_dir = tmp.path().join("schema_extract");
    let in_file = File::open(&archive_path).unwrap();
    let zstd_dec = zstd::stream::read::Decoder::new(in_file).unwrap();
    let mut tar = tar::Archive::new(zstd_dec);
    tar.unpack(&extract_dir).unwrap();

    let manifest_file = extract_dir.join("manifest.json");
    let mut manifest: BackupManifest =
        serde_json::from_str(&std::fs::read_to_string(&manifest_file).unwrap()).unwrap();
    manifest.schema_version = 999;
    std::fs::write(
        &manifest_file,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Recompute checksum for manifest.json in checksums.sha256
    let new_hash = compute_sha256(&manifest_file).unwrap();
    let checksums_file = extract_dir.join("checksums.sha256");
    let lines = std::fs::read_to_string(&checksums_file).unwrap();
    let mut updated_lines = String::new();
    for l in lines.lines() {
        if l.contains("manifest.json") {
            updated_lines.push_str(&format!("{new_hash}  manifest.json\n"));
        } else {
            updated_lines.push_str(&format!("{l}\n"));
        }
    }
    std::fs::write(&checksums_file, updated_lines).unwrap();

    // Re-pack into invalid schema archive
    let invalid_archive = tmp.path().join("invalid_schema.tar.zst");
    let out_file = File::create(&invalid_archive).unwrap();
    let zstd_enc = zstd::stream::write::Encoder::new(out_file, 3)
        .unwrap()
        .auto_finish();
    let mut tar_builder = tar::Builder::new(zstd_enc);
    tar_builder.append_dir_all(".", &extract_dir).unwrap();
    tar_builder.into_inner().unwrap();

    // Verify must reject incompatible schema
    let verify_res = verify_backup_archive(&invalid_archive);
    assert!(verify_res.is_err());
    let err_msg = verify_res.unwrap_err().to_string();
    assert!(
        err_msg.contains("Incompatible backup schema version"),
        "Error message should mention incompatible schema version, got: {err_msg}"
    );
}
