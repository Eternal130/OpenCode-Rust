//! Config handlers: merged entries view, shell options, and the (v2-dying)
//! experimental PATCH.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::State;
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{no_content, resolve_location, ApiError, SharedState};

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
    let mut config_loader = oc_core::config::Loader::new(
        location.as_path().to_path_buf(),
        oc_core::config::GlobalDirs {
            config: state.engine.home.join(".config").join("opencode"),
            home: state.engine.home.clone(),
        },
    );
    // CLI-N4 — the project-config disable switch (v2 Config.configured).
    crate::server_process_env::configure_loader(&mut config_loader);
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
    // v2 `Config.update` (core config.ts:329-349): the patch writes ONLY
    // the `shell` key (`Config.Patch = { shell: string | null }`,
    // schema config.ts:111-114) into the highest-precedence global config
    // document, JSONC-preserving (jsonc_edit — the same machinery oc-cli
    // uses), then requests a reload (the announce poller picks the change
    // up and publishes `config.updated`).
    let shell = match body.get("shell") {
        Some(Value::String(shell)) => Some(shell.clone()),
        Some(Value::Null) | None => None,
        Some(other) => {
            return Err(ApiError::invalid_request(format!(
                "Expected shell to be a string or null, got: {other}"
            )));
        }
    };
    let globals = oc_core::config::GlobalDirs {
        config: state.engine.home.join(".config").join("opencode"),
        home: state.engine.home.clone(),
    };
    oc_core::config::update_shell(&globals, shell.as_deref()).map_err(|error| {
        crate::errors::debug_cause("config:update", &error);
        ApiError::unknown_ref()
    })?;
    Ok(no_content())
}
