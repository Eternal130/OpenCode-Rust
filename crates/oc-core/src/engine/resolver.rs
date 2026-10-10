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

use crate::database::schema::SessionRow;
use crate::provider_catalog::facade::{
    custom_language_model, language_model, CustomProvider, FacadeContext, ModelConnection,
};
use crate::provider_catalog::Catalog;
use crate::session::runner::{ModelError, ModelResolver, ResolvedModel};

/// The production resolver: catalog + user connections + custom
/// providers over one shared transport.
pub struct CatalogModelResolver {
    context: FacadeContext,
    catalog: Arc<Catalog>,
    connections: BTreeMap<String, ModelConnection>,
    custom: Vec<CustomProvider>,
    /// The default model selection (config `model`).
    default: Option<oc_schema::config::ModelSelection>,
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
        Self { context, catalog, connections, custom, default }
    }

    /// The catalog's canonical provider id for a requested prefix: the
    /// exact key first, then a case-insensitive match (the config-key
    /// `own` behind a `Own/model` reference — the live beta.3.1 defect).
    /// v2 itself matches catalog ids exactly (`Model.Service.get` walks
    /// exact Maps, model.ts:255-257); the case-insensitive fallback is
    /// our tolerance so a mixed-case reference resolves to the
    /// configured provider instead of `provider.no-route`.
    fn canonical_catalog_provider(&self, requested: &str) -> Option<String> {
        if self.catalog.provider(requested).is_some() {
            return Some(requested.to_owned());
        }
        self.catalog
            .providers()
            .iter()
            .find(|snapshot| snapshot.info.id.0.eq_ignore_ascii_case(requested))
            .map(|snapshot| snapshot.info.id.0.clone())
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
    fn bind(
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
            let canonical_model = custom
                .models
                .iter()
                .find(|id| id.eq_ignore_ascii_case(&reference.id.0))
                .cloned()
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
        let connection = self.connection_for(&reference.provider_id.0);
        let model = language_model(
            &self.context,
            &self.catalog,
            &provider_id,
            &reference.id.0,
            &connection,
        )
        .map_err(|error| map_provider(error, &provider_id, &reference.id.0))?;
        let info =
            self.catalog.model(&provider_id, &reference.id.0).cloned().ok_or_else(|| {
                ModelError::Unavailable(provider_id.clone(), reference.id.0.clone())
            })?;
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
