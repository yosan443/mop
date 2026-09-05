use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use mop_plugin_sdk::{
    DescribeResult, InitializeParams, JobFinishedParams, JobLogParams, JobProgressParams,
    PluginJobInfo, RpcError, RpcNotification, RpcRequest, RpcResponse,
};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::config::VideoConfig;
use crate::doctor;
use crate::inspect;
use crate::video::{self, VideoConvertOptions, VideoResult};
use crate::watcher::ResidentWatcher;
use crate::worker::WorkerPool;

#[derive(Clone)]
pub struct PluginState {
    pub plugin_id: String,
    pub host_socket: PathBuf,
    pub config: Arc<RwLock<VideoConfig>>,
    pub worker_pool: Arc<RwLock<Option<Arc<WorkerPool>>>>,
    pub watcher: Arc<Mutex<Option<ResidentWatcher>>>,
    pub cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl PluginState {
    pub fn new(plugin_id: String, host_socket: PathBuf) -> Self {
        PluginState {
            plugin_id,
            host_socket,
            config: Arc::new(RwLock::new(VideoConfig::default())),
            worker_pool: Arc::new(RwLock::new(None)),
            watcher: Arc::new(Mutex::new(None)),
            cancels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub async fn handle_rpc_request(req: RpcRequest, state: &PluginState) -> RpcResponse {
    match req.method.as_str() {
        "initialize" => handle_initialize(req, state).await,
        "describe" => handle_describe(req),
        "doctor" => handle_doctor(req, state).await,
        "config.schema" => handle_config_schema(req),
        "config.validate" => handle_config_validate(req),
        "config.apply" => handle_config_apply(req, state).await,
        "job.submit" => handle_job_submit(req, state).await,
        "job.cancel" => handle_job_cancel(req, state),
        "shutdown" => {
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                std::process::exit(0);
            });
            RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }))
        }
        _ => RpcResponse::error(
            req.id,
            RpcError::method_not_found(format!("Unknown method '{}'", req.method)),
        ),
    }
}

async fn handle_initialize(req: RpcRequest, state: &PluginState) -> RpcResponse {
    let mut parsed_config = VideoConfig::default();

    if let Some(params) = req.params {
        if let Ok(init_params) = serde_json::from_value::<InitializeParams>(params) {
            if let Some(obj) = init_params.settings.as_object() {
                if !obj.is_empty() {
                    match serde_json::from_value::<VideoConfig>(init_params.settings.clone()) {
                        Ok(cfg) => parsed_config = cfg,
                        Err(e) => {
                            warn!(
                                "Failed to deserialize settings in initialize: {e}. Using default."
                            );
                        }
                    }
                }
            }
        }
    }

    info!(
        "Initializing {} with {} watch_dirs, video_dir={}",
        state.plugin_id,
        parsed_config.watch_dirs.len(),
        parsed_config.video_dir.display()
    );

    let cfg_arc = Arc::new(parsed_config);
    {
        let mut cfg_guard = state.config.write().await;
        *cfg_guard = (*cfg_arc).clone();
    }

    let pool = Arc::new(WorkerPool::new(cfg_arc.workers, Arc::clone(&cfg_arc)));
    {
        let mut pool_guard = state.worker_pool.write().await;
        *pool_guard = Some(Arc::clone(&pool));
    }

    match ResidentWatcher::start(Arc::clone(&cfg_arc), Arc::clone(&pool)) {
        Ok(w) => {
            let mut watcher_guard = state.watcher.lock().unwrap();
            *watcher_guard = Some(w);
            info!("Video ResidentWatcher initialized successfully.");
        }
        Err(e) => {
            warn!("Failed to start Video ResidentWatcher during initialize: {e}");
        }
    }

    RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }))
}

