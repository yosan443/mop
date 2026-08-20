use mop_core::config::DockerResourcesConfig;
use mop_watch::docker::DockerCollector;
use std::path::Path;
use tokio::sync::broadcast;

#[tokio::test]
async fn test_docker_real_daemon_integration() {
    let docker_sock = Path::new("/var/run/docker.sock");
    if !docker_sock.exists() && std::env::var("DOCKER_HOST").is_err() {
        println!(
            "Skipping Docker real integration test: Docker socket / DOCKER_HOST not available."
        );
        return;
    }

    let config = DockerResourcesConfig {
        containers: vec![],
        label_selector: "mop.managed=true".to_string(),
        allow_actions: vec![
            "start".to_string(),
            "stop".to_string(),
            "restart".to_string(),
        ],
    };

    let (tx, _) = broadcast::channel(16);
    let collector = match DockerCollector::new(config, tx).await {
        Ok(c) => c,
        Err(e) => {
            println!("Docker connection failed ({e}); skipping test.");
            return;
        }
    };

    // List resources should not panic and return containers matching filter
    let list_res = collector.list_resources().await;
    assert!(list_res.is_ok());
    let resources = list_res.unwrap();
    println!("Found {} mop-managed Docker containers", resources.len());
}
