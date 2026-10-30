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

/// The openai-compatible effort variants every custom/console provider
/// model carries (`Variant.resolve(model.package ?? provider.package)`,
/// packages/core/src/config/plugin/provider.ts:151-159 — this lane binds
/// both through the openai-compatible package).
fn custom_effort_variants(provider_id: &str, model_id: &str) -> Value {
    let variants = oc_core::provider_catalog::variant::resolve(
        &oc_core::provider_catalog::support::VariantModel {
            package: Some("@opencode/ai/providers/openai-compatible".to_owned()),
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
            limit_output: 0,
        },
        &[oc_core::provider_catalog::support::Support::Effort { values: None }],
    );
    serde_json::to_value(&variants).unwrap_or_else(|_| json!([]))
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
        "variants": custom_effort_variants(&provider.id, &model.id),
    })
}

/// A console-issued provider's model entry (I1): the SAME field set as
/// [`custom_model_json`] (clients parse one `Model.Info` shape), but the
/// values come from the resolver's console merge —
/// `custom_model_info` merges the console entry onto its catalog base
/// (v2 opencode.ts:263-306), so name/limits/cost/capabilities carry the
/// configured values exactly like v2, whose provider transform
/// re-registers console providers into the provider registry
/// (opencode.ts:308-314) and lists them merged. Returns the entry with
/// its sort key — the base's `time.released` when a base exists (the
/// merged model sorts among the dated catalog models), else 0 like
/// config customs.
fn console_model_json(
    provider: &oc_core::provider_catalog::facade::CustomProvider,
    model: &oc_core::provider_catalog::facade::CustomModel,
    catalog: &oc_core::provider_catalog::Catalog,
) -> (f64, Value) {
    let info = oc_core::engine::resolver::custom_model_info(provider, &model.id, catalog);
    let released = info.time.released.0;
    let entry = json!({
        "id": info.id.as_str(),
        "modelID": info.model_id.as_str(),
        "providerID": info.provider_id.as_str(),
        "name": info.name,
        "enabled": info.enabled,
        "status": serde_json::to_value(info.status).unwrap_or_else(|_| json!("active")),
        "time": { "released": released },
        "capabilities": serde_json::to_value(info.capabilities).unwrap_or_else(|_| json!({})),
        "limit": serde_json::to_value(info.limit).unwrap_or_else(|_| json!({})),
        "cost": serde_json::to_value(info.cost).unwrap_or_else(|_| json!([])),
        "variants": custom_effort_variants(&provider.id, &model.id),
    });
    (released, entry)
}

