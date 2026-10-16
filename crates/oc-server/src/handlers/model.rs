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
/// unspecified config-model limits to 0). A model without an explicit
/// config `variants` list inherits the provider package's protocol
/// variants (`packages/core/src/config/plugin/provider.ts:151-159` —
/// `Variant.resolve(model.package ?? provider.package)` with the default
/// effort support; the openai-compatible protocol yields low/medium/
/// high `reasoningEffort` overlays). The display name is the config's
/// `models.<id>.name` when given (provider.ts:112), else the id.
fn custom_model_json(
    provider: &oc_core::provider_catalog::facade::CustomProvider,
    model: &oc_core::provider_catalog::facade::CustomModel,
) -> Value {
    let variants = oc_core::provider_catalog::variant::resolve(
        &oc_core::provider_catalog::support::VariantModel {
            // The overlay binds every baseURL custom provider onto the
            // openai-compatible package (`provider_catalog/overlay.rs`).
            package: Some("@opencode/ai/providers/openai-compatible".to_owned()),
            provider_id: provider.id.clone(),
            model_id: model.id.clone(),
            limit_output: 0,
        },
        &[oc_core::provider_catalog::support::Support::Effort { values: None }],
    );
    json!({
        "id": model.id,
        "modelID": model.id,
        "providerID": provider.id,
        "name": model.name.clone().unwrap_or_else(|| model.id.clone()),
        "enabled": true,
        "status": "active",
        "time": { "released": 0 },
        "capabilities": { "input": ["text"], "output": ["text"], "tools": true },
        "limit": { "context": 0, "output": 0 },
        "cost": [],
        "variants": serde_json::to_value(&variants).unwrap_or_else(|_| json!([])),
    })
}

/// The location's ordered model entries — exactly what
/// [`model_list`] serves: `Model.available()` sorted by
/// `time.released` DESCENDING (core model.ts:218-220) over the
/// available providers, with config-defined custom providers
/// activation-"enabled" (config/plugin/provider.ts:67) riding the
/// same stable sort (released = 0, after every dated catalog model).
/// `Model.default`'s fallback (`available.find(supportsText)`,
/// model.ts:260-266) scans the SAME ordering.
fn location_model_entries(
    engine: &oc_core::engine::Engine,
    location: &oc_core::location::LocationRef,
) -> Vec<Value> {
    let catalog = &engine.catalog;
    let mut data: Vec<(f64, Value)> = catalog
        .available_models(|name| std::env::var_os(name).is_some())
        .into_iter()
        .map(|(snapshot, model)| (model.time.released.0, model_json(snapshot, model)))
        .collect();
    for provider in crate::boot::location_custom_providers(engine, location) {
        for model in &provider.models {
            data.push((0.0, custom_model_json(&provider, model)));
        }
    }
    data.sort_by(|a, b| b.0.total_cmp(&a.0));
    data.into_iter().map(|(_, value)| value).collect()
}

/// `Model.supportsText` (core model.ts:273-277): empty capabilities
/// wildcard to text, else some modality must start with `text`.
fn supports_text(model: &Value) -> bool {
    let modality_has_text = |key: &str| {
        model.get("capabilities").and_then(|caps| caps.get(key)).and_then(Value::as_array).map_or(
            true,
            |items| {
                // `length === 0 || some(startsWith "text")` — the empty
                // array is the wildcard, exactly like the reference.
                items.is_empty()
                    || items
                        .iter()
                        .any(|item| item.as_str().map_or(false, |text| text.starts_with("text")))
            },
        )
    };
    modality_has_text("input") && modality_has_text("output")
}

async fn model_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let data = location_model_entries(&state.engine, &location);
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
        // keeps the full (client-parseable) Model.Info shape. When the
        // selection resolves to NOTHING (v2: `byProvider.get(providerID)
        // ?.get(modelID)` — an exact-map miss, e.g. a `model: "Own/…"`
        // selection against a `providers.own` key — or a disabled
        // model), v2 serves `available.find(supportsText)` (model.ts:
        // 260-266): the first text model of the SAME ordering the list
        // serves, never a synthetic stub.
        let resolved = if let Some(provider) = provider {
            let model = provider
                .models
                .iter()
                .find(|model| model.id == selection.model.as_str())
                .cloned()
                .unwrap_or_else(|| oc_core::provider_catalog::facade::CustomModel {
                    id: selection.model.to_string(),
                    name: None,
                    headers: None,
                    body: None,
                });
            Some(custom_model_json(&provider, &model))
        } else {
            let catalog = &state.engine.catalog;
            catalog
                .providers()
                .iter()
                .find(|snapshot| snapshot.info.id.0 == selection.provider_id.as_str())
                .and_then(|snapshot| {
                    snapshot
                        .models
                        .iter()
                        .find(|model| model.id.0 == selection.model.as_str())
                        .map(|model| model_json(snapshot, model))
                })
        };
        let default = resolved.or_else(|| {
            location_model_entries(&state.engine, &location).into_iter().find(supports_text)
        });
        if let Some(default) = default {
            return Ok(json_location_data(&location.as_path().to_string_lossy(), default));
        }
    }
    // No configured selection (or an empty catalog): the same
    // available-first fallback v2's `Model.default()` takes.
    let default = location_model_entries(&state.engine, &location)
        .into_iter()
        .find(supports_text)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `Model.supportsText` (core model.ts:273-277): empty capabilities
    /// wildcard to text; otherwise some modality must lead with `text`.
    #[test]
    fn supports_text_matches_the_reference_predicate() {
        assert!(supports_text(&json!({"capabilities": {"input": [], "output": []}})));
        assert!(supports_text(
            &json!({"capabilities": {"input": ["text", "image"], "output": ["text"]}})
        ));
        assert!(!supports_text(&json!({"capabilities": {"input": ["image"], "output": ["text"]}})));
        assert!(!supports_text(&json!({"capabilities": {"input": ["text"], "output": ["audio"]}})));
        // Missing capability blocks wildcard to text (never a false no).
        assert!(!supports_text(&json!({"capabilities": {"input": ["image"]}})));
    }

    /// dogfood2 bug 3's miss path — when the configured `model` selection
    /// resolves to nothing (v2: `byProvider.get(providerID)?.get(modelID)`
    /// is an exact-map miss, model.ts:262-263 — e.g. "Own/glm-5.3"
    /// against a `providers.own` key), `Model.default()` serves
    /// `available.find(supportsText)`: the FIRST text model of the very
    /// ordering `/api/model` serves (released DESC, custom providers at
    /// released 0 last), never a synthetic stub of the missed selection.
    #[tokio::test]
    async fn model_entries_fallback_is_the_first_available_text_model() {
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let entries = location_model_entries(&engine, &location);
        assert!(!entries.is_empty(), "the keyless opencode tier is available offline");
        let fallback = entries
            .iter()
            .find(|model| supports_text(model))
            .cloned()
            .expect("an available text model exists");
        // A REAL registry entry (full Model.Info shape), not a stub of
        // the missed selection: providerID + id + capabilities blocks.
        assert!(fallback.get("providerID").is_some_and(Value::is_string));
        assert!(fallback.get("id").is_some_and(Value::is_string));
        assert!(
            fallback.get("capabilities").is_some_and(|caps| caps.is_object()),
            "the fallback keeps the full Model.Info shape"
        );
    }
}
