//! Wave B session runner — the LLM drain that turns admitted inbox input
//! into durable assistant steps (port of `session/runner/*` +
//! `session/{context,model-request,system-prompt,title,compaction}.ts`,
//! core-runner-map §1.1–§1.7, §2.4–§2.5, §3).
//!
//! Layout:
//!
//! - [`drain`]: the `drain()` outer loop + `advance_to_step()` boundary
//!   state machine (llm.ts) — plugs into
//!   [`crate::session::execution::SessionExecution`]'s injectable
//!   [`crate::session::execution::DrainFn`];
//! - [`step`]: one attempt's streaming/tool/settlement state machine
//!   (step.ts §1.4) with the finalization order 1–13;
//! - [`publisher`]: the single-writer `LLMEventPublisher`
//!   (publish-llm-event.ts; runner-concurrency.md §1);
//! - [`retry`]: the exponential/jittered schedule + `session.retry` hook;
//! - [`context`]: two-phase select/load (context.ts §3.1) + the
//!   instructions machinery + history boundaries;
//! - [`model_request`]: request preparation (model-request.ts §3.3);
//! - [`to_llm_message`]: projected history → canonical LLM messages
//!   (§3.4);
//! - [`system_prompt`]: the verbatim `system.txt` template + tool guidance;
//! - [`compaction`]: full §2.5 (due/ceiling math, summarize + native paths,
//!   deliver loop, shrink steps);
//! - [`title`]: §2.4 title generation;
//! - [`max_steps`]: the max-steps synthetic prompt.
//!
//! Lint posture: the runner ports TS numeric semantics verbatim
//! (JS `number` ⇄ `f64`), mirrors exhaustive match tables, and keeps the
//! Wave A style of allowing the mechanical-idiom lints crate-wide.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructors consume their parts
    clippy::too_many_lines, // the dispatch tables mirror the TS sources
    clippy::match_same_arms, // the exhaustive tables keep per-arm comments
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements, // per-arm helpers mirror the TS closures
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity, // hook/executor seams match the lib's
    clippy::large_enum_variant, // ToolExit/Outcome mirror the tagged unions
    clippy::struct_field_names, // *_message_id fields mirror the TS names
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty, // the TS interfaces only expose len
    clippy::module_name_repetitions,
    clippy::result_large_err, // DeliverError mirrors the TS union
    clippy::doc_markdown, // TS identifiers in prose stay unquoted
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::needless_continue,
    clippy::significant_drop_tightening, // publish-then-mark ordering is deliberate
    clippy::large_futures, // the attempt/drain futures mirror the TS fibers
    clippy::option_option, // patch-style Option<Option<T>> surfaces
    clippy::assert_is_empty, // "expected empty" reads clearer in ports
)]

pub mod compaction;
pub mod context;
pub mod drain;
pub mod max_steps;
pub mod model_request;
pub mod prompt_prepare;
pub mod publisher;
pub mod retry;
pub mod step;
pub mod system_prompt;
pub mod title;
pub mod to_llm_message;

pub mod generate;
pub use generate::{GenerateError, SessionGenerate};
pub use prompt_prepare::{PromptPrepare, PromptPrepareError};

#[cfg(test)]
mod tests_generate;

#[cfg(test)]
mod tests_compaction;
#[cfg(test)]
mod tests_e2e;
#[cfg(test)]
pub(crate) mod tests_harness;
#[cfg(test)]
mod tests_message;
#[cfg(test)]
mod tests_misc;
#[cfg(test)]
mod tests_rd11_rd13;
#[cfg(test)]
mod tests_recorded;
#[cfg(test)]
mod tests_request_path;
#[cfg(test)]
mod tests_retry;
#[cfg(test)]
mod tests_step;
#[cfg(test)]
mod tests_tool_events;
#[cfg(test)]
mod tests_usage_affinity;

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use oc_ai::schema::{
    AiError, AiErrorReason, FinishReasonDetails, LLMEvent, LLMRequest, Message, ReasonFields,
    TransportOperation, TransportRecovery, Usage,
};
use oc_schema::common::{Num, SessionError, TokenUsage, Usd};
use oc_schema::model;

pub use crate::session::execution::{Continuation, DrainOutcome, RunError};

/// One provider event stream (boxed; the only await surface the step loop
/// polls).
pub type LlmEventStream = Pin<Box<dyn futures::Stream<Item = Result<LLMEvent, AiError>> + Send>>;

