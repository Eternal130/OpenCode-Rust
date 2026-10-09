//! Model + provider handlers: catalog-driven, `{location, data}`
//! envelopes with 404 `ProviderNotFoundError` (deep-map §3).

#![allow(clippy::must_use_candidate)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use serde_json::{json, Value};

use super::common::{json_location_data, resolve_location, ApiError, SharedState};
use super::session::services;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/model", axum::routing::get(model_list))
        .route("/api/model/default", axum::routing::get(model_default))
        .route("/api/provider", axum::routing::get(provider_list))
        .route("/api/provider/:providerID", axum::routing::get(provider_get))
}

fn provider_json(snapshot: &oc_core::provider_catalog::Snapshot) -> Value {
    serde_json::to_value(&snapshot.info)
        .unwrap_or_else(|_| json!({ "id": snapshot.info.id.0, "name": snapshot.info.name }))
}

fn model_json(
    snapshot: &oc_core::provider_catalog::Snapshot,
    model: &oc_schema::model::Info,
) -> Value {
    let mut value = serde_json::to_value(model).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut value {
        map.insert("providerID".to_owned(), json!(snapshot.info.id.0));
    }
    value
}

/// A config-defined custom provider's model entry (the catalog
/// `Model.Info` shape — clients read `variants`/`capabilities`/`limit`
/// off list entries).
fn custom_model_json(
    provider: &oc_core::provider_catalog::facade::CustomProvider,
    model: &str,
) -> Value {
    json!({
        "id": model,
        "name": model,
        "providerID": provider.id,
        "enabled": true,
        "capabilities": { "input": ["text"], "output": ["text"], "tools": true },
        "limit": {},
        "cost": [],
        "variants": [],
    })
}

async fn model_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let catalog = &state.engine.catalog;
    let mut data = Vec::new();
    for snapshot in catalog.providers() {
        for model in &snapshot.models {
            data.push(model_json(snapshot, model));
        }
    }
    // Config-defined custom providers ride the list too (v2's model table
    // includes `provider.<id>` entries with a baseURL).
    for provider in crate::boot::location_custom_providers(&state.engine, &location) {
        for model in &provider.models {
            data.push(custom_model_json(&provider, model));
        }
    }
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn model_default(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // The configured `model` selection wins (v2 `Config.model`).
    let configured = crate::boot::location_default_model(&state.engine, &location);
    if let Some(selection) = &configured {
        let provider = crate::boot::location_custom_providers(&state.engine, &location)
            .into_iter()
            .find(|provider| provider.id == selection.provider_id.as_str());
        let default = match provider {
            Some(provider) => custom_model_json(&provider, selection.model.as_str()),
            None => json!({
                "id": selection.model.as_str(),
                "providerID": selection.provider_id.as_str(),
            }),
        };
        return Ok(json_location_data(&location.as_path().to_string_lossy(), default));
    }
    let catalog = &state.engine.catalog;
    // Default: the first active model of the first enabled provider
    // (the configured default model selection rides the resolver seam).
    let default = catalog
        .providers()
        .iter()
        .find_map(|snapshot| snapshot.models.first().map(|model| model_json(snapshot, model)))
        .unwrap_or(Value::Null);
    Ok(json_location_data(&location.as_path().to_string_lossy(), default))
}

async fn provider_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let catalog = &state.engine.catalog;
    let data: Vec<Value> = catalog.providers().iter().map(provider_json).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn provider_get(
    State(state): State<SharedState>,
    Path(provider_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let catalog = &state.engine.catalog;
    let snapshot =
        catalog.provider(&provider_id).ok_or_else(|| ApiError::provider_not_found(&provider_id))?;
    Ok(json_location_data(&location.as_path().to_string_lossy(), provider_json(snapshot)))
}
