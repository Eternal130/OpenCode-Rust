//! The catalog-driven model resolver (port of `SessionRunnerModel` +
//! `model-resolver.ts` over the Rust provider facade): session model ref
//! → oc-ai [`LanguageModel`] via the catalog or a user-defined custom
//! provider, with the resolved capabilities/cost/limit the runner records.

#![allow(
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructors consume their parts
    clippy::too_many_lines, // the assembly table mirrors the v2 layer graph
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity, // hook/executor seams match the lib's
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown, // TS identifiers in prose stay unquoted
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening, // assembly order is deliberate
    clippy::large_futures, // boot/assemble mirror the v2 layer composition
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation,
    clippy::elidable_lifetime_names,
    clippy::question_mark // the early-return shape mirrors the TS branch
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use oc_ai::route::auth;
use oc_ai::route::client::RoutePatch;

use crate::database::schema::SessionRow;
use crate::provider_catalog::facade::{
    custom_language_model, language_model_for_model, CustomProvider, FacadeContext, ModelConnection,
};
use crate::provider_catalog::native::package_name;
use crate::provider_catalog::Catalog;
use crate::session::runner::{ModelError, ModelResolver, ResolvedModel};

/// The production resolver: catalog + user connections + custom
/// providers over one shared transport.
pub struct CatalogModelResolver {
    context: FacadeContext,
    /// The catalog snapshot behind a shared cell (CRED-A1): boot seeds
    /// it; `models-dev.refreshed` swaps in the live service catalog so
    /// refreshes propagate to resolution without a re-boot (v2 re-reads
    /// `ModelsDev.get()` on every Refreshed event, models-dev.ts:61-66).
    catalog: parking_lot::RwLock<Arc<Catalog>>,
    connections: BTreeMap<String, ModelConnection>,
    custom: Vec<CustomProvider>,
    /// The default model selection (config `model`).
    default: Option<oc_schema::config::ModelSelection>,
    /// Credential-driven provider bindings (the generalized stored-
    /// credential fold — copilot/vertex transports plus every registered
    /// integration's active credential), shared with the integration
    /// host and refreshed on credential events.
    credentials: Option<Arc<crate::integration::copilot::CredentialBindings>>,
    /// Console-issued providers (integration `opencode`), shared with the
    /// host and refreshed on credential events.
    console: Option<Arc<crate::integration::console::ConsoleProviders>>,
}

impl CatalogModelResolver {
    /// Assembles the resolver.
    #[must_use]
    pub fn new(
        context: FacadeContext,
        catalog: Arc<Catalog>,
        connections: BTreeMap<String, ModelConnection>,
        custom: Vec<CustomProvider>,
        default: Option<oc_schema::config::ModelSelection>,
    ) -> Self {
        Self {
            context,
            catalog: parking_lot::RwLock::new(catalog),
            connections,
            custom,
            default,
            credentials: None,
            console: None,
        }
    }

    /// Swaps the served catalog snapshot (CRED-A1: the engine's
    /// `models-dev.refreshed` subscription keeps resolution live).
    pub fn refresh_catalog(&self, catalog: Arc<Catalog>) {
        *self.catalog.write() = catalog;
    }

