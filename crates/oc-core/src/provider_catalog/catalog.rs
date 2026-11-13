//! The models.dev catalog: parse `api.json`, normalize into
//! `Provider.Info` + `Model.Info` snapshots (port of
//! `packages/core/src/models-dev.ts` `normalize`), and expose the embedded
//! snapshot (also the pinned parity snapshot for gate-3).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use oc_schema::common::{Num, UsdPerMillionTokens};
use oc_schema::id::{ModelId, ProviderId};
use oc_schema::model as schema_model;
use oc_schema::model::{
    Capabilities, Compatibility, Cost, CostCache, CostTier, CostTierType, ModelLimit, ModelTime,
    Status,
};
use oc_schema::provider::{Activation, Info as ProviderInfo, Settings as ProviderSettings};
use serde_json::Value;

use crate::provider_catalog::native::native_package;
use crate::provider_catalog::source::{parse_catalog, SourceCost, SourceModel, SourceProvider};
use crate::provider_catalog::support::{supports_from_reasoning_options, VariantModel};
use crate::provider_catalog::variant;

/// The embedded snapshot of `https://models.opencode.ai/api.json`, committed
/// at `src/provider_catalog/models-dev-snapshot.txt` (v2 refreshes it via
/// `bun run script/update-models-snapshot.ts` — the Rust port re-copies the
/// pinned file).
pub const SNAPSHOT_TEXT: &str = include_str!("models-dev-snapshot.txt");

/// One provider's normalized catalog entry (v2 `Snapshot`).
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub info: ProviderInfo,
    pub models: Vec<schema_model::Info>,
    pub environment: Vec<String>,
}

/// The normalized catalog with lookup helpers. Immutable once built.
#[derive(Debug, Clone)]
pub struct Catalog {
    providers: Vec<Snapshot>,
}

impl Catalog {
    /// An empty catalog (tests / pre-hydration placeholder).
    #[must_use]
    pub fn empty() -> Self {
        Self { providers: Vec::new() }
    }

    /// Parse + normalize an `api.json` body.
    pub fn parse(text: &str) -> Result<Self, String> {
        Ok(Self { providers: normalize(&parse_catalog(text)?) })
    }

    /// Builder seam: one provider's activation (v2's normalize hardcodes
    /// `auto`; provider plugins — the config provider plugin sets
    /// `enabled` for every configured provider, the zen plugin for the
    /// keyless tier — flip it afterwards).
    #[must_use]
    pub fn with_activation(mut self, provider_id: &str, activation: Activation) -> Self {
        if let Some(snapshot) =
            self.providers.iter_mut().find(|snapshot| snapshot.info.id.0 == provider_id)
        {
            snapshot.info.activation = activation;
        }
        self
    }

