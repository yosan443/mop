use crate::handlers::auth::AppState;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Response, StatusCode},
    Json,
};
use mop_auth::{RequireAdmin, RequireAuth};
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::plugin::{PluginPermissionRecord, SettingsDiff};
use mop_core::models::JobStatus;
use mop_db::repos::{PluginPermissionsRepo, PluginRepo, PluginSettingsRepo};
use mop_plugin::rpc::UnixRpcClient;
use mop_plugin_sdk::{RpcError, RpcRequest, RpcResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

#[derive(Debug, Serialize)]
pub struct PluginDetailResponse {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub enabled: bool,
    pub state: String,
    pub installed_at: String,
    pub updated_at: String,
    pub manifest_json: Option<String>,
    pub permissions: Vec<PluginPermissionRecord>,
    pub applied_settings: Option<HashMap<String, Value>>,
}

async fn list_plugins_internal(
    state: &AppState,
) -> Result<Json<Vec<PluginDetailResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let plugin_repo = PluginRepo::new(state.pool.clone());
    let perms_repo = PluginPermissionsRepo::new(state.pool.clone());
    let settings_repo = PluginSettingsRepo::new(state.pool.clone());

    let plugins = plugin_repo.list_plugins().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(e)),
        )
    })?;

    let mut response = Vec::new();
    for p in plugins {
        let perms = perms_repo.list_permissions(&p.id).await.unwrap_or_default();
        let applied_settings = settings_repo
            .get_applied_settings(&p.id)
            .await
            .ok()
            .map(|s| s.into_iter().collect());

        response.push(PluginDetailResponse {
            id: p.id,
            name: p.name,
            version: p.version,
            api_version: "1".to_string(),
            enabled: p.enabled,
            state: p.state.to_string(),
            installed_at: p.installed_at.to_rfc3339(),
            updated_at: p
                .enabled_at
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| p.installed_at.to_rfc3339()),
            manifest_json: Some(p.manifest_json),
            permissions: perms,
            applied_settings,
        });
    }

    Ok(Json(response))
}

/// GET /api/v1/plugins (Viewer+)
pub async fn list_plugins(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
) -> Result<Json<Vec<PluginDetailResponse>>, (StatusCode, Json<ErrorResponse>)> {
    list_plugins_internal(&state).await
}

/// POST /api/v1/plugins/refresh (Admin)
pub async fn refresh_plugins(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
) -> Result<Json<Vec<PluginDetailResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let _ = state
        .plugin_supervisor
        .scan_and_register_plugins()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;

    list_plugins_internal(&state).await
}

/// POST /api/v1/plugins/{id}/enable (Admin)
pub async fn enable_plugin(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    state
        .plugin_supervisor
        .enable_plugin(&id, &admin.username)
        .await
        .map_err(|e| match e {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, Json(ErrorResponse::from(e))),
            _ => (StatusCode::BAD_REQUEST, Json(ErrorResponse::from(e))),
        })?;

    Ok(Json(serde_json::json!({ "status": "enabled", "id": id })))
}

/// POST /api/v1/plugins/{id}/disable (Admin)
pub async fn disable_plugin(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    state
        .plugin_supervisor
        .disable_plugin(&id)
        .await
        .map_err(|e| match e {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, Json(ErrorResponse::from(e))),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            ),
        })?;

    Ok(Json(serde_json::json!({ "status": "disabled", "id": id })))
}

#[derive(Debug, Serialize)]
pub struct PluginSettingsResponse {
    pub plugin_id: String,
    pub applied: HashMap<String, Value>,
    pub draft: HashMap<String, Value>,
    pub diff: SettingsDiff,
}

/// GET /api/v1/plugins/{id}/settings (Admin)
pub async fn get_plugin_settings(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
    Path(id): Path<String>,
) -> Result<Json<PluginSettingsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let settings_repo = PluginSettingsRepo::new(state.pool.clone());
    let applied = settings_repo
        .get_applied_settings(&id)
        .await
        .unwrap_or_default();
    let draft = settings_repo
        .get_draft_settings(&id)
        .await
        .unwrap_or_default();
    let diff = settings_repo
        .get_settings_diff(&id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::from(e))))?;

    Ok(Json(PluginSettingsResponse {
        plugin_id: id,
        applied: applied.into_iter().collect(),
        draft: draft.into_iter().collect(),
        diff,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SaveSettingsRequest {
    pub settings: HashMap<String, Value>,
}

/// PUT /api/v1/plugins/{id}/settings (Save Draft) (Admin)
pub async fn save_plugin_setting(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<String>,
    Json(payload): Json<SaveSettingsRequest>,
) -> Result<Json<SettingsDiff>, (StatusCode, Json<ErrorResponse>)> {
    let settings_repo = PluginSettingsRepo::new(state.pool.clone());
    let diff = settings_repo
        .save_draft_settings(&id, payload.settings, &admin.username)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::from(e))))?;

    Ok(Json(diff))
}