/// The location's ordered model entries — exactly what
/// [`model_list`] serves: `Model.available()` sorted by
/// `time.released` DESCENDING (core model.ts:218-220) over the
/// available providers, with config-defined custom providers
/// activation-"enabled" (config/plugin/provider.ts:67) riding the
/// same stable sort (released = 0, after every dated catalog model).
/// `Model.default`'s fallback (`available.find(supportsText)`,
/// model.ts:260-266) scans the SAME ordering.
async fn location_model_entries(
    engine: &oc_core::engine::Engine,
    location: &oc_core::location::LocationRef,
) -> Vec<Value> {
    // The live catalog (v2 `ModelsDev.get()` — invalidated by refresh).
    let catalog = engine.catalog_service.get();
    // CRED-B1: availability reads the credential store, not env alone —
    // v2's provider fold counts ANY integration connection
    // (provider.ts:393-406): a stored credential for the provider's
    // integration satisfies the provider exactly like its env
    // variables do (and a Console credential un-keylesses zen, since
    // the opencode integration's connection exists).
    let credential_integrations: std::collections::HashSet<String> = engine
        .integration
        .store()
        .all()
        .await
        .map(|rows| rows.into_iter().map(|row| row.integration_id).collect())
        .unwrap_or_default();
    let mut credential_env: std::collections::HashSet<String> = std::collections::HashSet::new();
    for snapshot in catalog.providers() {
        if credential_integrations.contains(snapshot.info.id.as_str()) {
            credential_env.extend(snapshot.environment.iter().cloned());
        }
    }
    let mut data: Vec<(f64, Value)> = catalog
        .available_models(|name| std::env::var_os(name).is_some() || credential_env.contains(name))
        .into_iter()
        .map(|(snapshot, model)| (model.time.released.0, model_json(snapshot, model)))
        .collect();
    let configured = crate::boot::location_custom_providers(engine, location);
    // RF-01 — running local providers (ollama/lmstudio/vllm runtime
    // discovery): the discovered inventory REPLACES a config custom
    // with the same provider id (the ref transform removes the
    // provider's existing models before injecting), so those ids skip
    // the config rendering below and list through the discovery
    // snapshot instead.
    let local = engine.local.providers();
    for provider in &configured {
        if local.iter().any(|entry| entry.id == provider.id) {
            continue;
        }
        for model in &provider.models {
            data.push((0.0, custom_model_json(provider, model)));
        }
    }
    // Console-issued providers (integration `opencode`) ride the same
    // listing seam (I1): rendered from the SAME shared snapshot `bind`
    // reads, each model through the console merge so limits/cost/
    // capabilities carry. A config custom with the same id wins — the
    // resolver checks its custom list before the console, so listing and
    // resolution stay in parity.
    for provider in crate::boot::console_custom_providers(engine) {
        if configured.iter().any(|custom| custom.id == provider.id)
            || local.iter().any(|entry| entry.id == provider.id)
        {
            continue;
        }
        for model in &provider.models {
            data.push(console_model_json(&provider, model, &catalog));
        }
    }
    // RF-01 — the discovered local-provider models ride the same
    // listing seam (rendered through the SAME resolver merge
    // `console_model_json` uses, so limits/capabilities carry and the
    // listing stays in parity with `bind`).
    for provider in &local {
        for model in &provider.models {
            data.push(console_model_json(provider, model, &catalog));
        }
    }
    data.sort_by(|a, b| b.0.total_cmp(&a.0));
    data.into_iter().map(|(_, value)| value).collect()
}

/// `Model.supportsText` (core model.ts:273-277): empty capabilities
/// wildcard to text, else some modality must start with `text`.
fn supports_text(model: &Value) -> bool {
    let modality_has_text = |key: &str| {
        model
            .get("capabilities")
            .and_then(|caps| caps.get(key))
            .and_then(Value::as_array)
            .is_none_or(|items| {
                // `length === 0 || some(startsWith "text")` — the empty
                // array is the wildcard, exactly like the reference.
                items.is_empty()
                    || items
                        .iter()
                        .any(|item| item.as_str().is_some_and(|text| text.starts_with("text")))
            })
    };
    modality_has_text("input") && modality_has_text("output")
}