/// The result of a native (provider-side) compaction call.
#[derive(Debug, Clone)]
pub struct CompactOutcome {
    /// Endpoint mechanism: the replacement message window.
    pub replacement: Vec<Message>,
    /// Trigger mechanism: the encrypted checkpoint part.
    pub checkpoint: Option<oc_ai::schema::ContentPart>,
    pub usage: Option<Usage>,
}

/// The compaction mechanism a route supports (`LLMClient.canCompact`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactMechanism {
    /// The standalone `/compact` endpoint.
    Endpoint,
    /// The `compaction_trigger` item inside a normal stream.
    Trigger,
}

/// The LLM client seam (`LLMClient.Service`). Production wires an
/// oc-ai `Route`; tests wire scripted streams.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Streams one request to its terminal event.
    async fn stream(&self, request: LLMRequest) -> Result<LlmEventStream, AiError>;

    /// Streams one request with the session's model transport consulted
    /// (REF model-request.ts:352 `webSocket: "session"` + runner llm.ts:242):
    /// when `transport` is websocket and the request's route offers the
    /// OpenResponses WS channel, the stream rides the session's bound
    /// channel executor (sticky HTTP fallback lives inside it). The
    /// default ignores the transport and streams over HTTP SSE.
    async fn stream_for_session(
        &self,
        session_id: &str,
        transport: Option<oc_schema::provider::Transport>,
        request: LLMRequest,
    ) -> Result<LlmEventStream, AiError> {
        let _ = (session_id, transport);
        self.stream(request).await
    }

    /// Closes the session's model transport (REF runner llm.ts
    /// `modelTransport.close(sessionID)` on location-closed and move).
    async fn close_transport(&self, _session_id: &str) {}

    /// Whether the request's route supports native compaction for
    /// `mechanism` (`LLMClient.canCompact`).
    fn can_compact(&self, request: &LLMRequest, mechanism: CompactMechanism) -> bool;

    /// Runs the native compaction (`LLMClient.compact`).
    async fn compact(
        &self,
        request: LLMRequest,
        mechanism: CompactMechanism,
    ) -> Result<CompactOutcome, AiError>;
}

/// The production client over oc-ai routes: streams through the request's
/// own route; native compaction through the OpenAI-Responses compact
/// endpoints and the Anthropic in-stream `context_management.edits`
/// trigger (the `compact-2026-01-12` beta the messages protocol carries).
pub struct RouteLlmClient;

/// The process-global session channel owner (v2 `SessionModelTransport`
/// is a `makeGlobalNode` service — one owner per process, one socket per
/// session id). `Lazy` so tests that never touch a websocket model pay
/// nothing for the connector.
static SESSION_CHANNELS: std::sync::OnceLock<Arc<oc_ai::route::transport::websocket::SessionChannels>> =
    std::sync::OnceLock::new();

fn session_channels() -> Arc<oc_ai::route::transport::websocket::SessionChannels> {
    Arc::clone(SESSION_CHANNELS.get_or_init(oc_ai::route::transport::websocket::default_session_channels))
}

#[async_trait::async_trait]
impl LlmClient for RouteLlmClient {
    async fn stream(&self, request: LLMRequest) -> Result<LlmEventStream, AiError> {
        let route = Arc::clone(&request.model.route);
        let stream = route.stream(request).await?;
        Ok(Box::pin(stream))
    }

    async fn stream_for_session(
        &self,
        session_id: &str,
        transport: Option<oc_schema::provider::Transport>,
        request: LLMRequest,
    ) -> Result<LlmEventStream, AiError> {
        let route = Arc::clone(&request.model.route);
        // REF model-request.ts:352: the WS channel is offered only when the
        // caller asked for the session transport (`webSocket: "session"`,
        // the runner always does) AND `model.transport === "websocket"`;
        // `channel_options_for_route` then gates on the route's protocol
        // (openai-responses incl. the Azure flavor; xai and everything
        // else stay on HTTP SSE).
        let channel_options = if transport == Some(oc_schema::provider::Transport::Websocket) {
            oc_ai::protocols::open_responses_channel::channel_options_for_route(&route)
        } else {
            None
        };
        let Some(options) = channel_options else {
            return self.stream(request).await;
        };
        // REF model-transport.ts `bind`: one long-lived socket per session,
        // sticky HTTP fallback inside the executor (refused connects,
        // repeated stream losses, oversized frames).
        let executor = session_channels().bind(session_id);
        let channel =
            oc_ai::protocols::open_responses_channel::stream_channel(&route, request, executor, &options)
                .await?;
        // Commit the continuation checkpoint once the events stream ended
        // cleanly (v2 `execution.complete` after a successful stream).
        let oc_ai::protocols::open_responses_channel::ChannelStream { events, complete, .. } =
            channel;
        let wrapped = futures::stream::unfold(
            (events, complete, true),
            |(mut events, complete, clean)| async move {
                use futures::StreamExt as _;
                match events.next().await {
                    Some(Ok(event)) => Some((Ok(event), (events, complete, clean))),
                    Some(Err(error)) => Some((Err(error), (events, complete, false))),
                    None => {
                        if clean {
                            let _ = complete();
                        }
                        None
                    }
                }
            },
        );
        Ok(Box::pin(wrapped))
    }