/// GET /api/v1/plugins/{id}/settings/diff (Admin)
pub async fn get_plugin_settings_diff(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
    Path(id): Path<String>,
) -> Result<Json<SettingsDiff>, (StatusCode, Json<ErrorResponse>)> {
    let settings_repo = PluginSettingsRepo::new(state.pool.clone());
    let diff = settings_repo
        .get_settings_diff(&id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::from(e))))?;

    Ok(Json(diff))
}

/// POST /api/v1/plugins/{id}/settings/apply (Admin)
pub async fn apply_plugin_settings(
    State(state): State<AppState>,
    RequireAdmin(_admin): RequireAdmin,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    let settings_repo = PluginSettingsRepo::new(state.pool.clone());
    let draft_settings = settings_repo.get_draft_settings(&id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(e)),
        )
    })?;

    let socket_path = state.plugin_supervisor.plugin_socket_path(&id);

    // 1. If plugin backend process is active (socket exists), run config.validate first
    if socket_path.exists() {
        let client = UnixRpcClient::new(&socket_path);
        let params = serde_json::to_value(&draft_settings).unwrap_or(serde_json::json!({}));
        match client.call("config.validate", Some(params)).await {
            Ok(res_val) => {
                if let Some(valid) = res_val.get("valid").and_then(|v| v.as_bool()) {
                    if !valid {
                        let msg = res_val
                            .get("message")
                            .and_then(|m| m.as_str())
                            .or_else(|| res_val.get("error").and_then(|e| e.as_str()))
                            .unwrap_or("Plugin validation rejected the draft settings");
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(ErrorResponse::from(AppError::Validation(msg.to_string()))),
                        ));
                    }
                }
            }
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::from(AppError::Validation(format!(
                        "Plugin config.validate RPC failed: {e}"
                    )))),
                ));
            }
        }
    }

    // 2. Promote draft settings to applied
    let new_applied = settings_repo
        .apply_draft_settings(&id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::from(e))))?;

    // 3. Forward config.apply RPC to running plugin process if socket exists
    if socket_path.exists() {
        let client = UnixRpcClient::new(&socket_path);
        let params = serde_json::to_value(&new_applied).unwrap_or(serde_json::json!({}));
        let _ = client.call("config.apply", Some(params)).await;
    }

    // 4. Gracefully restart plugin process
    state
        .plugin_supervisor
        .restart_plugin_process(&id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;

    Ok(Json(serde_json::json!({ "status": "applied", "id": id })))
}

/// POST /api/v1/plugins/{id}/rpc (Method-based RBAC)
pub async fn proxy_plugin_rpc(
    State(state): State<AppState>,
    RequireAuth(user): RequireAuth,
    Path(id): Path<String>,
    Json(req): Json<RpcRequest>,
) -> Result<Json<RpcResponse>, (StatusCode, Json<ErrorResponse>)> {
    // 1. Role-based Access Control per RPC method (SPEC §13.2 & M4 Requirements)
    match req.method.as_str() {
        "describe" | "doctor" | "config.schema" => {
            // Viewer+ allowed
        }
        "job.submit" | "job.cancel" => {
            if !user.role.can_operate() {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse::from(AppError::Forbidden(
                        "Operator role required for job RPC operations".into(),
                    ))),
                ));
            }
        }
        "config.validate" | "config.apply" => {
            if !user.role.can_administer() {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse::from(AppError::Forbidden(
                        "Admin role required for config RPC operations".into(),
                    ))),
                ));
            }
        }
        _ => {
            // Default to Operator+ for unknown / custom methods
            if !user.role.can_operate() {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse::from(AppError::Forbidden(
                        "Operator role required for this RPC method".into(),
                    ))),
                ));
            }
        }
    }

    // 2. Action Rate Limiting
    if let Err(e) = state.action_limiter.check(user.username.clone()).await {
        return Err((StatusCode::TOO_MANY_REQUESTS, Json(ErrorResponse::from(e))));
    }

    // 3. Connect to plugin unix socket & forward RPC request
    if req.method == "job.submit" {
        let params_val = req.params.as_ref();
        let kind = params_val
            .and_then(|p| p.get("job_type").or_else(|| p.get("kind")))
            .and_then(|k| k.as_str())
            .unwrap_or("hello.ping");

        // 3a. Verify capability: Check plugin_permissions for capability == "jobs"
        let perms_repo = PluginPermissionsRepo::new(state.pool.clone());
        let granted_perms = perms_repo.list_permissions(&id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;

        let mut allowed = false;
        for perm in &granted_perms {
            if perm.capability == "jobs" {
                if perm.value_json == kind {
                    allowed = true;
                    break;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&perm.value_json) {
                    if let Some(arr) = val.as_array() {
                        if arr.iter().any(|item| item.as_str() == Some(kind)) {
                            allowed = true;
                            break;
                        }
                    } else if val.as_str() == Some(kind) {
                        allowed = true;
                        break;
                    }
                }
            }
        }

        if !allowed {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::from(AppError::Forbidden(format!(
                    "CAPABILITY_REQUIRED: Job type '{kind}' is not granted in 'jobs' capability for plugin '{id}'"
                )))),
            ));
        }

        // 3b. Submit job to JobService
        let params_str = params_val
            .map(|p| serde_json::to_string(p).unwrap_or_default())
            .unwrap_or_else(|| "{}".to_string());
        let job = state
            .job_service
            .submit(kind, Some(&id), &params_str, &user.username)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::from(e)),
                )
            })?;

        // 3c. Inject job_id into params
        let mut req_clone = req.clone();
        if let Some(params_obj) = req_clone.params.as_mut().and_then(|p| p.as_object_mut()) {
            params_obj.insert(
                "job_id".to_string(),
                serde_json::Value::String(job.id.clone()),
            );
        } else {
            req_clone.params =
                Some(serde_json::json!({ "job_id": job.id.clone(), "job_type": kind }));
        }

        // 3d. Forward to plugin process
        let socket_path = state.plugin_supervisor.plugin_socket_path(&id);
        if !socket_path.exists() {
            let err_msg = format!("Plugin '{id}' process is not running");
            let _ = state
                .job_service
                .update_status(&job.id, JobStatus::Failed, Some(&err_msg))
                .await;
            return Ok(Json(RpcResponse::error(
                req.id,
                RpcError::internal_error(err_msg),
            )));
        }

        let client = UnixRpcClient::new(&socket_path);
        match client.call_raw(req_clone).await {
            Ok(rpc_res) => {
                if let Some(err) = &rpc_res.error {
                    let _ = state
                        .job_service
                        .update_status(&job.id, JobStatus::Failed, Some(&err.message))
                        .await;
                }
                Ok(Json(rpc_res))
            }
            Err(e) => {
                let err_msg = format!("Failed to forward job to plugin '{id}': {e}");
                let _ = state
                    .job_service
                    .update_status(&job.id, JobStatus::Failed, Some(&err_msg))
                    .await;
                Err((
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse::from(AppError::Plugin(err_msg))),
                ))
            }
        }
    } else {
        let socket_path = state.plugin_supervisor.plugin_socket_path(&id);
        if !socket_path.exists() {
            return Ok(Json(RpcResponse::error(
                req.id,
                RpcError::internal_error(format!("Plugin '{id}' process is not running")),
            )));
        }

        let client = UnixRpcClient::new(&socket_path);
        let rpc_res = client
            .call_raw(req.clone())
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse::from(e))))?;

        Ok(Json(rpc_res))
    }
}

