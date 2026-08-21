use mop_core::config::SystemdResourcesConfig;
use mop_watch::SystemdCollector;
use std::path::Path;
use tokio::sync::broadcast;

#[tokio::test]
async fn test_systemd_journal_real_logs_integration() {
    let var_journal = Path::new("/var/log/journal");
    let run_journal = Path::new("/run/log/journal");

    if !var_journal.exists() && !run_journal.exists() {
        eprintln!("Skipping test_systemd_journal_real_logs_integration: systemd journal directory not available");
        return;
    }

    let (event_tx, _) = broadcast::channel(128);
    let config = SystemdResourcesConfig {
        units: vec!["systemd-journald.service".to_string()],
        allow_actions: vec!["restart".to_string()],
    };

    let collector = SystemdCollector::new(config, event_tx)
        .await
        .expect("SystemdCollector should initialize");

    // Wait a brief moment for journald tailer to read entries
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let logs = collector
        .get_logs("systemd:systemd-journald.service", 50, None)
        .await
        .expect("get_logs should succeed for allowlisted unit");

    println!(
        "Collected {} journal log lines for systemd-journald.service",
        logs.len()
    );

    // Each collected log line must have valid stream 'journal' and non-empty line
    for line in &logs {
        assert_eq!(line.stream, "journal");
        assert!(!line.line.is_empty());
    }
}
