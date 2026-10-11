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

/// A config-defined custom provider's model entry (the FULL catalog
/// `Model.Info` shape — clients like `run`'s model.default fallback parse
/// the payload into `ModelInfo`, whose `limit.context`/`limit.output`,
/// `modelID`, `status`, and `time.released` are REQUIRED; v2 defaults
/// unspecified config-model limits to 0).
fn custom_model_json(
    provider: &oc_core::provider_catalog::facade::CustomProvider,
    model: &str,
) -> Value {
    json!({
        "id": model,
        "modelID": model,
        "providerID": provider.id,
        "name": model,
        "enabled": true,
        "status": "active",
        "time": { "released": 0 },
        "capabilities": { "input": ["text"], "output": ["text"], "tools": true },
        "limit": { "context": 0, "output": 0 },
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
    // v2's `Model.available()` serves the flattened list sorted by
    // `time.released` DESCENDING (core model.ts:218-220) — catalog
    // pairs through `flat_models_by_release`, config-defined custom
    // providers (released = 0) ride the same stable sort so they land
    // after every dated catalog model, ties keeping source order.
    let mut data: Vec<(f64, Value)> = catalog
        .flat_models_by_release()
        .into_iter()
        .map(|(snapshot, model)| (model.time.released.0, model_json(snapshot, model)))
        .collect();
    for provider in crate::boot::location_custom_providers(&state.engine, &location) {
        for model in &provider.models {
            data.push((0.0, custom_model_json(&provider, model.id.as_str())));
        }
    }
    data.sort_by(|a, b| b.0.total_cmp(&a.0));
    let data: Vec<Value> = data.into_iter().map(|(_, value)| value).collect();
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
        // A custom (config) provider renders its own entry; a catalog
        // provider's default resolves through the catalog so the payload
        // keeps the full (client-parseable) Model.Info shape.
        let default = if let Some(provider) = provider {
            custom_model_json(&provider, selection.model.as_str())
        } else {
            let catalog = &state.engine.catalog;
            let catalog_snapshot = catalog
                .providers()
                .iter()
                .find(|snapshot| snapshot.info.id.0 == selection.provider_id.as_str());
            catalog_snapshot
                .and_then(|snapshot| {
                    snapshot
                        .models
                        .iter()
                        .find(|model| model.id.0 == selection.model.as_str())
                        .map(|model| model_json(snapshot, model))
                })
                .or_else(|| {
                    catalog_snapshot.and_then(|snapshot| {
                        snapshot.models.first().map(|model| model_json(snapshot, model))
                    })
                })
                .unwrap_or_else(|| {
                    json!({
                        "id": selection.model.as_str(),
                        "modelID": selection.model.as_str(),
                        "providerID": selection.provider_id.as_str(),
                        "name": selection.model.as_str(),
                        "enabled": true,
                        "status": "active",
                        "time": { "released": 0 },
                        "capabilities": { "input": ["text"], "output": ["text"], "tools": true },
                        "limit": { "context": 0, "output": 0 },
                        "cost": [],
                        "variants": [],
                    })
                })
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