    /// The embedded snapshot, decoded + normalized once per process
    /// (v2 `bundledSnapshot` memo).
    #[must_use]
    pub fn embedded() -> &'static Catalog {
        static EMBEDDED: OnceLock<Catalog> = OnceLock::new();
        EMBEDDED.get_or_init(|| Catalog::parse(SNAPSHOT_TEXT).expect("embedded snapshot is valid"))
    }

    /// All provider snapshots (sorted by provider id — the source map's
    /// iteration order is not preserved; v2 insertion order is not load
    /// bearing).
    #[must_use]
    pub fn providers(&self) -> &[Snapshot] {
        &self.providers
    }

    /// Provider count.
    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Total model entries (including experimental modes).
    #[must_use]
    pub fn model_count(&self) -> usize {
        self.providers.iter().map(|snapshot| snapshot.models.len()).sum()
    }

    /// Builder seam: replace one provider's snapshot wholesale (the
    /// copilot dynamic-model merge, G-167 — the resolver folds the live
    /// `/models` listing over the static catalog).
    #[must_use]
    pub fn with_provider_snapshot(mut self, provider_id: &str, snapshot: Snapshot) -> Self {
        if let Some(entry) = self.providers.iter_mut().find(|entry| entry.info.id.0 == provider_id)
        {
            *entry = snapshot;
        }
        self
    }

    /// Look up one provider.
    #[must_use]
    pub fn provider(&self, provider_id: &str) -> Option<&Snapshot> {
        self.providers.iter().find(|snapshot| snapshot.info.id.as_str() == provider_id)
    }

    /// Look up one model of one provider.
    #[must_use]
    pub fn model(&self, provider_id: &str, model_id: &str) -> Option<&schema_model::Info> {
        self.provider(provider_id)?.models.iter().find(|model| model.id.as_str() == model_id)
    }

    /// The flattened catalog pairs **stable-sorted by `time.released`
    /// descending** — v2's `all` list (`packages/core/src/model.ts:
    /// 218-220`), the order `GET /api/model` serves
    /// (`models.available()` filters `all`, never re-sorts). Ties keep
    /// the provider-id/source iteration order (JS `toSorted` is
    /// stable; so is `sort_by`).
    #[must_use]
    pub fn flat_models_by_release(&self) -> Vec<(&Snapshot, &schema_model::Info)> {
        let mut pairs: Vec<(&Snapshot, &schema_model::Info)> = self
            .providers
            .iter()
            .flat_map(|snapshot| snapshot.models.iter().map(move |model| (snapshot, model)))
            .collect();
        pairs.sort_by(|(_, left), (_, right)| {
            right.time.released.0.total_cmp(&left.time.released.0)
        });
        pairs
    }

    /// The AVAILABLE catalog providers — the provider-level half of the
    /// availability fold `Provider.available()` serves over
    /// `GET /api/provider` (`packages/core/src/provider.ts:393-406`;
    /// the per-model filters of [`Catalog::available_models`] do not
    /// apply here). A catalog provider is available under the same
    /// rules documented on [`Catalog::available_models`]: keyless zen,
    /// an env/credential connection, or the bedrock activation chain.
    /// (The ref fold's other branches — `activation: "disabled"` and
    /// the console `sourceConnection` guard — never occur at the
    /// static-catalog tier: no catalog record is disabled and only
    /// console-configured providers carry a source connection, handled
    /// by the serving layer.)
    #[must_use]
    pub fn available_providers(&self, has_env: impl Fn(&str) -> bool) -> Vec<&Snapshot> {
        self.providers.iter().filter(|snapshot| snapshot_available(snapshot, &has_env)).collect()
    }

    /// The AVAILABLE catalog pairs — what v2's `Model.available()`
    /// serves over `GET /api/model` (`packages/core/src/model.ts:221`,
    /// `all.filter((model) => model.enabled)` over the providers that
    /// passed the availability fold, `packages/core/src/provider.ts:
    /// 393-406`). A catalog provider is available when
    ///
    /// * it is the built-in `opencode` zen provider — activation is
    ///   forced "enabled" with the public `apiKey` when no
    ///   `OPENCODE_API_KEY`/connection exists
    ///   (`packages/core/src/plugin/provider/opencode.ts:317-325`), and
    ///   in that keyless state every model with a non-zero cost tier
    ///   is disabled (`:327-342` — an empty cost table stays enabled);
    ///   or
    /// * one of its `environment` credential variables is set — the
    ///   env integration connection rule (`provider.ts:404`; the
    ///   models.dev plugin registers one env method per catalog
    ///   provider, `packages/core/src/plugin/models-dev.ts:35-52`); or
    /// * it is `amazon-bedrock` and the `AmazonBedrockPlugin` activation
    ///   chain applies (G-262②, `plugin/provider/amazon-bedrock.ts:
    ///   31-53`): the narrowed `AWS_BEARER_TOKEN_BEDROCK` env credential
    ///   or any ambient default-chain input (`AWS_PROFILE`,
    ///   `AWS_ACCESS_KEY_ID`, web-identity/container creds) flips
    ///   activation auto→enabled. The raw models.dev names
    ///   (`AWS_SECRET_ACCESS_KEY`, `AWS_REGION`) are `SigV4` chain config,
    ///   not keys, and never gate visibility on their own.
    ///
    /// Deprecated and non-text models never register in v2
    /// (`plugin/models-dev.ts:101-109`) — both filters apply here too.
    /// Sort/order contract matches [`Catalog::flat_models_by_release`].
    #[must_use]
    pub fn available_models(
        &self,
        has_env: impl Fn(&str) -> bool,
    ) -> Vec<(&Snapshot, &schema_model::Info)> {
        let mut pairs: Vec<(&Snapshot, &schema_model::Info)> = Vec::new();
        for snapshot in &self.providers {
            let keyless_zen =
                snapshot.info.id.as_str() == "opencode" && !has_env("OPENCODE_API_KEY");
            if !snapshot_available(snapshot, &has_env) {
                continue;
            }
            for model in &snapshot.models {
                if matches!(model.status, schema_model::Status::Deprecated) {
                    continue;
                }
                if !model.capabilities.output.iter().any(|item| item == "text") {
                    continue;
                }
                if keyless_zen && model.cost.iter().any(|cost| cost.input.0 .0 > 0.0) {
                    continue;
                }
                pairs.push((snapshot, model));
            }
        }
        pairs.sort_by(|(_, left), (_, right)| {
            right.time.released.0.total_cmp(&left.time.released.0)
        });
        pairs
    }
}

// -- normalization (models-dev.ts) --------------------------------------------

/// G-258 — the deprecated catalog aliases the ref filters at snapshot-load
/// time (`plugin/models-dev.ts:94-98`): both are replaced by the canonical
/// `azure`/`google-vertex` providers (config references canonicalize in
/// `config/normalize.rs`, so they never resolve through the catalog).
const FILTERED_PROVIDER_ALIASES: &[&str] = &["azure-cognitive-services", "google-vertex-anthropic"];

/// G-258 — Bedrock catalog entries that require inference profiles on
/// Bedrock Runtime (`plugin/models-dev.ts:11-25`); serving them without a
/// profile fails, so the models-dev plugin drops them at load
/// (`:106-108`). Opus/Sonnet 4.6 support in-region calls in eu-west-2 and
/// stay available.
const BEDROCK_PROFILE_ONLY_IDS: &[&str] = &[
    "amazon.nova-2-lite-v1:0",
    "anthropic.claude-fable-5",
    "anthropic.claude-fable-5-1",
    "anthropic.claude-haiku-4-5-20251001-v1:0",
    "anthropic.claude-opus-4-1-20250805-v1:0",
    "anthropic.claude-opus-4-5-20251101-v1:0",
    "anthropic.claude-opus-4-7",
    "anthropic.claude-opus-4-8",
    "anthropic.claude-opus-5",
    "anthropic.claude-sonnet-4-5-20250929-v1:0",
    "anthropic.claude-sonnet-5",
    "deepseek.r1-v1:0",
    "mistral.pixtral-large-2502-v1:0",
];

/// G-261 — OpenRouter-specific `OpenAI` chat aliases that do not work on the
/// generic path; the ref plugin disables them when present
/// (`plugin/provider/openrouter.ts:16-28`). Custom providers with matching
/// ids stay untouched (the plugin keys on the openrouter package).
const OPENROUTER_DISABLED_MODEL_IDS: &[&str] = &["gpt-5-chat-latest", "openai/gpt-5-chat"];

