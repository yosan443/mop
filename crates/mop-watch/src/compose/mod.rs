use crate::ring_buffer::ResourceLogBuffer;
use crate::traits::{LogLine, ResourceDetail, ResourceEvent};
use bollard::container::{
    ListContainersOptions, RestartContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::Docker;
use chrono::{DateTime, Utc};
use mop_core::error::AppError;
use mop_core::models::{Resource, ResourceKind, ResourceStatus};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Represents information parsed from Docker Compose labels on a container
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeContainerInfo {
    pub container_name: String,
    pub project: String,
    pub service: String,
    pub container_number: Option<u32>,
    pub depends_on: Vec<String>,
    pub is_managed: bool,
    pub status: ResourceStatus,
    pub raw_labels: HashMap<String, String>,
}

/// Parses depends_on string from compose label (e.g. "db:service_healthy:false,redis:service_started:false" or "db,redis")
pub fn parse_depends_on(label_val: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for part in label_val.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        // If formatted as "service_name:condition:restart", extract the first segment
        let svc_name = trimmed.split(':').next().unwrap_or(trimmed).trim();
        if !svc_name.is_empty() && !deps.iter().any(|d| d == svc_name) {
            deps.push(svc_name.to_string());
        }
    }
    deps
}

/// Computes topological sort order for starting services (dependencies come first).
/// Returns list of service names in execution order.
pub fn compute_start_order(
    services: &[String],
    deps_map: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let service_set: HashSet<String> = services.iter().cloned().collect();
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for svc in services {
        in_degree.entry(svc.clone()).or_insert(0);
        adj.entry(svc.clone()).or_default();
    }

    // A depends on B means B must start before A, so edge is B -> A
    for svc in services {
        if let Some(deps) = deps_map.get(svc) {
            for dep in deps {
                if service_set.contains(dep) {
                    adj.entry(dep.clone()).or_default().push(svc.clone());
                    *in_degree.entry(svc.clone()).or_default() += 1;
                }
            }
        }
    }

    let mut queue = VecDeque::new();
    for svc in services {
        if in_degree.get(svc).copied().unwrap_or(0) == 0 {
            queue.push_back(svc.clone());
        }
    }

    let mut order = Vec::new();
    while let Some(u) = queue.pop_front() {
        order.push(u.clone());
        if let Some(neighbors) = adj.get(&u) {
            for v in neighbors {
                if let Some(deg) = in_degree.get_mut(v) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(v.clone());
                    }
                }
            }
        }
    }

    // If cyclic dependency occurred, append remaining services in deterministic order
    if order.len() < services.len() {
        for svc in services {
            if !order.contains(svc) {
                order.push(svc.clone());
            }
        }
    }

    order
}

/// Computes reverse topological sort order for stopping services (dependents stopped first).
pub fn compute_stop_order(
    services: &[String],
    deps_map: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut start_order = compute_start_order(services, deps_map);
    start_order.reverse();
    start_order
}

/// Aggregates container statuses into service/project status
pub fn aggregate_statuses(statuses: &[ResourceStatus]) -> ResourceStatus {
    if statuses.is_empty() {
        return ResourceStatus::Stopped;
    }
    if statuses.contains(&ResourceStatus::Failed) {
        return ResourceStatus::Failed;
    }
    if statuses.contains(&ResourceStatus::Restarting) {
        return ResourceStatus::Restarting;
    }
    let running_count = statuses
        .iter()
        .filter(|s| **s == ResourceStatus::Running)
        .count();
    if running_count == statuses.len() {
        ResourceStatus::Running
    } else if running_count > 0 {
        ResourceStatus::Degraded
    } else {
        ResourceStatus::Stopped
    }
}

pub struct ComposeCollector {
    docker: Option<Docker>,
    log_buffers: Arc<RwLock<HashMap<String, ResourceLogBuffer>>>,
    docker_log_buffers: Arc<RwLock<HashMap<String, ResourceLogBuffer>>>,
    event_tx: broadcast::Sender<ResourceEvent>,
}

impl ComposeCollector {
    pub fn new(
        docker: Option<Docker>,
        event_tx: broadcast::Sender<ResourceEvent>,
        docker_log_buffers: Arc<RwLock<HashMap<String, ResourceLogBuffer>>>,
    ) -> Self {
        Self {
            docker,
            log_buffers: Arc::new(RwLock::new(HashMap::new())),
            docker_log_buffers,
            event_tx,
        }
    }

    /// Fetches all containers from Docker daemon and groups them by Compose project/service
    pub async fn fetch_compose_containers(&self) -> Result<Vec<ComposeContainerInfo>, AppError> {
        let Some(docker) = &self.docker else {
            return Ok(Vec::new());
        };

        let options = ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };

        let containers = docker.list_containers(Some(options)).await.map_err(|e| {
            AppError::Internal(format!("Failed to list docker containers for compose: {e}"))
        })?;

        let mut result = Vec::new();

        for c in containers {
            let labels = c.labels.unwrap_or_default();
            let project = match labels.get("com.docker.compose.project") {
                Some(p) if !p.is_empty() => p.clone(),
                _ => continue,
            };
            let service = match labels.get("com.docker.compose.service") {
                Some(s) if !s.is_empty() => s.clone(),
                _ => continue,
            };

            let names = c.names.unwrap_or_default();
            let container_name = names
                .first()
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_else(|| c.id.clone().unwrap_or_default());

            let container_number = labels
                .get("com.docker.compose.container-number")
                .and_then(|n| n.parse::<u32>().ok());

            let depends_on = labels
                .get("com.docker.compose.depends_on")
                .map(|d| parse_depends_on(d))
                .unwrap_or_default();

            let is_managed = labels
                .get("mop.managed")
                .map(|v| v == "true")
                .unwrap_or(false);

            let state_str = c.state.unwrap_or_default().to_lowercase();
            let status = match state_str.as_str() {
                "running" => ResourceStatus::Running,
                "restarting" => ResourceStatus::Restarting,
                "exited" | "dead" | "created" => ResourceStatus::Stopped,
                _ => ResourceStatus::Unknown,
            };

            result.push(ComposeContainerInfo {
                container_name,
                project,
                service,
                container_number,
                depends_on,
                is_managed,
                status,
                raw_labels: labels,
            });
        }

