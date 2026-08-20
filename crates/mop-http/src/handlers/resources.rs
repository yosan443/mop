use crate::handlers::auth::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use mop_auth::{RequireAuth, RequireOperator};
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::{AuditResult, JobStatus, Resource};
use mop_jobs::{AuditLogger, AuditParams};
use mop_watch::{LogLine, ResourceDetail};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub tail: Option<usize>,
    pub since: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct ActionRequest {
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct ActionAcceptedResponse {
    pub job_id: String,
    pub status: String,
}

pub async fn list_resources(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
) -> Result<Json<Vec<Resource>>, (StatusCode, Json<ErrorResponse>)> {
    let resources = state.collector.list_resources().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(e)),
        )
    })?;
    Ok(Json(resources))
}

pub async fn get_resource_detail(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<ResourceDetail>, (StatusCode, Json<ErrorResponse>)> {
    let detail = state
        .collector
        .get_resource_detail(&id)
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
                Json(ErrorResponse::from(AppError::ResourceNotFound(id.clone()))),
            )
        })?;

    Ok(Json(detail))
}

pub async fn get_resource_logs(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
    Path(id): Path<String>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<Vec<LogLine>>, (StatusCode, Json<ErrorResponse>)> {
    let tail = query.tail.unwrap_or(500).min(5000);
    let logs = state
        .collector
        .get_logs(&id, tail, query.since)
        .await
        .map_err(|e| {
            let status = match e {
                AppError::ResourceNotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(ErrorResponse::from(e)))
        })?;

    Ok(Json(logs))
}

pub async fn stream_resource_logs(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
    Path(id): Path<String>,
    Query(query): Query<LogsQuery>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let rx = state
        .collector
        .subscribe_logs(&id, query.since)
        .await
        .map_err(|e| {
            let status = match e {
                AppError::ResourceNotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(ErrorResponse::from(e)))
        })?;

    let stream = BroadcastStream::new(rx).filter_map(|res| async move {
        match res {
            Ok(log_line) => {
                let json = serde_json::to_string(&log_line).ok()?;
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15))))
}

pub async fn execute_resource_action(
    State(state): State<AppState>,
    RequireOperator(user): RequireOperator,
    Path(id): Path<String>,
    Json(payload): Json<ActionRequest>,
) -> Result<(StatusCode, Json<ActionAcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Validate action allowed
    match payload.action.as_str() {
        "start" | "stop" | "restart" => {}
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::from(AppError::ActionNotAllowed(
                    other.to_string(),
                    id,
                ))),
            ));
        }
    }

    // Verify resource exists
    let detail = state
        .collector
        .get_resource_detail(&id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;
    if detail.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::from(AppError::ResourceNotFound(id))),
        ));
    }

    let params_json = serde_json::json!({
        "resource_id": id,
        "action": payload.action,
    })
    .to_string();

    let job = state
        .job_service
        .submit("resource.action", None, &params_json, &user.username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::from(e)),
            )
        })?;

    // Spawn execution
    let job_id = job.id.clone();
    let job_service = state.job_service.clone();
    let pool = state.pool.clone();
    let collector = state.collector.clone();
    let res_id = id.clone();
    let action = payload.action.clone();
    let user_id = user.id.clone();
    let username = user.username.clone();

    tokio::spawn(async move {
        let _ = job_service
            .update_status(&job_id, JobStatus::Running, None)
            .await;
        let _ = job_service
            .append_event(
                &job_id,
                1,
                "INFO",
                &format!("Executing {action} on {res_id}"),
                None,
            )
            .await;

        let result = collector.execute_action(&res_id, &action).await;

        let kind_str = if res_id.starts_with("systemd:") {
            "systemd"
        } else {
            "docker"
        };
        let action_name = format!("resource.{action}");

        match result {
            Ok(()) => {
                let _ = job_service
                    .append_event(
                        &job_id,
                        2,
                        "INFO",
                        &format!("{action} completed successfully"),
                        None,
                    )
                    .await;
                let _ = job_service
                    .update_status(&job_id, JobStatus::Succeeded, None)
                    .await;

                let detail = format!(r#"{{"job_id":"{job_id}","action":"{action}"}}"#);
                let _ = AuditLogger::log(
                    &pool,
                    AuditParams {
                        user_id: Some(&user_id),
                        username: Some(&username),
                        action: &action_name,
                        resource_kind: Some(kind_str),
                        resource_id: Some(&res_id),
                        detail_json: Some(&detail),
                        result: AuditResult::Success,
                    },
                )
                .await;
            }
            Err(e) => {
                let err_msg = e.to_string();
                let _ = job_service
                    .append_event(
                        &job_id,
                        2,
                        "ERROR",
                        &format!("Action failed: {err_msg}"),
                        None,
                    )
                    .await;
                let _ = job_service
                    .update_status(&job_id, JobStatus::Failed, Some(&err_msg))
                    .await;

                let detail =
                    format!(r#"{{"job_id":"{job_id}","action":"{action}","error":"{err_msg}"}}"#);
                let _ = AuditLogger::log(
                    &pool,
                    AuditParams {
                        user_id: Some(&user_id),
                        username: Some(&username),
                        action: &action_name,
                        resource_kind: Some(kind_str),
                        resource_id: Some(&res_id),
                        detail_json: Some(&detail),
                        result: AuditResult::Failure,
                    },
                )
                .await;
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(ActionAcceptedResponse {
            job_id: job.id,
            status: "queued".to_string(),
        }),
    ))
}
