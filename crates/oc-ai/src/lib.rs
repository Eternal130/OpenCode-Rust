//! opencode LLM SDK (Rust port of `packages/ai` v2.0.24, foundation lane).
//!
//! Layout mirrors the TS package so protocol lanes slot in without moves:
//!
//! - [`schema`] — the wire contract: [`schema::events::LLMEvent`] (19-variant
//!   tagged union), messages/parts, [`schema::options`], ids, error taxonomy,
//!   media asset type. Anchored 1:1 to `packages/ai/src/schema/*.ts`.
//! - [`route`] — route composition (protocol + endpoint + auth + framing +
//!   transport), SSE/NDJSON framing, auth combinators, executor-level failure
//!   classification, and the process-agnostic [`route::transport::Transport`]
//!   seam.
//! - [`protocols`] — shared protocol utilities (partial JSON, tool-call
//!   accumulation, lifecycle emit) plus empty placeholder homes for the
//!   concrete protocol implementations (later lanes).
//! - [`provider_error`] — provider failure classification + retryability.
//! - [`cache_policy`] / [`effort_updates`] / [`tool_history`] /
//!   [`sanitize`] — pre-body request transforms (`sanitize` is a documented
//!   Rust-level no-op: Rust strings cannot carry lone surrogates).
//! - [`test_support`] — verbatim cassette replay infrastructure over the real
//!   route stack.
//! - [`testing`] — `TestLLM` equivalent for downstream core tests.

// Lint posture: the workspace enables pedantic lints as warnings; this crate
// runs clippy with `-D warnings`, so the lints below are deliberately
// allowed *here* with reasons. They are crate-design decisions, not noise.
#![allow(
    // `AiError` is the crate's single error currency (TS `AIError` parity);
    // boxing it inside every Result would break the taxonomy's ergonomics.
    clippy::result_large_err,
    // Builder-style `with`/`update` APIs mirror the TS immutable-patch style.
    clippy::return_self_not_must_use,
    // Struct-update idiom (`..current.clone()`) reads better than clone-assign.
    clippy::assigning_clones,
    // Ports keep the TS single-function shape (e.g. classifyProviderFailure).
    clippy::too_many_lines,
    // Taxonomy/reducer match arms are deliberately parallel to the TS switch.
    clippy::match_same_arms,
    // The erased protocol/transport seams need nested generic/pinned types.
    clippy::type_complexity,
    // f64 token counts are JS-number parity (oracle gate 2, finding 17); the
    // date math in executor.rs casts with the same precision as JS Date.
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    // Erased trait boundaries pass small owned values by design.
    clippy::needless_pass_by_value,
    // Reducer switches over the 19-variant event union keep a `_` arm for
    // future variants, mirroring the TS `default:` fallthrough.
    clippy::match_wildcard_for_single_variants,
    // `assert!(x.is_empty())` reads better than `assert_eq!(x.len(), 0)`.
    clippy::assert_is_empty,
    // Lifetimes bound to literal-backed constants mirror the TS shape.
    clippy::unnecessary_literal_bound,
    // Probe strings are built incrementally by design.
    clippy::format_push_string,
    // `Default::default()` in struct-update position vs explicit fields is
    // used where the TS spread (`...input`) sits.
    clippy::default_trait_access,
)]

pub mod cache_policy;
pub mod effort_updates;
pub mod protocols;
pub mod provider_error;
pub mod route;
pub mod sanitize;
pub mod schema;
pub mod test_support;
pub mod testing;
pub mod tool_history;

pub use schema::events::{LLMEvent, LLMResponse, Usage};
pub use schema::messages::{LLMRequest, Message};
pub use schema::options::LanguageModel;
