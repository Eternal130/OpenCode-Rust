//! Integration handlers (v2 `handlers/integration.ts`): the 11-operation
//! auth surface over the oc-core integration host — list/get, well-known
//! discovery, key connect, OAuth connect/status/complete/cancel, command
//! connect/status/cancel. Error mapping per v2: `AuthorizationError` → 400
//! `integration_authorization`, `CodeRequired` → 400
//! `integration_code_required`, unknown integration/attempt/method → 404
//! tagged envelopes, well-known failures → 400 `well_known_discovery`.

#![allow(clippy::too_many_lines)]
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::Value;

use super::common::{json_location_data, no_content, resolve_location, ApiError, SharedState};
use super::session::services;

use oc_core::integration::types::IntegrationError;

/// v2's `authorize` wrapper (handlers/integration.ts:11-21): typed
/// authorization failures flatten to 400 `integration_authorization`.
fn authorization_error(error: &IntegrationError) -> ApiError {
    let message = match error {
        IntegrationError::Authorization(message) => message.clone(),
        other => other.to_string(),
    };
    let trimmed = message.trim();
    let message =
        if trimmed.is_empty() { "Authentication failed".to_owned() } else { trimmed.to_owned() };
    ApiError::invalid_request_kind(message, "integration_authorization")
}

fn attempt_not_found(integration_id: &str, attempt_id: &str) -> ApiError {
    crate::errors::integration_attempt_not_found(
        integration_id,
        attempt_id,
        format!("OAuth attempt not found: {attempt_id}"),
    )
}

fn integration_missing(integration_id: &str) -> ApiError {
    crate::errors::integration_not_found(
        integration_id,
        format!("Integration not found: {integration_id}"),
    )
}

/// The v2 host error family surfaced as-is (store/db failures).
fn unknown(error: oc_core::integration::HostError) -> ApiError {
    crate::errors::debug_cause("integration:unknown", error);
    ApiError::unknown()
}

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/integration", axum::routing::get(integration_list))
        .route("/api/integration/:integrationID", axum::routing::get(integration_get))
        .route(
            "/api/experimental/integration/wellknown",
            axum::routing::post(integration_wellknown_add),
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
            axum::routing::get(integration_oauth_status),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth/:attemptID",
            axum::routing::delete(integration_oauth_cancel),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth/:attemptID/complete",
            axum::routing::post(integration_oauth_complete),
        )
        .route(
            "/api/integration/:integrationID/connect/command",
            axum::routing::post(integration_command_connect),
        )
        .route(
            "/api/integration/:integrationID/connect/command/:attemptID",
            axum::routing::get(integration_command_status),
        )
        .route(
            "/api/integration/:integrationID/connect/command/:attemptID",
            axum::routing::delete(integration_command_cancel),
        )
}

// ---------------------------------------------------------------------------
// GET /api/integration — integration.list
// ---------------------------------------------------------------------------

