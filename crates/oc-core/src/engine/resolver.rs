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
use oc_ai::schema::options::{merge_json_records, merge_string_records};

use crate::database::schema::SessionRow;
use crate::provider_catalog::Catalog;
use crate::provider_catalog::facade::{
    CustomModel, CustomProvider, FacadeContext, ModelConnection, custom_language_model,
    language_model_for_model,
};
use crate::provider_catalog::native::package_name;
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
                let info = custom_model_info(custom, &canonical_model, &self.catalog());
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
                // Console baseURL precedence (opencode.ts:273-281): the
                // model-level overlay (applied inside the facade) > the
                // provider-level console baseURL > the catalog base. A
                // provider-level console baseURL DELETES the inherited
                // base-model baseURL (opencode.ts:273), so the catalog
                // base only ever fills an empty provider endpoint.
                if provider.base_url.is_empty() {
                    if let Some(model) =
                        provider.models.iter().find(|model| model.id == canonical_model)
                    {
                        provider.base_url =
                            console_catalog_base_url(&provider, model, &self.catalog());
                    }
                }
                let model = custom_language_model(&self.context, &provider, &canonical_model)
                    .map_err(|error| map_custom(error, &provider.id, &canonical_model))?;
                // CRED-A3: the selected variant's overlays fold into the
                // merged Info exactly like the catalog path — console-
                // declared variants (opencode.ts:282-293, merged onto
                // the Info in `custom_model_info`) resolve through the
                // same `withVariant` (model-resolver.ts:137-161), so
                // `#id` references against console models work and
                // unknown variants fail with VariantUnavailableError.
                let info = with_variant(
                    custom_model_info(&provider, &canonical_model, &self.catalog()),
                    reference.variant.as_ref().map(|id| id.0.as_str()),
                )?;
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
        let mut chatgpt_shaping = None;
        if let Some(bindings) = &self.credentials {
            if let Some(binding) = bindings.get(&provider_id) {
                connection.api_key = Some(binding.api_key.clone());
                connection.base_url = binding.base_url.clone().or(connection.base_url);
                for (key, value) in &binding.headers {
                    connection.headers.insert(key.clone(), value.clone());
                }
                // CRED-D6: the `openai` provider reshape this credential
                // drives — applied to the resolved model info below.
                chatgpt_shaping = binding.chatgpt;
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
        let mut info = with_variant(base_info, reference.variant.as_ref().map(|id| id.0.as_str()))?;
        // CRED-D6: the credential-driven ChatGPT provider reshape — the
        // codex/token-sharing logins re-list the `openai` provider (the
        // fallback model set, cost=[], the codex limits, summary
        // compaction), the transform v2's openai/chatgpt plugins apply
        // over the active credential (openai.ts:256-315,
        // chatgpt.ts:267-306).
        if let Some(shaping) = chatgpt_shaping {
            if !crate::integration::copilot::chatgpt_model_available(&info, shaping) {
                return Err(ModelError::Unavailable(provider_id.clone(), reference.id.0.clone()));
            }
            crate::integration::copilot::apply_chatgpt_shaping(&mut info, shaping);
        }
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
        // REF model-resolver.ts:397 — `transport: provider?.settings?.transport`
        // (the provider plugins default openai/xai/azure to "websocket";
        // omitted means HTTP). RV-P1: the session runner consults this.
        let transport = self
            .catalog()
            .provider(&canonical.provider_id.0)
            .and_then(|snapshot| snapshot.info.settings.as_ref())
            .and_then(|settings| settings.transport);
        let mut resolved = resolved_from(model, info, &canonical);
        resolved.transport = transport;
        Ok(resolved)
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

/// The console/config custom model's catalog base (opencode.ts:247,264):
/// the `canonical` provider first, else the provider id; the base model
/// by `config.modelID ?? modelKey`, then the console key.
fn custom_base_model<'a>(
    catalog: &'a Catalog,
    custom: &CustomProvider,
    model: &CustomModel,
) -> Option<&'a oc_schema::model::Info> {
    let snapshot = custom
        .canonical
        .as_deref()
        .and_then(|canonical| catalog.provider(canonical))
        .or_else(|| catalog.provider(&custom.id))?;
    let lookup = model.model_id.as_deref().unwrap_or(&model.id);
    snapshot
        .models
        .iter()
        .find(|base| base.id.0 == lookup)
        .or_else(|| snapshot.models.iter().find(|base| base.id.0 == model.id))
}

