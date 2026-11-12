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

/// `Provider.mergeOverlay` (core provider.ts:172-189) at the JSON tier:
/// per-key deep merge, the overlay winning; nested records merge
/// recursively. `None` models the ref's `undefined` (absent overlay).
fn merge_overlay(base: Option<Value>, overlay: Option<Value>) -> Option<Value> {
    match (base, overlay) {
        (None, overlay) => overlay,
        (base, None) => base,
        (Some(mut base), Some(overlay)) => {
            merge_overlay_into(&mut base, &overlay);
            Some(base)
        }
    }
}

fn merge_overlay_into(base: &mut Value, overlay: &Value) {
    if let (Value::Object(base_map), Value::Object(overlay_map)) = (base, overlay) {
        for (key, right) in overlay_map {
            let merged = match base_map.get(key) {
                Some(left) if left.is_object() && right.is_object() => {
                    let mut nested = left.clone();
                    merge_overlay_into(&mut nested, right);
                    nested
                }
                _ => right.clone(),
            };
            base_map.insert(key.clone(), merged);
        }
    }
}

/// The keyless-zen flag (`plugin/provider/opencode.ts:319-325`): the
/// zen tier runs on the public `apiKey` when NO opencode credential
/// exists — neither the `OPENCODE_API_KEY` env var nor a stored
/// credential row for the `opencode` integration.
async fn zen_public(engine: &oc_core::engine::Engine) -> bool {
    if std::env::var_os("OPENCODE_API_KEY").is_some() {
        return false;
    }
    engine
        .integration
        .store()
        .all()
        .await
        .map(|rows| !rows.iter().any(|row| row.integration_id == "opencode"))
        .unwrap_or(false)
}

/// `Provider.modelSettings` (core provider.ts:142-151): the provider
/// settings a served model inherits — everything except the
/// provider-only keys (`timeout`, `headerTimeout`, `chunkTimeout`,
/// `transport`). The keyless zen tier's public `apiKey` rides the same
/// base (`plugin/provider/opencode.ts:323`).
fn provider_model_settings(
    snapshot: &oc_core::provider_catalog::Snapshot,
    public_zen: bool,
) -> Option<Value> {
    let mut value = serde_json::to_value(&snapshot.info.settings).unwrap_or(Value::Null);
    if value.is_null() {
        value = json!({});
    }
    let Value::Object(map) = &mut value else {
        return None;
    };
    for key in ["timeout", "headerTimeout", "chunkTimeout", "transport"] {
        map.remove(key);
    }
    if public_zen && snapshot.info.id.as_str() == "opencode" {
        map.insert("apiKey".to_owned(), json!("public"));
    }
    (!map.is_empty()).then_some(value)
}

