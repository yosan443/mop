use mop_plugin_sdk::{
    DescribeResult, DoctorCheck, DoctorResult, InitializeParams, JobFinishedParams, JobLogParams,
    JobProgressParams, PluginJobInfo, RpcError, RpcNotification, RpcRequest, RpcResponse,
};
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Clone)]
struct PluginState {
    #[allow(dead_code)]
    plugin_id: String,
    host_socket: PathBuf,
    greeting: Arc<RwLock<String>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mop_plugin_hello=info".into()),
        )
        .init();

    let plugin_id = env::var("MOP_PLUGIN_ID").unwrap_or_else(|_| "mop.hello".to_string());
    let socket_path = env::var("MOP_PLUGIN_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/mop/plugins/mop.hello.sock"));
    let host_socket = env::var("MOP_HOST_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/mop/host.sock"));

    info!("Starting {} on socket {}", plugin_id, socket_path.display());

    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = UnixListener::bind(&socket_path)?;
    let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660));

    let state = PluginState {
        plugin_id,
        host_socket,
        greeting: Arc::new(RwLock::new("Hello from mop plugin!".to_string())),
    };

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state).await {
                        error!("Connection error: {e}");
                    }
                });
            }
            Err(e) => {
                warn!("Accept error: {e}");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    state: PluginState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let res = RpcResponse::error(None, RpcError::parse_error(e.to_string()));
                let bytes = serde_json::to_vec(&res)?;
                writer.write_all(&bytes).await?;
                writer.write_all(b"\n").await?;
                continue;
            }
        };

        let response = match req.method.as_str() {
            "initialize" => {
                if let Some(params) = req.params {
                    if let Ok(init_params) = serde_json::from_value::<InitializeParams>(params) {
                        if let Some(greeting) = init_params
                            .settings
                            .get("greeting")
                            .and_then(|g| g.as_str())
                        {
                            let mut g = state.greeting.write().await;
                            *g = greeting.to_string();
                        }
                    }
                }
                RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }))
            }
            "describe" => {
                let desc = DescribeResult {
                    jobs: vec![PluginJobInfo {
                        kind: "hello.ping".to_string(),
                        title: "Hello Ping Job".to_string(),
                        description: Some("Ping test job for hello plugin".to_string()),
                    }],
                    ui_meta: Some(serde_json::json!({
                        "entry": "ui/index.js",
                        "element": "mop-plugin-hello",
                    })),
                };
                RpcResponse::success(
                    req.id,
                    serde_json::to_value(&desc).unwrap_or(serde_json::json!({})),
                )
            }
            "doctor" => {
                let doc = DoctorResult {
                    checks: vec![DoctorCheck {
                        name: "runtime".to_string(),
                        status: "ok".to_string(),
                        message: "Hello plugin backend is running smoothly".to_string(),
                    }],
                };
                RpcResponse::success(
                    req.id,
                    serde_json::to_value(&doc).unwrap_or(serde_json::json!({})),
                )
            }
            "config.schema" => {
                let schema = serde_json::json!({
                    "type": "object",
                    "properties": {
                        "greeting": {
                            "type": "string",
                            "title": "Greeting Message",
                            "default": "Hello from mop plugin!"
                        }
                    }
                });
                RpcResponse::success(req.id, schema)
            }
            "config.validate" => RpcResponse::success(req.id, serde_json::json!({ "valid": true })),
            "config.apply" => {
                if let Some(params) = req.params {
                    if let Some(greeting) = params.get("greeting").and_then(|g| g.as_str()) {
                        let mut g = state.greeting.write().await;
                        *g = greeting.to_string();
                    }
                }
                RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }))
            }
            "job.submit" => {
                let params = req.params.clone().unwrap_or(serde_json::json!({}));
                let job_id = params
                    .get("job_id")
                    .and_then(|j| j.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let job_type = params
                    .get("job_type")
                    .and_then(|j| j.as_str())
                    .unwrap_or("hello.ping")
                    .to_string();

                let state_clone = state.clone();
                let job_id_clone = job_id.clone();
                tokio::spawn(async move {
                    run_hello_job(state_clone, job_id_clone, job_type).await;
                });

                RpcResponse::success(
                    req.id,
                    serde_json::json!({ "job_id": job_id, "status": "accepted" }),
                )
            }
            "job.cancel" => {
                RpcResponse::success(req.id, serde_json::json!({ "status": "canceled" }))
            }
            "shutdown" => {
                let res = RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }));
                let bytes = serde_json::to_vec(&res)?;
                writer.write_all(&bytes).await?;
                writer.write_all(b"\n").await?;
                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    std::process::exit(0);
                });
                return Ok(());
            }
            _ => RpcResponse::error(
                req.id,
                RpcError::method_not_found(format!("Unknown method '{}'", req.method)),
            ),
        };

        let bytes = serde_json::to_vec(&response)?;
        writer.write_all(&bytes).await?;
        writer.write_all(b"\n").await?;
    }

    Ok(())
}

async fn run_hello_job(state: PluginState, job_id: String, _job_type: String) {
    let host_socket = state.host_socket.clone();
    let current_greeting = {
        let g = state.greeting.read().await;
        g.clone()
    };
    info!(
        "run_hello_job started for job_id={job_id} on host_socket={}",
        host_socket.display()
    );

    if let Ok(mut stream) = UnixStream::connect(&host_socket).await {
        // 1. Progress 25%
        let notif1 = RpcNotification::new(
            "job.progress",
            Some(
                serde_json::to_value(JobProgressParams {
                    job_id: job_id.clone(),
                    percent: 25,
                    message: "Starting hello ping...".to_string(),
                })
                .unwrap(),
            ),
        );
        let _ = send_notif(&mut stream, notif1).await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 2. Log message
        let log_notif = RpcNotification::new(
            "job.log",
            Some(
                serde_json::to_value(JobLogParams {
                    job_id: job_id.clone(),
                    level: "info".to_string(),
                    message: format!("Greeting configured: {current_greeting}"),
                    ts: chrono::Utc::now(),
                })
                .unwrap(),
            ),
        );
        let _ = send_notif(&mut stream, log_notif).await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 3. Progress 75%
        let notif2 = RpcNotification::new(
            "job.progress",
            Some(
                serde_json::to_value(JobProgressParams {
                    job_id: job_id.clone(),
                    percent: 75,
                    message: "Processing response...".to_string(),
                })
                .unwrap(),
            ),
        );
        let _ = send_notif(&mut stream, notif2).await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 4. Finished (Success)
        let finished_notif = RpcNotification::new(
            "job.finished",
            Some(
                serde_json::to_value(JobFinishedParams {
                    job_id: job_id.clone(),
                    status: "completed".to_string(),
                    error: None,
                })
                .unwrap(),
            ),
        );
        let _ = send_notif(&mut stream, finished_notif).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    } else {
        warn!(
            "Failed to connect to host socket at {}",
            host_socket.display()
        );
    }
}

async fn send_notif(
    stream: &mut UnixStream,
    notif: RpcNotification,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bytes = serde_json::to_vec(&notif)?;
    stream.write_all(&bytes).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    Ok(())
}
