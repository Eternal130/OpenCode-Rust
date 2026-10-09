//! Location + debug handlers: the location info view, reload, and the
//! debug location inspect/evict pair (deep-map §1).

#![allow(clippy::must_use_candidate)]
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{no_content, resolve_location, ApiError, SharedState};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/debug/location", axum::routing::delete(debug_evict))
        .route("/api/debug/location", axum::routing::get(debug_list))
        .route("/api/location", axum::routing::get(get))
        .route("/api/location/reload", axum::routing::post(reload))
}

async fn get(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    // Info read (v2 `Location.Service` info path): project identity is
    // resolved WITHOUT assembling the service graph — no boot cascade, no
    // watcher-driven service init. The location is only *tracked* for
    // config watching.
    let projects = Arc::new(oc_core::project::SqliteProjectStore::new((*state.engine.db).clone()));
    let resolver = oc_core::project::Resolver::new(projects, Arc::new(oc_core::project::NoEvents));
    let project = resolver.resolve(location.as_path()).await;
    crate::announce::note_tracked(&state, &location);
    Ok(axum::Json(json!({
        "directory": location.as_path().to_string_lossy(),
        "project": {
            "id": project.id,
            "directory": project.canonical.to_string_lossy(),
            "canonical": project.canonical.to_string_lossy(),
        },
    }))
    .into_response())
}

async fn reload(State(state): State<SharedState>) -> Result<Response, ApiError> {
    // Reload: shut every tracked location down and rebuild it — v2
    // responds once the replacement services have settled. The announce
    // layer owns the frame choreography (project activate → shutdown →
    // rebuild → service frames).
    let mut seen = std::collections::BTreeSet::new();
    let mut targets = Vec::new();
    for location in crate::announce::tracked_locations(&state) {
        if seen.insert(location.directory.display().to_string()) {
            targets.push(location);
        }
    }
    for location in state.engine.live_locations() {
        if seen.insert(location.directory.display().to_string()) {
            targets.push(location);
        }
    }
    for location in targets {
        crate::announce::reload_location(&state, &location).await;
    }
    Ok(no_content())
}

async fn debug_list(State(state): State<SharedState>) -> Result<Response, ApiError> {
    let data: Vec<Value> = state
        .engine
        .live_locations()
        .iter()
        .map(|location| json!({ "directory": location.as_path().to_string_lossy() }))
        .collect();
    Ok(axum::Json(Value::Array(data)).into_response())
}

async fn debug_evict(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    state.engine.invalidate(&location).await;
    Ok(no_content())
}