fn model_json(
    snapshot: &oc_core::provider_catalog::Snapshot,
    model: &oc_schema::model::Info,
    public_zen: bool,
) -> Value {
    let mut value = serde_json::to_value(model).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut value {
        map.insert("providerID".to_owned(), json!(snapshot.info.id.0));
        // The serving merge every served Model.Info carries (core
        // model.ts:200-210 — `read()` folds the provider record into
        // each model): the provider's canonical wins, `package` falls
        // back to the provider's, and settings/headers/body deep-merge
        // the provider overlays UNDER the model's own.
        if let Some(canonical) = &snapshot.info.canonical {
            map.insert("canonical".to_owned(), json!(canonical.0));
        }
        let package = model.package.clone().unwrap_or_else(|| snapshot.info.package.clone());
        if package.is_empty() {
            map.remove("package");
        } else {
            map.insert("package".to_owned(), json!(package));
        }
        let model_settings = map.get("settings").cloned().filter(|settings| !settings.is_null());
        match merge_overlay(provider_model_settings(snapshot, public_zen), model_settings) {
            Some(merged) => {
                map.insert("settings".to_owned(), merged);
            }
            None => {
                map.remove("settings");
            }
        }
        let model_headers = map.get("headers").cloned().filter(|headers| !headers.is_null());
        let provider_headers = snapshot.info.headers.as_ref().map(|headers| {
            Value::Object(headers.iter().map(|(key, value)| (key.clone(), json!(value))).collect())
        });
        match merge_overlay(provider_headers, model_headers) {
            Some(merged) => {
                map.insert("headers".to_owned(), merged);
            }
            None => {
                map.remove("headers");
            }
        }
        let model_body = map.get("body").cloned().filter(|body| !body.is_null());
        let provider_body = snapshot
            .info
            .body
            .as_ref()
            .map(|body| Value::Object(body.clone().into_iter().collect()));
        match merge_overlay(provider_body, model_body) {
            Some(merged) => {
                map.insert("body".to_owned(), merged);
            }
            None => {
                map.remove("body");
            }
        }
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

/// G-259 — the provider ids the dynamic credential-gated syncs own
/// (G-167 copilot, G-209 modal, G-259 azure/digitalocean): when a sync
/// has state, its merged snapshot replaces the static catalog record
/// for that id at the SAME tier `bind` replaces it
/// (resolver.rs `bind`'s snapshot-replacement chain) — the four ids
/// are distinct, so the resolution order never competes.
const DYNAMIC_SYNC_PROVIDER_IDS: &[&str] = &["github-copilot", "modal", "azure", "digitalocean"];

/// G-259 — the live merged snapshot of whichever dynamic sync owns
/// `provider_id`, if it has synced state (no credential / not yet
/// refreshed → `None`, the static catalog serves). Resolution reads
/// the SAME state through the resolver's `with_*_sync` handles; this
/// is the listing-side accessor keeping `/api/model` in parity with
/// `bind`.
fn dynamic_sync_snapshot(
    engine: &oc_core::engine::Engine,
    provider_id: &str,
) -> Option<oc_core::provider_catalog::Snapshot> {
    match provider_id {
        "github-copilot" => engine.copilot_sync.state().map(|state| state.merged.clone()),
        "modal" => engine.modal_sync.state().map(|state| state.merged.clone()),
        "azure" => engine.azure_sync.state().map(|state| state.merged.clone()),
        "digitalocean" => engine.digitalocean_sync.state().map(|state| state.merged.clone()),
        _ => None,
    }
}

/// The location's ordered model entries — exactly what
/// [`model_list`] serves: `Model.available()` sorted by
/// `time.released` DESCENDING (core model.ts:218-220) over the
/// available providers, with config-defined custom providers
/// activation-"enabled" (config/plugin/provider.ts:67) riding the
/// same stable sort (released = 0, after every dated catalog model).
/// `Model.default`'s fallback (`available.find(supportsText)`,
/// model.ts:260-266) scans the SAME ordering.
pub(crate) async fn location_model_entries(
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
    let public_zen = zen_public(engine).await;
    // G-259 — the dynamic credential-gated syncs ride the SAME listing
    // seam `bind` reads: when a sync has state for its provider id, its
    // merged snapshot REPLACES the static catalog record's models in
    // the listing exactly like the resolver's snapshot replacement —
    // azure's narrowing REMOVES undeployed models from the listing (the
    // "shows everything, fails on selection" defect), and the copilot/
    // modal/digitalocean merges add their dynamic models. The
    // replacement sits at the CATALOG tier — below config customs and
    // console providers exactly like bind's branch order (a config
    // custom with a catalog id falls through to the catalog path with
    // its settings merged, CRED-A4, so its own entries still list
    // alongside the synced catalog surface). Offline (no synced state)
    // the static records list unchanged.
    let mut dynamic: std::collections::BTreeMap<&str, oc_core::provider_catalog::Snapshot> =
        std::collections::BTreeMap::new();
    for &id in DYNAMIC_SYNC_PROVIDER_IDS {
        if let Some(snapshot) = dynamic_sync_snapshot(engine, id) {
            dynamic.insert(id, snapshot);
        }
    }
    let mut data: Vec<(f64, Value)> = Vec::new();
    let mut sync_gate_passed: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for (snapshot, model) in catalog
        .available_models(|name| std::env::var_os(name).is_some() || credential_env.contains(name))
    {
        let id = snapshot.info.id.as_str();
        if dynamic.contains_key(id) {
            // The provider lists (it passed the availability gate), but
            // its STATIC models defer to the synced snapshot below.
            sync_gate_passed.insert(id);
            continue;
        }
        data.push((model.time.released.0, model_json(snapshot, model, public_zen)));
    }
    for (id, synced) in &dynamic {
        if !sync_gate_passed.contains(id) {
            continue; // unavailable offline — the listing stays exactly today's
        }
        for model in &synced.models {
            // The catalog tier's per-model filters (available_models'
            // loop) apply to the synced surface identically.
            if matches!(model.status, oc_schema::model::Status::Deprecated) {
                continue;
            }
            if !model.capabilities.output.iter().any(|item| item == "text") {
                continue;
            }
            data.push((model.time.released.0, model_json(synced, model, public_zen)));
        }
    }
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
            let public_zen = zen_public(&state.engine).await;
            // G-259 — a dynamic-synced provider renders its default
            // through the SAME synced snapshot the listing serves
            // (listing/bind parity: azure's narrowed surface must not
            // serve an undeployed model's static entry). A synced
            // exact-map miss does NOT fall back to the static record —
            // bind refuses it too — and lands in the available-first
            // fallback below exactly like a static miss.
            if let Some(synced) =
                dynamic_sync_snapshot(&state.engine, selection.provider_id.as_str())
            {
                synced
                    .models
                    .iter()
                    .find(|model| model.id.0 == selection.model.as_str())
                    .map(|model| model_json(&synced, model, public_zen))
            } else {
                catalog
                    .providers()
                    .iter()
                    .find(|snapshot| snapshot.info.id.0 == selection.provider_id.as_str())
                    .and_then(|snapshot| {
                        snapshot
                            .models
                            .iter()
                            .find(|model| model.id.0 == selection.model.as_str())
                            .map(|model| model_json(snapshot, model, public_zen))
                    })
            }
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

/// The configured overlay rendered onto a provider record
/// (`config/plugin/provider.ts:81-93`): activation flips "enabled",
/// and `name`/`package`/`settings` merge over the record.
fn apply_custom_overlay(
    value: &mut Value,
    custom: &oc_core::provider_catalog::facade::CustomProvider,
) {
    let Value::Object(map) = value else { return };
    map.insert("activation".to_owned(), json!("enabled"));
    map.insert("name".to_owned(), json!(custom.name));
    if let Some(package) = custom.package.as_deref().filter(|package| !package.is_empty()) {
        map.insert("package".to_owned(), json!(package));
    }
    let mut overlay = json!({});
    if !custom.base_url.is_empty() {
        overlay["baseURL"] = json!(custom.base_url);
    }
    if let Some(api_key) = &custom.api_key {
        overlay["apiKey"] = json!(api_key);
    }
    if overlay.as_object().is_some_and(|settings| !settings.is_empty()) {
        let settings = map.entry("settings").or_insert_with(|| json!({}));
        merge_overlay_into(settings, &overlay);
    }
}

/// A standalone (non-catalog) provider record — `Info.empty` +
/// the configured/console overlay (`config/plugin/provider.ts:67`,
/// `opencode.ts:245-261`): activation "enabled", the configured
/// name/package, and the settings the facade carried.
fn standalone_provider_json(
    custom: &oc_core::provider_catalog::facade::CustomProvider,
    integration_id: Option<&str>,
) -> Value {
    let mut value = json!({
        "id": custom.id,
        "name": custom.name,
        "activation": "enabled",
        "package": custom.package.clone().unwrap_or_default(),
    });
    apply_custom_overlay(&mut value, custom);
    if let Some(integration_id) = integration_id {
        if let Value::Object(map) = &mut value {
            map.insert("integrationID".to_owned(), json!(integration_id));
        }
    }
    value
}

/// The served `Provider.Info` for a catalog snapshot — the record the
/// ref provider registry serves: the models-dev registration's
/// `integrationID` (`plugin/models-dev.ts:41`), the keyless zen flip
/// (activation "enabled" + the public `apiKey`,
/// `plugin/provider/opencode.ts:317-325`), and the configured overlay
/// when a config custom shares the id.
fn served_provider_json(
    snapshot: &oc_core::provider_catalog::Snapshot,
    configured: Option<&oc_core::provider_catalog::facade::CustomProvider>,
    public_zen: bool,
) -> Value {
    let mut value = provider_json(snapshot);
    if let Value::Object(map) = &mut value {
        // `plugin/models-dev.ts:41` — every catalog provider registers
        // its env/key integration under its own id.
        map.insert("integrationID".to_owned(), json!(snapshot.info.id.0));
    }
    if public_zen && snapshot.info.id.as_str() == "opencode" {
        if let Value::Object(map) = &mut value {
            map.insert("activation".to_owned(), json!("enabled"));
            let settings = map.entry("settings").or_insert_with(|| json!({}));
            if let Value::Object(settings) = settings {
                settings.insert("apiKey".to_owned(), json!("public"));
            }
        }
    }
    if let Some(custom) = configured {
        apply_custom_overlay(&mut value, custom);
    }
    value
}

/// The location's served provider entries — `Provider.available()`
/// over `GET /api/provider` (`core provider.ts:393-443`): ONLY the
/// providers that passed the availability fold (keyless zen, an
/// env/credential connection, the bedrock chain, or a configured
/// custom flipping its catalog record enabled), never the full
/// catalog. Console providers ride the `opencode` integration's
/// connection (`opencode.ts:245-261`); discovered local providers
/// register activation-enabled through the local plugin transform.
pub(crate) async fn location_provider_entries(
    engine: &oc_core::engine::Engine,
    location: &oc_core::location::LocationRef,
) -> Vec<Value> {
    let catalog = engine.catalog_service.get();
    // CRED-B1 — availability reads the credential store exactly like
    // the model listing's gate (a stored credential for a provider's
    // integration satisfies it like its env variables do).
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
    let has_env = |name: &str| std::env::var_os(name).is_some() || credential_env.contains(name);
    let public_zen = zen_public(engine).await;
    let configured = crate::boot::location_custom_providers(engine, location);
    let local = engine.local.providers();

    let available: std::collections::HashSet<&str> = catalog
        .available_providers(&has_env)
        .into_iter()
        .map(|snapshot| snapshot.info.id.as_str())
        .collect();
    let mut data: Vec<Value> = Vec::new();
    for snapshot in catalog.providers() {
        let id = snapshot.info.id.as_str();
        let configured_hit = configured.iter().find(|custom| custom.id == id);
        if !available.contains(id) && configured_hit.is_none() {
            continue;
        }
        data.push(served_provider_json(snapshot, configured_hit, public_zen));
    }
    // Standalone config customs (no catalog record under the id).
    for provider in &configured {
        if catalog.provider(&provider.id).is_none()
            && !local.iter().any(|entry| entry.id == provider.id)
        {
            data.push(standalone_provider_json(provider, None));
        }
    }
    // Console-issued providers (integration `opencode`, I1): visible
    // through the console connection the fetch required — a config
    // custom with the same id wins (the id already rendered above).
    if credential_integrations.contains("opencode") {
        for provider in crate::boot::console_custom_providers(engine) {
            if configured.iter().any(|custom| custom.id == provider.id)
                || local.iter().any(|entry| entry.id == provider.id)
            {
                continue;
            }
            data.push(standalone_provider_json(&provider, Some("opencode")));
        }
    }
    // Discovered local providers (RF-01): the runtime discovery
    // registers activation-enabled records.
    for provider in &local {
        data.push(standalone_provider_json(provider, None));
    }
    data
}

async fn provider_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let data = location_provider_entries(&state.engine, &location).await;
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
    let public_zen = zen_public(&state.engine).await;
    let configured = crate::boot::location_custom_providers(&state.engine, &location)
        .into_iter()
        .find(|custom| custom.id == provider_id);
    let snapshot =
        catalog.provider(&provider_id).ok_or_else(|| ApiError::provider_not_found(&provider_id))?;
    Ok(json_location_data(
        &location.as_path().to_string_lossy(),
        served_provider_json(snapshot, configured.as_ref(), public_zen),
    ))
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
                ..Default::default()
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

    // ------------------------------------------------------------------
    // G-259 — the dynamic credential-gated syncs (G-167 copilot, G-209
    // modal, G-259 azure/digitalocean) riding the listing seam.
    // ------------------------------------------------------------------

    /// Scrub ambient vars that would arm boot-time sync spawns (the
    /// azure discovery needs a resource NAME; modal reads its token
    /// synchronously at boot), then arm the listing's availability
    /// gate for the synced providers (their env credential vars —
    /// none of these alone arms a spawn: copilot needs a stored
    /// binding, DO a stored credential, azure a resource name).
    fn arm_sync_listing_env() {
        for name in
            ["AZURE_RESOURCE_NAME", "AZURE_COGNITIVE_SERVICES_RESOURCE_NAME", "MODAL_PROXY_TOKEN"]
        {
            std::env::remove_var(name);
        }
        std::env::set_var("AZURE_API_KEY", "test-key");
        std::env::set_var("GITHUB_TOKEN", "test-token");
        std::env::set_var("DIGITALOCEAN_ACCESS_TOKEN", "test-token");
    }

    fn copilot_remote(id: &str) -> oc_ai::protocols::copilot::CopilotRemoteModel {
        serde_json::from_value(json!({
            "model_picker_enabled": true,
            "id": id,
            "name": format!("Model {id}"),
            "version": format!("{id}-2026-03-05"),
            "supported_endpoints": ["/chat/completions"],
            "capabilities": {
                "family": "test-family",
                "limits": { "max_output_tokens": 4096, "max_prompt_tokens": 100_000 },
                "supports": { "tool_calls": true }
            }
        }))
        .expect("remote model decodes")
    }

    fn provider_entries<'a>(entries: &'a [Value], provider: &str) -> Vec<&'a Value> {
        entries
            .iter()
            .filter(|model| model.get("providerID").and_then(Value::as_str) == Some(provider))
            .collect()
    }

    /// The catalog tier's per-model filters (`available_models`' loop)
    /// over a static snapshot — the offline expectation.
    fn listable_static(snapshot: &oc_core::provider_catalog::Snapshot) -> usize {
        snapshot
            .models
            .iter()
            .filter(|model| !matches!(model.status, oc_schema::model::Status::Deprecated))
            .filter(|model| model.capabilities.output.iter().any(|item| item == "text"))
            .count()
    }

    /// G-259 — seeded sync states ride `/api/model`'s listing exactly
    /// like `bind`: azure's narrowed snapshot REPLACES the static azure
    /// models (only the deployed id lists, keyed by deployment name
    /// with the catalog model's facts), the copilot merge lists its
    /// dynamic-only model, and the `DigitalOcean` merge ADDS its `router:*`
    /// entry over the static surface.
    #[tokio::test]
    async fn dynamic_syncs_ride_the_listing_seam() {
        arm_sync_listing_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let catalog = engine.catalog_service.get();
        engine.azure_sync.seed_for_test(
            &catalog,
            "test-resource",
            vec![oc_core::provider_catalog::azure_sync::AzureDeployment {
                name: "prod-mini".to_owned(),
                model: "gpt-5-mini".to_owned(),
            }],
        );
        engine.digitalocean_sync.seed_for_test(&catalog, "do-token", vec!["alpha".to_owned()]);
        let mut remote = std::collections::BTreeMap::new();
        remote.insert("enterprise-only-model".to_owned(), copilot_remote("enterprise-only-model"));
        engine.copilot_sync.seed_for_test(
            &catalog,
            "https://api.githubcopilot.com",
            "ghu_token",
            remote,
        );

        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let entries = location_model_entries(&engine, &location).await;

        // azure: the narrowed surface — ONE entry, the deployment name,
        // carrying the catalog model's facts (azure.ts:513-529).
        let azure = provider_entries(&entries, "azure");
        assert_eq!(azure.len(), 1, "only the deployed model lists, not the whole catalog");
        let prod = azure[0];
        assert_eq!(prod["id"], json!("prod-mini"));
        assert_eq!(prod["modelID"], json!("prod-mini"));
        let static_mini = catalog
            .provider("azure")
            .and_then(|snapshot| snapshot.models.iter().find(|m| m.id.as_str() == "gpt-5-mini"))
            .expect("gpt-5-mini in the static catalog");
        assert!(prod["name"].as_str().unwrap_or_default().ends_with("(prod-mini)"));
        assert_eq!(prod["limit"]["context"], json!(static_mini.limit.context));
        assert!(
            !provider_entries(&entries, "azure")
                .iter()
                .any(|model| model["id"] == json!("gpt-5-mini")),
            "an undeployed catalog model is GONE from the listing (the gap's defect)"
        );

        // github-copilot: the merged dynamic surface — the dynamic-only
        // model lists, the static-only ids do not.
        let copilot = provider_entries(&entries, "github-copilot");
        assert_eq!(copilot.len(), 1, "the seeded remote listing replaces the static surface");
        assert_eq!(copilot[0]["id"], json!("enterprise-only-model"));
        assert_eq!(copilot[0]["name"], json!("Model enterprise-only-model"));

        // digitalocean: the merge ADDS router:alpha over the static
        // surface (digitalocean.ts:158-170) — the ref entry shape rides
        // model_json so limits/capabilities/settings carry.
        let router = entries
            .iter()
            .find(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("digitalocean")
                    && model.get("id").and_then(Value::as_str) == Some("router:alpha")
            })
            .expect("router:alpha lists");
        assert_eq!(router["name"], json!("alpha"));
        assert_eq!(router["family"], json!("digitalocean-inference-routers"));
        assert_eq!(router["limit"]["context"], json!(128_000));
        assert_eq!(router["limit"]["output"], json!(8_192));
        assert_eq!(router["settings"]["baseURL"], json!("https://inference.do-ai.run/v1"));
        assert!(
            provider_entries(&entries, "digitalocean")
                .iter()
                .any(|model| model["id"] == json!("deepseek-3.2")),
            "the static digitalocean surface stays (the merge adds, never removes)"
        );

        // No collateral damage: the zen tier still lists.
        assert!(
            !provider_entries(&entries, "opencode").is_empty(),
            "the keyless zen tier is unaffected"
        );
    }

    /// G-259 — offline inertness: without a synced state the listing is
    /// EXACTLY today's — the static azure/copilot surfaces list in full
    /// (same counts as the catalog tier's filters), no dynamic ids.
    #[tokio::test]
    async fn offline_listing_unchanged_without_sync_state() {
        arm_sync_listing_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let catalog = engine.catalog_service.get();
        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let entries = location_model_entries(&engine, &location).await;

        let azure = provider_entries(&entries, "azure");
        assert_eq!(
            azure.len(),
            listable_static(catalog.provider("azure").expect("azure in catalog")),
            "the full static azure surface lists (no narrowing without a credential)"
        );
        assert!(azure.iter().any(|model| model["id"] == json!("gpt-5-mini")));
        assert!(!azure.iter().any(|model| model["id"] == json!("prod-mini")));

        let copilot = provider_entries(&entries, "github-copilot");
        assert_eq!(
            copilot.len(),
            listable_static(catalog.provider("github-copilot").expect("copilot in catalog")),
            "the static copilot surface lists"
        );
        assert!(
            !copilot.is_empty()
                && !copilot.iter().any(|model| model["id"] == json!("enterprise-only-model")),
            "no dynamic-only ids without a sync"
        );
        let digitalocean = provider_entries(&entries, "digitalocean");
        assert!(
            digitalocean.iter().all(|model| model["id"] != json!("router:alpha")),
            "no router entries without a credential"
        );
    }

    /// G-259 — precedence vs a config custom with the same provider id:
    /// bind's branch order keeps a catalog-id custom at the catalog
    /// tier (CRED-A4 — its settings merge onto the catalog record, the
    /// custom is NOT a separate resolution branch), so the listing
    /// shows the custom's OWN entries alongside the SYNCED catalog
    /// surface for that id (never the static one).
    #[tokio::test]
    async fn config_custom_rides_alongside_the_synced_catalog_tier() {
        arm_sync_listing_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::fs::write(
            tmp.path().join("opencode.json"),
            r#"{
                "providers": {
                    "digitalocean": {
                        "package": "@opencode/ai/providers/openai-compatible",
                        "settings": { "baseURL": "http://127.0.0.1:9/v1" },
                        "models": { "my-gateway": { "name": "My Gateway" } }
                    }
                }
            }"#,
        )
        .expect("write config");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let catalog = engine.catalog_service.get();
        engine.digitalocean_sync.seed_for_test(&catalog, "do-token", vec!["alpha".to_owned()]);

        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let entries = location_model_entries(&engine, &location).await;

        // The config custom's own entry lists with its configured name.
        let gateway = entries
            .iter()
            .find(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("digitalocean")
                    && model.get("id").and_then(Value::as_str) == Some("my-gateway")
            })
            .expect("the config custom's model lists");
        assert_eq!(gateway["name"], json!("My Gateway"));

        // The catalog tier for the SAME id serves the SYNCED surface:
        // router:alpha (sync-only) lists, the static models ride the
        // merge unchanged.
        assert!(
            entries.iter().any(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("digitalocean")
                    && model.get("id").and_then(Value::as_str) == Some("router:alpha")
            }),
            "the synced catalog tier lists the router"
        );
        assert!(
            entries.iter().any(|model| {
                model.get("providerID").and_then(Value::as_str) == Some("digitalocean")
                    && model.get("id").and_then(Value::as_str) == Some("deepseek-3.2")
            }),
            "the static models ride the synced snapshot's merge"
        );
    }

    /// G-259 — `/api/model/default` parity: a synced provider's default
    /// renders through the synced snapshot (a deployment-name selection
    /// serves the narrowed entry), and a selection of an undeployed
    /// model is an exact-map miss that falls back to the FIRST
    /// available text model of the same ordering — never the static
    /// record's entry.
    #[tokio::test]
    async fn model_default_serves_the_synced_snapshot() {
        arm_sync_listing_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::fs::write(tmp.path().join("opencode.json"), r#"{ "model": "azure/prod-mini" }"#)
            .expect("write config");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let catalog = engine.catalog_service.get();
        engine.azure_sync.seed_for_test(
            &catalog,
            "test-resource",
            vec![oc_core::provider_catalog::azure_sync::AzureDeployment {
                name: "prod-mini".to_owned(),
                model: "gpt-5-mini".to_owned(),
            }],
        );

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
        let response = model_default(State(state.clone()), uri.clone(), HeaderMap::new())
            .await
            .expect("model.default responds");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&bytes).expect("envelope json");
        assert_eq!(payload["data"]["providerID"], json!("azure"));
        assert_eq!(payload["data"]["id"], json!("prod-mini"));
        assert_eq!(payload["data"]["modelID"], json!("prod-mini"));

        // The undeployed selection is a MISS against the synced
        // snapshot: v2's available-first fallback (model.ts:260-266),
        // never the undeployed model's static entry.
        std::fs::write(tmp.path().join("opencode.json"), r#"{ "model": "azure/gpt-5-mini" }"#)
            .expect("rewrite config");
        let response = model_default(State(state.clone()), uri.clone(), HeaderMap::new())
            .await
            .expect("model.default responds");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&bytes).expect("envelope json");
        let data = &payload["data"];
        assert!(
            !(data["providerID"] == json!("azure") && data["id"] == json!("gpt-5-mini")),
            "the undeployed static entry never serves: {data}"
        );
        let location = oc_core::location::LocationRef::new(tmp.path().to_owned(), None);
        let expected = location_model_entries(&state.engine, &location)
            .await
            .into_iter()
            .find(supports_text)
            .expect("an available text model exists");
        assert_eq!(data, &expected, "the miss falls back to the first available text model");
    }

    // ------------------------------------------------------------------
    // G-394/G-397 — the provider listing and the model serving merge,
    // pinned against the ref server's probed wire shapes (fresh ref
    // boot, no credentials: provider list = the single keyless zen
    // entry; model/default carries package + settings {apiKey,
    // baseURL, provider}).
    // ------------------------------------------------------------------

    /// Scrub every credential var the catalog gate reads so the
    /// keyless expectation holds regardless of ambient environment
    /// (nextest runs each test in its own process, so the mutation is
    /// race-free).
    fn scrub_provider_env() {
        let catalog =
            oc_core::provider_catalog::Catalog::parse(oc_core::provider_catalog::SNAPSHOT_TEXT)
                .expect("catalog parses");
        for snapshot in catalog.providers() {
            for name in &snapshot.environment {
                std::env::remove_var(name);
            }
        }
    }

    async fn provider_list_payload(
        tmp: &tempfile::TempDir,
    ) -> (std::sync::Arc<crate::state::AppState>, Value) {
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
        let state = std::sync::Arc::new(crate::state::AppState::new(
            engine,
            crate::state::AuthConfig { password: None },
            "test".to_owned(),
            std::sync::Arc::new(oc_util::determinism::UuidV4IdGen),
            std::sync::Arc::new(oc_util::determinism::SystemClock),
        ));
        let uri: Uri =
            format!("http://localhost/api/provider?location[directory]={}", tmp.path().display())
                .parse()
                .expect("request uri");
        let response = provider_list(State(state.clone()), uri, HeaderMap::new())
            .await
            .expect("provider.list responds");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&bytes).expect("envelope json");
        (state, payload)
    }

    /// G-394 — the provider listing serves ONLY the availability fold's
    /// survivors (the ref's single keyless zen entry), never the full
    /// catalog, and the entry carries the ref registry's served shape:
    /// the models-dev `integrationID`, the keyless flip (activation
    /// "enabled" + the public `apiKey`), and the catalog baseURL.
    #[tokio::test]
    async fn provider_list_serves_only_available_providers() {
        scrub_provider_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let (_state, payload) = provider_list_payload(&tmp).await;

        assert_eq!(
            payload["location"]["directory"],
            json!(tmp.path().display().to_string()),
            "the location-wrapped envelope"
        );
        let data = payload["data"].as_array().expect("data array");
        assert_eq!(
            data.len(),
            1,
            "keyless: only the zen provider lists, got {:?}",
            data.iter().filter_map(|e| e.get("id")).collect::<Vec<_>>()
        );
        let zen = &data[0];
        // The ref's probed entry (fresh boot): activation flipped
        // enabled, the public apiKey, the models-dev integrationID.
        assert_eq!(zen["id"], json!("opencode"));
        assert_eq!(zen["name"], json!("OpenCode Zen"));
        assert_eq!(zen["activation"], json!("enabled"));
        assert_eq!(zen["integrationID"], json!("opencode"));
        assert_eq!(zen["settings"]["apiKey"], json!("public"));
        assert_eq!(zen["settings"]["baseURL"], json!("https://opencode.ai/zen/v1"));
        let object = zen.as_object().expect("entry object");
        let expected_keys = ["id", "name", "activation", "integrationID", "package", "settings"];
        assert_eq!(object.len(), expected_keys.len(), "exact ref key set: {object:?}");
        for key in expected_keys {
            assert!(object.contains_key(key), "missing {key}");
        }
    }

    /// G-394 — an env credential surfaces the provider; without it the
    /// catalog record stays hidden (`provider.ts:393-406`'s connection
    /// rule).
    #[tokio::test]
    async fn provider_list_env_credential_gates_visibility() {
        scrub_provider_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        let (_state, payload) = provider_list_payload(&tmp).await;
        let ids: Vec<&str> = payload["data"]
            .as_array()
            .expect("data array")
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str))
            .collect();
        assert!(ids.contains(&"anthropic"), "the credentialed provider lists: {ids:?}");
        let anthropic = payload["data"]
            .as_array()
            .expect("data array")
            .iter()
            .find(|entry| entry["id"] == json!("anthropic"))
            .expect("anthropic entry");
        // No keyless flip, no public apiKey: the catalog record as the
        // models-dev registration serves it (integrationID; the
        // native-SDK anthropic record carries no settings at all).
        assert_eq!(anthropic["activation"], json!("auto"));
        assert_eq!(anthropic["integrationID"], json!("anthropic"));
        assert!(
            anthropic["settings"].get("apiKey").is_none(),
            "no public apiKey outside the keyless zen flip: {anthropic}"
        );
    }

    /// G-394 — a config custom flips its record enabled
    /// (`config/plugin/provider.ts:81-93`): a standalone custom lists
    /// as its own entry, and a catalog-id custom surfaces the catalog
    /// record WITHOUT any credential (activation enabled).
    #[tokio::test]
    async fn provider_list_configured_customs_list_enabled() {
        scrub_provider_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::fs::write(
            tmp.path().join("opencode.json"),
            r#"{
                "providers": {
                    "own": {
                        "name": "Own Gateway",
                        "package": "@opencode/ai/providers/openai-compatible",
                        "settings": { "baseURL": "http://127.0.0.1:9/v1" },
                        "models": { "own-model": { "name": "Own Model" } }
                    },
                    "anthropic": {
                        "settings": { "baseURL": "https://proxy.test/v1" }
                    }
                }
            }"#,
        )
        .expect("write config");
        let (_state, payload) = provider_list_payload(&tmp).await;
        let data = payload["data"].as_array().expect("data array");
        let own = data
            .iter()
            .find(|entry| entry["id"] == json!("own"))
            .expect("the standalone custom lists");
        assert_eq!(own["name"], json!("Own Gateway"));
        assert_eq!(own["activation"], json!("enabled"));
        assert_eq!(own["package"], json!("@opencode/ai/providers/openai-compatible"));
        assert_eq!(own["settings"]["baseURL"], json!("http://127.0.0.1:9/v1"));
        let anthropic = data
            .iter()
            .find(|entry| entry["id"] == json!("anthropic"))
            .expect("the catalog-id custom surfaces the catalog record");
        assert_eq!(anthropic["activation"], json!("enabled"));
        assert_eq!(anthropic["settings"]["baseURL"], json!("https://proxy.test/v1"));
        assert_eq!(anthropic["integrationID"], json!("anthropic"));
    }

    /// G-397 — the served Model.Info carries the provider fold
    /// (`model.ts:200-210`): `package` falls back to the provider's,
    /// and settings deep-merge the provider overlays (baseURL, the
    /// keyless public apiKey) UNDER the model's own (the
    /// openai-compatible `provider` key).
    #[tokio::test]
    async fn model_default_serves_the_provider_fold() {
        scrub_provider_env();
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            tmp.path().to_owned(),
            tmp.path().to_owned(),
        ))
        .await;
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
        let data = &payload["data"];
        assert_eq!(data["providerID"], json!("opencode"), "the keyless zen default");
        // The ref's probed default (fledge-alpha-free at probe time —
        // the exact model is release-order dependent, the FIELDS are
        // the contract): package + merged settings.
        assert_eq!(
            data["package"],
            json!("@opencode/ai/providers/openai-compatible"),
            "package falls back to the provider's"
        );
        assert_eq!(data["settings"]["apiKey"], json!("public"));
        assert_eq!(data["settings"]["baseURL"], json!("https://opencode.ai/zen/v1"));
        assert_eq!(data["settings"]["provider"], json!("opencode"));
    }
}
