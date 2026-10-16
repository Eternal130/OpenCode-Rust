//! The LLM event contract (port of `src/schema/events.ts` — THE wire
//! contract, oracle gate 2 finding 17). Kebab-case `type` tags, camelCase
//! fields, all 19 variants. `Usage` fields are optional `f64` (JS number
//! parity).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::ProviderFailureClassification;
use super::ids::ProviderId;
use super::media::Asset;
use super::messages::{ContentPart, Message, MessageRole, ToolOutput, ToolResultValue};
use super::options::ProviderMetadata;

/// Normalized stop reason (port of `LLM.FinishReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "length")]
    Length,
    #[serde(rename = "tool-calls")]
    ToolCalls,
    #[serde(rename = "content-filter")]
    ContentFilter,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "unknown")]
    Unknown,
}

/// Finish reason details: normalized value plus provider-specific context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishReasonDetails {
    pub normalized: FinishReason,
    /// Raw provider stop reason, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// The provider's policy area for a content-filter finish, such as `cyber`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Human-readable provider explanation for a content-filter finish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

impl FinishReasonDetails {
    #[must_use]
    pub fn new(normalized: FinishReason) -> Self {
        Self { normalized, raw: None, category: None, explanation: None }
    }
}

/// Token usage reported by an LLM provider (port of `AI.Usage`).
///
/// **Inclusive totals** (AI SDK / `OpenAI` / `LangChain` convention):
/// `input_tokens` includes cached reads/writes; `output_tokens` includes
/// reasoning; `total_tokens` is the provider total or `input + output`.
///
/// **Non-overlapping breakdown**: `non_cached_input_tokens`,
/// `cache_read_input_tokens`, `cache_write_input_tokens` are each
/// independently meaningful — invariant
/// `non_cached + cache_read + cache_write = input_tokens`.
///
/// Direction semantics by provider (oracle gate 2, finding 17):
/// `OpenAI` Chat/Responses/Gemini report inclusive input; mappers *subtract*
/// down ([`crate::protocols::shared::subtract_to_non_cached`]). Anthropic and
/// Bedrock report the breakdown natively; mappers *sum* up
/// ([`crate::protocols::shared::sum_non_cached_to_inclusive`]). Every
/// derivation clamps with `max(0, …)`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Effective input size of the final message iteration, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub non_cached_input_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<f64>,
    /// Raw provider usage payload keyed by provider name, for audit trails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl Usage {
    /// Non-reasoning output tokens, clamped to zero when a provider reports
    /// `reasoning_tokens > output_tokens` (port of `visibleOutputTokens`).
    #[must_use]
    pub fn visible_output_tokens(&self) -> f64 {
        (self.output_tokens.unwrap_or(0.0) - self.reasoning_tokens.unwrap_or(0.0)).max(0.0)
    }

    #[must_use]
    pub fn with_input_tokens(mut self, input_tokens: f64) -> Self {
        self.input_tokens = Some(input_tokens);
        self
    }

    #[must_use]
    pub fn with_output_tokens(mut self, output_tokens: f64) -> Self {
        self.output_tokens = Some(output_tokens);
        self
    }
}

/// Usage reported by media routes (port of `AI.MediaUsage`, events.ts:98-113).
///
/// Providers bill images, video, speech, and transcription in different
/// units, so each response carries the unit it was actually metered in
/// instead of forcing everything into tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum MediaUsage {
    #[serde(rename = "tokens")]
    Tokens {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Map<String, serde_json::Value>>,
    },
    #[serde(rename = "seconds")]
    Seconds { seconds: f64 },
    #[serde(rename = "characters")]
    Characters { characters: f64 },
    #[serde(rename = "credits")]
    Credits { credits: f64 },
    #[serde(rename = "compute")]
    Compute { seconds: f64 },
}