/// GET /api/v1/plugins/{id}/ui/*file_path (Serve Static Plugin UI Assets)
pub async fn serve_plugin_ui(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
    Path((id, file_path)): Path<(String, String)>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponse>)> {
    let plugin_repo = PluginRepo::new(state.pool.clone());
    let plugin = plugin_repo
        .find_by_id(&id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::from(AppError::NotFound(format!(
                    "Plugin '{id}'"
                )))),
            )
        })?;

    // Determine plugin directory: {plugins_dir}/{id}/{version}/ or {plugins_dir}/{id}/
    let base_dir_ver = state
        .config
        .plugins
        .dir
        .join(&plugin.id)
        .join(&plugin.version);
    let plugin_dir = if base_dir_ver.is_dir() {
        base_dir_ver
    } else {
        state.config.plugins.dir.join(&plugin.id)
    };

    let ui_dir = if plugin_dir.join("ui").is_dir() {
        plugin_dir.join("ui")
    } else {
        plugin_dir.clone()
    };

    let canonical_base = ui_dir.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::from(AppError::NotFound(format!(
                "Plugin UI directory for '{id}'"
            )))),
        )
    })?;

    let clean_path = file_path.trim_start_matches('/');
    let stripped_path = clean_path.strip_prefix("ui/").unwrap_or(clean_path);
    let target_path = canonical_base.join(stripped_path);

    let canonical_target = target_path.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::from(AppError::NotFound(format!(
                "Asset '{clean_path}'"
            )))),
        )
    })?;

    // Security: Check prefix to prevent directory traversal
    if !canonical_target.starts_with(&canonical_base) {
        warn!(
            "Path traversal attempt blocked: base={}, target={}",
            canonical_base.display(),
            canonical_target.display()
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::from(AppError::Forbidden(
                "Access denied: path traversal prohibited".into(),
            ))),
        ));
    }

    if !canonical_target.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::from(AppError::NotFound(format!(
                "Asset '{clean_path}' is not a file"
            )))),
        ));
    }

    let contents = tokio::fs::read(&canonical_target).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(AppError::Internal(e.to_string()))),
        )
    })?;

    let mime = mime_guess::from_path(&canonical_target)
        .first_or_octet_stream()
        .to_string();

    let content_type = if mime.starts_with("text/") || mime == "application/javascript" {
        format!("{mime}; charset=utf-8")
    } else {
        mime
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(contents))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(AppError::Internal(e.to_string()))),
            )
        })
}