/// G-262② — ambient inputs the AWS default credential chain can turn
/// into credentials without any key stored in opencode
/// (`plugin/provider/amazon-bedrock.ts:8-14`): any one of them (or a
/// configured shared-config `profile`) flips the provider's activation
/// auto→enabled (`:36-39`), which is what makes Bedrock visible for
/// users whose credentials live entirely in the environment.
const BEDROCK_CHAIN_ENV: [&str; 5] = [
    "AWS_PROFILE",
    "AWS_ACCESS_KEY_ID",
    "AWS_WEB_IDENTITY_TOKEN_FILE",
    "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
    "AWS_CONTAINER_CREDENTIALS_FULL_URI",
];

/// The provider-level availability gate shared by
/// [`Catalog::available_models`] and [`Catalog::available_providers`] —
/// the ref fold's catalog tier (`provider.ts:393-406`): the keyless zen
/// tier, an env credential connection, or (G-262②) the bedrock
/// activation chain (the raw models.dev names feed the `SigV4` chain but
/// are not keys and never gate visibility on their own,
/// `plugin/provider/amazon-bedrock.ts:31-53`).
fn snapshot_available(snapshot: &Snapshot, has_env: &impl Fn(&str) -> bool) -> bool {
    if snapshot.info.id.as_str() == "opencode" {
        // The zen provider is always available (keyless tier or keyed).
        return true;
    }
    if snapshot.info.id.as_str() == "amazon-bedrock" {
        return has_env("AWS_BEARER_TOKEN_BEDROCK")
            || BEDROCK_CHAIN_ENV.iter().any(|name| has_env(name));
    }
    snapshot.environment.iter().any(|name| has_env(name))
}

/// G-285 — catalog providers with chat-completions `prompt_cache_key` body
/// support but no dedicated AI package; the ref plugin sets
/// `supportsPromptCacheKey` on every model of these providers
/// (`plugin/provider/prompt-cache-key.ts:10-23`). Audited per provider;
/// scoped-only support (fastrouter, frogbot) is deliberately absent.
const PROMPT_CACHE_KEY_PROVIDERS: &[&str] = &[
    "abliteration-ai",
    "aihubmix",
    "anyapi",
    "auriko",
    "edenai",
    "jalapeno",
    "llama",
    "morph",
    "snowflake-cortex",
    "tencent-tokenhub",
    "upstage",
    "venice",
];

fn normalize(input: &BTreeMap<String, SourceProvider>) -> Vec<Snapshot> {
    // G-258 — the models-dev plugin's snapshot filter (`snapshots()`,
    // plugin/models-dev.ts:91-113): the two deprecated alias providers
    // never register, and the profile-only Bedrock ids drop at load. Both
    // levels apply to the embedded and every refreshed snapshot alike
    // (the ref memoizes the same filter over each fetched body).
    input
        .values()
        .filter(|item| !FILTERED_PROVIDER_ALIASES.contains(&item.id.as_str()))
        .map(normalize_provider)
        .collect()
}

fn normalize_provider(item: &SourceProvider) -> Snapshot {
    let provider_id = ProviderId::parse(item.id.clone());
    let package = native_package(&item.npm, &item.id, None, None);

    let mut settings = None;
    if let Some(api) = &item.api {
        // cloudflare-workers-ai keeps the api out of settings (v2 guard)
        if package != "@opencode/ai/providers/cloudflare-workers-ai" {
            settings = Some(ProviderSettings {
                rest: BTreeMap::from([("baseURL".to_string(), Value::String(api.clone()))]),
                ..Default::default()
            });
        }
    }
    let info = ProviderInfo {
        id: provider_id.clone(),
        canonical: None,
        integration_id: None,
        name: item.name.clone(),
        activation: Activation::Auto,
        package,
        settings,
        headers: None,
        body: None,
    };

    let bedrock = item.id == "amazon-bedrock";
    let mut models = Vec::new();
    for model in item.models.values() {
        // G-258 — profile-only Bedrock ids never register (the ref filters
        // on `model.modelID ?? model.id`; this port's source models carry
        // no modelID override, so the id key is authoritative).
        if bedrock && BEDROCK_PROFILE_ONLY_IDS.contains(&model.id.as_str()) {
            continue;
        }
        let base_cost = cost(&model.cost.clone().unwrap_or_default());
        let id = ModelId::parse(model.id.clone());
        let mut base =
            model_info(item, id.clone(), model, ModelInfoOverrides::base(base_cost.clone()));
        // G-261 — the OpenRouter chat aliases stay listed but disabled
        // (openrouter.ts:19-25 `model.enabled = false`); only the
        // openrouter provider's own models are touched.
        if item.id == "openrouter" && OPENROUTER_DISABLED_MODEL_IDS.contains(&model.id.as_str()) {
            base.enabled = false;
        }
        let supports = supports_from_reasoning_options(&model.reasoning_options);
        let variants = variant::resolve(
            &VariantModel {
                package: Some(native_package(
                    model
                        .provider
                        .as_ref()
                        .and_then(|p| p.npm.clone())
                        .as_deref()
                        .unwrap_or(&item.npm),
                    &item.id,
                    Some(&model.id),
                    model.provider.as_ref().and_then(|p| p.shape.as_deref()),
                )),
                provider_id: item.id.clone(),
                model_id: model.id.clone(),
                limit_output: f64_limit(model.limit.output),
            },
            &supports,
        );
        models.push(schema_model::Info { variants: variants.clone(), ..base });

        // experimental modes: `${model.id}-${mode}` entries sharing variants
        if let Some(experimental) = &model.experimental {
            for (mode, options) in &experimental.modes {
                let mode_id = ModelId::parse(format!("{}-{}", model.id, mode));
                let overridden = ModelInfoOverrides {
                    name: Some(mode_name(model, mode)),
                    cost: Some(merge_cost(&base_cost, &options.cost.clone().unwrap_or_default())),
                    request: options.provider.as_ref(),
                    variants: Some(variants.clone()),
                };
                models.push(model_info(item, mode_id, model, overridden));
            }
        }
    }

    Snapshot { info, models, environment: item.env.clone() }
}

