//! The remaining smaller namespaces: generate, migration, websearch,
//! plugin, rpc. Integration surfaces that need the OAuth/credential host
//! (phase-9) return their documented empty/failure shapes.

#![allow(clippy::must_use_candidate, clippy::fn_params_excessive_bools)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{json_location_data, no_content, resolve_location, ApiError, SharedState};
use super::session::services;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        // generate
        .route("/api/experimental/generate", axum::routing::post(generate_text))
        // integration (global + scoped)
        .route(
            "/api/experimental/integration/wellknown",
            axum::routing::post(integration_wellknown_add),
        )
        .route("/api/integration", axum::routing::get(integration_list))
        .route("/api/integration/:integrationID", axum::routing::get(integration_get))
        .route(
            "/api/integration/:integrationID/connect/command",
            axum::routing::post(integration_command_connect),
        )
        .route(
            "/api/integration/:integrationID/connect/command/:attemptID",
            axum::routing::delete(integration_command_cancel),
        )
        .route(
            "/api/integration/:integrationID/connect/command/:attemptID",
            axum::routing::get(integration_command_status),
        )
        .route(
            "/api/integration/:integrationID/connect/key",
            axum::routing::post(integration_connect_key),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth",
            axum::routing::post(integration_oauth_connect),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth/:attemptID",
            axum::routing::delete(integration_oauth_cancel),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth/:attemptID",
            axum::routing::get(integration_oauth_status),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth/:attemptID/complete",
            axum::routing::post(integration_oauth_complete),
        )
        // migration
        .route("/api/experimental/migration/v1", axum::routing::get(migration_status))
        // plugin
        .route("/api/plugin", axum::routing::get(plugin_list))
        .route("/api/plugin/check", axum::routing::post(plugin_check))
        .route("/api/plugin/update", axum::routing::post(plugin_update))
        // rpc
        .route("/api/rpc/:rpcID/:method", axum::routing::post(rpc_call))
        // websearch
        .route("/api/websearch", axum::routing::post(websearch_query))
        .route("/api/websearch/provider", axum::routing::get(websearch_providers))
}

// ---------------------------------------------------------------------------
// POST /api/experimental/generate
// ---------------------------------------------------------------------------

async fn generate_text(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = (state, body["prompt"].as_str(), body["model"].as_str());
    // One-shot generation needs the engine's LLM surface (TODO(phase-9)).
    Err(ApiError::service_unavailable("Text generation is not available", Some("opencode")))
}

// ---------------------------------------------------------------------------
// Integrations (OAuth host lands phase-9; surface documented shapes)
// ---------------------------------------------------------------------------

async fn integration_wellknown_add(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let url = body["url"].as_str().unwrap_or_default();
    // Well-known discovery requires the integration host (§3:
    // 400 well_known_discovery on failure).
    Err(ApiError::invalid_request_kind(format!("Failed to fetch {url}"), "well_known_discovery"))
}

async fn integration_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // No integrations are registered in this build (TODO(phase-9)).
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(Vec::new())))
}

async fn integration_get(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    Err(ApiError::integration_not_found(&integration_id))
}

async fn integration_command_connect(
    Path(integration_id): Path<String>,
    axum::Json(_body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    Err(ApiError::integration_not_found(&integration_id))
}

async fn integration_command_cancel(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let _ = state;
    // Cancelling an unknown attempt mirrors the attempt 404.
    Err(ApiError::integration_attempt_not_found(&integration_id, &attempt_id))
}

async fn integration_command_status(
    Path((integration_id, attempt_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    Err(ApiError::integration_attempt_not_found(&integration_id, &attempt_id))
}

async fn integration_connect_key(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let _ = body["key"].as_str();
    Err(ApiError::integration_not_found(&integration_id))
}

async fn integration_oauth_connect(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let _ = body["methodID"].as_str();
    Err(ApiError::integration_not_found(&integration_id))
}

async fn integration_oauth_cancel(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let _ = state;
    Err(ApiError::integration_attempt_not_found(&integration_id, &attempt_id))
}

async fn integration_oauth_status(
    Path((integration_id, attempt_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    Err(ApiError::integration_attempt_not_found(&integration_id, &attempt_id))
}

async fn integration_oauth_complete(
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    match body["code"].as_str() {
        None | Some("") => {
            // CodeRequired → 400 integration_code_required (§3).
            Err(ApiError::invalid_request_kind(
                "Authorization code required",
                "integration_code_required",
            ))
        }
        Some(_) => {
            Err(ApiError::invalid_request_kind("Authorization failed", "integration_authorization"))
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/experimental/migration/v1
// ---------------------------------------------------------------------------

async fn migration_status(State(state): State<SharedState>) -> Result<Response, ApiError> {
    // v1 migration state: the legacy store was migrated when the engine's
    // database booted (migrations run at open).
    let _ = state.engine.db.migration_ids().await;
    Ok(axum::Json(json!({ "status": "completed" })).into_response())
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

async fn plugin_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // Host plugins arrive with the plugin lane (TODO(phase-9)); the
    // documented shape is the empty list.
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(Vec::new())))
}

async fn plugin_check(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let _ = body["target"].as_str();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(Vec::new())))
}

async fn plugin_update(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let _targets = body["targets"].as_array();
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /api/rpc/:rpcID/:method — {input} → {output}
// ---------------------------------------------------------------------------

async fn rpc_call(
    State(state): State<SharedState>,
    Path((rpc_id, _method)): Path<(String, String)>,
    axum::Json(_body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = state;
    // No plugin host → rpc.internal (§3: rpc.internal|invalid_output → 500).
    Err(ApiError::rpc_internal(format!("rpc {rpc_id} not available"), "rpc.internal"))
}

// ---------------------------------------------------------------------------
// Websearch
// ---------------------------------------------------------------------------

async fn websearch_query(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let query = body["query"].as_str().unwrap_or_default();
    let provider = body["providerID"].as_str().map(String::from);
    // The websearch tool surface needs provider credentials; without one
    // configured the declared 503 is the v2 behavior.
    let _ = (services, provider);
    Err(ApiError::service_unavailable(
        format!("No websearch provider available for query {query:?}"),
        None,
    ))
}

async fn websearch_providers(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // The catalog of configured providers (tool/websearch_providers) is
    // empty until a websearch credential is set (TODO(phase-9) config
    // exposure; documented empty shape).
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(Vec::new())))
}