fn handle_describe(req: RpcRequest) -> RpcResponse {
    let desc = DescribeResult {
        jobs: vec![
            PluginJobInfo {
                kind: "video.convert".to_string(),
                title: "Convert Video".to_string(),
                description: Some("Transcode video or video archive to HEVC MP4".to_string()),
            },
            PluginJobInfo {
                kind: "video.batch".to_string(),
                title: "Batch Transcode Directory".to_string(),
                description: Some("Batch transcode all videos in a directory".to_string()),
            },
            PluginJobInfo {
                kind: "video.inspect".to_string(),
                title: "Inspect Video".to_string(),
                description: Some("Inspect codecs, streams, and resolution of a video".to_string()),
            },
        ],
        ui_meta: Some(serde_json::json!({
            "entry": "ui/index.js",
            "element": "mop-plugin-video",
        })),
    };

    RpcResponse::success(
        req.id,
        serde_json::to_value(desc).unwrap_or(serde_json::json!({})),
    )
}

async fn handle_doctor(req: RpcRequest, state: &PluginState) -> RpcResponse {
    let cfg = {
        let guard = state.config.read().await;
        (*guard).clone()
    };
    let doc = doctor::doctor(&cfg);
    RpcResponse::success(
        req.id,
        serde_json::to_value(doc).unwrap_or(serde_json::json!({})),
    )
}

fn handle_config_schema(req: RpcRequest) -> RpcResponse {
    RpcResponse::success(req.id, VideoConfig::json_schema())
}

fn handle_config_validate(req: RpcRequest) -> RpcResponse {
    let params = match req.params {
        Some(p) => p,
        None => {
            return RpcResponse::error(req.id, RpcError::invalid_params("Missing params"));
        }
    };

    let cfg: VideoConfig = match serde_json::from_value(params) {
        Ok(c) => c,
        Err(e) => {
            return RpcResponse::success(
                req.id,
                serde_json::json!({
                    "valid": false,
                    "errors": [format!("Schema validation error: {e}")]
                }),
            );
        }
    };

    match cfg.validate_layout() {
        Ok(()) => RpcResponse::success(req.id, serde_json::json!({ "valid": true, "errors": [] })),
        Err(e) => RpcResponse::success(
            req.id,
            serde_json::json!({
                "valid": false,
                "message": e.to_string(),
                "errors": [e]
            }),
        ),
    }
}

async fn handle_config_apply(req: RpcRequest, state: &PluginState) -> RpcResponse {
    let params = match req.params {
        Some(p) => p,
        None => {
            return RpcResponse::error(req.id, RpcError::invalid_params("Missing params"));
        }
    };

    let cfg: VideoConfig = match serde_json::from_value(params) {
        Ok(c) => c,
        Err(e) => {
            return RpcResponse::error(
                req.id,
                RpcError::invalid_params(format!("Invalid config parameters: {e}")),
            );
        }
    };

    if let Err(e) = cfg.validate_layout() {
        return RpcResponse::error(
            req.id,
            RpcError::invalid_params(format!("Layout validation failed: {e}")),
        );
    }

    {
        let mut guard = state.config.write().await;
        *guard = cfg;
    }

    RpcResponse::success(req.id, serde_json::json!({ "status": "ok" }))
}

