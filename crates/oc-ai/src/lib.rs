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
//! - [`cache_policy`] / [`effort_updates`] / [`tool_history`] — pre-body
//!   request transforms.
//! - [`test_support`] — verbatim cassette replay infrastructure over the real
//!   route stack.
//! - [`testing`] — `TestLLM` equivalent for downstream core tests.

pub mod cache_policy;
pub mod effort_updates;
pub mod provider_error;
pub mod protocols;
pub mod route;
pub mod schema;
pub mod test_support;
pub mod testing;
pub mod tool_history;

pub use schema::events::{LLMEvent, LLMResponse, Usage};
pub use schema::messages::{LLMRequest, Message};
pub use schema::options::LanguageModel;
