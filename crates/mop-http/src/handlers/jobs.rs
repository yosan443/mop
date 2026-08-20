use crate::handlers::auth::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::StreamExt;
use mop_auth::RequireAuth;
use mop_core::error::{AppError, ErrorResponse};
use mop_core::models::{Job, JobEvent};
use serde::Serialize;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Debug, Serialize)]
pub struct JobDetailResponse {
    pub job: Job,
    pub events: Vec<JobEvent>,
}

pub async fn list_jobs(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
) -> Result<Json<Vec<Job>>, (StatusCode, Json<ErrorResponse>)> {
    let jobs = state.job_service.list().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(e)),
        )
    })?;
    Ok(Json(jobs))
}

pub async fn get_job(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<JobDetailResponse>, (StatusCode, Json<ErrorResponse>)> {
    let job = state
        .job_service
        .get(&id)
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
                Json(ErrorResponse::from(AppError::NotFound(format!("Job {id}")))),
            )
        })?;

    let events = state.job_service.get_events(&id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::from(e)),
        )
    })?;

    Ok(Json(JobDetailResponse { job, events }))
}

pub async fn stream_jobs(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.job_service.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|res| async move {
        match res {
            Ok(job) => {
                let json = serde_json::to_string(&job).ok()?;
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
}