async fn handle_job_submit(req: RpcRequest, state: &PluginState) -> RpcResponse {
    let params = req.params.clone().unwrap_or(serde_json::json!({}));
    let job_id = params
        .get("job_id")
        .and_then(|j| j.as_str())
        .unwrap_or("unknown")
        .to_string();

    let job_type = params
        .get("job_type")
        .or_else(|| params.get("kind"))
        .and_then(|j| j.as_str())
        .unwrap_or("")
        .to_string();

    if job_type.is_empty() {
        return RpcResponse::error(
            req.id,
            RpcError::invalid_params("Missing 'job_type' or 'kind'"),
        );
    }

    let cancel_token = Arc::new(AtomicBool::new(false));
    {
        let mut cancels = state.cancels.lock().unwrap();
        cancels.insert(job_id.clone(), Arc::clone(&cancel_token));
    }

    let state_clone = state.clone();
    let job_id_clone = job_id.clone();
    let job_params = params.get("params").cloned().unwrap_or(params.clone());

    tokio::spawn(async move {
        match job_type.as_str() {
            "video.convert" => {
                run_convert_job(state_clone, job_id_clone, job_params, cancel_token).await;
            }
            "video.batch" => {
                run_batch_job(state_clone, job_id_clone, job_params, cancel_token).await;
            }
            "video.inspect" => {
                run_inspect_job(state_clone, job_id_clone, job_params).await;
            }
            other => {
                warn!("Unknown job_type: {other}");
                let _ = send_finished(
                    &state_clone.host_socket,
                    &job_id_clone,
                    "failed",
                    Some(format!("Unknown job_type: {other}")),
                )
                .await;
            }
        }
    });

    RpcResponse::success(
        req.id,
        serde_json::json!({
            "accepted": true,
            "job_id": job_id,
        }),
    )
}

fn handle_job_cancel(req: RpcRequest, state: &PluginState) -> RpcResponse {
    let params = req.params.unwrap_or(serde_json::json!({}));
    let job_id = params.get("job_id").and_then(|j| j.as_str()).unwrap_or("");

    let cancels = state.cancels.lock().unwrap();
    if let Some(token) = cancels.get(job_id) {
        token.store(true, Ordering::SeqCst);
        RpcResponse::success(req.id, serde_json::json!({ "status": "canceled" }))
    } else {
        RpcResponse::error(
            req.id,
            RpcError::invalid_params(format!("No running job with id '{job_id}'")),
        )
    }
}

async fn run_convert_job(
    state: PluginState,
    job_id: String,
    params: serde_json::Value,
    _cancel_token: Arc<AtomicBool>,
) {
    let host_socket = state.host_socket.clone();

    let input = match params.get("input").and_then(|p| p.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            let msg = "Missing 'input' parameter".to_string();
            let _ = send_log(&host_socket, &job_id, "error", msg.clone()).await;
            let _ = send_finished(&host_socket, &job_id, "failed", Some(msg)).await;
            return;
        }
    };

    let password = params
        .get("password")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    let dry_run = params
        .get("dry_run")
        .and_then(|d| d.as_bool())
        .unwrap_or(false);

    let mut cfg = {
        let guard = state.config.read().await;
        (*guard).clone()
    };
    if let Some(crf) = params.get("crf").and_then(|q| q.as_u64()) {
        cfg.crf = crf as u32;
    }
    if let Some(preset) = params.get("preset").and_then(|p| p.as_str()) {
        cfg.preset = preset.to_string();
    }
    if let Some(del) = params.get("delete_original").and_then(|d| d.as_bool()) {
        cfg.delete_original = del;
    }

    let _ = send_progress(
        &host_socket,
        &job_id,
        10,
        "Starting transcode...".to_string(),
    )
    .await;
    let _ = send_log(
        &host_socket,
        &job_id,
        "info",
        format!("Starting conversion of {}", input.display()),
    )
    .await;

    let opts = VideoConvertOptions {
        password,
        dry_run,
        keep_work_dir_on_error: false,
    };

    let input_clone = input.clone();
    let cfg_clone = cfg.clone();

    let res =
        tokio::task::spawn_blocking(move || video::convert_one(&input_clone, &cfg_clone, &opts))
            .await
            .unwrap_or_else(|e| VideoResult {
                status: "failed",
                reason: None,
                output: None,
                outputs: Vec::new(),
                error: Some(format!("Join error: {e}")),
            });

    let log_line = format_video_log(
        &res,
        params.get("input").and_then(|p| p.as_str()).unwrap_or(""),
    );
    let level = if res.status == "failed" {
        "error"
    } else {
        "info"
    };
    let _ = send_log(&host_socket, &job_id, level, log_line).await;

    if res.status == "success" {
        let _ = send_progress(&host_socket, &job_id, 100, "Transcode finished".to_string()).await;
        let _ = send_finished(&host_socket, &job_id, "completed", None).await;
    } else if res.status == "skipped" {
        let _ = send_progress(&host_socket, &job_id, 100, "File skipped".to_string()).await;
        let _ = send_finished(&host_socket, &job_id, "completed", None).await;
    } else {
        let _ = send_progress(&host_socket, &job_id, 100, "Transcode failed".to_string()).await;
        let _ = send_finished(
            &host_socket,
            &job_id,
            "failed",
            res.error.or_else(|| Some("Conversion failed".to_string())),
        )
        .await;
    }
}