    /// The current catalog snapshot.
    #[must_use]
    pub fn catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog.read())
    }

    /// Attaches the credential-driven provider bindings.
    #[must_use]
    pub fn with_credentials(
        mut self,
        bindings: Arc<crate::integration::copilot::CredentialBindings>,
    ) -> Self {
        self.credentials = Some(bindings);
        self
    }

    /// Attaches the console-issued provider snapshot.
    #[must_use]
    pub fn with_console(
        mut self,
        console: Arc<crate::integration::console::ConsoleProviders>,
    ) -> Self {
        self.console = Some(console);
        self
    }

    /// The catalog's canonical provider id for a requested prefix: the
    /// exact key first, then a case-insensitive match (the config-key
    /// `own` behind a `Own/model` reference — the live beta.3.1 defect).
    /// v2 itself matches catalog ids exactly (`Model.Service.get` walks
    /// exact Maps, model.ts:255-257); the case-insensitive fallback is
    /// our tolerance so a mixed-case reference resolves to the
    /// configured provider instead of `provider.no-route`.
    fn canonical_catalog_provider(&self, requested: &str) -> Option<String> {
        let catalog = self.catalog();
        if catalog.provider(requested).is_some() {
            return Some(requested.to_owned());
        }
        catalog
            .providers()
            .iter()
            .find(|snapshot| snapshot.info.id.0.eq_ignore_ascii_case(requested))
            .map(|snapshot| snapshot.info.id.0.clone())
    }

    /// Whether this binding takes the generic keyless `Auth.none`
    /// downgrade (CRED-AUTHNONE-DOWNGRADE port of
    /// model-resolver.ts:379-385). Returns the downgrade's diagnostic
    /// message when it applies.
    fn keyless_downgrade(
        &self,
        provider_id: &str,
        connection: &ModelConnection,
        model_id: &str,
    ) -> Option<String> {
        let catalog = self.catalog();
        let snapshot = catalog.provider(provider_id)?;
        if snapshot.info.activation != oc_schema::provider::Activation::Enabled {
            return None;
        }
        // `credential === undefined`: no stored credential binding, no
        // configured key, and no env connection (v2's connection.active
        // counts a set env method as a credential).
        let has_credential = connection.api_key.is_some()
            || self
                .credentials
                .as_ref()
                .is_some_and(|bindings| bindings.get(provider_id).is_some())
            || snapshot.environment.iter().any(|name| std::env::var_os(name).is_some());
        if has_credential {
            return None;
        }
        let model = snapshot.models.iter().find(|model| model.id.0 == model_id)?;
        let package = model.package.as_deref().unwrap_or(&snapshot.info.package).to_owned();
        if !uses_api_key_auth(&package)
            || has_configured_auth(&snapshot.info.settings, &model.settings)
        {
            return None;
        }
        Some(format!(
            "provider `{provider_id}` is activation-enabled with no credential; downgrading to anonymous auth"
        ))
    }

    /// The connections entry for a provider prefix: the exact key first,
    /// then case-insensitive (mirrors [`Self::canonical_catalog_provider`]).
    fn connection_for(&self, provider_id: &str) -> ModelConnection {
        self.connections
            .get(provider_id)
            .cloned()
            .or_else(|| {
                self.connections
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(provider_id))
                    .map(|(_, value)| value.clone())
            })
            .unwrap_or_default()
    }

    /// Binds one reference, returning the bound model, its catalog info,
    /// and the CANONICAL reference (the configured provider/model keys —
    /// v2 records the selected catalog identity on assistant messages,
    /// model-resolver.ts:386-392).
    pub(crate) fn bind(
        &self,
        reference: &oc_schema::model::Ref,
    ) -> Result<(oc_ai::LanguageModel, oc_schema::model::Info, oc_schema::model::Ref), ModelError>
    {
        // Custom (config) providers: exact key first, then
        // case-insensitive — the provider map key IS the provider id
        // (v2 config/plugin/provider.ts:56-57,166-169), and user configs
        // legitimately mix `providers: { own: { name: "Own" ... } }` with
        // `model: "Own/glm-5.3"` (the name-cased reference). Same
        // exact-then-case-insensitive rule for the model key: the
        // provider's own spelling wins.
        if let Some(custom) =
            self.custom.iter().find(|custom| custom.id == reference.provider_id.0).or_else(|| {
                self.custom
                    .iter()
                    .find(|custom| custom.id.eq_ignore_ascii_case(&reference.provider_id.0))
            })
        {
            // CRED-A4: an entry whose id names a CATALOG provider falls
            // through to the catalog path below — v2 merges its settings
            // (apiKey/baseURL) onto the existing provider record instead
            // of rebinding the provider to openai-compatible
            // (config/plugin/provider.ts:70-79).
            if self.catalog().provider(&custom.id).is_none() {
                let canonical_model = custom
                    .models
                    .iter()
                    .find(|model| model.id.eq_ignore_ascii_case(&reference.id.0))
                    .map(|model| model.id.clone())
                    .unwrap_or_else(|| reference.id.0.clone());
                let model = custom_language_model(&self.context, custom, &canonical_model)
                    .map_err(|error| map_custom(error, &custom.id, &canonical_model))?;
                let info = custom_model_info(custom);
                let canonical = oc_schema::model::Ref {
                    provider_id: oc_schema::id::ProviderId::parse(custom.id.clone()),
                    id: oc_schema::id::ModelId::parse(canonical_model),
                    variant: reference.variant.clone(),
                };
                return Ok((model, info, canonical));
            }
        }
        // Console-issued providers (integration `opencode`): resolved
        // before the catalog exactly like config customs — the console
        // owns these ids (opencode.ts:243-268 `providers.update/add`).
        if let Some(console) = &self.console {
            if let Some(provider) = console.get(&reference.provider_id.0).or_else(|| {
                self.console.as_ref().and_then(|console| {
                    console
                        .entries_iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(&reference.provider_id.0))
                        .map(|(_, value)| value)
                })
            }) {
                // CRED-B2: console-issued providers ride the Console
                // credential — their integrationID is "opencode"
                // (opencode.ts:251), and the resolver's credential fold
                // maps the active token onto the api key
                // (nativeCredentialSettings oauth → apiKey,
                // model-resolver.ts:308-319,364-371).
                let mut provider = provider;
                if let Some(token) = console.token() {
                    provider.api_key = Some(token);
                }
                let canonical_model = provider
                    .models
                    .iter()
                    .find(|model| model.id.eq_ignore_ascii_case(&reference.id.0))
                    .map(|model| model.id.clone())
                    .unwrap_or_else(|| reference.id.0.clone());
                let model = custom_language_model(&self.context, &provider, &canonical_model)
                    .map_err(|error| map_custom(error, &provider.id, &canonical_model))?;
                let info = custom_model_info(&provider);
                let canonical = oc_schema::model::Ref {
                    provider_id: oc_schema::id::ProviderId::parse(provider.id.clone()),
                    id: oc_schema::id::ModelId::parse(canonical_model),
                    variant: reference.variant.clone(),
                };
                return Ok((model, info, canonical));
            }
        }
        // Catalog providers: canonicalize the prefix, keep the model id
        // exact (catalog ids are authoritative — v2's runner compares
        // `model.id === session.model.id` exactly, session/runner/model.ts:82-84).
        let provider_id =
            self.canonical_catalog_provider(&reference.provider_id.0).ok_or_else(|| {
                // Both segments in the error — v2 never drops the model id
                // (ModelUnavailableError carries providerID + modelID,
                // session/runner/model.ts:20-31,86-89).
                ModelError::Unavailable(reference.provider_id.0.clone(), reference.id.0.clone())
            })?;
        let mut connection = self.connection_for(&reference.provider_id.0);
        // CRED-A4: a config entry with this provider id contributes its
        // apiKey/baseURL as the connection overlay (the custom branch
        // above deferred catalog-matching entries here; v2 merges the
        // same keys into the provider settings).
        if let Some(custom) = self
            .custom
            .iter()
            .find(|custom| custom.id == provider_id || custom.id.eq_ignore_ascii_case(&provider_id))
        {
            connection.api_key = custom.api_key.clone().or(connection.api_key);
            if !custom.base_url.is_empty() {
                connection.base_url = Some(custom.base_url.clone());
            }
        }
        // Credential-driven providers, the generalized fold (CRED-B1):
        // every integration's ACTIVE stored credential wins over
        // config/env — base URL, token, and client headers merge into
        // the connection before the facade builds the route (v2
        // resolves the same per attempt through the integration
        // connection, model-resolver.ts:364-371; copilot's transport
        // bindings were the BE-3b special case of this fold).
        if let Some(bindings) = &self.credentials {
            if let Some(binding) = bindings.get(&provider_id) {
                connection.api_key = Some(binding.api_key.clone());
                connection.base_url = binding.base_url.clone().or(connection.base_url);
                for (key, value) in binding.headers {
                    connection.headers.insert(key, value);
                }
            }
        }
        // Keyless zen (CRED-B3, opencode.ts:317-325): no `OPENCODE_API_KEY`,
        // no Console connection (the binding above would carry its
        // token), and no configured key → v2 forces the enabled zen
        // provider to `apiKey: "public"`, so the free tier
        // authenticates with the public key instead of failing.
        if provider_id == "opencode"
            && connection.api_key.is_none()
            && std::env::var_os("OPENCODE_API_KEY").is_none()
        {
            connection.api_key = Some("public".to_owned());
        }
        let catalog = self.catalog();
        let snapshot = catalog
            .provider(&provider_id)
            .ok_or_else(|| ModelError::Unavailable(provider_id.clone(), reference.id.0.clone()))?;
        let base_info = snapshot
            .models
            .iter()
            .find(|model| model.id.0 == reference.id.0)
            .ok_or_else(|| ModelError::Unavailable(provider_id.clone(), reference.id.0.clone()))?
            .clone();
        // CRED-A3: the selected variant's settings/headers/body merge
        // into the model info BEFORE binding (v2 withVariant,
        // model-resolver.ts:137-161) so `#high/#thinking` overlays reach
        // the route defaults exactly like the model's own overlays.
        let info = with_variant(base_info, reference.variant.as_ref().map(|id| id.0.as_str()))?;
        let model = language_model_for_model(&self.context, snapshot, &info, &connection)
            .map_err(|error| map_provider(error, &provider_id, &reference.id.0))?;
        // Generic keyless downgrade (CRED-AUTHNONE-DOWNGRADE, ref
        // model-resolver.ts:379-385): an activation-ENABLED provider
        // with no credential (no stored binding, no env connection, no
        // configured key), no configured auth, and an API-key package
        // resolves with `Auth.none` instead of failing on the missing
        // key — v2's config provider plugin enables every configured
        // provider, so a deliberately keyless local/enterprise gateway
        // stays usable. (The zen fold above already injected the public
        // key, so it never reaches this branch.)
        let model = match self.keyless_downgrade(&provider_id, &connection, &reference.id.0) {
            Some(message) => {
                let route = Arc::clone(&model.route).with(RoutePatch {
                    auth: Some(auth::Definition::none()),
                    ..Default::default()
                });
                route
                    .model(model.id.as_str(), None)
                    .map_err(|error| ModelError::Resolver(format!("{message}: {error}")))?
            }
            None => model,
        };

        let canonical = oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse(provider_id),
            id: reference.id.clone(),
            variant: reference.variant.clone(),
        };
        Ok((model, info, canonical))
    }
}