/// Provider-neutral streaming event (port of the `LLMEvent` tagged union,
/// events.ts:307 — all 19 variants).
///
/// Wire shape is the TS contract verbatim: kebab-case `type` tags,
/// camelCase fields throughout the event tree (`providerMetadata`,
/// `providerExecuted`, nested `Usage` camelCase — oracle gate 3 finding 1).
/// The (B)-lane bun-subprocess transport reuses the TS event guards on this
/// JSON, so field names must match `JSON.stringify(event)` in v2 exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum LLMEvent {
    /// Replacement context marker (a `CompactionPart`).
    #[serde(rename = "compaction")]
    Compaction {
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    #[serde(rename = "step-start")]
    StepStart { index: f64 },
    #[serde(rename = "text-start")]
    TextStart {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "text-delta")]
    TextDelta {
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "text-end")]
    TextEnd {
        id: String,
        /// Authoritative complete value; replaces accumulated deltas when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-start")]
    ReasoningStart {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta {
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-end")]
    ReasoningEnd {
        id: String,
        /// Authoritative complete value; replaces accumulated deltas when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-input-start")]
    ToolInputStart {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-input-delta")]
    ToolInputDelta {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        text: String,
        /// Best-effort parse of all input fragments through this delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<Value>,
    },
    #[serde(rename = "tool-input-end")]
    ToolInputEnd {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A local tool call whose final input could not be decoded.
    #[serde(rename = "tool-input-error")]
    ToolInputError {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        raw: String,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        input: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Client-side execution result (protocols rarely emit).
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        result: ToolResultValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<ToolOutput>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-error")]
    ToolError {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "media")]
    Media {
        media: Asset,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "step-finish")]
    StepFinish {
        index: f64,
        reason: FinishReasonDetails,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "finish")]
    Finish {
        reason: FinishReasonDetails,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "provider-error")]
    ProviderError {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classification: Option<ProviderFailureClassification>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

impl LLMEvent {
    /// The kebab-case `type` tag (matches TS `LLMEvent.guards` keys).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            LLMEvent::Compaction { .. } => "compaction",
            LLMEvent::StepStart { .. } => "step-start",
            LLMEvent::TextStart { .. } => "text-start",
            LLMEvent::TextDelta { .. } => "text-delta",
            LLMEvent::TextEnd { .. } => "text-end",
            LLMEvent::ReasoningStart { .. } => "reasoning-start",
            LLMEvent::ReasoningDelta { .. } => "reasoning-delta",
            LLMEvent::ReasoningEnd { .. } => "reasoning-end",
            LLMEvent::ToolInputStart { .. } => "tool-input-start",
            LLMEvent::ToolInputDelta { .. } => "tool-input-delta",
            LLMEvent::ToolInputEnd { .. } => "tool-input-end",
            LLMEvent::ToolInputError { .. } => "tool-input-error",
            LLMEvent::ToolCall { .. } => "tool-call",
            LLMEvent::ToolResult { .. } => "tool-result",
            LLMEvent::ToolError { .. } => "tool-error",
            LLMEvent::Media { .. } => "media",
            LLMEvent::StepFinish { .. } => "step-finish",
            LLMEvent::Finish { .. } => "finish",
            LLMEvent::ProviderError { .. } => "provider-error",
        }
    }

    #[must_use]
    pub fn is_finish(&self) -> bool {
        matches!(self, LLMEvent::Finish { .. })
    }

    #[must_use]
    pub fn is_provider_error(&self) -> bool {
        matches!(self, LLMEvent::ProviderError { .. })
    }

    /// Terminal for stream bookkeeping: `finish` or `provider-error`.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.is_finish() || self.is_provider_error()
    }

    #[must_use]
    pub fn step_start(index: f64) -> Self {
        LLMEvent::StepStart { index }
    }

    #[must_use]
    pub fn finish(reason: FinishReasonDetails) -> Self {
        LLMEvent::Finish { reason, usage: None, provider_metadata: None }
    }
}

// ---------------------------------------------------------------------------
// Response assembly (port of `LLMResponse` reducer, events.ts:405-742)
// ---------------------------------------------------------------------------

/// Joins deltas per fragment id; an authoritative end value replaces that
/// fragment's accumulated deltas (port of `joinFragments`).
fn join_fragments<'a>(
    events: impl IntoIterator<Item = &'a LLMEvent>,
    is_delta: fn(&LLMEvent) -> Option<(&str, &str)>,
    is_end: fn(&LLMEvent) -> Option<(&str, Option<&str>)>,
) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut parts: BTreeMap<String, String> = BTreeMap::new();
    for event in events {
        if let Some((id, text)) = is_delta(event) {
            if !parts.contains_key(id) {
                order.push(id.to_string());
            }
            *parts.entry(id.to_string()).or_default() += text;
        }
        if let Some((id, Some(text))) = is_end(event) {
            if !parts.contains_key(id) {
                order.push(id.to_string());
            }
            parts.insert(id.to_string(), text.to_string());
        }
    }
    order.into_iter().filter_map(|id| parts.get(&id).cloned()).collect::<String>()
}