        Ok(result)
    }

    pub async fn list_resources(&self) -> Result<Vec<Resource>, AppError> {
        let containers = self.fetch_compose_containers().await?;
        let now = Utc::now();

        let mut projects: HashMap<String, Vec<&ComposeContainerInfo>> = HashMap::new();
        let mut services: HashMap<(String, String), Vec<&ComposeContainerInfo>> = HashMap::new();

        for c in &containers {
            projects.entry(c.project.clone()).or_default().push(c);
            services
                .entry((c.project.clone(), c.service.clone()))
                .or_default()
                .push(c);
        }

        let mut resources = Vec::new();

        // 1. Compose Projects
        for (project_name, p_containers) in &projects {
            let managed_count = p_containers.iter().filter(|c| c.is_managed).count();
            let total_count = p_containers.len();
            let containers_json = p_containers
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.container_name,
                        "service": c.service,
                        "status": c.status.as_str(),
                        "is_managed": c.is_managed,
                    })
                })
                .collect::<Vec<_>>();

            let labels_json = serde_json::json!({
                "type": "compose_project",
                "project": project_name,
                "containers_count": total_count,
                "managed_containers_count": managed_count,
                "containers": containers_json,
            })
            .to_string();

            resources.push(Resource {
                id: format!("compose_project:{project_name}"),
                kind: ResourceKind::ComposeProject,
                name: project_name.clone(),
                display_name: Some(project_name.clone()),
                group_name: Some("Docker Compose".to_string()),
                source: "compose".to_string(),
                labels_json: Some(labels_json),
                first_seen: now,
                last_seen: now,
            });
        }

        // 2. Compose Services
        for ((project_name, service_name), s_containers) in &services {
            let managed = s_containers.iter().any(|c| c.is_managed);
            let deps = s_containers
                .first()
                .map(|c| c.depends_on.clone())
                .unwrap_or_default();
            let labels_json = serde_json::json!({
                "type": "compose_service",
                "project": project_name,
                "service": service_name,
                "depends_on": deps,
                "containers_count": s_containers.len(),
                "mop.managed": if managed { "true" } else { "false" },
            })
            .to_string();

            resources.push(Resource {
                id: format!("compose_service:{project_name}:{service_name}"),
                kind: ResourceKind::ComposeService,
                name: service_name.clone(),
                display_name: Some(format!("{project_name} / {service_name}")),
                group_name: Some(project_name.clone()),
                source: "compose".to_string(),
                labels_json: Some(labels_json),
                first_seen: now,
                last_seen: now,
            });
        }

        Ok(resources)
    }

    pub async fn get_resource_detail(&self, id: &str) -> Result<Option<ResourceDetail>, AppError> {
        let containers = self.fetch_compose_containers().await?;

        if let Some(project_name) = id.strip_prefix("compose_project:") {
            let p_containers: Vec<&ComposeContainerInfo> = containers
                .iter()
                .filter(|c| c.project == project_name)
                .collect();

            if p_containers.is_empty() {
                return Ok(None);
            }

            let statuses: Vec<ResourceStatus> = p_containers.iter().map(|c| c.status).collect();
            let status = aggregate_statuses(&statuses);
            let managed_count = p_containers.iter().filter(|c| c.is_managed).count();
            let labels_json = serde_json::json!({
                "project": project_name,
                "containers": p_containers.iter().map(|c| {
                    serde_json::json!({
                        "name": c.container_name,
                        "service": c.service,
                        "status": c.status.as_str(),
                        "is_managed": c.is_managed,
                    })
                }).collect::<Vec<_>>(),
                "managed_count": managed_count,
            })
            .to_string();

            return Ok(Some(ResourceDetail {
                resource: Resource {
                    id: id.to_string(),
                    kind: ResourceKind::ComposeProject,
                    name: project_name.to_string(),
                    display_name: Some(project_name.to_string()),
                    group_name: Some("Docker Compose".to_string()),
                    source: "compose".to_string(),
                    labels_json: Some(labels_json),
                    first_seen: Utc::now(),
                    last_seen: Utc::now(),
                },
                status,
                active_state: status.as_str().to_string(),
                sub_state: Some(format!(
                    "{}/{} managed running",
                    p_containers
                        .iter()
                        .filter(|c| c.is_managed && c.status == ResourceStatus::Running)
                        .count(),
                    managed_count
                )),
                uptime_secs: None,
                memory_bytes: None,
                cpu_percent: None,
            }));
        }

        if let Some(service_part) = id.strip_prefix("compose_service:") {
            let parts: Vec<&str> = service_part.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Ok(None);
            }
            let (project_name, service_name) = (parts[0], parts[1]);

            let s_containers: Vec<&ComposeContainerInfo> = containers
                .iter()
                .filter(|c| c.project == project_name && c.service == service_name)
                .collect();

            if s_containers.is_empty() {
                return Ok(None);
            }

            let statuses: Vec<ResourceStatus> = s_containers.iter().map(|c| c.status).collect();
            let status = aggregate_statuses(&statuses);
            let managed = s_containers.iter().any(|c| c.is_managed);
            let deps = s_containers
                .first()
                .map(|c| c.depends_on.clone())
                .unwrap_or_default();

            let labels_json = serde_json::json!({
                "project": project_name,
                "service": service_name,
                "depends_on": deps,
                "containers": s_containers.iter().map(|c| {
                    serde_json::json!({
                        "name": c.container_name,
                        "status": c.status.as_str(),
                        "is_managed": c.is_managed,
                    })
                }).collect::<Vec<_>>(),
                "is_managed": managed,
            })
            .to_string();

            return Ok(Some(ResourceDetail {
                resource: Resource {
                    id: id.to_string(),
                    kind: ResourceKind::ComposeService,
                    name: service_name.to_string(),
                    display_name: Some(format!("{project_name} / {service_name}")),
                    group_name: Some(project_name.to_string()),
                    source: "compose".to_string(),
                    labels_json: Some(labels_json),
                    first_seen: Utc::now(),
                    last_seen: Utc::now(),
                },
                status,
                active_state: status.as_str().to_string(),
                sub_state: Some(format!("depends_on: {:?}", deps)),
                uptime_secs: None,
                memory_bytes: None,
                cpu_percent: None,
            }));
        }

        Ok(None)
    }

    pub async fn execute_action(&self, id: &str, action: &str) -> Result<(), AppError> {
        let Some(docker) = &self.docker else {
            return Err(AppError::Internal(
                "Docker daemon is not available".to_string(),
            ));
        };

        let containers = self.fetch_compose_containers().await?;

        if let Some(service_part) = id.strip_prefix("compose_service:") {
            let parts: Vec<&str> = service_part.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(AppError::ResourceNotFound(id.to_string()));
            }
            let (project_name, service_name) = (parts[0], parts[1]);

            // Find managed containers for this service
            let managed_containers: Vec<&ComposeContainerInfo> = containers
                .iter()
                .filter(|c| c.project == project_name && c.service == service_name && c.is_managed)
                .collect();

            // Refuse action if 0 managed containers (SPEC §9.3 & 不変条件 3)
            if managed_containers.is_empty() {
                return Err(AppError::ActionNotAllowed(
                    action.to_string(),
                    format!("{id} has no managed containers (mop.managed=true required)"),
                ));
            }

            for c in &managed_containers {
                self.execute_container_action(docker, &c.container_name, action)
                    .await?;
                let _ = self.event_tx.send(ResourceEvent {
                    id: format!("docker:{}", c.container_name),
                    kind: ResourceKind::DockerContainer,
                    status: match action {
                        "start" => ResourceStatus::Running,
                        "stop" => ResourceStatus::Stopped,
                        "restart" => ResourceStatus::Restarting,
                        _ => ResourceStatus::Unknown,
                    },
                    ts: Utc::now(),
                    message: Some(format!("Executed {action} on {}", c.container_name)),
                });
            }

            let _ = self.event_tx.send(ResourceEvent {
                id: id.to_string(),
                kind: ResourceKind::ComposeService,
                status: match action {
                    "start" => ResourceStatus::Running,
                    "stop" => ResourceStatus::Stopped,
                    "restart" => ResourceStatus::Restarting,
                    _ => ResourceStatus::Unknown,
                },
                ts: Utc::now(),
                message: Some(format!("Executed {action} on service {service_name}")),
            });

            return Ok(());
        }

        if let Some(project_name) = id.strip_prefix("compose_project:") {
            let p_containers: Vec<&ComposeContainerInfo> = containers
                .iter()
                .filter(|c| c.project == project_name)
                .collect();

            if p_containers.is_empty() {
                return Err(AppError::ResourceNotFound(id.to_string()));
            }

            let managed_containers: Vec<&ComposeContainerInfo> =
                p_containers.into_iter().filter(|c| c.is_managed).collect();

            // Refuse action if 0 managed containers
            if managed_containers.is_empty() {
                return Err(AppError::ActionNotAllowed(
                    action.to_string(),
                    format!("{id} has no managed containers (mop.managed=true required)"),
                ));
            }

            // Build dependency graph of managed services
            let mut unique_services: Vec<String> = Vec::new();
            let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();

            for c in &managed_containers {
                if !unique_services.contains(&c.service) {
                    unique_services.push(c.service.clone());
                    deps_map.insert(c.service.clone(), c.depends_on.clone());
                }
            }

            let order = match action {
                "stop" => compute_stop_order(&unique_services, &deps_map),
                "start" | "restart" => compute_start_order(&unique_services, &deps_map),
                other => {
                    return Err(AppError::ActionNotAllowed(
                        other.to_string(),
                        id.to_string(),
                    ))
                }
            };

            for svc in order {
                for c in managed_containers.iter().filter(|c| c.service == svc) {
                    self.execute_container_action(docker, &c.container_name, action)
                        .await?;
                    let _ = self.event_tx.send(ResourceEvent {
                        id: format!("docker:{}", c.container_name),
                        kind: ResourceKind::DockerContainer,
                        status: match action {
                            "start" => ResourceStatus::Running,
                            "stop" => ResourceStatus::Stopped,
                            "restart" => ResourceStatus::Restarting,
                            _ => ResourceStatus::Unknown,
                        },
                        ts: Utc::now(),
                        message: Some(format!("Executed {action} on {}", c.container_name)),
                    });
                }
                let _ = self.event_tx.send(ResourceEvent {
                    id: format!("compose_service:{project_name}:{svc}"),
                    kind: ResourceKind::ComposeService,
                    status: match action {
                        "start" => ResourceStatus::Running,
                        "stop" => ResourceStatus::Stopped,
                        "restart" => ResourceStatus::Restarting,
                        _ => ResourceStatus::Unknown,
                    },
                    ts: Utc::now(),
                    message: Some(format!("Executed {action} on service {svc}")),
                });
            }

            let _ = self.event_tx.send(ResourceEvent {
                id: id.to_string(),
                kind: ResourceKind::ComposeProject,
                status: match action {
                    "start" => ResourceStatus::Running,
                    "stop" => ResourceStatus::Stopped,
                    "restart" => ResourceStatus::Restarting,
                    _ => ResourceStatus::Unknown,
                },
                ts: Utc::now(),
                message: Some(format!("Executed {action} on project {project_name}")),
            });

            return Ok(());
        }

        Err(AppError::ResourceNotFound(id.to_string()))
    }

    async fn execute_container_action(
        &self,
        docker: &Docker,
        container_name: &str,
        action: &str,
    ) -> Result<(), AppError> {
        match action {
            "start" => {
                docker
                    .start_container(container_name, None::<StartContainerOptions<String>>)
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to start container {container_name}: {e}"
                        ))
                    })?;
            }
            "stop" => {
                docker
                    .stop_container(container_name, None::<StopContainerOptions>)
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to stop container {container_name}: {e}"
                        ))
                    })?;
            }
            "restart" => {
                docker
                    .restart_container(container_name, None::<RestartContainerOptions>)
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to restart container {container_name}: {e}"
                        ))
                    })?;
            }
            other => {
                return Err(AppError::ActionNotAllowed(
                    other.to_string(),
                    container_name.to_string(),
                ))
            }
        }
        Ok(())
    }

    pub async fn get_logs(
        &self,
        id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<LogLine>, AppError> {
        let containers = self.fetch_compose_containers().await?;
        let target_containers: Vec<ComposeContainerInfo> =
            if let Some(service_part) = id.strip_prefix("compose_service:") {
                let parts: Vec<&str> = service_part.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return Err(AppError::ResourceNotFound(id.to_string()));
                }
                let (proj, svc) = (parts[0], parts[1]);
                containers
                    .into_iter()
                    .filter(|c| c.project == proj && c.service == svc)
                    .collect()
            } else if let Some(proj) = id.strip_prefix("compose_project:") {
                containers
                    .into_iter()
                    .filter(|c| c.project == proj)
                    .collect()
            } else {
                return Err(AppError::ResourceNotFound(id.to_string()));
            };

        if target_containers.is_empty() {
            let map = self.log_buffers.read().await;
            if let Some(buf) = map.get(id) {
                return Ok(buf.get_snapshot(tail, since).await);
            }
            return Ok(Vec::new());
        }

        let docker_map = self.docker_log_buffers.read().await;
        let mut merged_lines = Vec::new();

        for c in target_containers {
            let container_res_id = format!("docker:{}", c.container_name);
            if let Some(buf) = docker_map.get(&container_res_id) {
                let lines = buf.get_snapshot(tail, since).await;
                for line in lines {
                    let prefix = format!("[{}|{}]", c.service, c.container_name);
                    let line_content = if line.line.starts_with(&prefix) {
                        line.line
                    } else {
                        format!("{prefix} {}", line.line)
                    };
                    merged_lines.push(LogLine {
                        ts: line.ts,
                        stream: line.stream,
                        line: line_content,
                    });
                }
            }
        }

        // Also check if self buffer has any direct entries
        {
            let map = self.log_buffers.read().await;
            if let Some(buf) = map.get(id) {
                let lines = buf.get_snapshot(tail, since).await;
                merged_lines.extend(lines);
            }
        }

        merged_lines.sort_by_key(|l| l.ts);

        if merged_lines.len() > tail {
            let start = merged_lines.len() - tail;
            merged_lines = merged_lines[start..].to_vec();
        }

        Ok(merged_lines)
    }

    pub async fn subscribe_logs(
        &self,
        id: &str,
        _since: Option<DateTime<Utc>>,
    ) -> Result<broadcast::Receiver<LogLine>, AppError> {
        let (tx, rx) = broadcast::channel(512);
        let containers = self.fetch_compose_containers().await?;
        let target_containers: Vec<ComposeContainerInfo> =
            if let Some(service_part) = id.strip_prefix("compose_service:") {
                let parts: Vec<&str> = service_part.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return Err(AppError::ResourceNotFound(id.to_string()));
                }
                let (proj, svc) = (parts[0], parts[1]);
                containers
                    .into_iter()
                    .filter(|c| c.project == proj && c.service == svc)
                    .collect()
            } else if let Some(proj) = id.strip_prefix("compose_project:") {
                containers
                    .into_iter()
                    .filter(|c| c.project == proj)
                    .collect()
            } else {
                return Err(AppError::ResourceNotFound(id.to_string()));
            };

        let docker_map = self.docker_log_buffers.read().await;
        for c in target_containers {
            let container_res_id = format!("docker:{}", c.container_name);
            if let Some(buf) = docker_map.get(&container_res_id) {
                let mut container_rx = buf.subscribe();
                let tx_clone = tx.clone();
                let svc_name = c.service.clone();
                let cont_name = c.container_name.clone();
                tokio::spawn(async move {
                    while let Ok(line) = container_rx.recv().await {
                        let prefix = format!("[{svc_name}|{cont_name}]");
                        let line_content = if line.line.starts_with(&prefix) {
                            line.line
                        } else {
                            format!("{prefix} {}", line.line)
                        };
                        let _ = tx_clone.send(LogLine {
                            ts: line.ts,
                            stream: line.stream,
                            line: line_content,
                        });
                    }
                });
            }
        }

        // Also subscribe to self buffer if any direct lines pushed
        {
            let map = self.log_buffers.read().await;
            if let Some(buf) = map.get(id) {
                let mut self_rx = buf.subscribe();
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    while let Ok(line) = self_rx.recv().await {
                        let _ = tx_clone.send(line);
                    }
                });
            }
        }

        Ok(rx)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.event_tx.subscribe()
    }
}