/// The catalog-base endpoint for a console provider — the last-resort
/// tier of the console baseURL precedence (opencode.ts:273 + the
/// facade's catalog `api` precedence, provider.ts:346-354): the base
/// model's `settings.baseURL`, else the catalog provider's api. Only
/// consulted when neither console baseURL tier applies.
fn console_catalog_base_url(
    custom: &CustomProvider,
    model: &CustomModel,
    catalog: &Catalog,
) -> String {
    let rest_base = |rest: &oc_schema::common::Record| {
        rest.get("baseURL").and_then(Value::as_str).unwrap_or_default().to_owned()
    };
    if let Some(base) = custom_base_model(catalog, custom, model) {
        let url =
            base.settings.as_ref().map(|settings| rest_base(&settings.rest)).unwrap_or_default();
        if !url.is_empty() {
            return url;
        }
    }
    custom
        .canonical
        .as_deref()
        .and_then(|canonical| catalog.provider(canonical))
        .or_else(|| catalog.provider(&custom.id))
        .and_then(|snapshot| snapshot.info.settings.as_ref())
        .map(|settings| rest_base(&settings.rest))
        .unwrap_or_default()
}

/// `Model.mergeCapabilities` (model.ts:32-39): per-field `config ?? base`
/// for {tools, input, output} over the base's capabilities (already the
/// defaults `{tools:true, input:[text,image], output:[text]}` when no
/// base exists).
fn merge_capabilities(
    config: &Option<crate::provider_catalog::facade::PartialCapabilities>,
    base: &oc_schema::model::Capabilities,
) -> oc_schema::model::Capabilities {
    let Some(config) = config else { return base.clone() };
    oc_schema::model::Capabilities {
        tools: config.tools.unwrap_or(base.tools),
        input: config.input.clone().unwrap_or_else(|| base.input.clone()),
        output: config.output.clone().unwrap_or_else(|| base.output.clone()),
    }
}

/// The `{...base, ...config}` compatibility spread (opencode.ts:270-271):
/// per-field override, unset console fields keep the base.
fn merge_compatibility(
    config: &oc_schema::model::Compatibility,
    base: Option<oc_schema::model::Compatibility>,
) -> oc_schema::model::Compatibility {
    let mut merged = base.unwrap_or_default();
    if config.reasoning_field.is_some() {
        merged.reasoning_field.clone_from(&config.reasoning_field);
    }
    if config.require_reasoning.is_some() {
        merged.require_reasoning = config.require_reasoning;
    }
    if config.max_tokens_field.is_some() {
        merged.max_tokens_field = config.max_tokens_field;
    }
    if config.require_finish_reason.is_some() {
        merged.require_finish_reason = config.require_finish_reason;
    }
    if config.require_assistant_after_tool.is_some() {
        merged.require_assistant_after_tool = config.require_assistant_after_tool;
    }
    if config.supports_prompt_cache_key.is_some() {
        merged.supports_prompt_cache_key = config.supports_prompt_cache_key;
    }
    merged
}

