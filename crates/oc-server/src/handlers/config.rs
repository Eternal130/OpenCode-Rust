//! Config handlers: merged entries view, shell options, and the (v2-dying)
//! experimental PATCH.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::State;
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{resolve_location, ApiError, SharedState};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/config", axum::routing::get(get))
        .route("/api/config/shell", axum::routing::get(shells))
        .route("/api/experimental/config", axum::routing::patch(update))
}

async fn get(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let config_loader = oc_core::config::Loader::new(
        location.as_path().to_path_buf(),
        oc_core::config::GlobalDirs {
            config: state.engine.home.join(".config").join("opencode"),
            home: state.engine.home.clone(),
        },
    );
    let snapshot = config_loader.load();
    // The merged entries view (no wrapper, deep-map §3).
    let data: Vec<Value> = snapshot
        .entries
        .iter()
        .map(|entry| serde_json::to_value(entry).unwrap_or_else(|_| json!({})))
        .collect();
    Ok(axum::Json(Value::Array(data)).into_response())
}

async fn shells() -> Result<Response, ApiError> {
    let data: Vec<Value> = oc_core::shell::select::list()
        .iter()
        .map(|item| {
            json!({
                "name": item.name,
                "path": item.path,
                "acceptable": true,
            })
        })
        .collect();
    Ok(axum::Json(Value::Array(data)).into_response())
}

async fn update(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = (state, body);
    // v2 dies with an unsupported-operation 500 on the experimental
    // config PATCH (deep-map §3 "config.update PATCH dies 500 unsupported").
    Err(ApiError::unknown_ref())
}