    async fn close_transport(&self, session_id: &str) {
        session_channels().close(session_id).await;
    }

    fn can_compact(&self, request: &LLMRequest, mechanism: CompactMechanism) -> bool {
        match request.model.route.protocol.id() {
            "openai-responses" => true,
            // Anthropic native compaction is trigger-only: the
            // `context_management.edits` request rides a normal stream and
            // the provider answers with one `compaction` content block
            // (anthropic_messages.rs parser + beta header wiring).
            "anthropic-messages" => matches!(mechanism, CompactMechanism::Trigger),
            _ => false,
        }
    }

    async fn compact(
        &self,
        request: LLMRequest,
        mechanism: CompactMechanism,
    ) -> Result<CompactOutcome, AiError> {
        let route = Arc::clone(&request.model.route);
        match mechanism {
            CompactMechanism::Endpoint => {
                let response =
                    oc_ai::protocols::openai_responses::compact(&route, &request).await?;
                Ok(CompactOutcome {
                    replacement: response.replacement,
                    checkpoint: None,
                    usage: response.usage,
                })
            }
            CompactMechanism::Trigger => {
                if route.protocol.id() == "anthropic-messages" {
                    return anthropic_trigger_compact(route, request).await;
                }
                let response =
                    oc_ai::protocols::openai_responses::trigger_compaction(&route, &request)
                        .await?;
                Ok(CompactOutcome {
                    replacement: Vec::new(),
                    checkpoint: Some(oc_ai::schema::messages::ContentPart::Compaction {
                        provider: response.checkpoint.provider,
                        id: Some(response.checkpoint.id),
                        encrypted: Some(response.checkpoint.encrypted),
                        text: None,
                    }),
                    usage: response.usage,
                })
            }
        }
    }
}

/// The encode failure for the anthropic compaction options.
fn compaction_options_error(
    provider: &oc_ai::schema::ProviderId,
    error: serde_json::Error,
) -> AiError {
    AiError::new(AiErrorReason::ProviderConfiguration {
        provider: provider.clone(),
        message: format!("compaction options encode failed: {error}"),
    })
}

/// The Anthropic trigger mechanism: replay the window through a normal
/// stream carrying `contextManagement.edits: [{type:
/// "compact_20260112"}]`, collect the single `compaction` content block
/// as the checkpoint, and bill the stream's usage.
async fn anthropic_trigger_compact(
    route: Arc<oc_ai::route::client::Route>,
    request: LLMRequest,
) -> Result<CompactOutcome, AiError> {
    // The edit trigger requires an input-token threshold (>= 50k); the
    // compaction request targets the smallest window the model accepts.
    const ANTHROPIC_COMPACT_TRIGGER_TOKENS: f64 = 50_000.0;
    let mut request = request;
    let options = request.provider_options.clone().unwrap_or_default();
    let mut options = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
        serde_json::to_value(&options).unwrap_or(serde_json::Value::Null),
    )
    .unwrap_or_default();
    options.insert(
        "contextManagement".into(),
        serde_json::json!({
            "edits": [
                { "type": "compact_20260112", "trigger": { "type": "input_tokens", "value": ANTHROPIC_COMPACT_TRIGGER_TOKENS } }
            ]
        }),
    );
    request.provider_options = serde_json::from_value(serde_json::Value::Object(options))
        .map_err(|error| compaction_options_error(&request.model.provider, error))?;

    let stream = route.stream(request).await?;
    tokio::pin!(stream);
    use futures::StreamExt;
    let mut checkpoint: Option<oc_ai::schema::messages::ContentPart> = None;
    let mut usage = None;
    let mut failed: Option<AiError> = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(LLMEvent::Compaction { provider, id, encrypted, text }) => {
                checkpoint = Some(oc_ai::schema::messages::ContentPart::Compaction {
                    provider,
                    id,
                    encrypted,
                    text,
                });
            }
            Ok(LLMEvent::Finish { usage: finish_usage, .. }) => {
                usage = finish_usage.or(usage);
            }
            Ok(_) => {}
            Err(error) => {
                failed = Some(error);
                break;
            }
        }
    }
    if let Some(error) = failed {
        return Err(error);
    }
    let checkpoint = checkpoint.ok_or_else(|| {
        AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new("Compaction response must contain exactly one checkpoint"),
            classification: None,
            route: None,
        })
    })?;
    Ok(CompactOutcome { replacement: Vec::new(), checkpoint: Some(checkpoint), usage })
}