async fn run_batch_job(
    state: PluginState,
    job_id: String,
    params: serde_json::Value,
    cancel_token: Arc<AtomicBool>,
) {
    let host_socket = state.host_socket.clone();

    let dir = match params.get("dir").and_then(|p| p.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            let msg = "Missing 'dir' parameter".to_string();
            let _ = send_log(&host_socket, &job_id, "error", msg.clone()).await;
            let _ = send_finished(&host_socket, &job_id, "failed", Some(msg)).await;
            return;
        }
    };

    if !dir.is_dir() {
        let msg = format!("Target path is not a directory: {}", dir.display());
        let _ = send_log(&host_socket, &job_id, "error", msg.clone()).await;
        let _ = send_finished(&host_socket, &job_id, "failed", Some(msg)).await;
        return;
    }

    let password = params
        .get("password")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    let dry_run = params
        .get("dry_run")
        .and_then(|d| d.as_bool())
        .unwrap_or(false);

    let cfg = {
        let guard = state.config.read().await;
        (*guard).clone()
    };

    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&dir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if p.is_file() {
            files.push(p.to_path_buf());
        }
    }

    let total = files.len();
    let _ = send_progress(
        &host_socket,
        &job_id,
        0,
        format!("Found {total} files to process"),
    )
    .await;

    let mut processed = 0;
    for file in files {
        if cancel_token.load(Ordering::SeqCst) {
            let _ = send_log(
                &host_socket,
                &job_id,
                "warn",
                "Batch canceled by user".to_string(),
            )
            .await;
            let _ = send_finished(&host_socket, &job_id, "canceled", None).await;
            return;
        }

        let opts = VideoConvertOptions {
            password: password.clone(),
            keep_work_dir_on_error: false,
            dry_run,
        };
        let cfg_clone = cfg.clone();
        let file_clone = file.clone();

        let res = tokio::task::spawn_blocking(move || {
            crate::dispatch::process(&file_clone, &cfg_clone, &opts)
        })
        .await
        .unwrap_or_else(|e| VideoResult {
            status: "failed",
            reason: None,
            output: None,
            outputs: Vec::new(),
            error: Some(format!("Task panic: {e}")),
        });

        processed += 1;
        let pct = ((processed as f64 / total.max(1) as f64) * 100.0) as u8;

        let log_line = format_video_log(&res, &file.display().to_string());
        let level = if res.status == "failed" {
            "error"
        } else {
            "info"
        };
        let _ = send_log(&host_socket, &job_id, level, log_line).await;
        let _ = send_progress(
            &host_socket,
            &job_id,
            pct,
            format!("Processed {processed}/{total}"),
        )
        .await;
    }

    let _ = send_finished(&host_socket, &job_id, "completed", None).await;
}

