//! Provider catalog: the embedded models.dev snapshot, its normalization
//! into `Provider.Info`/`Model.Info`, the variant overlay system, provider
//! facade binding onto oc-ai routes, and KV-cached remote refresh
//! (ports of `packages/core/src/models-dev.ts`, `variant.ts`, and the
//! binding half of `provider.ts`).
//!
//! - [`Catalog::embedded`] — the pinned snapshot (also the parity snapshot
//!   for gate-3); [`SNAPSHOT_TEXT`] is the verbatim file.
//! - [`variant::resolve`] — `PROTOCOLS[package]` overlay generation.
//! - [`facade::language_model`] — catalog provider → oc-ai
//!   [`LanguageModel`](oc_ai::LanguageModel), with a typed
//!   [`UnsupportedProtocol`](facade::ProviderError::UnsupportedProtocol)
//!   error for unported protocols.
//! - [`refresh::CatalogService`] — offline-default refresh behind
//!   [`refresh::CatalogSource`]/[`refresh::CatalogKv`] traits.

pub mod catalog;
pub mod facade;
pub mod native;
pub mod overlay;
pub mod plugin_headers;
pub mod refresh;
pub mod source;
pub mod support;
pub mod variant;

pub use catalog::{body_digest, Catalog, Snapshot, SNAPSHOT_TEXT};
pub use facade::{
    custom_language_model, family_of, language_model, CustomModel, CustomProvider, FacadeContext,
    ModelConnection, ProtocolFamily, ProviderError,
};
pub use native::{aisdk, native, native_package, package_name, AISDK_PREFIX};
pub use overlay::{providers_from_config, ConfigProviders};
pub use refresh::{CatalogKv, CatalogService, CatalogSource, RefreshOutcome, DEFAULT_SOURCE};
pub use source::parse_catalog;
pub use support::{supports_from_reasoning_options, Support, VariantModel};
pub use variant::resolve;
