use crate::handlers::auth::AppState;
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::StreamExt;
use mop_auth::RequireAuth;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

pub async fn stream_events(
    State(state): State<AppState>,
    RequireAuth(_user): RequireAuth,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.collector.subscribe_events();
    let stream = BroadcastStream::new(rx).filter_map(|res| async move {
        match res {
            Ok(evt) => {
                let json = serde_json::to_string(&evt).ok()?;
                Some(Ok(Event::default().event("resource_event").data(json)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
}