impl ModelResolver for CatalogModelResolver {
    fn resolve(&self, session: &SessionRow) -> Result<ResolvedModel, ModelError> {
        let reference = session
            .model
            .as_deref()
            .and_then(|raw| serde_json::from_str::<oc_schema::model::Ref>(raw).ok())
            .or_else(|| {
                self.default.as_ref().map(|selection| oc_schema::model::Ref {
                    provider_id: selection.provider_id.clone(),
                    id: selection.model.clone(),
                    variant: selection.variant.clone(),
                })
            })
            .ok_or_else(|| ModelError::NotSelected(session.id.clone()))?;
        self.resolve_selected(session, &reference)
    }

    fn resolve_selected(
        &self,
        _session: &SessionRow,
        reference: &oc_schema::model::Ref,
    ) -> Result<ResolvedModel, ModelError> {
        let (model, info, canonical) = self.bind(reference)?;
        Ok(resolved_from(model, info, &canonical))
    }
}

/// A location-aware resolver: the location's config overlay FIRST, then
/// the engine-level bindings (global config custom providers + the
/// catalog) as fallback — v2 resolves providers per location
/// (`provider.ts` reads the layer's `Config`), so a daemon booted in one
/// directory serves every location's config-seeded providers.
///
/// Semantics:
///
/// - an explicit model ref consults the overlay (location custom
///   providers, then the shared catalog); when the overlay cannot bind
///   it, the base resolver answers (boot-seeded/global custom providers
///   the location config does not redefine);
/// - a session with no pinned model uses the location's `model` default
///   when the location config sets one (authoritative — a broken default
///   surfaces its error rather than silently falling back), else the
///   engine-level default.
pub struct LocationModelResolver {
    overlay: CatalogModelResolver,
    base: Arc<dyn ModelResolver>,
}