/// Extra inputs for one derived `Model.Info` (v2 `modelInfo` `input`).
struct ModelInfoOverrides<'a> {
    name: Option<String>,
    cost: Option<Vec<Cost>>,
    request: Option<&'a crate::provider_catalog::source::SourceModeProvider>,
    variants: Option<Vec<schema_model::Variant>>,
}

impl ModelInfoOverrides<'_> {
    fn base(cost: Vec<Cost>) -> Self {
        Self { name: None, cost: Some(cost), request: None, variants: None }
    }
}

fn model_info(
    provider: &SourceProvider,
    id: ModelId,
    model: &SourceModel,
    input: ModelInfoOverrides<'_>,
) -> schema_model::Info {
    let provider_id = ProviderId::parse(provider.id.clone());
    let resolved = native_package(
        model.provider.as_ref().and_then(|p| p.npm.clone()).as_deref().unwrap_or(&provider.npm),
        &provider.id,
        Some(&model.id),
        model.provider.as_ref().and_then(|p| p.shape.as_deref()),
    );
    let provider_package = native_package(&provider.npm, &provider.id, None, None);
    let package = if model.provider.as_ref().and_then(|p| p.npm.clone()).is_some()
        || resolved != provider_package
    {
        Some(resolved.clone())
    } else {
        None
    };

    // Per model, so it never merges into a model that overrides to a
    // different package (v2 comment).
    let mut settings_rest = BTreeMap::new();
    if let Some(api) = model.provider.as_ref().and_then(|p| p.api.clone()) {
        settings_rest.insert("baseURL".to_string(), Value::String(api));
    }
    if resolved == "@opencode/ai/providers/openai-compatible" {
        settings_rest.insert("provider".to_string(), Value::String(provider_id.0.clone()));
    }

    let mut info = schema_model::Info {
        id,
        model_id: ModelId::parse(model.id.clone()),
        provider_id,
        canonical: None,
        family: model.family.clone(),
        name: input.name.unwrap_or_else(|| model.name.clone()),
        compatibility: compat_from_interleaved(model.interleaved.as_ref()),
        package,
        settings: (!settings_rest.is_empty())
            .then(|| schema_model::Settings { rest: settings_rest, ..Default::default() }),
        headers: input.request.and_then(|request| request.headers.clone()),
        body: input.request.and_then(|request| {
            request
                .body
                .as_ref()
                .and_then(|body| body.as_object())
                .map(|object| object.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        }),
        capabilities: Capabilities {
            tools: model.tool_call,
            input: model.modalities.as_ref().map(|m| m.input.clone()).unwrap_or_default(),
            output: model.modalities.as_ref().map(|m| m.output.clone()).unwrap_or_default(),
        },
        variants: input.variants.unwrap_or_default(),
        time: ModelTime { released: Num(released(model.release_date.as_deref())) },
        cost: input.cost.unwrap_or_default(),
        status: model.status.map_or(Status::Active, source_status),
        enabled: true,
        limit: ModelLimit {
            context: f64_limit(model.limit.context),
            input: model.limit.input.map(|value| f64_limit(Some(value))),
            output: f64_limit(model.limit.output),
        },
    };
    // G-285 — the prompt-cache-key provider fold: EVERY model of the
    // listed catalog providers gains `supportsPromptCacheKey`
    // (prompt-cache-key.ts:28-36 — experimental modes included), merged
    // over the model's own compatibility like the ref's
    // `{...draft.compatibility, ...}`.
    if PROMPT_CACHE_KEY_PROVIDERS.contains(&provider.id.as_str()) {
        let mut compatibility = info.compatibility.clone().unwrap_or_default();
        compatibility.supports_prompt_cache_key = Some(true);
        info.compatibility = Some(compatibility);
    }
    info
}

/// v2 `Model.compatibility(model.interleaved)`: a string (or `{field}`) names
/// the reasoning field.
#[must_use]
pub fn compat_from_interleaved(interleaved: Option<&Value>) -> Option<Compatibility> {
    match interleaved? {
        Value::String(field) => {
            Some(Compatibility { reasoning_field: Some(field.clone()), ..Default::default() })
        }
        Value::Object(object) => {
            object.get("field").and_then(|field| field.as_str()).map(|field| Compatibility {
                reasoning_field: Some(field.to_string()),
                ..Default::default()
            })
        }
        _ => None,
    }
}

fn source_status(status: crate::provider_catalog::source::SourceStatus) -> Status {
    match status {
        crate::provider_catalog::source::SourceStatus::Alpha => Status::Alpha,
        crate::provider_catalog::source::SourceStatus::Beta => Status::Beta,
        crate::provider_catalog::source::SourceStatus::Deprecated => Status::Deprecated,
    }
}

/// v2 `released`: `Date.parse` of `YYYY-MM-DD` (UTC midnight); anything
/// unparseable is 0.
fn released(date: Option<&str>) -> f64 {
    date.and_then(parse_iso_date_ms).unwrap_or(0.0)
}

/// `Date.parse("YYYY-MM-DD")` — UTC midnight, milliseconds. Only the shape
/// models.dev emits is accepted (JS accepts more; the snapshot is
/// machine-generated).
fn parse_iso_date_ms(input: &str) -> Option<f64> {
    let bytes = input.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = input[0..4].parse().ok()?;
    let month: i64 = input[5..7].parse().ok()?;
    let day: i64 = input[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // days from civil (Howard Hinnant's algorithm)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month;
    let d = day;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    // JS `Date.parse` returns a JS number (f64) — parity, not precision loss
    #[allow(clippy::cast_precision_loss)]
    Some((days * 86_400_000) as f64)
}

fn f64_limit(value: Option<f64>) -> i64 {
    value.map_or(0, crate::provider_catalog::support::f64_to_i64)
}

fn usd(value: Option<f64>) -> UsdPerMillionTokens {
    UsdPerMillionTokens(Num(value.unwrap_or(0.0)))
}

/// v2 `cost`: a base entry (always present, zero-filled) plus tier entries.
fn cost(input: &SourceCost) -> Vec<Cost> {
    let mut entries = vec![Cost {
        tier: None,
        input: usd(input.input),
        output: usd(input.output),
        cache: CostCache { read: usd(input.cache_read), write: usd(input.cache_write) },
    }];
    for tier in &input.tiers {
        entries.push(Cost {
            tier: Some(CostTier {
                tier_type: CostTierType::Context,
                size: crate::provider_catalog::support::f64_to_i64(tier.tier.size),
            }),
            input: usd(tier.input),
            output: usd(tier.output),
            cache: CostCache { read: usd(tier.cache_read), write: usd(tier.cache_write) },
        });
    }
    entries
}

/// v2 `mergeCost`: override merges onto the base by tier key; base default
/// merges first, tiers merge or append.
fn merge_cost(base: &[Cost], override_cost: &SourceCost) -> Vec<Cost> {
    let next = cost(override_cost);
    let (base_default, base_tiers) = match base.split_first() {
        Some((first, rest)) => (Some(first.clone()), rest),
        None => (None, &[][..]),
    };
    let (next_default, next_tiers) = match next.split_first() {
        Some((first, rest)) => (Some(first.clone()), rest),
        None => (None, &[][..]),
    };
    let tier_key = |item: &Cost| -> String {
        match &item.tier {
            Some(tier) => format!("context:{}", tier.size),
            None => "base:0".to_string(),
        }
    };
    let merge = |left: &Cost, right: &Cost| Cost {
        tier: right.tier.or(left.tier),
        input: right.input,
        output: right.output,
        cache: right.cache,
    };
    let mut tiers: BTreeMap<String, Cost> =
        base_tiers.iter().map(|item| (tier_key(item), item.clone())).collect();
    for item in next_tiers {
        let key = tier_key(item);
        let merged = match tiers.get(&key) {
            Some(current) => merge(current, item),
            None => item.clone(),
        };
        tiers.insert(key, merged);
    }
    let mut merged = vec![match (base_default, next_default) {
        (Some(left), Some(right)) => merge(&left, &right),
        (None, Some(right)) => right,
        (Some(left), None) => left,
        (None, None) => Cost {
            tier: None,
            input: usd(None),
            output: usd(None),
            cache: CostCache { read: usd(None), write: usd(None) },
        },
    }];
    merged.extend(tiers.into_values());
    merged
}

fn mode_name(model: &SourceModel, mode: &str) -> String {
    let mut chars = mode.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("{} {capitalized}", model.name)
}

/// Digest of a raw catalog body (v2 `bodyDigest`); used by refresh to skip
/// republishing byte-identical bodies.
#[must_use]
pub fn body_digest(text: &str) -> String {
    oc_util::hash::sha256_hex(text)
}

#[cfg(test)]
mod tests {
    // `assert!(!x.is_empty())` reads better than the suggested alternative.
    #![allow(clippy::assert_is_empty)]

    use super::*;

    #[test]
    fn provider_catalog_parse_counts() {
        let catalog = Catalog::embedded();
        // Pinned parity counts from the committed snapshot (224 providers —
        // G-258 drops the azure-cognitive-services/google-vertex-anthropic
        // aliases — and 8380 models + experimental modes, 115 fewer after
        // the alias providers and the 13 Bedrock profile-only ids drop).
        assert_eq!(catalog.provider_count(), 224);
        assert_eq!(catalog.model_count(), 8380);
    }

    /// G-258 — the models-dev plugin's two-level snapshot filter
    /// (`plugin/models-dev.ts:94-109`): the deprecated alias providers
    /// never register, and the Bedrock inference-profile-only ids drop.
    #[test]
    fn provider_catalog_filters_alias_providers_and_bedrock_profile_only_models() {
        let catalog = Catalog::embedded();
        // Provider level (models-dev.ts:95-98) — the aliases are replaced
        // by the canonical azure/google-vertex providers, which remain.
        for alias in ["azure-cognitive-services", "google-vertex-anthropic"] {
            assert!(catalog.provider(alias).is_none(), "{alias} must not register");
        }
        assert!(catalog.provider("azure").is_some());
        assert!(catalog.provider("google-vertex").is_some());
        // Model level (models-dev.ts:11-25,106-108) — the 13 profile-only
        // Bedrock ids are absent while the in-region Opus/Sonnet 4.6
        // entries stay available.
        let bedrock = catalog.provider("amazon-bedrock").expect("bedrock registers");
        for id in [
            "amazon.nova-2-lite-v1:0",
            "anthropic.claude-fable-5",
            "anthropic.claude-fable-5-1",
            "anthropic.claude-haiku-4-5-20251001-v1:0",
            "anthropic.claude-opus-4-1-20250805-v1:0",
            "anthropic.claude-opus-4-5-20251101-v1:0",
            "anthropic.claude-opus-4-7",
            "anthropic.claude-opus-4-8",
            "anthropic.claude-opus-5",
            "anthropic.claude-sonnet-4-5-20250929-v1:0",
            "anthropic.claude-sonnet-5",
            "deepseek.r1-v1:0",
            "mistral.pixtral-large-2502-v1:0",
        ] {
            assert!(catalog.model("amazon-bedrock", id).is_none(), "{id} must not register");
        }
        assert!(bedrock
            .models
            .iter()
            .any(|model| model.id.as_str() == "anthropic.claude-opus-4-6-v1"));
        // The filter applies to every parsed snapshot, not just the
        // embedded one (the ref re-runs `snapshots()` over each refreshed
        // body, models-dev.ts:61-66).
        let synthetic = r#"{
            "azure-cognitive-services": {
                "id": "azure-cognitive-services", "name": "Azure Cognitive Services",
                "npm": "@ai-sdk/openai-compatible", "env": ["AZURE_API_KEY"],
                "models": {"gpt": {"id": "gpt", "name": "GPT"}}
            },
            "amazon-bedrock": {
                "id": "amazon-bedrock", "name": "Amazon Bedrock",
                "npm": "@ai-sdk/amazon-bedrock", "env": ["AWS_ACCESS_KEY_ID"],
                "models": {
                    "anthropic.claude-opus-5": {"id": "anthropic.claude-opus-5", "name": "Opus 5"},
                    "anthropic.claude-opus-4-6": {"id": "anthropic.claude-opus-4-6", "name": "Opus 4.6"}
                }
            }
        }"#;
        let catalog = Catalog::parse(synthetic).expect("parses");
        assert!(catalog.provider("azure-cognitive-services").is_none());
        assert!(catalog.model("amazon-bedrock", "anthropic.claude-opus-5").is_none());
        assert!(catalog.model("amazon-bedrock", "anthropic.claude-opus-4-6").is_some());
    }

    /// G-261 — the `OpenRouter` chat aliases stay listed but disabled when
    /// the snapshot carries them (openrouter.ts:16-28); providers that
    /// merely share the model id keep them enabled.
    #[test]
    fn provider_catalog_disables_openrouter_chat_aliases() {
        let synthetic = r#"{
            "openrouter": {
                "id": "openrouter", "name": "OpenRouter",
                "npm": "@openrouter/ai-sdk-provider", "env": ["OPENROUTER_API_KEY"],
                "models": {
                    "gpt-5-chat-latest": {"id": "gpt-5-chat-latest", "name": "GPT-5 Chat Latest"},
                    "openai/gpt-5-chat": {"id": "openai/gpt-5-chat", "name": "GPT-5 Chat"},
                    "other": {"id": "other", "name": "Other"}
                }
            },
            "other-gateway": {
                "id": "other-gateway", "name": "Other Gateway",
                "npm": "@ai-sdk/openai-compatible", "api": "https://other.test/v1", "env": [],
                "models": {"gpt-5-chat-latest": {"id": "gpt-5-chat-latest", "name": "GPT-5 Chat Latest"}}
            }
        }"#;
        let catalog = Catalog::parse(synthetic).expect("parses");
        for id in ["gpt-5-chat-latest", "openai/gpt-5-chat"] {
            let model = catalog.model("openrouter", id).expect("still listed");
            assert!(!model.enabled, "openrouter/{id} must be disabled");
        }
        assert!(catalog.model("openrouter", "other").expect("present").enabled);
        // Custom providers with matching ids stay untouched (the ref
        // plugin keys on the openrouter package).
        assert!(catalog.model("other-gateway", "gpt-5-chat-latest").expect("present").enabled);
    }

    /// G-285 — the prompt-cache-key provider fold: every model of the 12
    /// listed catalog providers carries `supportsPromptCacheKey`
    /// (prompt-cache-key.ts:10-23), merged over the model's own
    /// compatibility.
    #[test]
    fn provider_catalog_prompt_cache_key_providers() {
        let catalog = Catalog::embedded();
        for id in [
            "abliteration-ai",
            "aihubmix",
            "anyapi",
            "auriko",
            "edenai",
            "jalapeno",
            "llama",
            "morph",
            "snowflake-cortex",
            "tencent-tokenhub",
            "upstage",
            "venice",
        ] {
            let provider = catalog.provider(id).unwrap_or_else(|| panic!("{id} in snapshot"));
            assert!(
                provider.models.iter().all(|model| model
                    .compatibility
                    .as_ref()
                    .is_some_and(|compat| compat.supports_prompt_cache_key == Some(true))),
                "{id} models must set supportsPromptCacheKey"
            );
        }
        // Providers outside the list keep their own compatibility.
        let openai = catalog.provider("openai").expect("openai");
        assert!(
            openai.models.iter().all(|model| model
                .compatibility
                .as_ref()
                .and_then(|compat| compat.supports_prompt_cache_key)
                .is_none()),
            "openai catalog models do not set supportsPromptCacheKey (the chat-lane preset owns it)"
        );
    }

    /// D-05: the flat list is released-descending (model.ts:218-220 —
    /// the `GET /api/model` wire order), stable on ties.
    #[test]
    fn provider_catalog_flat_models_sort_by_release_descending() {
        let catalog = Catalog::embedded();
        let flat = catalog.flat_models_by_release();
        assert_eq!(flat.len(), catalog.model_count(), "every model exactly once");
        let mut prev = f64::INFINITY;
        for (_, model) in &flat {
            assert!(
                model.time.released.0 <= prev,
                "released descending: {} after {prev}",
                model.time.released.0
            );
            prev = model.time.released.0;
        }
    }

    /// The sort is stable: equal release dates keep provider-grouped
    /// source order (JS `toSorted` stability parity).
    #[test]
    fn provider_catalog_flat_models_sort_is_stable_on_ties() {
        let text = r#"{
            "a": {"id": "a", "name": "A", "models": {
                "old-a": {"id": "old-a", "name": "Old A", "release_date": "2024-01-01", "limit": {"context": 1, "output": 1}},
                "same-a": {"id": "same-a", "name": "Same A", "release_date": "2024-06-01", "limit": {"context": 1, "output": 1}}
            }},
            "b": {"id": "b", "name": "B", "models": {
                "same-b": {"id": "same-b", "name": "Same B", "release_date": "2024-06-01", "limit": {"context": 1, "output": 1}},
                "new-b": {"id": "new-b", "name": "New B", "release_date": "2025-01-01", "limit": {"context": 1, "output": 1}}
            }}
        }"#;
        let catalog = Catalog::parse(text).expect("valid catalog");
        let names: Vec<String> =
            catalog.flat_models_by_release().into_iter().map(|(_, m)| m.name.clone()).collect();
        assert_eq!(names, vec!["New B", "Same A", "Same B", "Old A"]);
    }

    /// `available_models` — the keyless `opencode` zen free tier
    /// (plugin/provider/opencode.ts:317-342): with NO
    /// `OPENCODE_API_KEY`, provider `opencode` serves exactly its
    /// zero-cost models (an empty cost table stays enabled) and every
    /// other provider is absent (no credentials).
    #[test]
    fn provider_catalog_available_keyless_zen_free_tier_only() {
        let catalog = Catalog::embedded();
        let pairs = catalog.available_models(|name| name == "NEVER_SET");
        let ids: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(snapshot, model)| (snapshot.info.id.as_str(), model.id.as_str()))
            .collect();
        assert!(
            ids.iter().all(|(provider, _)| *provider == "opencode"),
            "keyless: only the zen provider is available, got {:?}",
            ids.iter().map(|(p, _)| *p).collect::<std::collections::HashSet<_>>()
        );
        for (_, model) in &pairs {
            assert!(
                model.cost.iter().all(|cost| cost.input.0 .0 == 0.0),
                "keyless zen serves zero-cost models only: {} ({:?})",
                model.name,
                model.cost
            );
        }
        // The pinned free set from the committed snapshot (v2's rig
        // listing): 10 models, newest first.
        let names: Vec<&str> = pairs.iter().map(|(_, model)| model.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Fledge Alpha Free",
                "Ling 3.1 Flash Free",
                "LongCat 2.5 Preview Free",
                "Space Bunny Free",
                "MiMo-V2.6-Flash Free",
                "Muse Spark 1.3 Free",
                "Ling 3.0 Flash Fin Free",
                "Nemotron 3.5 Lightning Free",
                "Nemotron 3 Ultra Free",
                "Big Pickle",
            ]
        );
    }

    /// `OPENCODE_API_KEY` set → the zen provider serves its FULL
    /// catalog (paid models stay enabled). NB the same variable is
    /// also `opencode-go`'s env credential (snapshot `environment`),
    /// so it connects too — v2's rule is purely "env credential
    /// present → connection → available" (provider.ts:404). Other
    /// providers stay absent without their own credential.
    #[test]
    fn provider_catalog_available_with_zen_key_and_env_providers() {
        let catalog = Catalog::embedded();
        let pairs = catalog.available_models(|name| name == "OPENCODE_API_KEY");
        let providers: std::collections::HashSet<&str> =
            pairs.iter().map(|(snapshot, _)| snapshot.info.id.as_str()).collect();
        assert!(providers.contains("opencode"), "zen with key is available");
        assert!(
            providers.contains("opencode-go"),
            "the zen key is also opencode-go's env credential"
        );
        assert!(!providers.contains("anthropic"), "no ANTHROPIC_API_KEY → absent");
        let zen_paid = pairs.iter().any(|(snapshot, model)| {
            snapshot.info.id.as_str() == "opencode" && model.cost.iter().any(|c| c.input.0 .0 > 0.0)
        });
        assert!(zen_paid, "with a key the paid zen models are enabled");
    }

    /// A provider whose `environment` variable IS set is available
    /// with its full model list (the env connection rule,
    /// provider.ts:404).
    #[test]
    fn provider_catalog_available_env_credential_provider() {
        let catalog = Catalog::embedded();
        let pairs = catalog
            .available_models(|name| name == "ANTHROPIC_API_KEY" || name == "OPENCODE_API_KEY");
        let anthropic: Vec<&str> = pairs
            .iter()
            .filter(|(snapshot, _)| snapshot.info.id.as_str() == "anthropic")
            .map(|(_, model)| model.id.as_str())
            .collect();
        let full: Vec<&str> = catalog
            .provider("anthropic")
            .expect("anthropic")
            .models
            .iter()
            .filter(|model| !matches!(model.status, schema_model::Status::Deprecated))
            .filter(|model| model.capabilities.output.iter().any(|item| item == "text"))
            .map(|model| model.id.as_str())
            .collect();
        assert!(!anthropic.is_empty(), "anthropic with its env key is available");
        assert_eq!(anthropic.len(), full.len(), "full (non-deprecated, text) model list");
    }

    /// G-262② — the `AmazonBedrockPlugin` activation chain
    /// (`plugin/provider/amazon-bedrock.ts:8-52`): ambient AWS
    /// default-chain inputs (or the narrowed bearer token) activate the
    /// provider WITHOUT any stored key, while the non-key models.dev
    /// names (`AWS_SECRET_ACCESS_KEY`, `AWS_REGION`) alone do not —
    /// `SigV4` chain config is not a credential.
    #[test]
    fn provider_catalog_bedrock_chain_env_activation() {
        let catalog = Catalog::embedded();
        let bedrock_visible = |names: &[&str]| {
            catalog
                .available_models(|name| names.contains(&name))
                .iter()
                .any(|(snapshot, _)| snapshot.info.id.as_str() == "amazon-bedrock")
        };
        // Each CHAIN_ENV member alone activates (ref list, :8-14).
        for name in [
            "AWS_PROFILE",
            "AWS_ACCESS_KEY_ID",
            "AWS_WEB_IDENTITY_TOKEN_FILE",
            "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
            "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        ] {
            assert!(bedrock_visible(&[name]), "{name} alone must activate bedrock");
        }
        // The narrowed env key still works.
        assert!(bedrock_visible(&["AWS_BEARER_TOKEN_BEDROCK"]));
        // Non-key chain config alone does NOT.
        assert!(
            !bedrock_visible(&["AWS_SECRET_ACCESS_KEY", "AWS_REGION"]),
            "chain config without a key or chain input must not activate bedrock"
        );
        assert!(!bedrock_visible(&["NEVER_SET"]));
    }

    #[test]
    fn provider_catalog_anthropic_provider_info() {
        let catalog = Catalog::embedded();
        let anthropic = catalog.provider("anthropic").expect("anthropic in snapshot");
        assert_eq!(anthropic.info.name, "Anthropic");
        assert_eq!(anthropic.info.package, "@opencode/ai/providers/anthropic");
        assert_eq!(anthropic.environment, vec!["ANTHROPIC_API_KEY".to_string()]);
        assert!(!anthropic.models.is_empty());
    }

    #[test]
    fn provider_catalog_model_info_derivation() {
        let catalog = Catalog::embedded();
        let model = catalog.model("anthropic", "claude-opus-4-5").expect("claude-opus-4-5 present");
        assert_eq!(model.provider_id.as_str(), "anthropic");
        assert_eq!(model.model_id.as_str(), "claude-opus-4-5");
        assert!(model.capabilities.tools);
        assert!(model.capabilities.input.contains(&"text".to_string()));
        assert_eq!(model.limit.context, 200_000);
        assert_eq!(model.limit.output, 64_000);
        assert_eq!(model.status, Status::Active);
        assert!(model.enabled);
        assert!(!model.cost.is_empty());
        assert!(model.time.released.0 > 0.0);
        // effort variants present (opus 4.5 → manual thinking budgets)
        let ids: Vec<&str> = model.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["low", "medium", "high"]);
    }

    #[test]
    fn provider_catalog_openai_provider_uses_responses_package() {
        let catalog = Catalog::embedded();
        let openai = catalog.provider("openai").expect("openai in snapshot");
        assert_eq!(openai.info.package, "@opencode/ai/providers/openai");
        let gpt = catalog.model("openai", "gpt-5.4").expect("gpt-5.4 present");
        let ids: Vec<&str> = gpt.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["none", "low", "medium", "high", "xhigh"]);
    }

    #[test]
    fn provider_catalog_alibaba_host_redirect_and_budget_clamp() {
        let catalog = Catalog::embedded();
        let alibaba = catalog.provider("alibaba").expect("alibaba in snapshot");
        assert_eq!(alibaba.info.package, "@opencode/ai/providers/alibaba/chat");
        let qwen = catalog.model("alibaba", "qwen3.5-flash").expect("qwen3.5-flash present");
        let ids: Vec<&str> = qwen.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["none", "high", "max"]);
        let max = qwen
            .variants
            .iter()
            .find(|v| v.id.as_str() == "max")
            .and_then(|v| v.settings.as_ref())
            .expect("max variant settings");
        assert_eq!(max.rest.get("thinkingBudget"), Some(&serde_json::json!(63_999)));
        assert_eq!(max.rest.get("enableThinking"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn provider_catalog_custom_baseurl_settings() {
        // A provider with an api (200 of 226 have one) carries it as
        // `settings.baseURL` on Provider.Info.
        let catalog = Catalog::embedded();
        let fireworks = catalog.provider("fireworks-ai").expect("fireworks-ai in snapshot");
        assert_eq!(fireworks.info.package, "@opencode/ai/providers/fireworks");
        let settings = fireworks.info.settings.as_ref().expect("api-derived settings");
        assert_eq!(
            settings.rest.get("baseURL"),
            Some(&serde_json::json!("https://api.fireworks.ai/inference/v1/"))
        );
    }

    #[test]
    fn provider_catalog_unknown_lookups() {
        let catalog = Catalog::embedded();
        assert!(catalog.provider("no-such-provider").is_none());
        assert!(catalog.model("anthropic", "no-such-model").is_none());
    }

    #[test]
    fn provider_catalog_experimental_mode_models() {
        // 105 mode entries exist; find one and check the derived id/name.
        let catalog = Catalog::embedded();
        let modes = catalog
            .providers()
            .iter()
            .flat_map(|snapshot| snapshot.models.iter().map(move |m| (snapshot, m)))
            .filter(|(_, model)| model.id.as_str() != model.model_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(modes.len(), 105, "mode entries derived as separate models");
        let (_, first) = modes.first().expect("at least one mode model");
        assert!(first.id.as_str().contains('-'));
        assert!(first.name.contains(' '));
    }

    #[test]
    fn provider_catalog_released_date_parsing() {
        assert_eq!(parse_iso_date_ms("1970-01-01"), Some(0.0));
        assert_eq!(parse_iso_date_ms("2026-07-06"), Some(1_783_296_000_000.0));
        assert_eq!(parse_iso_date_ms("bogus"), None);
        assert_eq!(released(Some("bogus")), 0.0);
        assert_eq!(released(None), 0.0);
    }

    #[test]
    fn provider_catalog_body_digest_stable() {
        assert_eq!(body_digest("abc").len(), 64);
        assert_ne!(body_digest("abc"), body_digest("abd"));
    }
}
