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

    fn bind(
        &self,
        reference: &oc_schema::model::Ref,
    ) -> Result<(oc_ai::LanguageModel, oc_schema::model::Info), ModelError> {
        if let Some(custom) = self.custom.iter().find(|custom| custom.id == reference.provider_id.0)
        {
            let model = custom_language_model(&self.context, custom, &reference.id.0)
                .map_err(map_custom)?;
            let info = custom_model_info(custom);
            return Ok((model, info));
        }
        let connection =
            self.connections.get(&reference.provider_id.0).cloned().unwrap_or_default();
        let model = language_model(
            &self.context,
            &self.catalog,
            &reference.provider_id.0,
            &reference.id.0,
            &connection,
        )
        .map_err(map_provider)?;
        let info =
            self.catalog.model(&reference.provider_id.0, &reference.id.0).cloned().ok_or_else(
                || ModelError::Unavailable(reference.provider_id.0.clone(), reference.id.0.clone()),
            )?;
        Ok((model, info))
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
        let (model, info) = self.bind(reference)?;
        Ok(resolved_from(model, info, reference))
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

fn map_provider(error: crate::provider_catalog::facade::ProviderError) -> ModelError {
    match error {
        crate::provider_catalog::facade::ProviderError::UnknownProvider(provider) => {
            ModelError::Unavailable(provider, String::new())
        }
        crate::provider_catalog::facade::ProviderError::UnknownModel(provider, model) => {
            ModelError::Unavailable(provider, model)
        }
        other => ModelError::Resolver(other.to_string()),
    }
}

fn map_custom(error: crate::provider_catalog::facade::ProviderError) -> ModelError {
    map_provider(error)
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
