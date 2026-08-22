use mop_core::models::{ResourceKind, ResourceStatus};
use mop_watch::compose::{
    aggregate_statuses, compute_start_order, compute_stop_order, parse_depends_on,
};
use mop_watch::{FakeResourceCollector, ResourceCollector};
use std::collections::HashMap;

#[test]
fn test_parse_depends_on() {
    let raw = "db:service_healthy:false,redis:service_started:false";
    let deps = parse_depends_on(raw);
    assert_eq!(deps, vec!["db", "redis"]);

    let raw_simple = "db, redis, auth";
    let deps_simple = parse_depends_on(raw_simple);
    assert_eq!(deps_simple, vec!["db", "redis", "auth"]);
}

#[test]
fn test_topological_sort_start_order() {
    // web -> api -> db
    let services = vec!["web".to_string(), "api".to_string(), "db".to_string()];
    let mut deps = HashMap::new();
    deps.insert("web".to_string(), vec!["api".to_string()]);
    deps.insert("api".to_string(), vec!["db".to_string()]);
    deps.insert("db".to_string(), vec![]);

    let start_order = compute_start_order(&services, &deps);
    // db must start before api, api before web
    let db_idx = start_order.iter().position(|s| s == "db").unwrap();
    let api_idx = start_order.iter().position(|s| s == "api").unwrap();
    let web_idx = start_order.iter().position(|s| s == "web").unwrap();

    assert!(db_idx < api_idx);
    assert!(api_idx < web_idx);
}

#[test]
fn test_reverse_topological_sort_stop_order() {
    // web -> api -> db
    let services = vec!["web".to_string(), "api".to_string(), "db".to_string()];
    let mut deps = HashMap::new();
    deps.insert("web".to_string(), vec!["api".to_string()]);
    deps.insert("api".to_string(), vec!["db".to_string()]);
    deps.insert("db".to_string(), vec![]);

    let stop_order = compute_stop_order(&services, &deps);
    // web must stop before api, api before db
    let web_idx = stop_order.iter().position(|s| s == "web").unwrap();
    let api_idx = stop_order.iter().position(|s| s == "api").unwrap();
    let db_idx = stop_order.iter().position(|s| s == "db").unwrap();

    assert!(web_idx < api_idx);
    assert!(api_idx < db_idx);
}

#[test]
fn test_cyclic_dependency_fallback() {
    // A -> B -> A (cyclic)
    let services = vec!["svc-a".to_string(), "svc-b".to_string()];
    let mut deps = HashMap::new();
    deps.insert("svc-a".to_string(), vec!["svc-b".to_string()]);
    deps.insert("svc-b".to_string(), vec!["svc-a".to_string()]);

    let order = compute_start_order(&services, &deps);
    assert_eq!(order.len(), 2);
    assert!(order.contains(&"svc-a".to_string()));
    assert!(order.contains(&"svc-b".to_string()));
}

#[test]
fn test_status_aggregation() {
    // All running -> Running
    assert_eq!(
        aggregate_statuses(&[ResourceStatus::Running, ResourceStatus::Running]),
        ResourceStatus::Running
    );

    // Partial running -> Degraded
    assert_eq!(
        aggregate_statuses(&[ResourceStatus::Running, ResourceStatus::Stopped]),
        ResourceStatus::Degraded
    );

    // Any failed -> Failed
    assert_eq!(
        aggregate_statuses(&[ResourceStatus::Running, ResourceStatus::Failed]),
        ResourceStatus::Failed
    );

    // Any restarting -> Restarting
    assert_eq!(
        aggregate_statuses(&[ResourceStatus::Running, ResourceStatus::Restarting]),
        ResourceStatus::Restarting
    );

    // All stopped -> Stopped
    assert_eq!(
        aggregate_statuses(&[ResourceStatus::Stopped, ResourceStatus::Stopped]),
        ResourceStatus::Stopped
    );
}

#[tokio::test]
async fn test_fake_compose_resources_and_managed_action() {
    let collector = FakeResourceCollector::new();

    // 1. List resources includes compose_project and compose_service
    let resources = collector.list_resources().await.unwrap();
    let project = resources
        .iter()
        .find(|r| r.id == "compose_project:media-stack")
        .expect("compose_project:media-stack should exist");
    assert_eq!(project.kind, ResourceKind::ComposeProject);

    let managed_service = resources
        .iter()
        .find(|r| r.id == "compose_service:media-stack:manga-worker")
        .expect("manga-worker service should exist");
    assert_eq!(managed_service.kind, ResourceKind::ComposeService);

    let unmanaged_service = resources
        .iter()
        .find(|r| r.id == "compose_service:media-stack:db")
        .expect("db service should exist");
    assert_eq!(unmanaged_service.kind, ResourceKind::ComposeService);

    // 2. Resource detail
    let detail = collector
        .get_resource_detail("compose_project:media-stack")
        .await
        .unwrap()
        .expect("Project detail should exist");
    assert_eq!(detail.status, ResourceStatus::Running);

    // 3. Restart managed service succeeds
    let res = collector
        .execute_action("compose_service:media-stack:manga-worker", "restart")
        .await;
    assert!(res.is_ok());

    // 4. Restart unmanaged service is forbidden (SPEC §9.3 & 不変条件 3)
    let unmanaged_res = collector
        .execute_action("compose_service:media-stack:db", "restart")
        .await;
    assert!(unmanaged_res.is_err());
    let err_str = unmanaged_res.unwrap_err().to_string();
    assert!(err_str.contains("has no managed containers"));

    // 5. Restart project restarts only managed containers
    let proj_res = collector
        .execute_action("compose_project:media-stack", "restart")
        .await;
    assert!(proj_res.is_ok());
}

#[tokio::test]
async fn test_compose_service_and_project_aggregated_logs() {
    let collector = FakeResourceCollector::new();
    // Allow background logs to populate
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // 1. Get logs for compose_service
    let svc_logs = collector
        .get_logs("compose_service:media-stack:manga-worker", 20, None)
        .await
        .expect("Failed to get compose service logs");

    assert!(!svc_logs.is_empty(), "Service logs should not be empty");
    assert!(
        svc_logs
            .iter()
            .any(|l| l.line.contains("[manga-worker|media-stack-manga-worker-1]")),
        "Service logs must contain [service|container] prefix, found: {:?}",
        svc_logs.iter().map(|l| &l.line).collect::<Vec<_>>()
    );

    // 2. Get logs for compose_project
    let proj_logs = collector
        .get_logs("compose_project:media-stack", 20, None)
        .await
        .expect("Failed to get compose project logs");

    assert!(!proj_logs.is_empty(), "Project logs should not be empty");
    assert!(
        proj_logs
            .iter()
            .any(|l| l.line.contains("[manga-worker|media-stack-manga-worker-1]")),
        "Project logs must contain manga-worker prefix"
    );
    assert!(
        proj_logs
            .iter()
            .any(|l| l.line.contains("[db|media-stack-db-1]")),
        "Project logs must contain db prefix"
    );
}
