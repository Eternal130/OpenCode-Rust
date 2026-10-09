//! Server-level handlers: `/api/info`, pairing (`POST /api/pair`,
//! `GET /auth/connect/:code`), and the event SSE feed — all thin
//! delegation to the app-shell lane's primitives (info.rs / pairing.rs /
//! sse.rs), which own the wire shapes. Kept here so the standalone
//! `handlers::mount` table covers every operation.

#![allow(clippy::must_use_candidate)]
use std::sync::{Arc, OnceLock};

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;

use super::common::{ApiError, SharedState};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/event", axum::routing::get(event_subscribe))
        .route("/api/info", axum::routing::get(info))
        .route("/api/pair", axum::routing::post(pair))
        .route("/auth/connect/:code", axum::routing::get(connect))
}

async fn info(State(state): State<SharedState>) -> Response {
    crate::info::info_response(&state)
}

async fn pair(State(state): State<SharedState>) -> Result<Response, ApiError> {
    Ok(crate::pairing::pair_response(&state))
}

async fn connect(
    State(state): State<SharedState>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Response {
    crate::pairing::connect_response(&state, &code, &headers)
}

/// The process-wide feed for standalone mounts. The app-shell's own
/// dispatch serves `/api/event` through its `AppShell.feed`; this tap
/// exists so `handlers::mount` alone also serves a live feed.
fn shared_feed(state: &SharedState) -> Arc<crate::sse::EventFeed> {
    static FEED: OnceLock<Arc<crate::sse::EventFeed>> = OnceLock::new();
    FEED.get_or_init(|| {
        let feed = Arc::new(crate::sse::EventFeed::new());
        feed.spawn_tap(&state.engine.bus);
        feed
    })
    .clone()
}

async fn event_subscribe(State(state): State<SharedState>) -> Response {
    crate::sse::event_response(&state, &shared_feed(&state), crate::sse::HEARTBEAT_INTERVAL)
}
