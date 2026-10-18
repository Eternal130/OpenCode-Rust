//! The remaining smaller namespaces: generate, migration, websearch,
//! plugin, rpc. The integration/auth handlers live in `integration.rs`
//! (BE-3 host).

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
        // integration handlers live in handlers/integration.rs (BE-3)
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

/// `Generate.UnavailableError` mapping (generate.ts:47-49).
fn generate_unavailable(error: &oc_ai::schema::AiError) -> ApiError {
    ApiError::service_unavailable(error.message().to_string(), Some("opencode"))
}

async fn generate_text(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    use futures::StreamExt as _;
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let Some(prompt) = body["prompt"].as_str() else {
        return Err(ApiError::invalid_request("expected string prompt"));
    };
    // One stateless generation over the base configuration's model
    // (generate.ts): resolve the optional reference or the configured
    // default, then stream one request with session-attribution headers
    // and no stored session.
    let synthetic = oc_core::database::schema::SessionRow::new(
        "generate",
        services.project.id.clone(),
        &services.project.directory.to_string_lossy(),
        "generate",
        "0",
    )
    .map_err(|error| {
        crate::errors::debug_cause("generate:text", &error);
        ApiError::service_unavailable("Text generation is not available", Some("opencode"))
    })?;
    let resolved = match &body["model"] {
        Value::Null => services.resolver.resolve(&synthetic).map_err(|error| match error {
            oc_core::session::runner::ModelError::NotSelected(_) => {
                ApiError::invalid_request("No model specified and no supported model is available")
            }
            other => ApiError::service_unavailable(other.to_string(), Some("opencode")),
        })?,
        reference => {
            let parsed: Result<oc_schema::model::Ref, _> =
                serde_json::from_value(reference.clone());
            match parsed {
                Ok(reference) => {
                    let provider = reference.provider_id.0.clone();
                    let model = reference.id.0.clone();
                    services.resolver.resolve_selected(&synthetic, &reference).map_err(|error| {
                        crate::errors::debug_cause("generate:text", &error);
                        ApiError::invalid_request(format!("Model unavailable: {provider}/{model}"))
                    })?
                }
                Err(_) => return Err(ApiError::invalid_request("expected string prompt")),
            }
        }
    };
    let mut request = oc_ai::schema::LLMRequest::new(resolved.model);
    request.messages = vec![oc_ai::schema::messages::Message::user(prompt)];
    // Gateways require session attribution even for a stateless call; no
    // session is stored (generate.ts:57-59).
    let mut http_headers = std::collections::BTreeMap::new();
    http_headers.insert(
        "x-opencode-session".to_string(),
        oc_schema::id::SessionId::new().as_str().to_string(),
    );
    request.http = Some(oc_ai::schema::HttpOptions {
        headers: Some(http_headers),
        ..oc_ai::schema::HttpOptions::default()
    });
    let stream_result = services.runner.llm().stream(request).await;
    let mut stream = stream_result.map_err(|error| generate_unavailable(&error))?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|error| generate_unavailable(&error))? {
            oc_ai::schema::LLMEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
            oc_ai::schema::LLMEvent::ProviderError { message, .. } => {
                return Err(ApiError::service_unavailable(message, Some("opencode")));
            }
            _ => {}
        }
    }
    Ok(super::common::json_data(json!({ "text": text })))
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
    // The booted plugin subsystem's inventory (wave-12): every resolved
    // plugin with its live status (active / failed / crash-disabled)
    // plus the resolution failures, in v2's `Plugin.Info` shape. The
    // subsystem is process-global (documented divergence from v2's
    // per-location activation); a plugin-less boot lists empty.
    let plugins = state.plugins.as_ref().map_or_else(Vec::new, |boot| boot.list_info());
    let data: Vec<Value> =
        plugins.iter().map(|info| serde_json::to_value(info).unwrap_or(Value::Null)).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
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

/// `/api/rpc` dispatch (PLUG-D14): route the call into the booted
/// plugin host's guest (`Rpc.Service.call` parity — handlers/rpc.ts):
/// unknown ids/methods map to `RpcError` 400 with the v2 failure type;
/// handler defects and the invalid-output/internal types map to
/// `RpcInternalError` 500; a successful output returns `{output}`
/// (omitted when `undefined`, per the v2 handler).
async fn rpc_call(
    State(state): State<SharedState>,
    Path((rpc_id, method)): Path<(String, String)>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let input = body.get("input").cloned().unwrap_or(Value::Null);
    let Some(boot) = state.plugins.clone() else {
        // No plugin subsystem at all = an empty registry: v2's
        // `rpc.unavailable` (RpcError 400), never a 500 stub.
        return Err(ApiError::rpc(format!("RPC is unavailable: {rpc_id}"), "rpc.unavailable"));
    };
    match boot.rpc_call(&rpc_id, &method, input).await {
        // `output === undefined ? {} : { output }` (rpc.ts:10-11).
        Ok(output) => Ok(axum::Json(match output {
            Some(output) => json!({ "output": output }),
            None => json!({}),
        })
        .into_response()),
        Err(crate::plugins::RpcCallError::Failure { type_, message, .. }) => {
            Err(ApiError::rpc(message, type_))
        }
        Err(crate::plugins::RpcCallError::Internal(message)) => {
            Err(ApiError::rpc_internal(message, "rpc.internal"))
        }
    }
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
    let payload: oc_schema::misc::WebSearchInput =
        serde_json::from_value(body).map_err(|error| {
            ApiError::invalid_request(format!("Invalid websearch payload: {error}"))
        })?;
    // v2 handlers/websearch.ts — the location's WebSearch service with the
    // tagged-error mapping (ProviderRequired/ProviderNotFound/Disabled →
    // 400 kinds; Request → 503 `service`).
    match services
        .websearch
        .query(&payload.query, None, payload.provider_id.as_ref().map(|id| id.0.as_str()))
        .await
    {
        Ok(response) => Ok(json_location_data(
            &location.as_path().to_string_lossy(),
            serde_json::to_value(&response).unwrap_or(Value::Null),
        )),
        Err(error) => Err(match error {
            oc_core::tool::websearch::WebSearchError::ProviderRequired => {
                crate::errors::invalid_request_field(
                    "Web search provider is required",
                    "websearch_provider_required",
                    "providerID",
                )
            }
            oc_core::tool::websearch::WebSearchError::ProviderNotFound(provider_id) => {
                crate::errors::invalid_request_field(
                    format!("Web search provider not found: {provider_id}"),
                    "websearch_provider_not_found",
                    "providerID",
                )
            }
            oc_core::tool::websearch::WebSearchError::Disabled => {
                crate::errors::invalid_request_kind("Web search is disabled", "websearch_disabled")
            }
            oc_core::tool::websearch::WebSearchError::Request { provider_id, .. } => {
                crate::errors::service_unavailable_named(
                    format!("Web search request failed: {provider_id}"),
                    provider_id,
                )
            }
        }),
    }
}

async fn websearch_providers(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    // v2 `websearch.providers`: id/name pairs sorted by display name.
    let providers: Vec<Value> = services
        .websearch
        .providers()
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(providers)))
}