impl LocationModelResolver {
    /// Wraps `base` with the location overlay (`providers_from_config`
    /// over the location's loaded config).
    #[must_use]
    pub fn new(overlay: CatalogModelResolver, base: Arc<dyn ModelResolver>) -> Self {
        Self { overlay, base }
    }

    /// The overlay half (tests + diagnostics).
    #[must_use]
    pub fn overlay(&self) -> &CatalogModelResolver {
        &self.overlay
    }
}

impl ModelResolver for LocationModelResolver {
    fn resolve(&self, session: &SessionRow) -> Result<ResolvedModel, ModelError> {
        // A session-pinned model takes the same path on both layers
        // (resolve_selected); only the DEFAULT selection differs.
        if let Some(reference) = session
            .model
            .as_deref()
            .and_then(|raw| serde_json::from_str::<oc_schema::model::Ref>(raw).ok())
        {
            return self.resolve_selected(session, &reference);
        }
        match self.overlay.resolve(session) {
            Ok(resolved) => Ok(resolved),
            // The location config sets no default — the engine default
            // (global config / boot seeding) answers.
            Err(ModelError::NotSelected(_)) => self.base.resolve(session),
            // The location default exists but is unresolvable — the
            // config is authoritative; surface the error.
            Err(other) => Err(other),
        }
    }