// ---------------------------------------------------------------------------
// Model resolution (runner/model.ts)
// ---------------------------------------------------------------------------

/// A fully resolved model: the configured [`oc_ai::LanguageModel`] plus the
/// catalog identity the runner records (`SessionRunnerModel.Resolved`).
#[derive(Clone)]
pub struct ResolvedModel {
    pub model: oc_ai::LanguageModel,
    /// `Model.Ref` recorded on assistant messages.
    pub reference: model::Ref,
    pub capabilities: model::Capabilities,
    pub cost: Vec<model::Cost>,
    pub limit: model::ModelLimit,
    pub compaction: Option<oc_schema::provider::Compaction>,
    pub transport: Option<oc_schema::provider::Transport>,
}

impl std::fmt::Debug for ResolvedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedModel")
            .field("reference", &self.reference)
            .field("route", &self.model.route.id)
            .finish_non_exhaustive()
    }
}

impl ResolvedModel {
    /// The provider metadata key for provider state replay
    /// (`route.providerMetadataKey ?? model.provider`).
    #[must_use]
    pub fn provider_metadata_key(&self) -> &str {
        self.model.route.provider_metadata_key.as_deref().unwrap_or(&self.model.provider.0)
    }
}

/// `SessionRunnerModel.Error`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("No model is available for session {0}")]
    NotSelected(String),
    #[error("Model unavailable: {0}/{1}")]
    Unavailable(String, String),
    #[error("{0}")]
    Resolver(String),
}

/// `SessionRunnerModel.Interface.resolve` — availability is sampled lazily
/// per resolution; tests inject a fixed resolver.
pub trait ModelResolver: Send + Sync {
    /// Resolves the session's selected model (`session.model`), or the
    /// configured default when none is selected.
    fn resolve(
        &self,
        session: &crate::database::schema::SessionRow,
    ) -> Result<ResolvedModel, ModelError>;

    /// Resolves one explicit reference (the compaction agent's model, a
    /// title model override).
    fn resolve_selected(
        &self,
        session: &crate::database::schema::SessionRow,
        reference: &model::Ref,
    ) -> Result<ResolvedModel, ModelError>;
}

// ---------------------------------------------------------------------------
// to-session-error (to-session-error.ts, AIError branch + content filter)
// ---------------------------------------------------------------------------

const TOKEN_SHARING_MESSAGES: &[(&str, &str)] = &[
    (
        "subscription_sharing_user_not_eligible",
        "ChatGPT token sharing isn't available for this account. Connect with an API key or choose another provider.",
    ),
    (
        "subscription_sharing_usage_limit_exceeded",
        "ChatGPT usage limit reached. Check ChatGPT Settings → Usage for details.",
    ),
    (
        "subscription_sharing_usage_unavailable",
        "ChatGPT usage can't be checked right now. Try again later.",
    ),
    (
        "subscription_sharing_unsupported_capability",
        "This request uses a feature ChatGPT token sharing doesn't support. Remove the unsupported feature and try again.",
    ),
    (
        "subscription_sharing_route_not_supported",
        "ChatGPT token sharing doesn't support this API route. Check the configured endpoint and HTTP method.",
    ),
    (
        "subscription_sharing_invalid_user",
        "This ChatGPT connection is no longer valid. Reconnect to ChatGPT.",
    ),
    (
        "subscription_sharing_user_unavailable",
        "Your ChatGPT account is temporarily unavailable. Try again later.",
    ),
    (
        "chatpass_v2_scope_not_authorized",
        "This ChatGPT connection isn't authorized for this request. Reconnect to ChatGPT or choose another connection.",
    ),
    (
        "chatpass_v2_invalid_authorization_context",
        "This ChatGPT connection isn't authorized for this request. Reconnect to ChatGPT or choose another connection.",
    ),
];

