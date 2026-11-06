//! Modal dynamic model surface (G-209; port of
//! `plugin/provider/modal.ts` over `modal/models.ts`): when a Modal
//! API credential exists, fetch the model list from the Modal API and
//! synthesize the provider's models with price/limit/variant templates
//! per the ref's derive logic.
//!
//! The fetch itself is [`ModalSync::refresh`] — production callers
//! (engine boot) run it against the env/credential binding; tests seed
//! the state directly through [`ModalSync::seed_for_test`], so no
//! network access ever happens in the test harness (ADR-0006).
//!
//! Deviations (bounded): the ref reads the `baseURL` from the
//! CONFIGURED provider record (`providers.get("modal").settings`);
//! the port uses the catalog provider's settings (the static snapshot
//! bakes the default Modal API origin in), and the API key comes from
//! the env binding (`MODAL_PROXY_TOKEN`) or a stored credential — the
//! same sources the ref's `ctx.integration.connection` resolves from.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_ai::route::transport::{read_body_limited, SharedTransport, TransportRequest};
use oc_schema::common::{Num, UsdPerMillionTokens};
use oc_schema::id::{ModelId, ModelVariantId, ProviderId};
use oc_schema::model as schema_model;
use oc_schema::model::{Cost, CostCache, ModelLimit, ModelTime, Status};
use serde_json::Value;

use crate::provider_catalog::{Catalog, Snapshot};

/// The modal provider id (modal.ts `providerID`).
pub const MODAL_PROVIDER_ID: &str = "modal";

/// The package every derived modal model runs on (models.ts:110).
pub const MODAL_PACKAGE: &str = "@opencode/ai/providers/openai-compatible";

/// `load`'s 3-second fetch timeout (models.ts:61).
pub const MODAL_MODELS_TIMEOUT_MS: u64 = 3_000;

/// The `/models` body cap for the refresh fetch (the ref relies on
/// Bun's stream; the port bounds it like the copilot loader).
pub const MODAL_MODELS_BODY_LIMIT: usize = 4 * 1024 * 1024;

/// The env var carrying the Modal proxy token (the provider's `env`
/// surface in the models.dev snapshot; modal.ts:30's
/// `process.env.MODAL_PROXY_TOKEN` fallback).
pub const MODAL_PROXY_TOKEN_ENV: &str = "MODAL_PROXY_TOKEN";

// ---------------------------------------------------------------------------
// The remote model (models.ts RemoteModel, decoded tolerantly)
// ---------------------------------------------------------------------------

/// `RemoteModel.pricing` (models.ts:24-30): string-or-number per-token
/// USD prices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModalPricing {
    pub prompt: Option<Value>,
    pub completion: Option<Value>,
    pub input_cache_read: Option<Value>,
}

/// One `reasoning_options` entry (`{type: "effort", values}`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModalReasoningOption {
    pub values: Vec<Option<String>>,
}

/// One Modal `/models` row, already tolerance-filtered (a row missing
/// its `id` drops, exactly the ref's `decodeUnknownOption` skip).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModalRemoteModel {
    pub id: String,
    pub base_model_id: Option<String>,
    pub hugging_face_id: Option<String>,
    pub name: Option<String>,
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    pub context_length: Option<f64>,
    pub max_output_length: Option<f64>,
    pub pricing: Option<ModalPricing>,
    pub supported_features: Option<Vec<String>>,
    pub reasoning_options: Option<Vec<ModalReasoningOption>>,
    pub interleaved: Option<Value>,
}

fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

fn number(value: Option<&Value>) -> Option<f64> {
    value?.as_f64()
}