/// Concatenated assistant text; each fragment's `text-end` replaces its
/// accumulated deltas when present.
#[must_use]
pub fn response_text(events: &[LLMEvent]) -> String {
    join_fragments(
        events,
        |e| match e {
            LLMEvent::TextDelta { id, text, .. } => Some((id.as_str(), text.as_str())),
            _ => None,
        },
        |e| match e {
            LLMEvent::TextEnd { id, text, .. } => Some((id.as_str(), text.as_deref())),
            _ => None,
        },
    )
}

/// Concatenated reasoning text; same replacement rule per fragment.
#[must_use]
pub fn response_reasoning(events: &[LLMEvent]) -> String {
    join_fragments(
        events,
        |e| match e {
            LLMEvent::ReasoningDelta { id, text, .. } => Some((id.as_str(), text.as_str())),
            _ => None,
        },
        |e| match e {
            LLMEvent::ReasoningEnd { id, text, .. } => Some((id.as_str(), text.as_deref())),
            _ => None,
        },
    )
}

/// Latest usage-bearing event's usage.
#[must_use]
pub fn response_usage(events: &[LLMEvent]) -> Option<Usage> {
    events.iter().rev().find_map(|event| match event {
        LLMEvent::StepFinish { usage, .. } | LLMEvent::Finish { usage, .. } => usage.clone(),
        _ => None,
    })
}

/// Assembled assistant response (port of `LLMResponse`).
#[derive(Debug, Clone, PartialEq)]
pub struct LLMResponse {
    pub message: Message,
    pub events: Vec<LLMEvent>,
    pub usage: Option<Usage>,
    pub finish_reason: FinishReasonDetails,
}

/// Reducer state for assembling one provider attempt (port of
/// `LLMResponse.State`).
#[derive(Debug, Clone)]
pub struct ResponseState {
    pub events: Vec<LLMEvent>,
    pub message: Message,
    pub usage: Option<Usage>,
    pub finish_reason: Option<FinishReasonDetails>,
    text_parts: BTreeMap<String, AssemblyPart>,
    reasoning_parts: BTreeMap<String, AssemblyPart>,
    tool_inputs: BTreeMap<String, ToolInputAssembly>,
}

#[derive(Debug, Clone)]
struct AssemblyPart {
    content_index: usize,
    text: String,
    provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone, Default)]
struct ToolInputAssembly {
    name: String,
    namespace: Option<String>,
    text: String,
    #[allow(dead_code)]
    provider_metadata: Option<ProviderMetadata>,
}

impl Default for ResponseState {
    fn default() -> Self {
        Self::empty()
    }
}