/// `toSessionError(cause)` for `AIError` causes.
#[must_use]
pub fn to_session_error(error: &AiError) -> SessionError {
    let error_type = match &error.reason {
        AiErrorReason::RateLimit { .. } => "provider.rate-limit",
        AiErrorReason::Authentication { .. } => "provider.auth",
        AiErrorReason::QuotaExceeded { .. } => "provider.quota",
        AiErrorReason::ContentPolicy { .. } => "provider.content-filter",
        AiErrorReason::Transport { .. } => "provider.transport",
        AiErrorReason::ProviderInternal { .. } => "provider.internal",
        AiErrorReason::InvalidProviderOutput { .. } => "provider.invalid-output",
        AiErrorReason::InvalidRequest { .. } => "provider.invalid-request",
        AiErrorReason::UnsupportedOperation { .. } => "provider.unsupported-operation",
        AiErrorReason::NoRoute { .. } => "provider.no-route",
        AiErrorReason::UnknownProvider { .. } => "provider.unknown",
        AiErrorReason::Timeout { .. } => "provider.timeout",
        AiErrorReason::ProviderConfiguration { .. } => "provider.no-route",
    };
    let fields = error.reason.fields();
    let message = fields
        .body
        .as_deref()
        .and_then(|body| {
            TOKEN_SHARING_MESSAGES
                .iter()
                .find(|(code, _)| body.contains(code))
                .map(|(_, text)| *text)
        })
        .unwrap_or(fields.message.as_str())
        .to_string();
    SessionError {
        error_type: error_type.to_string(),
        message,
        status: fields.http.as_ref().map(|http| http.status),
        response: fields.body.clone().map(|body| oc_schema::common::SessionErrorResponse { body }),
    }
}

/// `contentFilterError` (to-session-error.ts:102-108).
#[must_use]
pub fn content_filter_error(summary: &str, reason: &FinishReasonDetails) -> SessionError {
    let mut parts = Vec::new();
    match &reason.category {
        Some(category) => parts.push(format!("{summary} ({category})")),
        None => parts.push(summary.to_string()),
    }
    if let Some(explanation) = &reason.explanation {
        parts.push(explanation.clone());
    }
    SessionError {
        error_type: "provider.content-filter".into(),
        message: parts.join(": "),
        status: None,
        response: None,
    }
}

// ---------------------------------------------------------------------------
// SessionUsage (usage.ts)
// ---------------------------------------------------------------------------

/// `SessionUsage` — token normalization + cost math (usage.ts).
pub struct SessionUsage;

fn safe(value: Option<f64>) -> f64 {
    value.map_or(0.0, |value| value.max(0.0))
}

fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

impl SessionUsage {
    /// `tokens` — non-overlapping token counters from an LLM usage report.
    #[must_use]
    pub fn tokens(usage: Option<&Usage>) -> TokenUsage {
        match usage {
            None => TokenUsage::default(),
            Some(usage) => TokenUsage {
                input: Num(safe(usage.non_cached_input_tokens)),
                output: Num(safe(Some(usage.visible_output_tokens()))),
                reasoning: Num(safe(usage.reasoning_tokens)),
                cache: oc_schema::common::TokenUsageCache {
                    read: Num(safe(usage.cache_read_input_tokens)),
                    write: Num(safe(usage.cache_write_input_tokens)),
                },
            },
        }
    }

    /// `calculateCost` — context-tier pricing per million tokens.
    #[must_use]
    pub fn calculate_cost(costs: &[model::Cost], usage: &TokenUsage) -> Usd {
        let context = usage.input.0 + usage.cache.read.0 + usage.cache.write.0;
        let tier = costs
            .iter()
            .filter(|cost| match &cost.tier {
                Some(tier) => {
                    tier.tier_type == model::CostTierType::Context && context > tier.size as f64
                }
                None => false,
            })
            // Ref usage.ts:26-29 — stable descending sort by tier size,
            // take the FIRST: on equal sizes the earlier tier in the
            // original order wins (`max_by_key` would take the last).
            .fold(None, |best: Option<&model::Cost>, cost| match best {
                Some(best)
                    if best.tier.as_ref().map_or(0, |t| t.size)
                        >= cost.tier.as_ref().map_or(0, |t| t.size) =>
                {
                    Some(best)
                }
                _ => Some(cost),
            });
        let cost = tier.into_iter().chain(costs.iter().filter(|cost| cost.tier.is_none())).next();
        let Some(cost) = cost else { return Usd(Num(0.0)) };
        let total = (usage.input.0 * finite(cost.input.0 .0)
            + (usage.output.0 + usage.reasoning.0) * finite(cost.output.0 .0)
            + usage.cache.read.0 * finite(cost.cache.read.0 .0)
            + usage.cache.write.0 * finite(cost.cache.write.0 .0))
            / 1_000_000.0;
        Usd(Num(total))
    }