async fn run_inspect_job(state: PluginState, job_id: String, params: serde_json::Value) {
    let host_socket = state.host_socket.clone();

    let input = match params.get("input").and_then(|p| p.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            let msg = "Missing 'input' parameter".to_string();
            let _ = send_log(&host_socket, &job_id, "error", msg.clone()).await;
            let _ = send_finished(&host_socket, &job_id, "failed", Some(msg)).await;
            return;
        }
    };

    let password = params
        .get("password")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());

    let _ = send_progress(
        &host_socket,
        &job_id,
        20,
        format!("Inspecting {}", input.display()),
    )
    .await;

    let input_clone = input.clone();
    let inspect_res =
        tokio::task::spawn_blocking(move || inspect::inspect(&input_clone, password.as_deref()))
            .await;

    match inspect_res {
        Ok(Ok(info)) => {
            let msg = format!(
                "Format: {}, video_codec: {:?}, audio_codec: {:?}, resolution: {:?}, size: {} bytes",
                info.format,
                info.video_codec,
                info.audio_codec,
                info.resolution,
                info.size_bytes
            );
            let _ = send_log(&host_socket, &job_id, "info", msg).await;

            for detail in &info.details {
                let _ = send_log(&host_socket, &job_id, "info", format!(" - {detail}")).await;
            }

            let _ = send_progress(
                &host_socket,
                &job_id,
                100,
                "Inspection completed successfully".to_string(),
            )
            .await;
            let _ = send_finished(&host_socket, &job_id, "completed", None).await;
        }
        Ok(Err(e)) => {
            let msg = format!("Failed to inspect: {e}");
            let _ = send_log(&host_socket, &job_id, "error", msg.clone()).await;
            let _ = send_finished(&host_socket, &job_id, "failed", Some(msg)).await;
        }
        Err(e) => {
            let msg = format!("Inspect thread panicked: {e}");
            let _ = send_log(&host_socket, &job_id, "error", msg.clone()).await;
            let _ = send_finished(&host_socket, &job_id, "failed", Some(msg)).await;
        }
    }
}

fn format_video_log(res: &VideoResult, input: &str) -> String {
    let mut parts = Vec::new();
    parts.push(format!("status={}", res.status));
    parts.push(format!("input={input}"));
    if let Some(r) = res.reason {
        parts.push(format!("reason={}", r.as_str()));
    }
    if let Some(out) = &res.output {
        parts.push(format!("output={}", out.display()));
    }
    if let Some(err) = &res.error {
        parts.push(format!("error=\"{err}\""));
    }
    parts.join(" ")
}

async fn send_progress(
    host_socket: &Path,
    job_id: &str,
    percent: u8,
    message: String,
) -> Result<(), ()> {
    if let Ok(mut stream) = UnixStream::connect(host_socket).await {
        let notif = RpcNotification::new(
            "job.progress",
            Some(
                serde_json::to_value(JobProgressParams {
                    job_id: job_id.to_string(),
                    percent,
                    message,
                })
                .unwrap(),
            ),
        );
        let bytes = serde_json::to_vec(&notif).unwrap();
        let _ = stream.write_all(&bytes).await;
        let _ = stream.write_all(b"\n").await;
    }
    Ok(())
}

async fn send_log(
    host_socket: &Path,
    job_id: &str,
    level: &str,
    message: String,
) -> Result<(), ()> {
    if let Ok(mut stream) = UnixStream::connect(host_socket).await {
        let notif = RpcNotification::new(
            "job.log",
            Some(
                serde_json::to_value(JobLogParams {
                    job_id: job_id.to_string(),
                    level: level.to_string(),
                    message,
                    ts: Utc::now(),
                })
                .unwrap(),
            ),
        );
        let bytes = serde_json::to_vec(&notif).unwrap();
        let _ = stream.write_all(&bytes).await;
        let _ = stream.write_all(b"\n").await;
    }
    Ok(())
}

async fn send_finished(
    host_socket: &Path,
    job_id: &str,
    status: &str,
    error: Option<String>,
) -> Result<(), ()> {
    if let Ok(mut stream) = UnixStream::connect(host_socket).await {
        let notif = RpcNotification::new(
            "job.finished",
            Some(
                serde_json::to_value(JobFinishedParams {
                    job_id: job_id.to_string(),
                    status: status.to_string(),
                    error,
                })
                .unwrap(),
            ),
        );
        let bytes = serde_json::to_vec(&notif).unwrap();
        let _ = stream.write_all(&bytes).await;
        let _ = stream.write_all(b"\n").await;
    }
    Ok(())
}