/// Decode one raw `/models` row; `None` when it is not a RemoteModel
/// (models.ts:67-70 — one malformed entry never discards the batch).
#[must_use]
pub fn parse_modal_model(raw: &Value) -> Option<ModalRemoteModel> {
    let object = raw.as_object()?;
    let id = object.get("id")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    let pricing = object.get("pricing").and_then(Value::as_object).map(|pricing| ModalPricing {
        prompt: pricing.get("prompt").cloned(),
        completion: pricing.get("completion").cloned(),
        input_cache_read: pricing.get("input_cache_read").cloned(),
    });
    let reasoning_options =
        object.get("reasoning_options").and_then(Value::as_array).map(|options| {
            options
                .iter()
                .filter_map(|option| {
                    // Only `{type: "effort"}` rows decode (ReasoningOption).
                    if option.get("type").and_then(Value::as_str) != Some("effort") {
                        return None;
                    }
                    Some(ModalReasoningOption {
                        values: option
                            .get("values")
                            .and_then(Value::as_array)
                            .map(|values| {
                                values
                                    .iter()
                                    .map(|value| value.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        });
    Some(ModalRemoteModel {
        id,
        base_model_id: object.get("base_model_id").and_then(Value::as_str).map(str::to_string),
        hugging_face_id: object.get("hugging_face_id").and_then(Value::as_str).map(str::to_string),
        name: object.get("name").and_then(Value::as_str).map(str::to_string),
        input_modalities: strings(object.get("input_modalities")),
        output_modalities: strings(object.get("output_modalities")),
        context_length: number(object.get("context_length")),
        max_output_length: number(object.get("max_output_length")),
        pricing,
        supported_features: strings(object.get("supported_features")),
        reasoning_options,
        interleaved: object.get("interleaved").cloned(),
    })
}

/// `load`'s decode half (models.ts:56-71) over a response body.
#[must_use]
pub fn parse_modal_models_body(body: &str) -> Result<Vec<ModalRemoteModel>, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| format!("Failed to fetch Modal models: {error}"))?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Failed to fetch Modal models: malformed response".to_string())?;
    Ok(data.iter().filter_map(parse_modal_model).collect())
}

// ---------------------------------------------------------------------------
// Derive (models.ts:74-151)
// ---------------------------------------------------------------------------

/// `price` (models.ts:85-89): per-token USD × 1e6 → per-million; a
/// non-finite parse keeps the fallback.
fn price(value: Option<&Value>, fallback: Num) -> Num {
    let Some(value) = value else { return fallback };
    let parsed = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    };
    match parsed.map(|value| value * 1_000_000.0) {
        Some(parsed) if parsed.is_finite() => Num(parsed),
        _ => fallback,
    }
}

/// `limit` (models.ts:91-94): `Math.trunc`, integers ≥ 0 only.
fn limit(value: Option<f64>, fallback: i64) -> i64 {
    let Some(value) = value else { return fallback };
    let parsed = value.trunc();
    if parsed >= 0.0 && parsed.fract() == 0.0 {
        parsed as i64
    } else {
        fallback
    }
}

/// `variants` (models.ts:141-151): one variant per distinct effort
/// value across the reasoning options (`null` → "none"),
/// `{reasoningEffort: effort}` settings, first-seen order.
fn variants(remote: &ModalRemoteModel) -> Vec<schema_model::Variant> {
    let mut seen: Vec<String> = Vec::new();
    for option in remote.reasoning_options.as_deref().unwrap_or(&[]) {
        for value in &option.values {
            let effort = value.clone().unwrap_or_else(|| "none".to_string());
            if !seen.contains(&effort) {
                seen.push(effort);
            }
        }
    }
    seen.into_iter()
        .map(|effort| schema_model::Variant {
            id: ModelVariantId::parse(effort.clone()),
            settings: Some(oc_schema::provider::Settings {
                rest: BTreeMap::from([("reasoningEffort".to_string(), Value::String(effort))]),
                ..Default::default()
            }),
            headers: None,
            body: None,
        })
        .collect()
}

/// `build` (models.ts:96-139): one derived model over the template.
#[must_use]
pub fn modal_model_info(
    remote: &ModalRemoteModel,
    base_url: &str,
    previous: Option<&schema_model::Info>,
) -> schema_model::Info {
    let provider_id = ProviderId::parse(MODAL_PROVIDER_ID);
    let cost = previous.and_then(|info| info.cost.first());
    let input_limit = previous.and_then(|info| info.limit.input);
    // `Provider.mergeOverlay(previous?.settings, {baseURL, provider})`:
    // the previous settings overlaid with the Modal endpoint pair.
    let mut settings = previous.and_then(|info| info.settings.clone()).unwrap_or_default();
    settings.rest.insert("baseURL".to_string(), Value::String(base_url.to_string()));
    settings.rest.insert("provider".to_string(), Value::String(MODAL_PROVIDER_ID.to_string()));
    schema_model::Info {
        id: ModelId::parse(remote.id.clone()),
        model_id: ModelId::parse(remote.id.clone()),
        provider_id,
        canonical: None,
        name: remote
            .name
            .clone()
            .or_else(|| previous.map(|info| info.name.clone()))
            .unwrap_or_else(|| remote.id.clone()),
        family: previous.and_then(|info| info.family.clone()),
        compatibility: match remote.interleaved.as_ref() {
            // `Model.compatibility(interleaved) ?? previous?.compatibility`:
            // booleans yield undefined (model.ts:292-296); `{field}` maps.
            Some(Value::Object(object)) => object
                .get("field")
                .and_then(Value::as_str)
                .map(|field| schema_model::Compatibility {
                    reasoning_field: Some(field.to_string()),
                    ..Default::default()
                })
                .or(previous.and_then(|info| info.compatibility.clone())),
            _ => previous.and_then(|info| info.compatibility.clone()),
        },
        package: Some(MODAL_PACKAGE.to_string()),
        settings: Some(settings),
        headers: previous.and_then(|info| info.headers.clone()),
        body: previous.and_then(|info| info.body.clone()),
        capabilities: oc_schema::model::Capabilities {
            tools: match &remote.supported_features {
                Some(features) => features.iter().any(|feature| feature == "tools"),
                None => previous.map(|info| info.capabilities.tools).unwrap_or(true),
            },
            input: remote
                .input_modalities
                .clone()
                .or_else(|| previous.map(|info| info.capabilities.input.clone()))
                .unwrap_or_else(|| vec!["text".to_string()]),
            output: remote
                .output_modalities
                .clone()
                .or_else(|| previous.map(|info| info.capabilities.output.clone()))
                .unwrap_or_else(|| vec!["text".to_string()]),
        },
        variants: match &remote.reasoning_options {
            None => previous.map(|info| info.variants.clone()).unwrap_or_default(),
            Some(_) => variants(remote),
        },
        time: previous.map(|info| info.time).unwrap_or(ModelTime { released: Num(0.0) }),
        cost: vec![Cost {
            tier: None,
            input: UsdPerMillionTokens(price(
                remote.pricing.as_ref().and_then(|pricing| pricing.prompt.as_ref()),
                cost.map(|cost| cost.input.0).unwrap_or(Num(0.0)),
            )),
            output: UsdPerMillionTokens(price(
                remote.pricing.as_ref().and_then(|pricing| pricing.completion.as_ref()),
                cost.map(|cost| cost.output.0).unwrap_or(Num(0.0)),
            )),
            cache: CostCache {
                read: UsdPerMillionTokens(price(
                    remote.pricing.as_ref().and_then(|pricing| pricing.input_cache_read.as_ref()),
                    cost.map(|cost| cost.cache.read.0).unwrap_or(Num(0.0)),
                )),
                write: UsdPerMillionTokens(cost.map(|cost| cost.cache.write.0).unwrap_or(Num(0.0))),
            },
        }],
        status: previous.map(|info| info.status).unwrap_or(Status::Active),
        enabled: previous.map(|info| info.enabled).unwrap_or(true),
        limit: ModelLimit {
            context: limit(
                remote.context_length,
                previous.map(|info| info.limit.context).unwrap_or(0),
            ),
            input: input_limit,
            output: limit(
                remote.max_output_length,
                previous.map(|info| info.limit.output).unwrap_or(0),
            ),
        },
    }
}

/// `derive` (models.ts:74-83): combine remote facts with the source
/// templates — the template is looked up by
/// `base_model_id ?? hugging_face_id ?? id` over the provider's
/// EXISTING models ("never a previously transformed model result"),
/// and the derived list (keyed by remote id, later rows winning)
/// REPLACES the provider's model set (modal.ts:59-65 `evt.add`).
#[must_use]
pub fn derive_modal_models(
    base_url: &str,
    remote: &[ModalRemoteModel],
    existing: &[schema_model::Info],
) -> Vec<schema_model::Info> {
    let templates: BTreeMap<&str, &schema_model::Info> =
        existing.iter().map(|model| (model.id.as_str(), model)).collect();
    let mut result: BTreeMap<String, schema_model::Info> = BTreeMap::new();
    for item in remote {
        let template_key =
            item.base_model_id.as_deref().or(item.hugging_face_id.as_deref()).unwrap_or(&item.id);
        let template = templates.get(template_key).copied();
        result.insert(item.id.clone(), modal_model_info(item, base_url, template));
    }
    result.into_values().collect()
}

/// The provider transform (modal.ts:55-66): the derived models replace
/// the static modal listing; provider info/environment stay the static
/// record's.
#[must_use]
pub fn merge_modal_snapshot(
    snapshot: &Snapshot,
    base_url: &str,
    remote: &[ModalRemoteModel],
) -> Snapshot {
    Snapshot {
        info: snapshot.info.clone(),
        models: derive_modal_models(base_url, remote, &snapshot.models),
        environment: snapshot.environment.clone(),
    }
}

// ---------------------------------------------------------------------------
// The sync (the plugin's `load` + transform, offline-testable)
// ---------------------------------------------------------------------------

/// One successful sync: the resolved base URL, the bound token, the
/// raw remote listing, and the derived provider snapshot.
pub struct ModalSyncState {
    pub base_url: String,
    pub token: String,
    pub remote: Vec<ModalRemoteModel>,
    pub merged: Snapshot,
}

/// The synced modal surface shared by the resolver (dynamic model
/// merge): refreshed by the engine against the env/credential binding.
/// A missing key/baseURL clears the state (modal.ts:33-38); a failed
/// fetch keeps the previous state (modal.ts:42-43).
pub struct ModalSync {
    transport: SharedTransport,
    state: parking_lot::RwLock<Option<Arc<ModalSyncState>>>,
}

impl ModalSync {
    #[must_use]
    pub fn new(transport: SharedTransport) -> Self {
        Self { transport, state: parking_lot::RwLock::new(None) }
    }

    /// The current sync state (shared handle, cloned out of the cell).
    #[must_use]
    pub fn state(&self) -> Option<Arc<ModalSyncState>> {
        self.state.read().clone()
    }

    /// Clear the loaded state (no key / no baseURL — modal.ts:33-38).
    pub fn clear(&self) {
        *self.state.write() = None;
    }

    /// Production refresh (modal.ts `load`): fetch `{base_url}/models`
    /// with the bearer token (3s timeout) and rebuild the derived
    /// snapshot. A failed fetch keeps the previous state.
    pub async fn refresh(&self, catalog: &Catalog, base_url: String, token: String) {
        let url = format!("{}/models", base_url.trim_end_matches('/'));
        let request = TransportRequest {
            method: "GET".to_string(),
            url: url.clone(),
            headers: vec![("Authorization".to_string(), format!("Bearer {token}"))],
            body: Default::default(),
        };
        let Ok(response) = tokio::time::timeout(
            std::time::Duration::from_millis(MODAL_MODELS_TIMEOUT_MS),
            self.transport.execute(request),
        )
        .await
        else {
            return; // modal.ts:42-43 — log + keep the previous state
        };
        let Ok(response) = response else {
            return;
        };
        if !(200..300).contains(&response.status) {
            // models.ts:63 — `!response.ok` fails the fetch.
            return;
        }
        if let Ok(body) = read_body_limited(response.body, MODAL_MODELS_BODY_LIMIT).await {
            if let Ok(remote) = parse_modal_models_body(&body) {
                self.store(catalog, base_url, token, remote);
            }
        }
    }

    /// Rebuild + store the derived state over an already-loaded remote
    /// snapshot (the offline seam: tests seed state without network).
    pub fn store(
        &self,
        catalog: &Catalog,
        base_url: String,
        token: String,
        remote: Vec<ModalRemoteModel>,
    ) {
        let Some(static_snapshot) = catalog.provider(MODAL_PROVIDER_ID) else {
            return;
        };
        let merged = merge_modal_snapshot(static_snapshot, &base_url, &remote);
        *self.state.write() = Some(Arc::new(ModalSyncState { base_url, token, remote, merged }));
    }

    /// Test seam: seed the synced state directly (ADR-0006 — the
    /// production fetch path must not run during tests).
    pub fn seed_for_test(
        &self,
        catalog: &Catalog,
        base_url: &str,
        token: &str,
        remote: Vec<ModalRemoteModel>,
    ) {
        self.store(catalog, base_url.to_string(), token.to_string(), remote);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sync() -> ModalSync {
        ModalSync::new(
            Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new()),
        )
    }

    fn remote_row(raw: Value) -> Option<ModalRemoteModel> {
        parse_modal_model(&raw)
    }

    /// G-209 — a seeded Modal API payload derives model entries with
    /// the ref's exact per-field math (models.ts:96-151): templates
    /// resolve by `base_model_id ?? hugging_face_id ?? id`, prices
    /// multiply per-token USD by 1e6, limits trunc, variants dedupe
    /// efforts (`null` → "none"), and the derived list REPLACES the
    /// provider's static models.
    #[test]
    fn seeded_payload_derives_models_per_ref_math() {
        let catalog = Catalog::embedded();
        let static_snapshot =
            catalog.provider(MODAL_PROVIDER_ID).expect("modal provider in the catalog");
        let template = static_snapshot
            .models
            .iter()
            .find(|model| model.id.as_str() == "thinkingmachines/Inkling-NVFP4")
            .expect("the static template");
        // The template's carried facts (the fallbacks the ref uses).
        let template_cost = &template.cost[0];
        let template_family = template.family.clone();

        // The seeded Modal API `/models` rows.
        let body = json!({
            "data": [
                {
                    "id": "my-finetune",
                    "base_model_id": "thinkingmachines/Inkling-NVFP4",
                    "name": "My Finetune",
                    "input_modalities": ["text", "image"],
                    "context_length": 123456.7,
                    "max_output_length": 8192.25,
                    "pricing": { "prompt": "0.0000035", "completion": 0.000012 },
                    "supported_features": ["tools", "json"],
                    "reasoning_options": [
                        { "type": "effort", "values": ["low", null, "low"] }
                    ],
                    "interleaved": { "field": "reasoning_details" }
                },
                {
                    "id": "Qwen/Qwen3.8-2.4T-A95B",
                    "context_length": 42
                    // No name/pricing → the template's facts carry over.
                },
                { "no_id": true },       // malformed row drops
                "not even an object",    // ditto
            ]
        });
        let remote = parse_modal_models_body(&body.to_string()).expect("decodes");
        assert_eq!(remote.len(), 2, "malformed rows drop, valid ones stay");

        let merged = merge_modal_snapshot(static_snapshot, "https://modal.example/v1", &remote);
        assert_eq!(
            merged.models.len(),
            2,
            "the derived list replaces the static models (modal.ts evt.add)"
        );
        assert_eq!(merged.info.id.as_str(), MODAL_PROVIDER_ID, "info stays the static record's");

        let finetune = merged
            .models
            .iter()
            .find(|model| model.id.as_str() == "my-finetune")
            .expect("the derived finetune");
        // Price math (models.ts:85-89): per-token × 1e6, strings parse.
        assert_eq!(finetune.cost[0].input.0, Num(3.5));
        assert_eq!(finetune.cost[0].output.0, Num(12.0));
        // Absent input_cache_read → the template's cache read carries.
        assert_eq!(finetune.cost[0].cache.read.0, template_cost.cache.read.0);
        assert_eq!(finetune.cost[0].cache.write.0, template_cost.cache.write.0);
        // Limit math (models.ts:91-94): Math.trunc, ≥ 0 integers only.
        assert_eq!(finetune.limit.context, 123_456);
        assert_eq!(finetune.limit.output, 8_192);
        assert_eq!(finetune.limit.input, template.limit.input);
        // Name: remote ?? template ?? id; family from the template.
        assert_eq!(finetune.name, "My Finetune");
        assert_eq!(finetune.family, template_family);
        // Package + the settings overlay (models.ts:110-111).
        assert_eq!(finetune.package.as_deref(), Some(MODAL_PACKAGE));
        let settings = finetune.settings.as_ref().expect("settings");
        assert_eq!(settings.rest.get("baseURL"), Some(&json!("https://modal.example/v1")));
        assert_eq!(settings.rest.get("provider"), Some(&json!("modal")));
        // Compatibility: `{field}` maps (model.ts:292-296).
        assert_eq!(
            finetune.compatibility.as_ref().and_then(|c| c.reasoning_field.as_deref()),
            Some("reasoning_details")
        );
        // Capabilities (models.ts:114-118).
        assert!(finetune.capabilities.tools, "supported_features includes tools");
        assert_eq!(finetune.capabilities.input, vec!["text".to_string(), "image".to_string()]);
        assert_eq!(finetune.capabilities.output, vec!["text".to_string()]);
        // Variants (models.ts:141-151): dedupe, null → "none".
        let efforts: Vec<&str> = finetune.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(efforts, vec!["low", "none"]);
        let low = &finetune.variants[0];
        assert_eq!(
            low.settings.as_ref().expect("variant settings").rest.get("reasoningEffort"),
            Some(&json!("low"))
        );
        // Status/enabled default when no template (new remote model).
        assert_eq!(finetune.status, Status::Active);
        assert!(finetune.enabled);
        assert_eq!(
            finetune.time.released.0, template.time.released.0,
            "time carries from the template"
        );

        // The same-id row keeps the template's name and falls back to
        // the template's per-million cost (no ×1e6 on fallbacks).
        let qwen = merged
            .models
            .iter()
            .find(|model| model.id.as_str() == "Qwen/Qwen3.8-2.4T-A95B")
            .expect("the same-id model");
        let qwen_template = static_snapshot
            .models
            .iter()
            .find(|model| model.id.as_str() == "Qwen/Qwen3.8-2.4T-A95B")
            .expect("static qwen");
        assert_eq!(qwen.name, qwen_template.name);
        assert_eq!(qwen.cost[0].input.0, qwen_template.cost[0].input.0);
        assert_eq!(qwen.limit.context, 42, "remote context_length wins");
        // No reasoning_options → the template's variants carry over.
        assert_eq!(qwen.variants.len(), qwen_template.variants.len());
    }

    /// G-209 — absent credential → inert: an unseeded sync has no
    /// state (the resolver keeps the static modal record), and `clear`
    /// (the ref's no-key/no-baseURL path) empties a loaded one.
    #[test]
    fn absent_credential_is_inert() {
        let catalog = Catalog::embedded();
        let sync = sync();
        assert!(sync.state().is_none(), "no credential → no state");

        // Seed, then clear: modal.ts:33-38 clears on missing key/baseURL.
        sync.seed_for_test(
            catalog,
            "https://modal.example/v1",
            "tk_modal",
            vec![remote_row(json!({ "id": "solo" })).expect("row")],
        );
        assert!(sync.state().is_some());
        let state = sync.state().expect("seeded");
        assert_eq!(state.base_url, "https://modal.example/v1");
        assert_eq!(state.token, "tk_modal");
        assert_eq!(state.merged.models.len(), 1);
        sync.clear();
        assert!(sync.state().is_none(), "clear → inert again");
    }

    /// The `load` decode is tolerant per row but strict on the
    /// envelope: non-`{data: []}` bodies fail (models.ts:44-45,63-70).
    #[test]
    fn body_decode_tolerances_match_ref() {
        assert!(parse_modal_models_body("not json").is_err());
        assert!(parse_modal_models_body("{}").is_err(), "data is required");
        let rows = parse_modal_models_body(
            &json!({ "data": [
                { "id": "ok", "interleaved": true },
                { "id": "", "name": "empty id drops" },
            ]})
            .to_string(),
        )
        .expect("decodes");
        assert_eq!(rows.len(), 1);
        // A boolean interleaved yields compatibility undefined — the
        // template's compatibility carries (model.ts:292-296).
        let derived = modal_model_info(&rows[0], "https://m", None);
        assert!(derived.compatibility.is_none());
    }
}