    /// `record` — normalize usage and price it.
    #[must_use]
    pub fn record(usage: Option<&Usage>, costs: &[model::Cost]) -> RecordedUsage {
        let tokens = Self::tokens(usage);
        let cost = Self::calculate_cost(costs, &tokens);
        RecordedUsage { tokens, cost }
    }

    /// `add` — sum two recorded usages.
    #[must_use]
    pub fn add(left: &RecordedUsage, right: &RecordedUsage) -> RecordedUsage {
        RecordedUsage {
            cost: Usd(Num(left.cost.0 .0 + right.cost.0 .0)),
            tokens: TokenUsage {
                input: Num(left.tokens.input.0 + right.tokens.input.0),
                output: Num(left.tokens.output.0 + right.tokens.output.0),
                reasoning: Num(left.tokens.reasoning.0 + right.tokens.reasoning.0),
                cache: oc_schema::common::TokenUsageCache {
                    read: Num(left.tokens.cache.read.0 + right.tokens.cache.read.0),
                    write: Num(left.tokens.cache.write.0 + right.tokens.cache.write.0),
                },
            },
        }
    }
}

/// `SessionUsage.Recorded`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedUsage {
    pub tokens: TokenUsage,
    pub cost: Usd,
}

impl Default for RecordedUsage {
    fn default() -> Self {
        Self { tokens: TokenUsage::default(), cost: Usd(Num(0.0)) }
    }
}

/// `TokenUsage` → JSON (wire shape).
#[must_use]
pub fn tokens_json(tokens: &TokenUsage) -> serde_json::Value {
    // Through `Num` so whole values serialize as integers (the v2 wire
    // emits `11`, not `11.0`).
    let num =
        |value: f64| serde_json::to_value(oc_schema::common::Num::from(value)).unwrap_or_default();
    serde_json::json!({
        "input": num(tokens.input.0),
        "output": num(tokens.output.0),
        "reasoning": num(tokens.reasoning.0),
        "cache": { "read": num(tokens.cache.read.0), "write": num(tokens.cache.write.0) },
    })
}

/// `Usd` → JSON.
#[must_use]
pub fn usd_json(cost: &Usd) -> serde_json::Value {
    serde_json::to_value(cost.0).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Stream-failure classification (step.ts helpers)
// ---------------------------------------------------------------------------

/// `isInterruptedStream` (step.ts:281-285): an incomplete stream or a
/// transport read failure — a step with visible output continues after it.
#[must_use]
pub fn is_interrupted_stream(error: &AiError) -> bool {
    match &error.reason {
        AiErrorReason::InvalidProviderOutput { classification, .. } => {
            classification.as_deref() == Some(oc_ai::schema::CLASSIFICATION_INCOMPLETE_STREAM)
        }
        AiErrorReason::Transport { operation, .. } => *operation == TransportOperation::Read,
        _ => false,
    }
}

/// `isContextOverflowFailure` re-export seam.
#[must_use]
pub fn is_context_overflow_failure(error: &AiError) -> bool {
    oc_ai::provider_error::is_context_overflow_failure(error)
}

/// `isRetryable` re-export seam.
#[must_use]
pub fn is_retryable(error: &AiError) -> bool {
    oc_ai::provider_error::is_retryable(error)
}

/// `RecoverFull` condition inputs (step.ts:176-179).
#[must_use]
pub fn is_retry_full_transport(error: &AiError) -> bool {
    matches!(
        &error.reason,
        AiErrorReason::Transport {
            recovery: Some(TransportRecovery::RetryFull | TransportRecovery::RotateAndRetryFull),
            ..
        }
    )
}

/// The unknown-finish defect error (step.ts:165-171).
#[must_use]
pub fn unknown_finish_error() -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields::new("The provider response ended with an unknown finish reason."),
        classification: Some(oc_ai::schema::CLASSIFICATION_INCOMPLETE_STREAM.into()),
        route: None,
    })
}

/// A JSON object record (metadata/provider state).
pub type Record = BTreeMap<String, serde_json::Value>;