impl ResponseState {
    /// Initial reducer state.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            events: Vec::new(),
            message: Message::new(MessageRole::Assistant, Vec::new()),
            usage: None,
            finish_reason: None,
            text_parts: BTreeMap::new(),
            reasoning_parts: BTreeMap::new(),
            tool_inputs: BTreeMap::new(),
        }
    }

    /// Purely fold one provider-neutral event into the assembly state
    /// (port of `LLMResponse.reduce`).
    #[must_use]
    pub fn reduce(mut self, event: LLMEvent) -> Self {
        self.events.push(event.clone());
        match &event {
            LLMEvent::Finish { reason, usage, .. } => {
                if usage.is_some() {
                    self.usage = usage.clone();
                }
                self.finish_reason = Some(reason.clone());
            }
            LLMEvent::StepFinish { usage, .. } => {
                if usage.is_some() {
                    self.usage = usage.clone();
                }
            }
            LLMEvent::ProviderError { .. } => {
                if self.finish_reason.is_none() {
                    self.finish_reason = Some(FinishReasonDetails::new(FinishReason::Error));
                }
            }
            LLMEvent::Compaction { .. } => self.push_content(event_to_compaction_part(&event)),
            LLMEvent::TextStart { id, provider_metadata, .. } => {
                self.ensure_text(id, provider_metadata.clone());
            }
            LLMEvent::TextDelta { id, text, provider_metadata, .. } => {
                self.ensure_text(id, None);
                if let Some(current) = self.text_parts.get(id).cloned() {
                    let next_text = format!("{}{}", current.text, text);
                    let metadata =
                        provider_metadata.clone().or_else(|| current.provider_metadata.clone());
                    self.replace_content(
                        current.content_index,
                        ContentPart::Text {
                            text: next_text.clone(),
                            cache: None,
                            metadata: None,
                            provider_metadata: metadata.clone(),
                        },
                    );
                    self.text_parts.insert(
                        id.clone(),
                        AssemblyPart { text: next_text, provider_metadata: metadata, ..current },
                    );
                }
            }
            LLMEvent::TextEnd { id, text, provider_metadata, .. } => {
                if let Some(current) = self.text_parts.get(id).cloned() {
                    let next_text = text.clone().unwrap_or_else(|| current.text.clone());
                    let metadata =
                        provider_metadata.clone().or_else(|| current.provider_metadata.clone());
                    self.replace_content(
                        current.content_index,
                        ContentPart::Text {
                            text: next_text.clone(),
                            cache: None,
                            metadata: None,
                            provider_metadata: metadata.clone(),
                        },
                    );
                    self.text_parts.insert(
                        id.clone(),
                        AssemblyPart { text: next_text, provider_metadata: metadata, ..current },
                    );
                }
            }
            LLMEvent::ReasoningStart { id, provider_metadata, .. } => {
                self.ensure_reasoning(id, provider_metadata.clone());
            }
            LLMEvent::ReasoningDelta { id, text, provider_metadata, .. } => {
                self.ensure_reasoning(id, None);
                if let Some(current) = self.reasoning_parts.get(id).cloned() {
                    let next_text = format!("{}{}", current.text, text);
                    let metadata =
                        provider_metadata.clone().or_else(|| current.provider_metadata.clone());
                    self.replace_content(
                        current.content_index,
                        ContentPart::Reasoning {
                            text: next_text.clone(),
                            encrypted: None,
                            cache: None,
                            metadata: None,
                            provider_metadata: metadata.clone(),
                        },
                    );
                    self.reasoning_parts.insert(
                        id.clone(),
                        AssemblyPart { text: next_text, provider_metadata: metadata, ..current },
                    );
                }
            }
            LLMEvent::ReasoningEnd { id, text, provider_metadata, .. } => {
                if let Some(current) = self.reasoning_parts.get(id).cloned() {
                    let next_text = text.clone().unwrap_or_else(|| current.text.clone());
                    let metadata =
                        provider_metadata.clone().or_else(|| current.provider_metadata.clone());
                    self.replace_content(
                        current.content_index,
                        ContentPart::Reasoning {
                            text: next_text.clone(),
                            encrypted: None,
                            cache: None,
                            metadata: None,
                            provider_metadata: metadata.clone(),
                        },
                    );
                    self.reasoning_parts.insert(
                        id.clone(),
                        AssemblyPart { text: next_text, provider_metadata: metadata, ..current },
                    );
                }
            }
            LLMEvent::ToolInputStart { id, name, namespace, provider_metadata, .. } => {
                self.tool_inputs.insert(
                    id.clone(),
                    ToolInputAssembly {
                        name: name.clone(),
                        namespace: namespace.clone(),
                        text: String::new(),
                        provider_metadata: provider_metadata.clone(),
                    },
                );
            }
            LLMEvent::ToolInputDelta { id, name, namespace, text, .. } => {
                let entry =
                    self.tool_inputs.entry(id.clone()).or_insert_with(|| ToolInputAssembly {
                        name: name.clone(),
                        namespace: namespace.clone(),
                        text: String::new(),
                        provider_metadata: None,
                    });
                entry.text += text;
            }
            LLMEvent::ToolInputEnd { id, name, namespace, provider_metadata, .. } => {
                let entry =
                    self.tool_inputs.entry(id.clone()).or_insert_with(|| ToolInputAssembly {
                        name: name.clone(),
                        namespace: namespace.clone(),
                        text: String::new(),
                        provider_metadata: None,
                    });
                entry.name = name.clone();
                entry.namespace = namespace.clone();
                if provider_metadata.is_some() {
                    entry.provider_metadata = provider_metadata.clone();
                }
            }
            LLMEvent::ToolInputError { id, .. } => {
                self.tool_inputs.remove(id);
            }
            LLMEvent::ToolCall {
                id,
                name,
                namespace,
                input,
                provider_executed,
                provider_metadata,
                ..
            } => {
                self.tool_inputs.remove(id);
                self.push_content(ContentPart::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    namespace: namespace.clone(),
                    input: input.clone(),
                    provider_executed: *provider_executed,
                    cache: None,
                    metadata: None,
                    provider_metadata: provider_metadata.clone(),
                });
            }
            LLMEvent::ToolResult {
                id,
                name,
                namespace,
                result,
                output: _,
                provider_executed,
                provider_metadata,
                ..
            } => {
                self.push_content(ContentPart::ToolResult {
                    id: id.clone(),
                    name: name.clone(),
                    namespace: namespace.clone(),
                    result: result.clone(),
                    provider_executed: *provider_executed,
                    cache: None,
                    metadata: None,
                    provider_metadata: provider_metadata.clone(),
                });
            }
            LLMEvent::Media { media, provider_metadata, .. } => {
                self.push_content(ContentPart::Media {
                    media: media.clone(),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: provider_metadata.clone(),
                });
            }
            _ => {}
        }
        self
    }

    /// A completed response only after a terminal finish or provider error
    /// (port of `LLMResponse.complete`).
    #[must_use]
    pub fn complete(&self) -> Option<LLMResponse> {
        self.finish_reason.as_ref().map(|reason| LLMResponse {
            message: self.message.clone(),
            events: self.events.clone(),
            usage: self.usage.clone(),
            finish_reason: reason.clone(),
        })
    }

    fn push_content(&mut self, part: ContentPart) {
        self.message.content.push(part);
    }

    fn replace_content(&mut self, index: usize, part: ContentPart) {
        if index < self.message.content.len() {
            self.message.content[index] = part;
        }
    }

    fn ensure_text(&mut self, id: &str, provider_metadata: Option<ProviderMetadata>) {
        if self.text_parts.contains_key(id) {
            return;
        }
        let content_index = self.message.content.len();
        self.push_content(ContentPart::Text {
            text: String::new(),
            cache: None,
            metadata: None,
            provider_metadata: provider_metadata.clone(),
        });
        self.text_parts.insert(
            id.to_string(),
            AssemblyPart { content_index, text: String::new(), provider_metadata },
        );
    }

    fn ensure_reasoning(&mut self, id: &str, provider_metadata: Option<ProviderMetadata>) {
        if self.reasoning_parts.contains_key(id) {
            return;
        }
        let content_index = self.message.content.len();
        self.push_content(ContentPart::Reasoning {
            text: String::new(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: provider_metadata.clone(),
        });
        self.reasoning_parts.insert(
            id.to_string(),
            AssemblyPart { content_index, text: String::new(), provider_metadata },
        );
    }
}