async fn model_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let data = location_model_entries(&state.engine, &location).await;
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn model_default(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // The configured `model` selection wins (v2 `Config.model`).
    let configured = crate::boot::location_default_model(&state.engine, &location);
    if let Some(selection) = &configured {
        let configured_custom = crate::boot::location_custom_providers(&state.engine, &location)
            .into_iter()
            .find(|provider| provider.id == selection.provider_id.as_str());
        // Console-issued providers resolve their default through the
        // same merged seam the listing serves (I1) — a config custom
        // wins the id, exactly like the resolver's branch order.
        let console_custom = if configured_custom.is_some() {
            None
        } else {
            crate::boot::console_custom_providers(&state.engine)
                .into_iter()
                .find(|provider| provider.id == selection.provider_id.as_str())
        };
        // RF-01 — a discovered local provider renders its default
        // through the same discovery snapshot the listing serves.
        let local_custom = if configured_custom.is_some() || console_custom.is_some() {
            None
        } else {
            state.engine.local.provider(selection.provider_id.as_str())
        };
        // A custom (config/console) provider renders its own entry; a
        // catalog provider's default resolves through the catalog so the
        // payload keeps the full (client-parseable) Model.Info shape.
        // When the selection resolves to NOTHING (v2:
        // `byProvider.get(providerID)?.get(modelID)` — an exact-map
        // miss, e.g. a `model: "Own/…"` selection against a
        // `providers.own` key — or a disabled model), v2 serves
        // `available.find(supportsText)` (model.ts:260-266): the first
        // text model of the SAME ordering the list serves, never a
        // synthetic stub. Every branch honors that: the catalog lookup
        // and the console provider both yield None on a model-id miss —
        // a console provider only lists models it actually carries, so
        // a miss never renders `custom_model_info`'s empty-identity
        // zero shape (`{"id":"","modelID":"",…}`).
        let resolved = if let Some(provider) = configured_custom {
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
                    ..oc_core::provider_catalog::facade::CustomModel::default()
                });
            Some(custom_model_json(&provider, &model))
        } else if let Some(provider) = local_custom {
            let catalog = state.engine.catalog_service.get();
            provider
                .models
                .iter()
                .find(|model| model.id == selection.model.as_str())
                .map(|model| console_model_json(&provider, model, &catalog).1)
        } else if let Some(provider) = console_custom {
            let catalog = state.engine.catalog_service.get();
            provider
                .models
                .iter()
                .find(|model| model.id == selection.model.as_str())
                .map(|model| console_model_json(&provider, model, &catalog).1)
        } else {
            let catalog = state.engine.catalog_service.get();
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
        let default = match resolved {
            Some(value) => Some(value),
            None => location_model_entries(&state.engine, &location)
                .await
                .into_iter()
                .find(supports_text),
        };
        if let Some(default) = default {
            return Ok(json_location_data(&location.as_path().to_string_lossy(), default));
        }
    }
    // No configured selection (or an empty catalog): the same
    // available-first fallback v2's `Model.default()` takes.
    let default = location_model_entries(&state.engine, &location)
        .await
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
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let catalog = state.engine.catalog_service.get();
    let data: Vec<Value> = catalog.providers().iter().map(provider_json).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn provider_get(
    State(state): State<SharedState>,
    Path(provider_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let catalog = state.engine.catalog_service.get();
    let snapshot =
        catalog.provider(&provider_id).ok_or_else(|| ApiError::provider_not_found(&provider_id))?;
    Ok(json_location_data(&location.as_path().to_string_lossy(), provider_json(snapshot)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// I1 (review): console-issued providers (integration `opencode`)
    /// must be VISIBLE on the model listing — rendered through the same
    /// seam as config customs, each model through the resolver's console
    /// merge so name/limits/cost/capabilities carry the configured
    /// values. v2 re-registers console providers into the provider
    /// registry (opencode.ts:308-314 via ctx.provider.transform), so its
    /// model list shows them WITH merged info; before the fix the
    /// `ConsoleProviders` snapshot was consumed solely by
    /// `CatalogModelResolver::bind` and never reached any listing.
    #[tokio::test]
    async fn console_provider_models_list_with_merged_info() {
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        // A console org-config projection: one standalone provider with
        // cost/limit/capabilities config, one merged onto a catalog base
        // (canonical anthropic, base model claude-opus-4-5).
        let config = serde_json::from_str::<oc_core::integration::console::ConsoleConfig>(
            r#"{
            "providers": {
                "acme-console": {
                    "name": "Acme Console",
                    "settings": { "baseURL": "https://acme.test/v1" },
                    "models": {
                        "acme-large": {
                            "name": "Acme Large",
                            "capabilities": { "input": ["text"] },
                            "cost": { "input": 3.5, "output": 15.25 },
                            "limit": { "context": 200000, "output": 32768 }
                        }
                    }
                },
                "acme-anthropic": {
                    "canonical": "anthropic",
                    "settings": { "baseURL": "https://acme.test/v2" },
                    "models": {
                        "acme-opus": { "modelID": "claude-opus-4-5", "limit": { "output": 12345 } }
                    }
                }
            }
        }"#,
        )
        .expect("console config decodes");
        // The same shared snapshot `bind` reads (engine/mod.rs:486,827):
        // the host's projection (`to_custom_providers`) already produced
        // these entries; the test swaps them in directly.
        engine
            .integration
            .console_providers()
            .swap_for_test(oc_core::integration::console::to_custom_providers(&config));
        let entries = location_model_entries(&engine, &location).await;

        // (1) The standalone console provider lists with its merged
        // values: configured name, capability override + default
        // fallbacks, per-field limit, replacement cost with zero cache.
        let large = entries
            .iter()
            .find(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("acme-console")
                    && model.get("id").and_then(Value::as_str) == Some("acme-large")
            })
            .expect("the console provider's model appears in the listing");
        assert_eq!(large["name"], json!("Acme Large"));
        assert_eq!(large["capabilities"]["input"], json!(["text"]), "configured input wins");
        assert_eq!(large["capabilities"]["output"], json!(["text"]), "default output fallback");
        assert_eq!(large["capabilities"]["tools"], json!(true), "default tools fallback");
        assert_eq!(large["limit"]["context"], json!(200_000));
        assert_eq!(large["limit"]["output"], json!(32_768));
        assert_eq!(
            large["cost"],
            json!([{ "input": 3.5, "output": 15.25, "cache": { "read": 0, "write": 0 } }]),
            "cost REPLACES with cache read/write defaulting to 0.0"
        );
        // Shape parity with the config-custom entries: the SAME field
        // set, no more no less (clients parse one Model.Info shape).
        let expected = [
            "id",
            "modelID",
            "providerID",
            "name",
            "enabled",
            "status",
            "time",
            "capabilities",
            "limit",
            "cost",
            "variants",
        ];
        let object = large.as_object().expect("model entry is an object");
        assert_eq!(object.len(), expected.len(), "same fields as config customs: {object:?}");
        for key in expected {
            assert!(object.contains_key(key), "missing field {key}");
        }

        // (2) The catalog-base merge carries into the listing: the base's
        // context limit survives, the console output override wins, and
        // the base's release date sorts it among the dated models —
        // BEFORE the zero-released standalone entry.
        let opus = entries
            .iter()
            .find(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("acme-anthropic")
                    && model.get("id").and_then(Value::as_str) == Some("acme-opus")
            })
            .expect("the base-merged console model appears in the listing");
        let catalog = engine.catalog_service.get();
        let base = catalog
            .provider("anthropic")
            .expect("anthropic in catalog")
            .models
            .iter()
            .find(|model| model.id.as_str() == "claude-opus-4-5")
            .expect("claude-opus-4-5 in catalog");
        assert_eq!(opus["limit"]["context"], json!(base.limit.context), "base context survives");
        assert_eq!(opus["limit"]["output"], json!(12_345), "console output override wins");
        assert_eq!(opus["time"]["released"], json!(base.time.released.0), "base release date");
        assert_eq!(opus["modelID"], json!("claude-opus-4-5"), "modelID redirects to the base");
        let opus_at = entries.iter().position(|model| std::ptr::eq(model, opus)).expect("pos");
        let large_at = entries.iter().position(|model| std::ptr::eq(model, large)).expect("pos");
        assert!(
            opus_at < large_at,
            "the dated base-merged entry sorts before the zero-released standalone"
        );
    }

    /// Review round 2 (console model-miss): when the config `model`
    /// default pins a console provider at a model id the provider does
    /// NOT carry, `/api/model/default` serves v2's exact-map-miss
    /// fallback — `available.find(supportsText)` (model.ts:260-266),
    /// the FIRST text model of the same ordering the list serves —
    /// never the empty-identity stub the miss used to synthesize
    /// (`custom_model_info`'s zero shape: `{"id":"","modelID":"",…}`).
    #[tokio::test]
    async fn model_default_console_model_miss_falls_back() {
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        // The location config pins the default at the console provider
        // with a BOGUS model id (an exact-map miss against the console
        // provider's models).
        std::fs::write(
            tmp.path().join("opencode.json"),
            r#"{ "model": "acme-console/no-such-model" }"#,
        )
        .expect("write config");
        let config = serde_json::from_str::<oc_core::integration::console::ConsoleConfig>(
            r#"{
            "providers": {
                "acme-console": {
                    "name": "Acme Console",
                    "settings": { "baseURL": "https://acme.test/v1" },
                    "models": { "acme-large": { "name": "Acme Large" } }
                }
            }
        }"#,
        )
        .expect("console config decodes");
        engine
            .integration
            .console_providers()
            .swap_for_test(oc_core::integration::console::to_custom_providers(&config));

        let state = std::sync::Arc::new(crate::state::AppState::new(
            engine,
            crate::state::AuthConfig { password: None },
            "test".to_owned(),
            std::sync::Arc::new(oc_util::determinism::UuidV4IdGen),
            std::sync::Arc::new(oc_util::determinism::SystemClock),
        ));
        let uri: Uri = format!(
            "http://localhost/api/model/default?location[directory]={}",
            tmp.path().display()
        )
        .parse()
        .expect("request uri");
        let response = model_default(State(state.clone()), uri, HeaderMap::new())
            .await
            .expect("model.default responds");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&bytes).expect("envelope json");
        let data = &payload["data"];
        // A REAL registry entry, never the empty-identity stub.
        let id = data.get("id").and_then(Value::as_str).unwrap_or_default();
        assert!(!id.is_empty(), "the miss falls back, never an empty entry: {data}");
        // Exactly v2's fallback: the first supportsText entry of the
        // same ordering /api/model serves.
        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let expected = location_model_entries(&state.engine, &location)
            .await
            .into_iter()
            .find(supports_text)
            .expect("an available text model exists");
        assert_eq!(data, &expected, "the default is the first available text model");
    }

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
        let entries = location_model_entries(&engine, &location).await;
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
            fallback.get("capabilities").is_some_and(serde_json::Value::is_object),
            "the fallback keeps the full Model.Info shape"
        );
    }

    /// RF-01 — running local providers (ollama/lmstudio/vllm runtime
    /// discovery) list through the same seam as console providers,
    /// rendered with the discovered names/capabilities/limits, and the
    /// `model` default selection resolves a discovered model.
    #[tokio::test]
    async fn local_discovery_models_list_and_default() {
        use axum::routing::{get, post};
        use serde_json::json;

        // A loopback Ollama (ADR-0006: loopback-only mock, no real
        // network): tags + a tool-capable show for llama3.
        let app = axum::Router::new()
            .route(
                "/api/tags",
                get(|| async {
                    axum::Json(json!({ "models": [
                        { "name": "Llama 3", "model": "llama3", "digest": "d1",
                          "details": { "family": "llama" } }
                    ] }))
                }),
            )
            .route(
                "/api/show",
                post(|| async {
                    axum::Json(json!({
                        "capabilities": ["completion", "tools"],
                        "details": { "family": "llama" },
                        "model_info": { "llama3.context_length": 8192 }
                    }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move { axum::serve(listener, app).await.expect("mock serves") });

        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::fs::write(tmp.path().join("opencode.json"), r#"{ "model": "ollama/llama3" }"#)
            .expect("write config");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let mut connections = std::collections::BTreeMap::new();
        connections.insert(
            "ollama".to_owned(),
            oc_core::provider_catalog::facade::ModelConnection {
                base_url: Some(format!("http://127.0.0.1:{port}/v1")),
                api_key: None,
                headers: Default::default(),
            },
        );
        engine.local.refresh(&connections).await;

        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let entries = location_model_entries(&engine, &location).await;
        let llama = entries
            .iter()
            .find(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("ollama")
                    && model.get("id").and_then(Value::as_str) == Some("llama3")
            })
            .expect("the discovered ollama model appears in the listing");
        assert_eq!(llama["name"], json!("Llama 3"));
        assert_eq!(llama["capabilities"]["tools"], json!(true));
        assert_eq!(llama["capabilities"]["input"], json!(["text"]));
        assert_eq!(llama["limit"]["context"], json!(8192));
        assert_eq!(llama["cost"], json!([]), "discovered models are cost-free");

        // The configured default `ollama/llama3` resolves through the
        // discovery snapshot, not the (missing) catalog entry.
        let state = std::sync::Arc::new(crate::state::AppState::new(
            engine,
            crate::state::AuthConfig { password: None },
            "test".to_owned(),
            std::sync::Arc::new(oc_util::determinism::UuidV4IdGen),
            std::sync::Arc::new(oc_util::determinism::SystemClock),
        ));
        let uri: Uri = format!(
            "http://localhost/api/model/default?location[directory]={}",
            tmp.path().display()
        )
        .parse()
        .expect("request uri");
        let response = model_default(State(state), uri, HeaderMap::new())
            .await
            .expect("model.default responds");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&bytes).expect("envelope json");
        assert_eq!(payload["data"]["providerID"], json!("ollama"));
        assert_eq!(payload["data"]["id"], json!("llama3"));
    }
}