    fn resolve_selected(
        &self,
        session: &SessionRow,
        reference: &oc_schema::model::Ref,
    ) -> Result<ResolvedModel, ModelError> {
        match self.overlay.resolve_selected(session, reference) {
            Ok(resolved) => Ok(resolved),
            // Not a location custom provider nor a resolvable catalog
            // entry — the base may know it (global/boot custom
            // providers); its error surfaces when neither does.
            Err(_) => self.base.resolve_selected(session, reference),
        }
    }
}

/// `SessionRunnerModel.Resolved` assembly: catalog identity + limits.
fn resolved_from(
    model: oc_ai::LanguageModel,
    info: oc_schema::model::Info,
    reference: &oc_schema::model::Ref,
) -> ResolvedModel {
    let mut reference = reference.clone();
    if reference.variant.is_none() {
        // A catalog resolution picks a default variant when none is pinned
        // (v2 `Model.VariantID` default selection).
        reference.variant = Some(oc_schema::id::ModelVariantId::parse("default"));
    }
    let compaction = info.settings.as_ref().and_then(|settings| settings.compaction);
    ResolvedModel {
        reference,
        model,
        capabilities: info.capabilities.clone(),
        cost: info.cost.clone(),
        limit: info.limit,
        compaction,
        transport: None,
    }
}

/// `withVariant` (model-resolver.ts:137-161): fold the selected
/// variant's overlays into the model info — settings (deep merge,
/// variant wins per key), headers, and body — without changing the
/// committed catalog snapshot. `"default"` means no overlay; an unknown
/// variant fails the resolution (`VariantUnavailableError`).
fn with_variant(
    mut info: oc_schema::model::Info,
    variant: Option<&str>,
) -> Result<oc_schema::model::Info, ModelError> {
    let Some(id) = variant.filter(|id| *id != "default") else {
        return Ok(info);
    };
    let Some(found) = info.variants.iter().find(|candidate| candidate.id.0 == id) else {
        return Err(ModelError::Resolver(format!(
            "variant `{id}` is not available for {}/{}",
            info.provider_id.0, info.id.0
        )));
    };
    if let Some(variant_settings) = &found.settings {
        let mut next = info.settings.clone().unwrap_or_default();
        next.rest = merge_overlay_records(
            info.settings.as_ref().map(|settings| &settings.rest),
            Some(&variant_settings.rest),
        )
        .unwrap_or_default();
        info.settings = Some(next);
    }
    if found.headers.is_some() {
        info.headers = oc_ai::schema::options::merge_string_records(&[
            info.headers.as_ref(),
            found.headers.as_ref(),
        ]);
    }
    if found.body.is_some() {
        info.body = merge_overlay_records(info.body.as_ref(), found.body.as_ref());
    }
    Ok(info)
}

/// `Provider.mergeOverlay`'s value rule (provider.ts:176-190): nested
/// records merge recursively, anything else takes the overlay.
fn merge_overlay_value(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut merged = base_map.clone();
            for (key, value) in overlay_map {
                let merged_value = merged
                    .get(key)
                    .map(|existing| merge_overlay_value(existing, value))
                    .unwrap_or_else(|| value.clone());
                merged.insert(key.clone(), merged_value);
            }
            Value::Object(merged)
        }
        (_, overlay) => overlay.clone(),
    }
}

/// `Provider.mergeOverlay` over `BTreeMap` records: union of keys, the
/// overlay winning per key (nested records deep-merge).
fn merge_overlay_records(
    base: Option<&BTreeMap<String, Value>>,
    overlay: Option<&BTreeMap<String, Value>>,
) -> Option<BTreeMap<String, Value>> {
    let (Some(base), Some(overlay)) = (base, overlay) else {
        return base.or(overlay).cloned();
    };
    let mut merged = base.clone();
    for (key, value) in overlay {
        let value = merged
            .get(key.as_str())
            .map(|existing| merge_overlay_value(existing, value))
            .unwrap_or_else(|| value.clone());
        merged.insert(key.clone(), value);
    }
    Some(merged)
}