/// The effective model info for a custom/console provider entry — the
/// v2 console per-model merge (opencode.ts:263-306) onto the catalog
/// base when one exists (`canonical` provider, base model by
/// `config.modelID ?? key`), else the zero defaults with the console
/// fields alone: cost REPLACES, limit and capabilities merge per field,
/// base cost/limits/capabilities survive where the console is silent.
/// Public for the listing seam: the oc-server model handlers render
/// console-issued providers through this merge so `/api/model` shows
/// the merged values (I1) — the same single source of truth `bind`
/// uses.
pub fn custom_model_info(
    custom: &CustomProvider,
    model_key: &str,
    catalog: &Catalog,
) -> oc_schema::model::Info {
    let Some(model) = custom.models.iter().find(|model| model.id == model_key) else {
        // Unreachable via bind() (the model key comes from the same list);
        // keep the zero shape for safety.
        return oc_schema::model::Info {
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
        };
    };
    // `Object.assign(model, structuredClone(base ?? model))`
    // (opencode.ts:266): the base clone is the starting point; without
    // one, the config-plugin zero shape (limits 0 — handlers/model.rs
    // `custom_model_json` documents the same default).
    let mut info = custom_base_model(catalog, custom, model).cloned().unwrap_or_else(|| {
        oc_schema::model::Info {
            id: oc_schema::id::ModelId::parse(model.id.clone()),
            model_id: oc_schema::id::ModelId::parse(model.id.clone()),
            provider_id: oc_schema::id::ProviderId::parse(custom.id.clone()),
            canonical: None,
            family: None,
            name: model.name.clone().unwrap_or_else(|| model.id.clone()),
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
    });
    // The console entry's identity rides the result (opencode.ts:265,
    // 267-269): the console key stays the id; modelID redirects the
    // catalog identity.
    info.id = oc_schema::id::ModelId::parse(model.id.clone());
    info.provider_id = oc_schema::id::ProviderId::parse(custom.id.clone());
    if let Some(model_id) = &model.model_id {
        info.model_id = oc_schema::id::ModelId::parse(model_id.clone());
    }
    if let Some(family) = &model.family {
        info.family = Some(family.clone());
    }
    if let Some(name) = &model.name {
        info.name.clone_from(name);
    }
    if let Some(compatibility) = &model.compatibility {
        info.compatibility = Some(merge_compatibility(compatibility, info.compatibility.clone()));
    }
    // Capabilities per-field over the base (opencode.ts:274-275).
    info.capabilities = merge_capabilities(&model.capabilities, &info.capabilities);
    // Cost REPLACES the base wholesale (opencode.ts:294-303).
    if let Some(cost) = &model.cost {
        info.cost.clone_from(cost);
    }
    // `model.enabled = !config.disabled` (opencode.ts:304) — projection
    // filters disabled entries before they reach the snapshot (the
    // filter stays as-is), so every projected entry is enabled.
    info.enabled = true;
    // Limit per-field override (opencode.ts:305).
    if let Some(limit) = model.limit {
        info.limit = oc_schema::model::ModelLimit {
            context: limit.context.unwrap_or(info.limit.context),
            input: limit.input.or(info.limit.input),
            output: limit.output.unwrap_or(info.limit.output),
        };
    }
    // M3 — v2's model-overlay rules on the merged Info (opencode.ts:272,
    // 276-281): `model.package = config.package ?? (item.package !==
    // undefined ? undefined : model.package)` — the MODEL-level package
    // overrides the base, else a PROVIDER-level package clears the
    // base's model-level package (the binding reads the provider
    // package). The model's own overlays — settings.baseURL, headers,
    // body — merge onto the base Info per key (mergeOverlay semantics:
    // the base record under the model overlay).
    if let Some(package) = &model.package {
        info.package = Some(package.clone());
    } else if custom.package.is_some() {
        info.package = None;
    }
    if let Some(settings) = &model.settings {
        // `model.settings = mergeOverlay(withoutCredentials(model.settings),
        // withoutCredentials(config.settings))` (opencode.ts:277-280): the
        // base model's settings under the config overlay, per key.
        let mut rest =
            info.settings.as_ref().map(|settings| settings.rest.clone()).unwrap_or_default();
        for (key, value) in &settings.rest {
            rest.insert(key.clone(), value.clone());
        }
        info.settings = Some(oc_schema::model::Settings {
            compaction: settings
                .compaction
                .clone()
                .or_else(|| info.settings.as_ref().and_then(|s| s.compaction.clone())),
            rest,
        });
    }
    if let Some(headers) = &model.headers {
        info.headers = merge_string_records(&[info.headers.as_ref(), Some(headers)]);
    }
    if let Some(body) = &model.body {
        let record = |body: &BTreeMap<String, Value>| {
            serde_json::Map::from_iter(body.iter().map(|(key, value)| (key.clone(), value.clone())))
        };
        let base = info.body.as_ref().map(|body| record(body));
        let overlay = Some(record(body));
        info.body = merge_json_records(&[base.as_ref(), overlay.as_ref()])
            .map(|merged| BTreeMap::from_iter(merged));
    }
    // Console baseURL precedence on the Info seam (opencode.ts:273,
    // the same ladder `bind` applies, opencode.ts:273-281): a console
    // endpoint — the model-level overlay first, else the provider's —
    // DELETES the inherited base-model `settings.baseURL` and carries
    // the console value instead, so the merged Info never keeps the
    // stale catalog endpoint while the binding routes to the console
    // one. With no console endpoint the inherited one survives: it IS
    // the last-resort binding tier.
    let console_base_url = model
        .base_url
        .as_deref()
        .filter(|url| !url.is_empty())
        .or_else(|| (!custom.base_url.is_empty()).then_some(custom.base_url.as_str()));
    if let Some(base_url) = console_base_url {
        let mut settings = info.settings.take().unwrap_or_default();
        settings.rest.insert("baseURL".to_owned(), Value::String(base_url.to_owned()));
        info.settings = Some(settings);
    }
    // Console variants merge PER ID over the base's (opencode.ts:282-293):
    // an existing base variant with the same id is overlay-merged
    // (settings deep mergeOverlay, headers union, body overlay wins per
    // key); an unknown id is appended. Variants MERGE — they never
    // replace the base's list (unlike cost).
    for variant in &model.variants {
        let index = match info.variants.iter().position(|existing| existing.id.0 == variant.id) {
            Some(index) => index,
            None => {
                info.variants.push(oc_schema::model::Variant {
                    id: oc_schema::id::ModelVariantId::parse(variant.id.clone()),
                    settings: None,
                    headers: None,
                    body: None,
                });
                info.variants.len() - 1
            }
        };
        let entry = &mut info.variants[index];
        if let Some(variant_settings) = &variant.settings {
            let mut next = entry.settings.clone().unwrap_or_default();
            next.rest = merge_overlay_records(
                entry.settings.as_ref().map(|settings| &settings.rest),
                Some(&variant_settings.rest),
            )
            .unwrap_or_default();
            if let Some(compaction) = variant_settings.compaction {
                next.compaction = Some(compaction);
            }
            entry.settings = Some(next);
        }
        if variant.headers.is_some() {
            entry.headers = oc_ai::schema::options::merge_string_records(&[
                entry.headers.as_ref(),
                variant.headers.as_ref(),
            ]);
        }
        if variant.body.is_some() {
            entry.body = merge_overlay_records(entry.body.as_ref(), variant.body.as_ref());
        }
    }
    info
}

#[cfg(test)]
mod console_merge_tests {
    use super::*;
    use crate::provider_catalog::facade::{
        CustomModel, CustomProvider, PartialCapabilities, PartialModelLimit,
    };
    use oc_schema::common::UsdPerMillionTokens;
    use oc_schema::model::{Cost, CostCache, ModelLimit};
    use oc_schema::provider::Settings as ProviderSettings;

    fn catalog() -> Arc<Catalog> {
        Arc::new(Catalog::embedded().clone())
    }

    fn console_cost(input: f64, output: f64) -> Cost {
        Cost {
            tier: None,
            input: UsdPerMillionTokens(input.into()),
            output: UsdPerMillionTokens(output.into()),
            cache: CostCache {
                read: UsdPerMillionTokens::zero(),
                write: UsdPerMillionTokens::zero(),
            },
        }
    }

    fn console_custom() -> CustomProvider {
        CustomProvider {
            id: "acme-ai".into(),
            name: "Acme AI".into(),
            base_url: "https://gw.acme.test/v1".into(),
            api_key: None,
            headers: None,
            body: None,
            canonical: None,
            package: None,
            models: vec![CustomModel {
                id: "acme-large".into(),
                name: Some("Acme Large".into()),
                headers: None,
                body: None,
                model_id: None,
                family: Some("acme".into()),
                compatibility: None,
                capabilities: Some(PartialCapabilities {
                    tools: Some(false),
                    input: None,
                    output: Some(vec!["text".into(), "image".into()]),
                }),
                cost: Some(vec![console_cost(3.0, 15.0)]),
                limit: Some(PartialModelLimit {
                    context: Some(400_000),
                    input: Some(100_000),
                    output: Some(64_000),
                }),
                settings: None,
                base_url: None,
                package: None,
                variants: Vec::new(),
            }],
        }
    }

    /// F1: the model settings overlay merges base-under-config per key on
    /// the Info seam (`model.settings = mergeOverlay(
    /// withoutCredentials(model.settings), withoutCredentials(
    /// config.settings))`, opencode.ts:277-280).
    #[test]
    fn custom_model_info_merges_model_settings_over_base() {
        let mut custom = console_custom();
        // A catalog base carrying settings (anthropic/claude-opus-4-5).
        custom.canonical = Some("anthropic".into());
        let model = &mut custom.models[0];
        model.model_id = Some("claude-opus-4-5".into());
        model.settings = Some(oc_schema::model::Settings {
            compaction: None,
            rest: BTreeMap::from([("reasoningEffort".to_string(), serde_json::json!("high"))]),
        });
        let info = custom_model_info(&custom, "acme-large", &catalog());
        let rest = info.settings.as_ref().expect("merged settings").rest.clone();
        assert_eq!(rest.get("reasoningEffort"), Some(&serde_json::json!("high")));
        // base model settings survive where the overlay is silent
        // (mergeOverlay: base under config).
        let base =
            catalog().model("anthropic", "claude-opus-4-5").expect("base model").settings.clone();
        for (key, value) in &base.unwrap_or_default().rest {
            assert_eq!(rest.get(key), Some(value), "base key {key} survives");
        }
    }

    /// No catalog base ⇒ defaults with the console fields populated
    /// (opencode.ts:263-306 over a missing base): default capabilities
    /// `{tools:true,input:[text,image],output:[text]}` per-field merged,
    /// zero limits per-field overridden, console cost carried.
    #[test]
    fn custom_model_info_without_base_populates_console_fields() {
        let custom = console_custom();
        let info = custom_model_info(&custom, "acme-large", &catalog());
        assert_eq!(info.id.as_str(), "acme-large");
        assert_eq!(info.model_id.as_str(), "acme-large");
        assert_eq!(info.provider_id.as_str(), "acme-ai");
        assert_eq!(info.name, "Acme Large");
        assert_eq!(info.family.as_deref(), Some("acme"));
        // Partial capability merge over the defaults: tools overridden,
        // input falls back to the default (text+image), output overridden.
        assert!(!info.capabilities.tools, "console tools=false wins");
        assert_eq!(info.capabilities.input, vec!["text".to_string(), "image".to_string()]);
        assert_eq!(info.capabilities.output, vec!["text".to_string(), "image".to_string()]);
        assert_eq!(
            info.limit,
            ModelLimit { context: 400_000, input: Some(100_000), output: 64_000 }
        );
        assert_eq!(info.cost, vec![console_cost(3.0, 15.0)]);
        assert!(info.enabled);
    }

    /// Catalog-base merge (opencode.ts:247,264-305): base limits, cost and
    /// capabilities survive where the console is silent; console fields
    /// win per key; console cost REPLACES the base cost wholesale.
    #[test]
    fn custom_model_info_catalog_base_merge() {
        let catalog = catalog();
        let base = catalog
            .model("anthropic", "claude-opus-4-5")
            .expect("catalog anthropic/claude-opus-4-5")
            .clone();
        let mut custom = console_custom();
        custom.id = "acme-anthropic".into();
        custom.canonical = Some("anthropic".into());
        custom.models[0].id = "acme-opus".into();
        custom.models[0].model_id = Some("claude-opus-4-5".into());
        custom.models[0].name = None;
        // Only tools + context overridden; input/output + input/output
        // limits come from the base; cost REPLACES.
        custom.models[0].capabilities =
            Some(PartialCapabilities { tools: Some(false), input: None, output: None });
        custom.models[0].limit =
            Some(PartialModelLimit { context: Some(123), input: None, output: None });
        let info = custom_model_info(&custom, "acme-opus", &catalog);
        // Identity: the console key stays the id; modelID redirects.
        assert_eq!(info.id.as_str(), "acme-opus");
        assert_eq!(info.model_id.as_str(), "claude-opus-4-5");
        assert_eq!(info.provider_id.as_str(), "acme-anthropic");
        // Name: config name absent ⇒ the base name survives.
        assert_eq!(info.name, base.name);
        // Capabilities: per-field over the base.
        assert!(!info.capabilities.tools);
        assert_eq!(info.capabilities.input, base.capabilities.input);
        assert_eq!(info.capabilities.output, base.capabilities.output);
        // Limit: per-field override, base keeps the unset fields.
        assert_eq!(info.limit.context, 123);
        assert_eq!(info.limit.input, base.limit.input);
        assert_eq!(info.limit.output, base.limit.output);
        // Cost: console cost REPLACES the base cost wholesale.
        assert_eq!(info.cost, vec![console_cost(3.0, 15.0)]);
    }

    /// The base lookup honors `config.modelID` first, then the console
    /// key (opencode.ts:264).
    #[test]
    fn custom_model_info_base_lookup_by_model_id_then_key() {
        let catalog = catalog();
        let base = catalog
            .model("anthropic", "claude-opus-4-5")
            .expect("catalog anthropic/claude-opus-4-5")
            .clone();
        let mut custom = console_custom();
        custom.canonical = Some("anthropic".into());
        custom.models[0].id = "claude-opus-4-5".into();
        custom.models[0].model_id = None;
        custom.models[0].capabilities = None;
        custom.models[0].cost = None;
        custom.models[0].limit = None;
        let info = custom_model_info(&custom, "claude-opus-4-5", &catalog);
        assert_eq!(info.capabilities, base.capabilities, "base found by the console key");
    }

    /// M3 — v2's model-overlay rules on the merged Info (opencode.ts:272,
    /// 276-281): the provider-level package clears the base's model-level
    /// package, and the model's own headers/body/settings.baseURL ride
    /// the merged Info (mergeOverlay semantics, base under model — the
    /// catalog base carries no headers/body here, so the model's own
    /// overlays land directly).
    #[test]
    fn custom_model_info_applies_model_overlays_and_clears_package() {
        let catalog = catalog();
        let mut custom = console_custom();
        custom.canonical = Some("anthropic".into());
        custom.package = Some("@opencode/ai/providers/openai-compatible".into());
        custom.models[0].model_id = Some("claude-opus-4-5".into());
        custom.models[0].capabilities = None;
        custom.models[0].cost = None;
        custom.models[0].limit = None;
        custom.models[0].headers =
            Some(BTreeMap::from([("x-model".to_owned(), "model".to_owned())]));
        custom.models[0].body =
            Some(BTreeMap::from([("temperature".to_owned(), serde_json::json!(0.3))]));
        custom.models[0].base_url = Some("https://model.acme.test/v1".into());
        let info = custom_model_info(&custom, "acme-large", &catalog);
        // The base merged (its release date rides the result) — the
        // provider-level package still clears the model package.
        assert!(info.time.released.0 > 0.0, "the catalog base merged");
        assert_eq!(info.package, None, "provider-level package clears the model package");
        assert_eq!(
            info.headers.as_ref().and_then(|headers| headers.get("x-model")).map(String::as_str),
            Some("model")
        );
        assert_eq!(
            info.body.as_ref().and_then(|body| body.get("temperature")),
            Some(&serde_json::json!(0.3))
        );
        assert_eq!(
            info.settings.as_ref().and_then(|settings| settings.rest.get("baseURL")),
            Some(&serde_json::json!("https://model.acme.test/v1"))
        );
    }

    fn console_resolver(
        console: Arc<crate::integration::console::ConsoleProviders>,
    ) -> CatalogModelResolver {
        CatalogModelResolver::new(
            FacadeContext::new(Arc::new(oc_ai::route::transport::FailingTransport {
                message: "console merge test",
            })),
            catalog(),
            BTreeMap::new(),
            Vec::new(),
            None,
        )
        .with_console(console)
    }

    /// Console variants merge PER ID over the base's — never replacing
    /// the list (opencode.ts:282-293): an existing base variant with the
    /// same id gets its overlays deep-merged (variant wins per key), an
    /// unknown id is appended.
    #[test]
    fn custom_model_info_merges_console_variants_over_base() {
        let catalog = catalog();
        let base = catalog
            .model("anthropic", "claude-opus-4-5")
            .expect("catalog anthropic/claude-opus-4-5")
            .clone();
        let base_ids: Vec<String> = base.variants.iter().map(|v| v.id.0.clone()).collect();
        assert!(base_ids.iter().any(|id| id == "high"), "base carries an effort `high` variant");
        let mut custom = console_custom();
        custom.canonical = Some("anthropic".into());
        custom.models[0].id = "acme-opus".into();
        custom.models[0].model_id = Some("claude-opus-4-5".into());
        custom.models[0].capabilities = None;
        custom.models[0].cost = None;
        custom.models[0].limit = None;
        // Overlay onto the EXISTING base `high` variant + a brand-new id
        // (opencode.ts:283-286: find by id, else push `{ id }`).
        custom.models[0].variants = vec![
            crate::provider_catalog::facade::CustomModelVariant {
                id: "high".into(),
                settings: Some(oc_schema::model::Settings {
                    rest: BTreeMap::from([("temperature".to_owned(), serde_json::json!(0.3))]),
                    ..Default::default()
                }),
                headers: Some(BTreeMap::from([("x-v".to_owned(), "1".to_owned())])),
                body: None,
            },
            crate::provider_catalog::facade::CustomModelVariant {
                id: "turbo".into(),
                settings: None,
                headers: None,
                body: Some(BTreeMap::from([("top_k".to_owned(), serde_json::json!(7))])),
            },
        ];
        let info = custom_model_info(&custom, "acme-opus", &catalog);
        let ids: Vec<String> = info.variants.iter().map(|v| v.id.0.clone()).collect();
        // MERGE, not replace: every base variant survives plus `turbo`.
        for id in &base_ids {
            assert!(ids.contains(id), "base variant `{id}` survives the console merge");
        }
        assert!(ids.contains(&"turbo".to_owned()), "the new console variant is appended");
        // The base `high` variant's overlays deep-merge under the
        // console's (mergeOverlay: variant wins per key, opencode.ts:288-292).
        let high = info.variants.iter().find(|v| v.id.0 == "high").unwrap();
        assert_eq!(
            high.settings.as_ref().and_then(|s| s.rest.get("temperature")),
            Some(&serde_json::json!(0.3)),
            "console variant setting lands on the base variant"
        );
        assert_eq!(
            high.headers.as_ref().and_then(|h| h.get("x-v")).cloned(),
            Some("1".to_owned()),
            "console variant headers merge onto the base variant"
        );
        let turbo = info.variants.iter().find(|v| v.id.0 == "turbo").unwrap();
        assert_eq!(
            turbo.body.as_ref().and_then(|b| b.get("top_k")),
            Some(&serde_json::json!(7)),
            "the appended variant carries its own body overlay"
        );
    }

    /// `model.package = config.package ?? (item.package !== undefined
    /// ? undefined : model.package)` (opencode.ts:272): the MODEL-level
    /// package overrides the base; else a provider-level package CLEARS
    /// the base's; with neither the base package survives.
    #[test]
    fn custom_model_info_package_override_ladder() {
        let catalog = catalog();
        let base = catalog
            .model("anthropic", "claude-opus-4-5")
            .expect("catalog anthropic/claude-opus-4-5")
            .clone();
        let base_package = base.package.clone();
        let mut custom = console_custom();
        custom.canonical = Some("anthropic".into());
        custom.models[0].model_id = Some("claude-opus-4-5".into());
        custom.models[0].capabilities = None;
        custom.models[0].cost = None;
        custom.models[0].limit = None;

        // Neither console package: the base model package survives.
        let info = custom_model_info(&custom, "acme-large", &catalog);
        assert_eq!(info.package, base_package, "no console package keeps the base's");

        // Provider-level only: clears the base model package.
        custom.package = Some("@opencode/ai/providers/anthropic".into());
        let info = custom_model_info(&custom, "acme-large", &catalog);
        assert_eq!(info.package, None, "provider package clears the model package");

        // Model-level wins over both (opencode.ts:272 config.package first).
        custom.models[0].package = Some("@opencode/ai/providers/openai-compatible".into());
        let info = custom_model_info(&custom, "acme-large", &catalog);
        assert_eq!(
            info.package.as_deref(),
            Some("@opencode/ai/providers/openai-compatible"),
            "model package overrides even with a provider package set"
        );
    }

    /// CRED-A3 wiring: console-declared variants resolve through
    /// `withVariant` at bind (model-resolver.ts:137-161) — the selected
    /// variant's settings fold into the bound Info, and an unknown
    /// variant fails the resolution.
    #[test]
    fn console_bind_resolves_variant_overlays() {
        let shared = crate::integration::console::ConsoleProviders::new();
        let mut custom = console_custom();
        custom.models[0].variants = vec![crate::provider_catalog::facade::CustomModelVariant {
            id: "fast".into(),
            settings: Some(oc_schema::model::Settings {
                rest: BTreeMap::from([("temperature".to_owned(), serde_json::json!(0.1))]),
                ..Default::default()
            }),
            headers: None,
            body: None,
        }];
        shared.swap_for_test(vec![custom]);
        let resolver = console_resolver(shared);
        let reference = |variant: Option<&str>| oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("acme-ai".to_owned()),
            id: oc_schema::id::ModelId::parse("acme-large".to_owned()),
            variant: variant.map(|id| oc_schema::id::ModelVariantId::parse(id.to_owned())),
        };
        let (_model, info, _canonical) =
            resolver.bind(&reference(Some("fast"))).expect("console variant binds");
        assert_eq!(
            info.settings.as_ref().and_then(|s| s.rest.get("temperature")),
            Some(&serde_json::json!(0.1)),
            "the selected console variant's settings fold into the Info"
        );
        let error = resolver.bind(&reference(Some("nope"))).expect_err("unknown variant fails");
        assert!(error.to_string().contains("variant"), "{error}");
    }

    /// The `cohere/north-mini-code-1-0` catalog model carries its own
    /// model-level api → `settings.baseURL` — the inherited base-model
    /// endpoint the precedence ladder must override or honor.
    fn inherited_base_model_url(catalog: &Catalog) -> String {
        catalog
            .model("cohere", "north-mini-code-1-0")
            .expect("catalog cohere/north-mini-code-1-0")
            .settings
            .as_ref()
            .and_then(|settings| settings.rest.get("baseURL"))
            .and_then(Value::as_str)
            .expect("base model settings baseURL")
            .to_owned()
    }

    fn cohere_custom() -> CustomProvider {
        CustomProvider {
            id: "acme-cohere".into(),
            name: "Acme Cohere".into(),
            base_url: String::new(),
            api_key: None,
            headers: None,
            body: None,
            canonical: Some("cohere".into()),
            package: None,
            models: vec![CustomModel {
                id: "north-mini-code-1-0".into(),
                name: None,
                headers: None,
                body: None,
                model_id: None,
                family: None,
                compatibility: None,
                capabilities: None,
                cost: None,
                limit: None,
                settings: None,
                base_url: None,
                package: None,
                variants: Vec::new(),
            }],
        }
    }

    fn bound_base_url(custom: CustomProvider) -> String {
        let shared = crate::integration::console::ConsoleProviders::new();
        shared.swap_for_test(vec![custom]);
        let resolver = console_resolver(shared);
        let (model, _info, _canonical) = resolver
            .bind(&oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("acme-cohere".to_owned()),
                id: oc_schema::id::ModelId::parse("north-mini-code-1-0".to_owned()),
                variant: None,
            })
            .expect("console provider binds");
        model.route.endpoint.base_url.as_deref().expect("bound endpoint").to_owned()
    }

    /// baseURL precedence (opencode.ts:273-281): console model >
    /// console provider > base/catalog model — and a console PROVIDER
    /// baseURL deletes the inherited base-model baseURL.
    #[test]
    fn console_bind_base_url_precedence() {
        let catalog = catalog();
        let inherited = inherited_base_model_url(&catalog);
        assert!(!inherited.is_empty());

        // 3) no console baseURL ⇒ the inherited base-model endpoint binds.
        assert_eq!(bound_base_url(cohere_custom()), inherited);

        // 2) a console provider baseURL deletes the inherited base-model
        // baseURL (opencode.ts:273) and becomes the endpoint.
        let mut provider_level = cohere_custom();
        provider_level.base_url = "https://gw.cohere.test/v1".into();
        assert_eq!(bound_base_url(provider_level), "https://gw.cohere.test/v1");

        // 1) a console model baseURL outranks the provider's.
        let mut model_level = cohere_custom();
        model_level.base_url = "https://gw.cohere.test/v1".into();
        model_level.models[0].base_url = Some("https://model.cohere.test/v1".into());
        assert_eq!(bound_base_url(model_level), "https://model.cohere.test/v1");
    }

    /// Review round 2 (stale inherited baseURL): when the console
    /// PROVIDER-level baseURL is set and the base model carries its own
    /// `settings.baseURL`, the merged Info must not keep the stale
    /// inherited endpoint while `bind` routes to the console one —
    /// opencode.ts:273 deletes the inherited base-model baseURL when a
    /// console endpoint exists (model-level > provider-level, the same
    /// precedence `bind` applies). With no console endpoint the
    /// inherited one survives (it IS the binding's last-resort tier);
    /// with neither console nor base endpoint the Info simply has none.
    #[test]
    fn custom_model_info_console_base_url_replaces_inherited() {
        let catalog = catalog();
        let inherited = inherited_base_model_url(&catalog);

        // Provider-level console endpoint: the listed Info carries the
        // console endpoint, not the stale inherited cohere one.
        let mut provider_level = cohere_custom();
        provider_level.base_url = "https://gw.cohere.test/v1".into();
        let info = custom_model_info(&provider_level, "north-mini-code-1-0", &catalog);
        assert_eq!(
            info.settings.as_ref().and_then(|settings| settings.rest.get("baseURL")),
            Some(&serde_json::json!("https://gw.cohere.test/v1")),
            "the console provider endpoint replaces the inherited baseURL"
        );

        // Model-level outranks the provider's on the Info, exactly like
        // the binding precedence.
        let mut model_level = provider_level;
        model_level.models[0].base_url = Some("https://model.cohere.test/v1".into());
        let info = custom_model_info(&model_level, "north-mini-code-1-0", &catalog);
        assert_eq!(
            info.settings.as_ref().and_then(|settings| settings.rest.get("baseURL")),
            Some(&serde_json::json!("https://model.cohere.test/v1")),
            "the console model endpoint outranks the provider's"
        );

        // No console endpoint: the inherited base-model endpoint
        // survives (the last-resort binding tier).
        let info = custom_model_info(&cohere_custom(), "north-mini-code-1-0", &catalog);
        assert_eq!(
            info.settings.as_ref().and_then(|settings| settings.rest.get("baseURL")),
            Some(&serde_json::json!(inherited)),
            "no console baseURL keeps the inherited endpoint"
        );

        // Neither console nor base endpoint: absent, never synthesized.
        let mut orphan = cohere_custom();
        orphan.canonical = None;
        let info = custom_model_info(&orphan, "north-mini-code-1-0", &catalog);
        assert_eq!(
            info.settings.as_ref().and_then(|settings| settings.rest.get("baseURL")),
            None,
            "no endpoint anywhere leaves the Info without a baseURL"
        );
    }

    /// The catalog provider-level api is the last-resort endpoint when
    /// the base model carries no baseURL of its own.
    #[test]
    fn console_bind_falls_back_to_catalog_provider_base_url() {
        let catalog = catalog();
        let expected = catalog
            .provider("fireworks-ai")
            .expect("catalog fireworks-ai")
            .info
            .settings
            .as_ref()
            .and_then(|settings: &ProviderSettings| settings.rest.get("baseURL"))
            .and_then(Value::as_str)
            .expect("fireworks catalog api")
            .to_owned();
        let shared = crate::integration::console::ConsoleProviders::new();
        let mut custom = cohere_custom();
        custom.canonical = Some("fireworks-ai".into());
        let model_id = catalog
            .provider("fireworks-ai")
            .expect("catalog fireworks-ai")
            .models
            .first()
            .expect("fireworks model")
            .id
            .as_str()
            .to_owned();
        custom.models[0].id = model_id.clone();
        shared.swap_for_test(vec![custom]);
        let resolver = console_resolver(shared);
        let (model, _info, _canonical) = resolver
            .bind(&oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("acme-cohere".to_owned()),
                id: oc_schema::id::ModelId::parse(model_id),
                variant: None,
            })
            .expect("console provider binds at the catalog api");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some(expected.as_str()),
            "the catalog provider api is the last-resort endpoint"
        );
    }

    /// No endpoint anywhere (no console baseURLs, no catalog base) ⇒ the
    /// clean typed Configuration error, never a half-bound route.
    #[test]
    fn console_bind_without_any_base_url_fails_cleanly() {
        let shared = crate::integration::console::ConsoleProviders::new();
        let mut custom = cohere_custom();
        custom.canonical = None; // no catalog base either
        shared.swap_for_test(vec![custom]);
        let resolver = console_resolver(shared);
        let error = resolver
            .bind(&oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("acme-cohere".to_owned()),
                id: oc_schema::id::ModelId::parse("north-mini-code-1-0".to_owned()),
                variant: None,
            })
            .expect_err("no endpoint anywhere");
        assert!(error.to_string().contains("baseURL"), "{error}");
    }
}