fn event_to_compaction_part(event: &LLMEvent) -> ContentPart {
    match event {
        LLMEvent::Compaction { provider, id, encrypted, text } => ContentPart::Compaction {
            provider: provider.clone(),
            id: id.clone(),
            encrypted: encrypted.clone(),
            text: text.clone(),
        },
        _ => unreachable!("event_to_compaction_part called on non-compaction event"),
    }
}

impl LLMResponse {
    /// Concatenated assistant text.
    #[must_use]
    pub fn text(&self) -> String {
        response_text(&self.events)
    }

    /// Concatenated reasoning text.
    #[must_use]
    pub fn reasoning(&self) -> String {
        response_reasoning(&self.events)
    }

    /// Completed tool calls emitted by the provider.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<&LLMEvent> {
        self.events.iter().filter(|e| matches!(e, LLMEvent::ToolCall { .. })).collect()
    }

    /// Fold a collected event list into a completed response, if terminal
    /// (port of `LLMResponse.fromEvents`).
    pub fn from_events(events: Vec<LLMEvent>) -> Option<Self> {
        events.into_iter().fold(ResponseState::empty(), ResponseState::reduce).complete()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(id: &str, text: &str) -> LLMEvent {
        LLMEvent::TextDelta { id: id.into(), text: text.into(), provider_metadata: None }
    }

    #[test]
    fn event_tags_are_kebab_case_and_exhaustive() {
        let expected = [
            "compaction",
            "step-start",
            "text-start",
            "text-delta",
            "text-end",
            "reasoning-start",
            "reasoning-delta",
            "reasoning-end",
            "tool-input-start",
            "tool-input-delta",
            "tool-input-end",
            "tool-input-error",
            "tool-call",
            "tool-result",
            "tool-error",
            "media",
            "step-finish",
            "finish",
            "provider-error",
        ];
        // one instance per variant
        let samples = vec![
            LLMEvent::Compaction {
                provider: ProviderId::new("p"),
                id: None,
                encrypted: Some("e".into()),
                text: None,
            },
            LLMEvent::StepStart { index: 0.0 },
            LLMEvent::TextStart { id: "t1".into(), provider_metadata: None },
            LLMEvent::TextDelta { id: "t1".into(), text: "hi".into(), provider_metadata: None },
            LLMEvent::TextEnd { id: "t1".into(), text: None, provider_metadata: None },
            LLMEvent::ReasoningStart { id: "r1".into(), provider_metadata: None },
            LLMEvent::ReasoningDelta {
                id: "r1".into(),
                text: "hm".into(),
                provider_metadata: None,
            },
            LLMEvent::ReasoningEnd { id: "r1".into(), text: None, provider_metadata: None },
            LLMEvent::ToolInputStart {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                provider_executed: None,
                provider_metadata: None,
            },
            LLMEvent::ToolInputDelta {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                text: "{}".into(),
                input: None,
            },
            LLMEvent::ToolInputEnd {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                provider_metadata: None,
            },
            LLMEvent::ToolInputError {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                raw: "{".into(),
            },
            LLMEvent::ToolCall {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                input: serde_json::json!({}),
                provider_executed: None,
                provider_metadata: None,
            },
            LLMEvent::ToolResult {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                result: ToolResultValue::Json { value: serde_json::json!(1) },
                output: None,
                provider_executed: None,
                provider_metadata: None,
            },
            LLMEvent::ToolError {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                message: "boom".into(),
                error: None,
                provider_metadata: None,
            },
            LLMEvent::Media {
                media: crate::schema::media::base64("AQID", "image/png"),
                provider_metadata: None,
            },
            LLMEvent::StepFinish {
                index: 0.0,
                reason: FinishReasonDetails::new(FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
            LLMEvent::Finish {
                reason: FinishReasonDetails::new(FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
            LLMEvent::ProviderError {
                message: "nope".into(),
                classification: None,
                provider_metadata: None,
            },
        ];
        assert_eq!(samples.len(), 19, "exactly 19 variants");
        for (event, tag) in samples.iter().zip(expected) {
            assert_eq!(event.kind(), tag);
            let json = serde_json::to_value(event).unwrap();
            assert_eq!(json["type"], tag, "wire tag mismatch for {tag}");
            let back: LLMEvent = serde_json::from_value(json).unwrap();
            assert_eq!(back.kind(), tag, "round-trip mismatch for {tag}");
        }
    }

    #[test]
    fn unknown_event_type_fails_to_decode() {
        assert!(serde_json::from_value::<LLMEvent>(serde_json::json!({"type": "bogus"})).is_err());
    }

    /// Oracle gate 3 finding 1: the serialized event tree is the TS wire
    /// shape verbatim — camelCase fields at every level, `snake_case` keys
    /// (`provider_metadata`, `provider_executed`) must not leak.
    #[test]
    fn event_tree_serializes_camel_case_like_the_ts_contract() {
        let event = LLMEvent::ToolCall {
            id: "c1".into(),
            name: "lookup".into(),
            namespace: Some("ns".into()),
            input: serde_json::json!({"q": 1}),
            provider_executed: Some(true),
            provider_metadata: Some(ProviderMetadata::from([(
                "openai".to_string(),
                BTreeMap::from([("itemId".to_string(), serde_json::json!("i_1"))]),
            )])),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "tool-call");
        assert_eq!(json["providerExecuted"], true);
        assert_eq!(json["providerMetadata"]["openai"]["itemId"], "i_1");
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("provider_metadata"), "snake_case leaked: {encoded}");
        assert!(!encoded.contains("provider_executed"), "snake_case leaked: {encoded}");
        // decodes back from the TS-shaped JSON
        let back: LLMEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);

        let finish = LLMEvent::Finish {
            reason: FinishReasonDetails::new(FinishReason::Stop),
            usage: Some(Usage {
                input_tokens: Some(10.0),
                cache_read_input_tokens: Some(4.0),
                ..Default::default()
            }),
            provider_metadata: None,
        };
        let json = serde_json::to_value(&finish).unwrap();
        assert_eq!(json["usage"]["inputTokens"], 10.0);
        assert_eq!(json["usage"]["cacheReadInputTokens"], 4.0);
        assert!(json.get("providerMetadata").is_none(), "None fields are dropped");
        let back: LLMEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, finish);
    }

    #[test]
    fn visible_output_tokens_clamps_reasoning_above_output_to_zero() {
        assert_eq!(
            Usage { output_tokens: Some(10.0), reasoning_tokens: Some(4.0), ..Default::default() }
                .visible_output_tokens(),
            6.0
        );
        assert_eq!(
            Usage { output_tokens: Some(10.0), ..Default::default() }.visible_output_tokens(),
            10.0
        );
        assert_eq!(
            Usage { output_tokens: Some(4.0), reasoning_tokens: Some(10.0), ..Default::default() }
                .visible_output_tokens(),
            0.0
        );
        assert_eq!(Usage::default().visible_output_tokens(), 0.0);
    }

    #[test]
    fn usage_round_trips_provider_metadata() {
        let usage = Usage {
            input_tokens: Some(100.0),
            non_cached_input_tokens: Some(40.0),
            cache_read_input_tokens: Some(60.0),
            provider_metadata: Some(ProviderMetadata::from([(
                "anthropic".to_string(),
                BTreeMap::from([("raw".to_string(), serde_json::json!({"a": 1}))]),
            )])),
            ..Default::default()
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["inputTokens"], 100.0);
        assert_eq!(json["cacheReadInputTokens"], 60.0);
        assert_eq!(json["providerMetadata"]["anthropic"]["raw"]["a"], 1);
        let back: Usage = serde_json::from_value(json).unwrap();
        assert_eq!(back, usage);
    }

    #[test]
    fn authoritative_end_values_replace_accumulated_deltas() {
        let events = vec![
            delta("t1", "hello "),
            LLMEvent::TextEnd {
                id: "t1".into(),
                text: Some("WORLD".into()),
                provider_metadata: None,
            },
            delta("t2", "second"),
            LLMEvent::TextEnd { id: "t2".into(), text: None, provider_metadata: None },
        ];
        assert_eq!(response_text(&events), "WORLDsecond");
    }

    #[test]
    fn reasoning_end_replaces_only_its_own_fragment() {
        let events = vec![
            LLMEvent::ReasoningDelta {
                id: "r1".into(),
                text: "think ".into(),
                provider_metadata: None,
            },
            LLMEvent::ReasoningDelta {
                id: "r2".into(),
                text: "other ".into(),
                provider_metadata: None,
            },
            LLMEvent::ReasoningEnd {
                id: "r1".into(),
                text: Some("THINK".into()),
                provider_metadata: None,
            },
            LLMEvent::ReasoningEnd { id: "r2".into(), text: None, provider_metadata: None },
        ];
        assert_eq!(response_reasoning(&events), "THINKother ");
    }

    #[test]
    fn response_assembles_interleaved_reasoning_and_text() {
        let events = vec![
            LLMEvent::StepStart { index: 0.0 },
            LLMEvent::ReasoningStart { id: "r1".into(), provider_metadata: None },
            LLMEvent::ReasoningDelta {
                id: "r1".into(),
                text: "thinking".into(),
                provider_metadata: None,
            },
            LLMEvent::ReasoningEnd { id: "r1".into(), text: None, provider_metadata: None },
            LLMEvent::TextStart { id: "t1".into(), provider_metadata: None },
            LLMEvent::TextDelta { id: "t1".into(), text: "answer".into(), provider_metadata: None },
            LLMEvent::TextEnd { id: "t1".into(), text: None, provider_metadata: None },
            LLMEvent::StepFinish {
                index: 0.0,
                reason: FinishReasonDetails::new(FinishReason::Stop),
                usage: Some(Usage { input_tokens: Some(5.0), ..Default::default() }),
                provider_metadata: None,
            },
            LLMEvent::Finish {
                reason: FinishReasonDetails::new(FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
        ];
        let response = LLMResponse::from_events(events).expect("terminal finish present");
        assert_eq!(response.text(), "answer");
        assert_eq!(response.reasoning(), "thinking");
        assert_eq!(response.usage.unwrap().input_tokens, Some(5.0));
        // message content preserves emission order: reasoning then text
        assert_eq!(response.message.content[0].kind(), "reasoning");
        assert_eq!(response.message.content[1].kind(), "text");
        assert!(matches!(response.message.role, MessageRole::Assistant));
    }

    #[test]
    fn response_does_not_complete_without_terminal_event() {
        let events = vec![delta("t1", "partial")];
        let state = events.into_iter().fold(ResponseState::empty(), ResponseState::reduce);
        assert!(state.complete().is_none());
    }

    #[test]
    fn provider_error_completes_with_error_reason_but_keeps_partial_content() {
        let events = vec![
            delta("t1", "partial"),
            LLMEvent::ProviderError {
                message: "boom".into(),
                classification: None,
                provider_metadata: None,
            },
        ];
        let response = LLMResponse::from_events(events).expect("provider error is terminal");
        assert_eq!(response.finish_reason.normalized, FinishReason::Error);
        assert_eq!(response.text(), "partial");
    }

    #[test]
    fn tool_calls_assemble_only_after_the_completed_tool_call_event() {
        let events = vec![
            LLMEvent::ToolInputStart {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                provider_executed: None,
                provider_metadata: None,
            },
            LLMEvent::ToolInputDelta {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                text: "{\"q\":".into(),
                input: None,
            },
            LLMEvent::ToolCall {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                input: serde_json::json!({"q": 1}),
                provider_executed: None,
                provider_metadata: None,
            },
            LLMEvent::Finish {
                reason: FinishReasonDetails::new(FinishReason::ToolCalls),
                usage: None,
                provider_metadata: None,
            },
        ];
        let response = LLMResponse::from_events(events).unwrap();
        assert_eq!(response.tool_calls().len(), 1);
        assert_eq!(response.message.content.len(), 1);
        assert_eq!(response.message.content[0].kind(), "tool-call");
        assert_eq!(response.finish_reason.normalized, FinishReason::ToolCalls);
    }

    #[test]
    fn finish_reason_details_carry_raw_and_category() {
        let json = serde_json::json!({
            "normalized": "content-filter",
            "raw": "SAFETY",
            "category": "cyber",
            "explanation": "flagged"
        });
        let details: FinishReasonDetails = serde_json::from_value(json).unwrap();
        assert_eq!(details.normalized, FinishReason::ContentFilter);
        assert_eq!(details.raw.as_deref(), Some("SAFETY"));
        assert_eq!(details.category.as_deref(), Some("cyber"));
        let encoded = serde_json::to_value(&details).unwrap();
        assert_eq!(encoded["normalized"], "content-filter");
    }
}