/// `usesAPIKeyAuth` (model-resolver.ts:430-468): the packages whose auth
/// is a plain API key — the only ones the generic keyless downgrade may
/// run anonymously. Ported verbatim (aisdk names apply through
/// `package_name`'s unwrap), including the v2-era
/// `@opencode/ai/providers/*` npm names fireworks/openai-compatible/google
/// at model-resolver.ts:455-457.
fn uses_api_key_auth(package: &str) -> bool {
    const LIST: &[&str] = &[
        "@ai-sdk/openai",
        "@ai-sdk/anthropic",
        "@ai-sdk/cerebras",
        "@ai-sdk/deepinfra",
        "@ai-sdk/openai-compatible",
        "@ai-sdk/google",
        "@ai-sdk/groq",
        "@ai-sdk/mistral",
        "@ai-sdk/togetherai",
        "@ai-sdk/xai",
        "@openrouter/ai-sdk-provider",
        "@ai-sdk/azure",
        "@opencode/ai/providers/openai",
        "@opencode/ai/providers/anthropic",
        "@opencode/ai/providers/anthropic-compatible",
        "@opencode/ai/providers/baseten",
        "@opencode/ai/providers/cerebras",
        "@opencode/ai/providers/cloudflare-ai-gateway",
        "@opencode/ai/providers/cloudflare-workers-ai",
        "@opencode/ai/providers/deepinfra",
        "@opencode/ai/providers/deepseek",
        "@opencode/ai/providers/fireworks",
        "@opencode/ai/providers/openai-compatible",
        "@opencode/ai/providers/google",
        "@opencode/ai/providers/google/interactions",
        "@opencode/ai/providers/groq",
        "@opencode/ai/providers/mistral",
        "@opencode/ai/providers/togetherai",
        "@opencode/ai/providers/vercel-ai-gateway",
        "@opencode/ai/providers/xai",
        "@opencode/ai/providers/openrouter",
        "@opencode/ai/providers/azure",
    ];
    let name = package_name(package);
    LIST.contains(&name.as_str())
        || name.starts_with("@opencode/ai/providers/openai/")
        || name.starts_with("@opencode/ai/providers/azure/")
}

/// `hasConfiguredAuth` (model-resolver.ts:424-428): a non-empty
/// apiKey/authToken/accessToken in the provider or model settings.
fn has_configured_auth(
    provider_settings: &Option<oc_schema::provider::Settings>,
    model_settings: &Option<oc_schema::model::Settings>,
) -> bool {
    fn has_key(settings: &BTreeMap<String, Value>) -> bool {
        ["apiKey", "authToken", "accessToken"].iter().any(|key| {
            settings.get(*key).and_then(Value::as_str).is_some_and(|value| !value.is_empty())
        })
    }
    provider_settings.as_ref().is_some_and(|settings| has_key(&settings.rest))
        || model_settings.as_ref().is_some_and(|settings| has_key(&settings.rest))
}

fn map_provider(
    error: crate::provider_catalog::facade::ProviderError,
    provider: &str,
    model: &str,
) -> ModelError {
    match error {
        crate::provider_catalog::facade::ProviderError::UnknownProvider(_) => {
            // Keep the requested model segment: "Model unavailable: Own/"
            // (empty model id) hid the actual reference from the user.
            ModelError::Unavailable(provider.to_string(), model.to_string())
        }
        crate::provider_catalog::facade::ProviderError::UnknownModel(provider, model) => {
            ModelError::Unavailable(provider, model)
        }
        other => ModelError::Resolver(other.to_string()),
    }
}

fn map_custom(
    error: crate::provider_catalog::facade::ProviderError,
    provider: &str,
    model: &str,
) -> ModelError {
    map_provider(error, provider, model)
}

/// A minimal model info for custom providers (cost-less; limits unknown).
fn custom_model_info(custom: &CustomProvider) -> oc_schema::model::Info {
    oc_schema::model::Info {
        id: oc_schema::id::ModelId::parse(String::new()),
        model_id: oc_schema::id::ModelId::parse(String::new()),
        provider_id: oc_schema::id::ProviderId::parse(custom.id.clone()),
        canonical: None,
        family: None,
        name: String::new(),
        compatibility: None,
        package: None,
        settings: None,
        headers: None,
        body: None,
        capabilities: oc_schema::model::Capabilities::default(),
        variants: Vec::new(),
        time: oc_schema::model::ModelTime { released: oc_schema::common::Num(0.0) },
        cost: Vec::new(),
        status: oc_schema::model::Status::Active,
        enabled: true,
        limit: oc_schema::model::ModelLimit { context: 0, input: None, output: 0 },
    }
}