async fn integration_list(
    State(state): State<SharedState>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    ensure_wellknown_registered(&state).await;
    let infos = state.engine.integration.list().await.map_err(unknown)?;
    let data: Vec<Value> =
        infos.iter().map(|info| serde_json::to_value(info).unwrap_or(Value::Null)).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

// ---------------------------------------------------------------------------
// GET /api/integration/:integrationID — integration.get
// ---------------------------------------------------------------------------

async fn integration_get(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let info = state
        .engine
        .integration
        .get(&integration_id)
        .await
        .map_err(unknown)?
        .ok_or_else(|| integration_missing(&integration_id))?;
    let value = serde_json::to_value(&info).unwrap_or(Value::Null);
    Ok(json_location_data(&location.as_path().to_string_lossy(), value))
}

// ---------------------------------------------------------------------------
// POST /api/experimental/integration/wellknown — wellknown.add
// ---------------------------------------------------------------------------

/// `{origin}/.well-known/opencode` manifest (wellknown.ts:31-45).
#[derive(serde::Deserialize)]
struct WellKnownManifest {
    #[serde(default)]
    auth: Option<WellKnownAuth>,
}

#[derive(serde::Deserialize)]
struct WellKnownAuth {
    command: Vec<String>,
    #[allow(dead_code)]
    env: Option<String>,
}

/// The persisted-sources kv key (wellknown.ts:97).
const WELLKNOWN_SOURCES_KEY: &str = "wellknown:sources";

async fn integration_wellknown_add(
    State(state): State<SharedState>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let url = body["url"].as_str().unwrap_or_default().trim_end_matches('/').to_owned();
    let manifest_url = format!("{url}/.well-known/opencode");
    let discovery =
        |message: String| ApiError::invalid_request_kind(message, "well_known_discovery");
    let response = state
        .engine
        .integration
        .http()
        .get(&manifest_url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| {
            discovery(format!("Failed to load wellknown manifest from {manifest_url}: {error}"))
        })?;
    if !response.status().is_success() {
        return Err(discovery(format!(
            "Failed to load wellknown manifest from {manifest_url} (status {})",
            response.status()
        )));
    }
    let manifest: WellKnownManifest = response.json().await.map_err(|error| {
        discovery(format!("Failed to parse wellknown manifest from {manifest_url}: {error}"))
    })?;
    let Some(auth) = manifest.auth else {
        return Err(discovery(format!("No authentication method found at {url}")));
    };
    if auth.command.is_empty() {
        return Err(discovery(format!("No authentication command found at {url}")));
    }
    // Persist the source (wellknown.ts:142-148: union of origins).
    let db = state.engine.db.clone();
    let origin = url.clone();
    db.transaction(move |tx| {
        let sources = tx.kv_get(WELLKNOWN_SOURCES_KEY)?.unwrap_or(Value::Array(Vec::new()));
        let mut origins: Vec<String> = sources
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default();
        if !origins.contains(&origin) {
            origins.push(origin.clone());
        }
        let value = Value::Array(origins.into_iter().map(Value::String).collect());
        tx.kv_set(WELLKNOWN_SOURCES_KEY, &value)
    })
    .await
    .map_err(|error| {
        crate::errors::debug_cause("integration:wellknown_persist", &error);
        ApiError::unknown()
    })?;
    // Register the integration: name = hostname, method = command "login"
    // (wellknown/plugin.ts:14-28).
    let name = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(&url)
        .to_owned();
    let host = &state.engine.integration;
    host.update_integration(&url, &name);
    host.update_method(
        &url,
        oc_core::integration::types::Method::Command {
            id: "login".into(),
            label: "Log in".into(),
            command: auth.command,
        },
        None,
    );
    host.notify_updated().await;
    Ok(no_content())
}

/// Re-registers persisted well-known sources at first use (the
/// `WellKnownPlugin` boot transform, wellknown/plugin.ts:11-29).
pub async fn ensure_wellknown_registered(state: &SharedState) {
    // `set` fails when already restored — the once-gate.
    if state.wellknown_restore.set(true).is_err() {
        return;
    }
    let db = state.engine.db.clone();
    let Ok(sources) = db.transaction(|tx| tx.kv_get(WELLKNOWN_SOURCES_KEY)).await else {
        return;
    };
    let Some(sources) = sources else { return };
    let origins: Vec<String> = sources
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    for origin in origins {
        let manifest_url = format!("{origin}/.well-known/opencode");
        let Ok(response) = state
            .engine
            .integration
            .http()
            .get(&manifest_url)
            .header("Accept", "application/json")
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(manifest) = response.json::<WellKnownManifest>().await else { continue };
        let Some(auth) = manifest.auth else { continue };
        if auth.command.is_empty() {
            continue;
        }
        let name = origin
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(&origin)
            .to_owned();
        let host = &state.engine.integration;
        host.update_integration(&origin, &name);
        host.update_method(
            &origin,
            oc_core::integration::types::Method::Command {
                id: "login".into(),
                label: "Log in".into(),
                command: auth.command,
            },
            None,
        );
    }
    state.engine.integration.notify_updated().await;
}

// ---------------------------------------------------------------------------
// POST /api/integration/:integrationID/connect/key — connect.key
// ---------------------------------------------------------------------------

async fn integration_connect_key(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    if state.engine.integration.get(&integration_id).await.map_err(unknown)?.is_none() {
        return Err(integration_missing(&integration_id));
    }
    let Some(key) = body["key"].as_str() else {
        return Err(ApiError::invalid_request("key is required"));
    };
    let answer = parse_answer(&body["answer"]);
    let label = body["label"].as_str();
    state.engine.integration.connect_key(&integration_id, key, &answer, label).await.map_err(
        |error| match error {
            IntegrationError::Authorization(_) | IntegrationError::MethodNotFound(_, _) => {
                authorization_error(&error)
            }
            IntegrationError::CodeRequired { .. } | IntegrationError::AttemptNotFound => {
                ApiError::unknown()
            }
        },
    )?;
    Ok(no_content())
}

/// `Form.Answer` body → the map form (string values; nested scalars kept).
fn parse_answer(value: &Value) -> std::collections::BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.clone().into_iter().collect(),
        _ => std::collections::BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/integration/:integrationID/connect/oauth — oauth.connect
// ---------------------------------------------------------------------------

async fn integration_oauth_connect(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let method_id = body["methodID"].as_str().unwrap_or_default().to_owned();
    let answer = parse_answer(&body["answer"]);
    let label = body["label"].as_str().map(String::from);
    let attempt = state
        .engine
        .integration
        .oauth_connect(&integration_id, &method_id, &answer, label.as_deref())
        .await
        .map_err(|error| match error {
            IntegrationError::Authorization(_) | IntegrationError::MethodNotFound(_, _) => {
                authorization_error(&error)
            }
            IntegrationError::CodeRequired { .. } | IntegrationError::AttemptNotFound => {
                ApiError::unknown()
            }
        })?;
    let value = serde_json::to_value(&attempt).unwrap_or(Value::Null);
    Ok(json_location_data(&location.as_path().to_string_lossy(), value))
}

// ---------------------------------------------------------------------------
// GET/DELETE /api/integration/:integrationID/connect/oauth/:attemptID
// ---------------------------------------------------------------------------

async fn integration_oauth_status(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    if state.engine.integration.get(&integration_id).await.map_err(unknown)?.is_none() {
        return Err(integration_missing(&integration_id));
    }
    let status = state
        .engine
        .integration
        .oauth_status(&integration_id, &attempt_id)
        .await
        .map_err(|_| attempt_not_found(&integration_id, &attempt_id))?;
    let value = serde_json::to_value(&status).unwrap_or(Value::Null);
    Ok(json_location_data(&location.as_path().to_string_lossy(), value))
}

async fn integration_oauth_cancel(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    state.engine.integration.oauth_cancel(&integration_id, &attempt_id).await;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /api/integration/:integrationID/connect/oauth/:attemptID/complete
// ---------------------------------------------------------------------------

async fn integration_oauth_complete(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    if state.engine.integration.get(&integration_id).await.map_err(unknown)?.is_none() {
        return Err(integration_missing(&integration_id));
    }
    let code = body["code"].as_str().filter(|code| !code.is_empty());
    state.engine.integration.oauth_complete(&integration_id, &attempt_id, code).await.map_err(
        |error| match error {
            IntegrationError::CodeRequired { .. } => ApiError::invalid_request_kind(
                "Authorization code required",
                "integration_code_required",
            ),
            IntegrationError::Authorization(_) | IntegrationError::MethodNotFound(_, _) => {
                authorization_error(&error)
            }
            IntegrationError::AttemptNotFound => attempt_not_found(&integration_id, &attempt_id),
        },
    )?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Command attempts
// ---------------------------------------------------------------------------

async fn integration_command_connect(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    if state.engine.integration.get(&integration_id).await.map_err(unknown)?.is_none() {
        return Err(integration_missing(&integration_id));
    }
    let method_id = body["methodID"].as_str().unwrap_or_default().to_owned();
    let label = body["label"].as_str().map(String::from);
    let attempt = state
        .engine
        .integration
        .command_connect(&integration_id, &method_id, label.as_deref())
        .await
        .map_err(|error| match error {
            IntegrationError::MethodNotFound(integration, method) => {
                crate::errors::integration_method_not_found(
                    integration,
                    &method,
                    format!("Integration method not found: {method}"),
                )
            }
            IntegrationError::Authorization(_) => authorization_error(&error),
            IntegrationError::CodeRequired { .. } | IntegrationError::AttemptNotFound => {
                ApiError::unknown()
            }
        })?;
    let value = serde_json::to_value(&attempt).unwrap_or(Value::Null);
    Ok(json_location_data(&location.as_path().to_string_lossy(), value))
}

async fn integration_command_status(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    if state.engine.integration.get(&integration_id).await.map_err(unknown)?.is_none() {
        return Err(integration_missing(&integration_id));
    }
    let status = state
        .engine
        .integration
        .command_status(&integration_id, &attempt_id)
        .await
        .map_err(|_| attempt_not_found(&integration_id, &attempt_id))?;
    let value = serde_json::to_value(&status).unwrap_or(Value::Null);
    Ok(json_location_data(&location.as_path().to_string_lossy(), value))
}

async fn integration_command_cancel(
    State(state): State<SharedState>,
    Path((integration_id, attempt_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    state.engine.integration.command_cancel(&integration_id, &attempt_id).await;
    Ok(no_content())
}
