//! OpenAI Chat Completions protocol (port of `src/protocols/openai-chat.ts`,
//! 1,316 LOC + the six provider body extensions).
//!
//! - **Request lowering** ([`from_request`]): messages (system/developer
//!   wrappers, media, tool history bridging), tools with strict function
//!   schemas, `stream_options:{include_usage:true}`, `reasoning_effort`,
//!   `prompt_cache_key` (64-char unicode clamp), `max_tokens` vs
//!   `max_completion_tokens` per [`MaxTokensFieldCompatibility`], and the
//!   remaining generation knobs.
//! - **Stream machine** ([`OpenAiChat`] implementing
//!   [`Protocol`]): `sse_with_done` frames carrying `choices[0].delta`
//!   (text / refusal / scalar reasoning via the compatibility reasoning field
//!   / `reasoning_details` arrays / index-keyed `tool_calls` accumulation
//!   through [`tool_stream`]), usage in the final chunk with the **subtract**
//!   direction, `[DONE]` terminal, and `on_halt` finishing the response
//!   (synthesizing a finish when `require_finish_reason === false`).
//! - **Provider extensions** ([`OpenAiChatExt`]): deepseek / groq / openrouter
//!   / venice / zai / alibaba differ from the base protocol **only** in body
//!   construction (plus Venice's stream wrapper, which filters an encrypted
//!   reasoning marker and replays thought signatures).
//!
//! Route factories: [`openai_chat_route`] plus one preset per extension
//! provider ([`deepseek_chat_route`], [`groq_chat_route`],
//! [`openrouter_chat_route`], [`venice_chat_route`], [`zai_chat_route`],
//! [`alibaba_chat_route`]) with the model compatibility each provider
//! configures in TS.

// Lint posture for this module (same policy as the crate-level allow list
// in src/lib.rs, with lane-specific reasons):
#![allow(
    // Wire field names (`reasoning_content`, `prompt_cache_key`, …) appear in
    // prose docs as identifiers; backticking every one adds noise, not clarity.
    clippy::doc_markdown,
    // ParserState mirrors the TS state machine's flag set 1:1.
    clippy::struct_excessive_bools,
    // Route/compatibility factories mirror the TS immutable-builder exports.
    clippy::must_use_candidate,
    // `Box<dyn Any>::downcast` cannot name its error type in a match arm.
    clippy::match_wild_err_arm,
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{
    self, event_error, fit_thinking_budget, flatten_tools, prompt_cache_key, subtract_tokens,
    sum_tokens, total_tokens,
};
use crate::protocols::utils::{lifecycle, tool_stream};
use crate::provider_error::{classify_provider_failure, ProviderFailure};
use crate::route::auth::Definition as AuthDefinition;
use crate::route::client::{Route, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::schema::options::MaxTokensFieldCompatibility;
use crate::schema::{
    AiError, AiErrorReason, CacheHint, ContentPart, FinishReason, FinishReasonDetails, LLMEvent,
    LLMRequest, LanguageModelCompatibility, Message, MessageRole, ProviderMetadata,
    ProviderOptions, ReasonFields, ToolChoice, ToolDefinition, Usage,
};

pub const ADAPTER: &str = "openai-chat";
const DONE: &str = "[DONE]";
const RESERVED_REASONING_FIELDS: [&str; 4] = ["role", "content", "refusal", "tool_calls"];
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const PATH: &str = "/chat/completions";
const MISSING_TOOL_MESSAGE: &str = "OpenAI Chat tool call delta is missing id or name";
const VENICE_MARKER: &str = "__ENCRYPTED_REASONING__";

// =============================================================================
// Provider extension seam
// =============================================================================

/// Overlay applied to a lowered assistant message (source message parts are
/// available; Venice replays thought signatures this way).
pub type AssistantOverlay = fn(&Message, &mut Map<String, Value>);

/// Overlay applied to the whole provider body after base lowering (groq /
/// openrouter / venice / zai / alibaba body differences).
pub type BodyOverlay = fn(&LLMRequest, &mut Map<String, Value>);

/// The per-provider body seams of the openai-chat family (TS providers wrap
/// `OpenAIChat.fromRequest` with `{...body, <fields>}` — this is that spread).
#[derive(Clone)]
pub struct OpenAiChatExt {
    /// Protocol id (`"openai-chat"`, `"groq-chat"`, …).
    pub id: &'static str,
    /// Lower inline cache hints as `cache_control` breakpoints (shared
    /// 4-breakpoint budget, port of `utils/cache.ts` `cacheControl()`).
    pub cache_control: bool,
    /// Venice's event schema also accepts a bare string `error`.
    pub allow_string_error: bool,
    pub assistant: Option<AssistantOverlay>,
    pub body: Option<BodyOverlay>,
}

impl OpenAiChatExt {
    #[must_use]
    pub fn base() -> Self {
        Self {
            id: ADAPTER,
            cache_control: false,
            allow_string_error: false,
            assistant: None,
            body: None,
        }
    }

    #[must_use]
    pub fn with_id(mut self, id: &'static str) -> Self {
        self.id = id;
        self
    }
}

// =============================================================================
// Stream event decoding (port of the OpenAIChat* event schemas)
// =============================================================================

/// One decoded `data:` payload (port of `OpenAIChatEvent` + Venice deltas).
#[derive(Debug, Clone)]
struct ChatEvent {
    /// Original frame JSON (error bodies, provider usage metadata, Venice
    /// extras).
    raw: Value,
    choices: Vec<ChatChoice>,
    usage: Option<Value>,
    error: Option<ChatEventError>,
}

#[derive(Debug, Clone)]
enum ChatEventError {
    Structured { code: Option<Value>, message: String },
    Plain(String),
}

#[derive(Debug, Clone, Default)]
struct ChatChoice {
    delta: Option<ChatDelta>,
    finish_reason: Option<String>,
    native_finish_reason: Option<String>,
    usage: Option<Value>,
}

#[derive(Debug, Clone, Default)]
struct ChatDelta {
    content: Option<String>,
    refusal: Option<String>,
    reasoning_content: Option<String>,
    reasoning: Option<String>,
    reasoning_text: Option<String>,
    reasoning_details: Option<Value>,
    tool_calls: Vec<ChatToolCallDelta>,
    thought_signature: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ChatToolCallDelta {
    index: Option<f64>,
    id: Option<String>,
    function_name: Option<String>,
    function_arguments: Option<String>,
    extra_content: Option<Value>,
    thought_signature: Option<String>,
}

type DecodeResult<T> = Result<T, ()>;

fn opt_string(value: Option<&Value>) -> DecodeResult<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(()),
    }
}

fn opt_number(value: Option<&Value>) -> DecodeResult<Option<f64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_f64().map(Some).ok_or(()),
        Some(_) => Err(()),
    }
}

/// `Schema.Struct` decode keeps declared fields and drops the rest; malformed
/// declared fields fail the whole event (strict, like the TS schema).
fn decode_usage_value(value: &Value) -> DecodeResult<Value> {
    let object = value.as_object().ok_or(())?;
    for key in ["prompt_tokens", "completion_tokens", "total_tokens", "cached_tokens"] {
        opt_number(object.get(key))?;
    }
    for key in ["prompt_cache_hit_tokens", "cache_read_input_tokens", "cache_created_input_tokens"]
    {
        opt_number(object.get(key))?;
    }
    if let Some(details) = object.get("prompt_tokens_details").filter(|v| !v.is_null()) {
        let details = details.as_object().ok_or(())?;
        for key in ["cached_tokens", "cache_write_tokens", "cache_creation_input_tokens"] {
            opt_number(details.get(key))?;
        }
    }
    if let Some(details) = object.get("completion_tokens_details").filter(|v| !v.is_null()) {
        let details = details.as_object().ok_or(())?;
        for key in ["reasoning_tokens", "accepted_prediction_tokens", "rejected_prediction_tokens"]
        {
            opt_number(details.get(key))?;
        }
    }
    Ok(value.clone())
}

fn decode_delta(value: &Value) -> DecodeResult<ChatDelta> {
    let object = value.as_object().ok_or(())?;
    let mut delta = ChatDelta {
        content: opt_string(object.get("content"))?,
        refusal: opt_string(object.get("refusal"))?,
        reasoning_content: opt_string(object.get("reasoning_content"))?,
        reasoning: opt_string(object.get("reasoning"))?,
        reasoning_text: opt_string(object.get("reasoning_text"))?,
        reasoning_details: match object.get("reasoning_details") {
            None | Some(Value::Null) => None,
            Some(other) => Some(other.clone()),
        },
        thought_signature: opt_string(object.get("thought_signature"))?,
        tool_calls: Vec::new(),
    };
    match object.get("tool_calls") {
        None | Some(Value::Null) => {}
        Some(Value::Array(calls)) => {
            for call in calls {
                delta.tool_calls.push(decode_tool_call_delta(call)?);
            }
        }
        Some(_) => return Err(()),
    }
    Ok(delta)
}

fn decode_tool_call_delta(value: &Value) -> DecodeResult<ChatToolCallDelta> {
    let object = value.as_object().ok_or(())?;
    let function = match object.get("function") {
        None | Some(Value::Null) => None,
        Some(function) => {
            let function = function.as_object().ok_or(())?;
            Some((opt_string(function.get("name"))?, opt_string(function.get("arguments"))?))
        }
    };
    Ok(ChatToolCallDelta {
        index: opt_number(object.get("index"))?,
        id: opt_string(object.get("id"))?,
        function_name: function.as_ref().and_then(|(name, _)| name.clone()),
        function_arguments: function.as_ref().and_then(|(_, args)| args.clone()),
        extra_content: match object.get("extra_content") {
            None | Some(Value::Null) => None,
            Some(other) => Some(other.clone()),
        },
        thought_signature: opt_string(object.get("thought_signature"))?,
    })
}

fn decode_error(value: &Value, allow_string: bool) -> DecodeResult<ChatEventError> {
    if let Value::String(text) = value {
        return if allow_string { Ok(ChatEventError::Plain(text.clone())) } else { Err(()) };
    }
    let object = value.as_object().ok_or(())?;
    let code = match object.get("code") {
        None | Some(Value::Null) => None,
        Some(value @ (Value::String(_) | Value::Number(_))) => Some(value.clone()),
        Some(_) => return Err(()),
    };
    let message = object.get("message").and_then(Value::as_str).ok_or(())?.to_string();
    Ok(ChatEventError::Structured { code, message })
}

fn decode_event(raw: Value, allow_string_error: bool) -> DecodeResult<ChatEvent> {
    let mut event = ChatEvent { choices: Vec::new(), usage: None, error: None, raw: raw.clone() };
    let object = raw.as_object().ok_or(())?;
    match object.get("choices") {
        None | Some(Value::Null) => {}
        Some(Value::Array(choices)) => {
            for choice in choices {
                let choice_object = choice.as_object().ok_or(())?;
                event.choices.push(ChatChoice {
                    delta: match choice_object.get("delta") {
                        None | Some(Value::Null) => None,
                        Some(delta) => Some(decode_delta(delta)?),
                    },
                    finish_reason: opt_string(choice_object.get("finish_reason"))?,
                    native_finish_reason: opt_string(choice_object.get("native_finish_reason"))?,
                    usage: match choice_object.get("usage") {
                        None | Some(Value::Null) => None,
                        Some(usage) => Some(decode_usage_value(usage)?),
                    },
                });
            }
        }
        Some(_) => return Err(()),
    }
    event.usage = match object.get("usage") {
        None | Some(Value::Null) => None,
        Some(usage) => Some(decode_usage_value(usage)?),
    };
    event.error = match object.get("error") {
        None | Some(Value::Null) => None,
        Some(error) => Some(decode_error(error, allow_string_error)?),
    };
    Ok(event)
}

fn encode_event(event: &ChatEvent) -> String {
    serde_json::to_string(&event.raw).unwrap_or_default()
}

// =============================================================================
// Parser state (port of `ParserState`)
// =============================================================================

#[derive(Debug, Clone, Default)]
struct PendingToolDelta {
    id: Option<String>,
    name: Option<String>,
    input: String,
    extra_content: Option<Value>,
}

#[derive(Debug, Clone)]
struct ParserState {
    provider_metadata_key: String,
    tools: tool_stream::State,
    /// Mirror of `tools` (index → call id): the shared ToolStream state keeps
    /// its keys private, and the index identity matters for delta matching.
    tool_indices: BTreeMap<i64, String>,
    pending_tools: BTreeMap<i64, PendingToolDelta>,
    tool_call_events: Vec<LLMEvent>,
    usage: Option<Usage>,
    finish_reason: Option<FinishReasonDetails>,
    lifecycle: lifecycle::State,
    reasoning_field: Option<String>,
    reasoning_text_observed: bool,
    reasoning_details: Vec<Value>,
    reasoning_details_observed: bool,
    reasoning_emitted: bool,
    latest_tool_index: Option<i64>,
    next_tool_index: i64,
    require_finish_reason: bool,
    /// Mirrors `lifecycle.reasoning.has("reasoning-0")` (the shared lifecycle
    /// keeps its block sets private).
    reasoning_started: bool,
    /// `{provider}/{route}` label for stream-event diagnostics.
    provider_label: String,
}

impl ParserState {
    fn initial(request: &LLMRequest) -> Self {
        let compatibility = request.model.compatibility.as_ref();
        Self {
            provider_metadata_key: request
                .model
                .route
                .provider_metadata_key
                .clone()
                .unwrap_or_else(|| request.model.provider.0.clone()),
            tools: tool_stream::State::new(),
            tool_indices: BTreeMap::new(),
            pending_tools: BTreeMap::new(),
            tool_call_events: Vec::new(),
            usage: None,
            finish_reason: None,
            lifecycle: lifecycle::State::initial(),
            reasoning_field: compatibility.and_then(|c| c.reasoning_field.clone()),
            reasoning_text_observed: false,
            reasoning_details: Vec::new(),
            reasoning_details_observed: false,
            reasoning_emitted: false,
            latest_tool_index: None,
            next_tool_index: 0,
            require_finish_reason: compatibility
                .and_then(|c| c.require_finish_reason)
                .unwrap_or(true),
            reasoning_started: false,
            provider_label: format!("{}/{}", request.model.provider.0, request.model.route.id),
        }
    }
}

// =============================================================================
// Usage / finish mapping
// =============================================================================

/// OpenAI Chat reports inclusive `prompt_tokens` with cached-read /
/// cache-write subsets: pass the totals through and **subtract down** to the
/// non-cached breakdown (oracle gate 2, finding 17). Providers differ on the
/// cache-hit location: `prompt_tokens_details.cached_tokens` (OpenAI),
/// `prompt_cache_hit_tokens` (DeepSeek), top-level `cached_tokens` (Zai),
/// `cache_read_input_tokens` (DigitalOcean).
fn map_usage(usage: &Value, provider_metadata_key: &str) -> Option<Usage> {
    if !usage.is_object() {
        return None;
    }
    let number = |pointer: &str| usage.pointer(pointer).and_then(Value::as_f64);
    let input = number("/prompt_tokens");
    let output = number("/completion_tokens");
    let cached = number("/prompt_tokens_details/cached_tokens")
        .or(number("/prompt_cache_hit_tokens"))
        .or(number("/cached_tokens"))
        .or(number("/cache_read_input_tokens"));
    let cache_write = number("/prompt_tokens_details/cache_write_tokens")
        .or(number("/cache_created_input_tokens"));
    let reasoning = number("/completion_tokens_details/reasoning_tokens");
    let non_cached = subtract_tokens(input, sum_tokens(&[cached, cache_write]));
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cached,
        cache_write_input_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens: total_tokens(input, output, number("/total_tokens")),
        context_tokens: None,
        provider_metadata: Some(
            [(provider_metadata_key.to_string(), json_value_map(usage.clone()))]
                .into_iter()
                .collect(),
        ),
    })
}

fn json_value_map(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(object) => object.into_iter().collect(),
        Value::Null => BTreeMap::new(),
        other => BTreeMap::from([("value".to_string(), other)]),
    }
}

fn body_fields(message: &str, body: String) -> ReasonFields {
    ReasonFields { message: message.to_string(), body: Some(body), ..Default::default() }
}

fn map_finish_reason(event: &ChatEvent, reason: &str) -> Result<FinishReason, AiError> {
    let body = encode_event(event);
    match reason {
        "error" => Err(AiError::new(AiErrorReason::UnknownProvider {
            fields: body_fields("Provider reported an error (finish_reason: error)", body),
        })),
        "network_error" => Err(AiError::new(AiErrorReason::ProviderInternal {
            fields: body_fields(
                "Provider reported a network error (finish_reason: network_error)",
                body,
            ),
            retry_after_ms: None,
        })),
        "stop" | "end" => Ok(FinishReason::Stop),
        "length" => Ok(FinishReason::Length),
        "content_filter" => Ok(FinishReason::ContentFilter),
        "function_call" | "tool_calls" => Ok(FinishReason::ToolCalls),
        other => Err(AiError::new(AiErrorReason::UnknownProvider {
            fields: body_fields(&format!("Provider finish_reason: {other}"), body),
        })),
    }
}

// =============================================================================
// Reasoning details (OpenRouter + Kimi dialects)
// =============================================================================

fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(bool) => *bool,
        Value::String(text) => !text.is_empty(),
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn defined_non_null(value: Option<&Value>) -> bool {
    value.is_some_and(|v| !v.is_null())
}

/// Port of `conflictingDetailValue`: both sides defined and non-null, unequal.
fn conflicting_detail_value(previous: Option<&Value>, current: Option<&Value>) -> bool {
    defined_non_null(previous) && defined_non_null(current) && previous != current
}

fn conflicting_reasoning_details(previous: &Value, current: &Value) -> bool {
    conflicting_detail_value(previous.get("id"), current.get("id"))
        || conflicting_detail_value(previous.get("index"), current.get("index"))
        || conflicting_detail_value(previous.get("format"), current.get("format"))
        || (previous.get("signature").is_some_and(js_truthy)
            && current.get("signature").is_some_and(js_truthy)
            && previous.get("signature") != current.get("signature"))
}

fn detail_type(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

/// Port of `mergeDetailValue`: truthy-wins, else the first defined value.
fn merge_detail_value(previous: Option<&Value>, current: Option<&Value>) -> Option<Value> {
    if previous.is_some_and(js_truthy) {
        return previous.cloned();
    }
    if current.is_some_and(js_truthy) {
        return current.cloned();
    }
    previous.or(current).cloned()
}

fn merge_detail_identity(previous: &Value, current: &Value, merged: &mut Map<String, Value>) {
    let signature = merge_detail_value(previous.get("signature"), current.get("signature"));
    let format = merge_detail_value(previous.get("format"), current.get("format"));
    if let Some(signature) = signature {
        merged.insert("signature".into(), signature);
    }
    if let Some(format) = format {
        merged.insert("format".into(), format);
    }
}

/// Consecutive same-kind deltas accumulate into one entry; encrypted entries
/// never merge (port of `mergeReasoningDetails`). Returns `None` when the
/// details stay separate.
fn merge_reasoning_details(previous: &Value, detail: &Value) -> Option<Value> {
    if conflicting_reasoning_details(previous, detail) {
        return None;
    }
    let (previous_type, detail_type) = (detail_type(previous)?, detail_type(detail)?);
    if previous_type != detail_type {
        return None;
    }
    match previous_type {
        "reasoning.text" => {
            let mut merged = previous.as_object()?.clone();
            merged.extend(detail.as_object()?.clone());
            let text = format!(
                "{}{}",
                previous.get("text").and_then(Value::as_str).unwrap_or_default(),
                detail.get("text").and_then(Value::as_str).unwrap_or_default()
            );
            merged.insert("text".into(), Value::String(text));
            merge_detail_identity(previous, detail, &mut merged);
            Some(Value::Object(merged))
        }
        "reasoning.summary" => {
            let mut merged = previous.as_object()?.clone();
            merged.extend(detail.as_object()?.clone());
            let summary = format!(
                "{}{}",
                previous.get("summary").and_then(Value::as_str).unwrap_or_default(),
                detail.get("summary").and_then(Value::as_str).unwrap_or_default()
            );
            merged.insert("summary".into(), Value::String(summary));
            merge_detail_identity(previous, detail, &mut merged);
            Some(Value::Object(merged))
        }
        "summary" => {
            let mut merged = previous.as_object()?.clone();
            merged.extend(detail.as_object()?.clone());
            let summary = format!(
                "{}{}",
                previous.get("summary").and_then(Value::as_str)?,
                detail.get("summary").and_then(Value::as_str)?
            );
            merged.insert("summary".into(), Value::String(summary));
            Some(Value::Object(merged))
        }
        _ => None,
    }
}

/// Accumulate decoded detail deltas into the response-wide list (port of
/// `appendReasoningDetails`).
fn append_reasoning_details(result: &mut Vec<Value>, details: &[Value]) {
    for detail in details {
        match result.last() {
            Some(previous) => match merge_reasoning_details(previous, detail) {
                Some(merged) => {
                    let length = result.len();
                    result[length - 1] = merged;
                }
                None => result.push(detail.clone()),
            },
            None => result.push(detail.clone()),
        }
    }
}

/// Keep only recognized detail shapes (port of the `ReasoningDetail` union
/// decoder; anything else is dropped at the boundary).
fn known_reasoning_details(details: &[Value]) -> Vec<Value> {
    let known = details.iter().filter(|detail| valid_reasoning_detail(detail)).cloned().collect();
    known
}

fn string_field(value: &Value, key: &str) -> DecodeResult<Option<String>> {
    opt_string(value.get(key))
}

fn optional_index(value: &Value) -> DecodeResult<Option<f64>> {
    opt_number(value.get("index"))
}

fn valid_reasoning_detail(detail: &Value) -> bool {
    let Some(kind) = detail_type(detail) else { return false };
    let identity = || -> DecodeResult<()> {
        match detail.get("id") {
            None | Some(Value::Null | Value::String(_)) => {}
            Some(_) => return Err(()),
        }
        string_field(detail, "format")?;
        optional_index(detail)?;
        match detail.get("signature") {
            None | Some(Value::Null | Value::String(_)) => {}
            Some(_) => return Err(()),
        }
        Ok(())
    };
    match kind {
        "reasoning.text" => identity().is_ok() && string_field(detail, "text").is_ok(),
        "reasoning.summary" => identity().is_ok() && string_field(detail, "summary").is_ok(),
        "reasoning.encrypted" => {
            identity().is_ok() && matches!(detail.get("data"), Some(Value::String(_)))
        }
        "summary" => {
            matches!(detail.get("summary"), Some(Value::String(_)))
                && optional_index(detail).is_ok()
        }
        "encrypted" => {
            matches!(detail.get("encrypted"), Some(Value::String(_)))
                && optional_index(detail).is_ok()
        }
        _ => false,
    }
}

/// Display text carried by a detail batch (port of `detailText`); Kimi
/// summaries show only when no scalar reasoning field has carried text.
fn detail_text(details: &[Value], hide_kimi_summary: bool) -> Option<String> {
    let mut text = String::new();
    for detail in details {
        let value = match detail_type(detail) {
            Some("reasoning.text") => detail.get("text").and_then(Value::as_str),
            Some("reasoning.summary") => detail.get("summary").and_then(Value::as_str),
            // Kimi streams the full thinking through `reasoning_content` and a
            // separate summary through details; show the summary only when
            // nothing else does.
            Some("summary") if !hide_kimi_summary => detail.get("summary").and_then(Value::as_str),
            _ => None,
        };
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            text.push_str(value);
        }
    }
    (!text.is_empty()).then_some(text)
}

/// `{key: {reasoningField?, reasoningDetails?}}` (port of `reasoningMetadata`;
/// always an object, even with neither field set).
fn reasoning_metadata(
    provider_metadata_key: &str,
    field: Option<&str>,
    details: Option<&[Value]>,
) -> ProviderMetadata {
    let mut inner = BTreeMap::new();
    if let Some(field) = field {
        inner.insert("reasoningField".to_string(), Value::String(field.to_string()));
    }
    if let Some(details) = details {
        inner.insert("reasoningDetails".to_string(), Value::Array(details.to_vec()));
    }
    BTreeMap::from([(provider_metadata_key.to_string(), inner)])
}

/// The scalar reasoning delta in field-priority order: configured field first,
/// then `reasoning_content`, `reasoning`, `reasoning_text` (port of
/// `reasoningDelta`).
fn reasoning_delta_text(
    delta: &ChatDelta,
    raw_delta: Option<&Value>,
    configured_field: Option<&str>,
) -> Option<(String, String)> {
    let mut fields: Vec<&str> = Vec::new();
    if let Some(field) = configured_field {
        fields.push(field);
    }
    fields.extend(["reasoning_content", "reasoning", "reasoning_text"]);
    for field in fields {
        let text = if field == "reasoning_content" && delta.reasoning_content.is_some() {
            delta.reasoning_content.clone()
        } else if field == "reasoning" && delta.reasoning.is_some() {
            delta.reasoning.clone()
        } else if field == "reasoning_text" && delta.reasoning_text.is_some() {
            delta.reasoning_text.clone()
        } else if field != "reasoning_content" && field != "reasoning" && field != "reasoning_text"
        {
            raw_delta.and_then(|raw| raw.get(field)).and_then(Value::as_str).map(str::to_string)
        } else {
            None
        };
        if let Some(text) = text {
            if !text.is_empty() {
                return Some((field.to_string(), text));
            }
        }
    }
    None
}

// =============================================================================
// Step machine (port of `step`)
// =============================================================================

fn classify_stream_error(message: &str, status: Option<u16>, body: &str) -> AiError {
    AiError::new(classify_provider_failure(ProviderFailure {
        message,
        status,
        raw_body: Some(body),
        data: None,
        http: None,
        retry_after_ms: None,
        rate_limit: None,
    }))
}

fn tool_index_by_id(
    tool_indices: &BTreeMap<i64, String>,
    pending_tools: &BTreeMap<i64, PendingToolDelta>,
    id: Option<&str>,
) -> Option<i64> {
    let id = id?;
    let mut keys: std::collections::BTreeSet<i64> = pending_tools.keys().copied().collect();
    keys.extend(tool_indices.keys().copied());
    keys.into_iter().find(|&index| {
        tool_indices
            .get(&index)
            .map(|tool_id| tool_id == id)
            .or_else(|| {
                pending_tools
                    .get(&index)
                    .and_then(|tool| tool.id.as_deref())
                    .map(|tool_id| tool_id == id)
            })
            .unwrap_or(false)
    })
}

fn step_state(
    state: ParserState,
    event: &ChatEvent,
) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    if let Some(error) = &event.error {
        let body = encode_event(event);
        let (message, status) = match error {
            ChatEventError::Structured { code, message } => (
                message.clone(),
                code.as_ref().and_then(Value::as_i64).and_then(|code| u16::try_from(code).ok()),
            ),
            ChatEventError::Plain(message) => (message.clone(), None),
        };
        return Err(classify_stream_error(&message, status, &body));
    }
    let mut events: Vec<LLMEvent> = Vec::new();
    let choice = event.choices.first();
    // Moonshot and friends attach usage to `choice.usage` instead of the
    // top-level field.
    let usage =
        map_usage(event.usage.as_ref().unwrap_or(&Value::Null), &state.provider_metadata_key)
            .or_else(|| {
                choice
                    .and_then(|choice| choice.usage.as_ref())
                    .and_then(|usage| map_usage(usage, &state.provider_metadata_key))
            })
            .or(state.usage.clone());
    let raw_finish_reason =
        choice.and_then(|choice| choice.finish_reason.clone()).filter(|r| !r.is_empty());
    let finish_reason = match raw_finish_reason {
        Some(raw) => {
            let normalized = map_finish_reason(event, &raw)?;
            let raw_value =
                choice.and_then(|choice| choice.native_finish_reason.clone()).unwrap_or(raw);
            Some(FinishReasonDetails {
                normalized,
                raw: Some(raw_value),
                category: None,
                explanation: None,
            })
        }
        None => state.finish_reason.clone(),
    };
    let delta = choice.and_then(|choice| choice.delta.as_ref());
    let raw_delta = event.raw.pointer("/choices/0/delta").filter(|value| value.is_object());
    let tool_deltas: &[ChatToolCallDelta] = delta.map_or(&[], |delta| delta.tool_calls.as_slice());

    let mut tools = state.tools.clone();
    let mut tool_indices = state.tool_indices.clone();
    let mut pending_tools = state.pending_tools.clone();
    let mut latest_tool_index = state.latest_tool_index;
    let mut next_tool_index = state.next_tool_index;
    let mut lifecycle_state = state.lifecycle.clone();

    let reasoning = delta
        .and_then(|delta| reasoning_delta_text(delta, raw_delta, state.reasoning_field.as_deref()));
    let has_late_content = delta.is_some_and(|delta| {
        delta.content.as_deref().is_some_and(|t| !t.is_empty())
            || delta.refusal.as_deref().is_some_and(|t| !t.is_empty())
            || delta
                .reasoning_details
                .as_ref()
                .is_some_and(|details| details.as_array().is_some_and(|d| !d.is_empty()))
    }) || reasoning.is_some()
        || tool_deltas.iter().any(|tool| {
            tool.id.as_deref().is_some_and(|t| !t.is_empty())
                || tool.function_name.as_deref().is_some_and(|t| !t.is_empty())
                || tool.function_arguments.as_deref().is_some_and(|t| !t.is_empty())
        });
    if state.finish_reason.is_some() {
        if has_late_content {
            let mut error = event_error(
                state.provider_label.clone(),
                "OpenAI Chat received content after the finish reason",
            );
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.body = Some(encode_event(event));
            }
            return Err(error);
        }
        return Ok((ParserState { usage, ..state }, events));
    }

    let reasoning_field = state
        .reasoning_field
        .clone()
        .or_else(|| reasoning.as_ref().map(|(field, _)| field.clone()));
    let reasoning_text_observed = state.reasoning_text_observed || reasoning.is_some();
    let detail_delta = delta
        .and_then(|delta| delta.reasoning_details.as_ref())
        .filter(|details| details.is_array())
        .map(|details| known_reasoning_details(details.as_array().map_or(&[], Vec::as_slice)));
    let mut reasoning_details = state.reasoning_details.clone();
    if let Some(detail_delta) = &detail_delta {
        append_reasoning_details(&mut reasoning_details, detail_delta);
    }
    let reasoning_details_observed = state.reasoning_details_observed || detail_delta.is_some();
    let delta_metadata =
        Some(reasoning_metadata(&state.provider_metadata_key, reasoning_field.as_deref(), None));
    let text = match &detail_delta {
        Some(details) if !details.is_empty() => detail_text(details, reasoning_text_observed)
            .or_else(|| reasoning.as_ref().map(|(_, text)| text.clone())),
        _ => reasoning.as_ref().map(|(_, text)| text.clone()),
    };
    let mut reasoning_started = state.reasoning_started;
    if let Some(text) = text {
        // The shared lifecycle port does not forward delta metadata onto the
        // implicit block start, so open the block explicitly first (matches
        // TS `Lifecycle.reasoningDelta` seeding `reasoningStart` with the same
        // metadata).
        lifecycle_state = lifecycle::reasoning_start(
            &lifecycle_state,
            &mut events,
            "reasoning-0",
            delta_metadata.clone(),
        );
        reasoning_started = true;
        lifecycle_state = lifecycle::reasoning_delta(
            &lifecycle_state,
            &mut events,
            "reasoning-0",
            &text,
            delta_metadata,
        );
    } else if reasoning_details_observed
        && !reasoning_started
        && (delta.is_some_and(|delta| {
            delta.content.as_deref().is_some_and(|t| !t.is_empty())
                || delta.refusal.as_deref().is_some_and(|t| !t.is_empty())
        }) || !tool_deltas.is_empty())
    {
        lifecycle_state = lifecycle::reasoning_start(
            &lifecycle_state,
            &mut events,
            "reasoning-0",
            delta_metadata,
        );
        reasoning_started = true;
    }
    let reasoning_emitted = state.reasoning_emitted || reasoning_started;

    // Reasoning is one response-wide channel: it stays open alongside text and
    // refusal output, and `finish_events` closes it once with the complete
    // metadata.
    if let Some(delta) = delta {
        if let Some(content) = delta.content.as_deref().filter(|t| !t.is_empty()) {
            lifecycle_state =
                lifecycle::text_delta(&lifecycle_state, &mut events, "text-0", content, None);
        }
        if let Some(refusal) = delta.refusal.as_deref().filter(|t| !t.is_empty()) {
            lifecycle_state =
                lifecycle::text_delta(&lifecycle_state, &mut events, "text-0", refusal, None);
        }
    }

    // Compatible providers may omit indexes: prefer durable identity, then
    // batch position for parallel deltas or the latest call for sparse chunks.
    for (position, tool) in tool_deltas.iter().enumerate() {
        let position = i64::try_from(position).expect("tool delta count fits i64");
        let tool_id = tool.id.clone().filter(|id| !id.is_empty());
        let matched = tool_index_by_id(&tool_indices, &pending_tools, tool_id.as_deref());
        let fallback = if tool_deltas.len() > 1 {
            Some(position)
        } else {
            latest_tool_index.or(Some(position))
        };
        let fallback_tool_id = fallback.and_then(|index| {
            tool_indices
                .get(&index)
                .cloned()
                .or_else(|| pending_tools.get(&index).and_then(|tool| tool.id.clone()))
        });
        let index = tool.index.map(|index| index as i64).or(matched).unwrap_or_else(|| {
            match (tool_id.clone(), fallback_tool_id) {
                (Some(tool_id), Some(fallback_id)) if fallback_id != tool_id => next_tool_index,
                _ => fallback.unwrap_or(position),
            }
        });
        let current = tools.get(&index.to_string()).cloned();
        let pending = pending_tools.get(&index).cloned();
        let id = current
            .as_ref()
            .map(|tool| tool.id.clone())
            .or_else(|| pending.as_ref().and_then(|tool| tool.id.clone()))
            .or(tool_id.clone());
        let name = current
            .as_ref()
            .map(|tool| tool.name.clone())
            .or_else(|| pending.as_ref().and_then(|tool| tool.name.clone()))
            .or_else(|| tool.function_name.clone().filter(|name| !name.is_empty()));
        let text = format!(
            "{}{}",
            pending.as_ref().map_or(String::new(), |tool| tool.input.clone()),
            tool.function_arguments.clone().unwrap_or_default()
        );
        let extra_content = pending
            .as_ref()
            .and_then(|tool| tool.extra_content.clone())
            .or_else(|| decode_extra_content(tool.extra_content.as_ref()));
        latest_tool_index = Some(index);
        next_tool_index = next_tool_index.max(index + 1);
        if current.is_none() && (id.is_none() || name.is_none()) {
            pending_tools.insert(
                index,
                PendingToolDelta {
                    id: id.clone().filter(|id| !id.is_empty()),
                    name: name.clone().filter(|name| !name.is_empty()),
                    input: text,
                    extra_content,
                },
            );
            continue;
        }
        if pending.is_some() {
            pending_tools.remove(&index);
        }
        let provider_metadata = extra_content.map(|extra| {
            BTreeMap::from([(
                state.provider_metadata_key.clone(),
                BTreeMap::from([("extraContent".to_string(), extra)]),
            )])
        });
        let result = tool_stream::append_or_start(
            ADAPTER,
            &tools,
            &index.to_string(),
            tool_stream::Delta {
                id: id.as_deref(),
                name: name.as_deref(),
                text: &text,
                provider_metadata: provider_metadata.clone(),
            },
            MISSING_TOOL_MESSAGE,
        )
        .map_err(|error| attach_body(error, encode_event(event)))?;
        tools = result.tools;
        tool_indices.insert(index, result.tool.id.clone());
        if !result.events.is_empty() {
            lifecycle_state = lifecycle::step_start(&lifecycle_state, &mut events);
        }
        events.extend(result.events);
    }

    let incomplete_tools = matches!(
        finish_reason.as_ref().map(|reason| reason.normalized),
        Some(FinishReason::ContentFilter | FinishReason::Length)
    );
    if finish_reason.is_some()
        && !incomplete_tools
        && state.finish_reason.is_none()
        && !pending_tools.is_empty()
    {
        let mut error = event_error(state.provider_label.clone(), MISSING_TOOL_MESSAGE);
        if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
            fields.body = Some(encode_event(event));
        }
        return Err(error);
    }

    // Filtering or truncation terminates the response without confirming
    // pending tool calls.
    let finished = if finish_reason.is_some()
        && !incomplete_tools
        && state.finish_reason.is_none()
        && !tool_indices.is_empty()
    {
        Some(
            tool_stream::finish_all(ADAPTER, &tools)
                .map_err(|error| attach_body(error, encode_event(event)))?,
        )
    } else {
        None
    };
    let (tools, tool_indices, tool_call_events) = match finished {
        Some(finished) => (finished.tools, BTreeMap::new(), finished.events),
        None => (tools, tool_indices, state.tool_call_events.clone()),
    };

    Ok((
        ParserState {
            provider_metadata_key: state.provider_metadata_key,
            tools,
            tool_indices,
            pending_tools,
            tool_call_events,
            usage,
            finish_reason,
            lifecycle: lifecycle_state,
            reasoning_field,
            reasoning_text_observed,
            reasoning_details,
            reasoning_details_observed,
            reasoning_emitted,
            latest_tool_index,
            next_tool_index,
            require_finish_reason: state.require_finish_reason,
            reasoning_started,
            provider_label: state.provider_label,
        },
        events,
    ))
}

fn attach_body(mut error: AiError, body: String) -> AiError {
    if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
        if fields.body.is_none() {
            fields.body = Some(body);
        }
    }
    error
}

/// `{google: {thought_signature}}` payload check (Gemini's OpenAI-compatible
/// surface), used for tool-call `extra_content`.
fn decode_extra_content(value: Option<&Value>) -> Option<Value> {
    let value = value?;
    let signature = value.pointer("/google/thought_signature")?;
    signature.as_str()?;
    Some(value.clone())
}

// =============================================================================
// onHalt (port of `finishEvents`)
// =============================================================================

fn finish_events(state: ParserState) -> Result<Vec<LLMEvent>, AiError> {
    if state.finish_reason.is_none() && state.require_finish_reason {
        return Err(AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new("OpenAI Chat stream ended without finish_reason"),
            classification: Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM.into()),
            route: Some(ADAPTER.into()),
        }));
    }
    let mut events: Vec<LLMEvent> = Vec::new();
    let tool_call_events = if state.finish_reason.is_none() && !state.tool_indices.is_empty() {
        tool_stream::finish_all(ADAPTER, &state.tools)?.events
    } else {
        state.tool_call_events.clone()
    };
    let has_tool_calls = !tool_call_events.is_empty();
    let reason = match state.finish_reason.clone() {
        Some(reason) => {
            let normalized = if reason.normalized == FinishReason::Stop && has_tool_calls {
                FinishReason::ToolCalls
            } else {
                reason.normalized
            };
            FinishReasonDetails { normalized, ..reason }
        }
        None => FinishReasonDetails::new(if has_tool_calls {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        }),
    };
    // Snapshot details at publish time so the emitted event never observes
    // later mutation of the accumulated list.
    let metadata = Some(reasoning_metadata(
        &state.provider_metadata_key,
        state.reasoning_field.as_deref(),
        state.reasoning_details_observed.then_some(state.reasoning_details.as_slice()),
    ));
    let started = if state.reasoning_details_observed && !state.reasoning_emitted {
        lifecycle::reasoning_start(
            &state.lifecycle,
            &mut events,
            "reasoning-0",
            Some(reasoning_metadata(
                &state.provider_metadata_key,
                state.reasoning_field.as_deref(),
                None,
            )),
        )
    } else {
        state.lifecycle.clone()
    };
    let ended = lifecycle::reasoning_end(&started, &mut events, "reasoning-0", metadata, None);
    let lifecycle_state = if tool_call_events.is_empty() {
        ended
    } else {
        lifecycle::step_start(&ended, &mut events)
    };
    events.extend(tool_call_events);
    lifecycle::finish(
        &lifecycle_state,
        &mut events,
        lifecycle::FinishInput { reason, usage: state.usage.clone(), provider_metadata: None },
    );
    Ok(events)
}

// =============================================================================
// Protocol implementation
// =============================================================================

/// The OpenAI Chat protocol (base + optional provider body seams).
pub struct OpenAiChat {
    pub ext: OpenAiChatExt,
}

impl OpenAiChat {
    #[must_use]
    pub fn new(ext: OpenAiChatExt) -> Self {
        Self { ext }
    }
}

impl Protocol for OpenAiChat {
    fn id(&self) -> &str {
        self.ext.id
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request(request, &self.ext)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState::initial(request))
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        if frame == DONE {
            return Ok((state, Vec::new()));
        }
        let parser_state = downcast_state(state);
        let raw = shared::decode_json(frame).map_err(|cause| {
            let mut error = event_error(
                self.ext.id,
                format!("Invalid {} stream event", parser_state.provider_label),
            );
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.body = Some(frame.to_string());
                fields.cause = Some(cause);
            }
            error
        })?;
        let event = decode_event(raw, self.ext.allow_string_error).map_err(|()| {
            let mut error = event_error(
                self.ext.id,
                format!("Invalid {} stream event", parser_state.provider_label),
            );
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.body = Some(frame.to_string());
            }
            error
        })?;
        let (next, events) = step_state(parser_state, &event)?;
        Ok((Box::new(next), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        frame == DONE
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let parser_state = downcast_state(state);
        let events = finish_events(parser_state)?;
        Ok((Box::new(()), events))
    }
}

fn downcast_state(state: ProtocolState) -> ParserState {
    match state.downcast::<ParserState>() {
        Ok(inner) => *inner,
        Err(_) => panic!("openai-chat protocol state of unexpected type"),
    }
}

// =============================================================================
// Venice stream wrapper (port of `venice-chat.ts` normalization)
// =============================================================================

#[derive(Debug, Clone)]
struct VeniceState {
    shared: ParserState,
    pending: String,
    reasoning: String,
    encrypted: bool,
    signature: Option<String>,
    tools: BTreeMap<String, String>,
}

/// Venice's OpenAI-compatible surface streams encrypted thinking as a marker
/// inside `reasoning_content` and carries per-call thought signatures. The
/// wrapper filters the marker, normalizes cache-write usage, replays
/// signatures, and tags finish metadata.
pub struct VeniceChatProtocol;

impl Protocol for VeniceChatProtocol {
    fn id(&self) -> &str {
        "venice-chat"
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request(request, &venice_ext())
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(VeniceState {
            shared: ParserState::initial(request),
            pending: String::new(),
            reasoning: String::new(),
            encrypted: false,
            signature: None,
            tools: BTreeMap::new(),
        })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        if frame == DONE {
            return Ok((state, Vec::new()));
        }
        let venice = downcast_venice(state);
        let raw = shared::decode_json(frame).map_err(|cause| {
            let mut error = event_error(
                "venice-chat",
                format!("Invalid {} stream event", venice.shared.provider_label),
            );
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.body = Some(frame.to_string());
                fields.cause = Some(cause);
            }
            error
        })?;
        let event = decode_event(raw.clone(), true).map_err(|()| {
            let mut error = event_error(
                "venice-chat",
                format!("Invalid {} stream event", venice.shared.provider_label),
            );
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.body = Some(frame.to_string());
            }
            error
        })?;
        venice_step(venice, event, raw)
            .map(|(state, events)| (Box::new(state) as ProtocolState, events))
    }

    fn terminal(&self, frame: &str) -> bool {
        frame == DONE
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let venice = downcast_venice(state);
        let events = venice_finish(venice)?;
        Ok((Box::new(()), events))
    }
}

fn downcast_venice(state: ProtocolState) -> VeniceState {
    match state.downcast::<VeniceState>() {
        Ok(inner) => *inner,
        Err(_) => panic!("venice-chat protocol state of unexpected type"),
    }
}

fn venice_step(
    state: VeniceState,
    event: ChatEvent,
    raw: Value,
) -> Result<(VeniceState, Vec<LLMEvent>), AiError> {
    if let Some(ChatEventError::Plain(message)) = &event.error {
        let issues = raw
            .get("issues")
            .and_then(Value::as_array)
            .map(|issues| {
                issues
                    .iter()
                    .map(|issue| {
                        let path = issue
                            .get("path")
                            .and_then(Value::as_array)
                            .filter(|path| !path.is_empty())
                            .map(|path| {
                                path.iter()
                                    .map(|part| match part {
                                        Value::String(text) => text.clone(),
                                        other => other.to_string(),
                                    })
                                    .collect::<Vec<_>>()
                                    .join(".")
                            });
                        let text = issue.get("message").and_then(Value::as_str).unwrap_or_default();
                        match path {
                            Some(path) if !path.is_empty() => format!("{path}: {text}"),
                            _ => text.to_string(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let joined = [vec![message.clone()], issues].concat().join("; ");
        return Err(classify_stream_error(&joined, None, &encode_event(&event)));
    }

    let delta = event.choices.first().and_then(|choice| choice.delta.as_ref());
    let scalar = delta.and_then(|delta| delta.reasoning_content.clone()).unwrap_or_default();
    let text: Vec<char> = state.pending.chars().chain(scalar.chars()).collect();
    let marker_chars: Vec<char> = VENICE_MARKER.chars().collect();
    let marker = find_marker(&text, &marker_chars);
    let pending = if marker.is_some() || state.encrypted {
        0
    } else {
        let max = text.len().min(marker_chars.len() - 1);
        let mut found = 0;
        for length in 1..=max {
            let prefix: String = marker_chars[..length].iter().collect();
            let tail: String = text[(text.len() - length)..].iter().collect();
            if prefix == tail {
                found = length;
            }
        }
        found
    };
    let visible = if state.encrypted {
        String::new()
    } else if let Some(marker) = marker {
        text[..marker].iter().collect()
    } else {
        text[..text.len() - pending].iter().collect()
    };

    // Rewrite the event for the shared machine: visible reasoning only, an
    // empty details array once the marker was seen, and the cache-write
    // fallback inside prompt_tokens_details.
    let mut rewritten = raw.clone();
    if let Some(delta_value) =
        rewritten.pointer_mut("/choices/0/delta").and_then(Value::as_object_mut)
    {
        if visible.is_empty() {
            delta_value.remove("reasoning_content");
        } else {
            delta_value.insert("reasoning_content".into(), Value::String(visible.clone()));
        }
        if marker.is_some() && !delta_value.contains_key("reasoning_details") {
            delta_value.insert("reasoning_details".into(), Value::Array(Vec::new()));
        }
    }
    if let Some(usage) = rewritten.get_mut("usage").and_then(Value::as_object_mut) {
        if let Some(details) = usage.get_mut("prompt_tokens_details").and_then(Value::as_object_mut)
        {
            let write = details.get("cache_write_tokens").and_then(Value::as_f64);
            if write.is_none() {
                if let Some(creation) = details.get("cache_creation_input_tokens").cloned() {
                    details.insert("cache_write_tokens".into(), creation);
                }
            }
        }
    }
    let shared_event = decode_event(rewritten, true)
        .map_err(|()| event_error("venice-chat", "Invalid venice-chat stream event"))?;
    let (shared, events) = step_state(state.shared, &shared_event)?;

    let mut tools = state.tools.clone();
    if let Some(delta) = delta {
        for call in &delta.tool_calls {
            let Some(signature) = call.thought_signature.as_ref() else { continue };
            let index = call.index.map(|index| index as i64).or(shared.latest_tool_index);
            let id = call.id.clone().or_else(|| {
                index.and_then(|index| {
                    shared.tools.get(&index.to_string()).map(|tool| tool.id.clone()).or_else(|| {
                        shared.pending_tools.get(&index).and_then(|tool| tool.id.clone())
                    })
                })
            });
            if let Some(id) = id {
                tools.insert(id, signature.clone());
            }
        }
    }

    Ok((
        VeniceState {
            shared,
            pending: if pending > 0 {
                text[(text.len() - pending)..].iter().collect()
            } else {
                String::new()
            },
            reasoning: format!("{}{}", state.reasoning, scalar),
            encrypted: state.encrypted || marker.is_some(),
            signature: delta.and_then(|delta| delta.thought_signature.clone()).or(state.signature),
            tools,
        },
        events,
    ))
}

fn find_marker(text: &[char], marker: &[char]) -> Option<usize> {
    if marker.len() > text.len() {
        return None;
    }
    (0..=text.len() - marker.len()).find(|&start| text[start..start + marker.len()] == *marker)
}

fn venice_finish(state: VeniceState) -> Result<Vec<LLMEvent>, AiError> {
    let base = finish_events(state.shared)?;
    let mut events: Vec<LLMEvent> = Vec::with_capacity(base.len());
    for event in base {
        let (id, is_tool) = match &event {
            LLMEvent::ReasoningEnd { id, .. } => (id.clone(), false),
            LLMEvent::TextEnd { id, .. } => (id.clone(), false),
            LLMEvent::ToolCall { id, .. } | LLMEvent::ToolInputEnd { id, .. } => (id.clone(), true),
            _ => {
                events.push(event);
                continue;
            }
        };
        let signature = if is_tool { state.tools.get(&id).cloned() } else { None };
        let mut metadata = match &event {
            LLMEvent::ReasoningEnd { provider_metadata, .. }
            | LLMEvent::TextEnd { provider_metadata, .. }
            | LLMEvent::ToolCall { provider_metadata, .. }
            | LLMEvent::ToolInputEnd { provider_metadata, .. } => provider_metadata.clone(),
            _ => None,
        }
        .unwrap_or_default();
        let venice_entry = metadata.entry("venice".to_string()).or_default();
        if let Some(signature) = &state.signature {
            venice_entry.insert("messageThoughtSignature".into(), Value::String(signature.clone()));
        }
        if let Some(signature) = &signature {
            venice_entry.insert("thoughtSignature".into(), Value::String(signature.clone()));
        }
        if matches!(event, LLMEvent::ReasoningEnd { .. }) && state.encrypted {
            venice_entry
                .insert("encryptedReasoningContent".into(), Value::String(state.reasoning.clone()));
        }
        let metadata = Some(metadata);
        if matches!(event, LLMEvent::ReasoningEnd { .. }) && !state.pending.is_empty() {
            events.push(LLMEvent::ReasoningDelta {
                id: id.clone(),
                text: state.pending.clone(),
                provider_metadata: metadata.clone(),
            });
        }
        events.push(match event {
            LLMEvent::ReasoningEnd { text, .. } => {
                LLMEvent::ReasoningEnd { id, text, provider_metadata: metadata }
            }
            LLMEvent::TextEnd { text, .. } => {
                LLMEvent::TextEnd { id, text, provider_metadata: metadata }
            }
            LLMEvent::ToolCall { name, namespace, input, provider_executed, .. } => {
                LLMEvent::ToolCall {
                    id,
                    name,
                    namespace,
                    input,
                    provider_executed,
                    provider_metadata: metadata,
                }
            }
            LLMEvent::ToolInputEnd { name, namespace, .. } => {
                LLMEvent::ToolInputEnd { id, name, namespace, provider_metadata: metadata }
            }
            other => other,
        });
    }
    Ok(events)
}

// =============================================================================
// Request lowering (port of `lowerMessages` + `fromRequest`)
// =============================================================================

/// Shared 4-breakpoint cache-control budget (port of `utils/cache.ts`).
#[derive(Debug, Clone)]
struct Breakpoints {
    remaining: usize,
}

impl Breakpoints {
    fn new() -> Self {
        Self { remaining: 4 }
    }

    /// Requests of at least one hour use the explicit `"1h"` bucket.
    fn cache_control(&mut self, cache: Option<&CacheHint>) -> Option<Value> {
        let cache = cache?;
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(match cache.ttl_seconds {
            Some(ttl) if ttl >= 3600.0 => json!({"type": "ephemeral", "ttl": "1h"}),
            _ => json!({"type": "ephemeral"}),
        })
    }
}

struct FlattenedRequest {
    tools: Vec<ToolDefinition>,
    request: LLMRequest,
}

/// Port of `flattenToolRequest`: namespace-prefixed tool-call/tool-result
/// parts flatten to `namespace_name`, and the tool list flattens the same way.
fn flatten_tool_request(request: &LLMRequest) -> FlattenedRequest {
    let mut changed = false;
    let messages = request
        .messages
        .iter()
        .map(|message| {
            let content = message
                .content
                .iter()
                .map(|part| match part {
                    ContentPart::ToolCall { namespace: Some(namespace), name, .. } => {
                        changed = true;
                        ContentPart::ToolCall {
                            id: part_tool_call_id(part),
                            name: format!("{namespace}_{name}"),
                            namespace: None,
                            input: part_tool_call_input(part).clone(),
                            provider_executed: part_tool_executed(part),
                            cache: part.cache().cloned(),
                            metadata: part_metadata(part),
                            provider_metadata: part_provider_metadata(part),
                        }
                    }
                    ContentPart::ToolResult { namespace: Some(namespace), name, .. } => {
                        changed = true;
                        ContentPart::ToolResult {
                            id: part_tool_result_id(part),
                            name: format!("{namespace}_{name}"),
                            namespace: None,
                            result: part_tool_result(part).clone(),
                            provider_executed: part_tool_executed(part),
                            cache: part.cache().cloned(),
                            metadata: part_metadata(part),
                            provider_metadata: part_provider_metadata(part),
                        }
                    }
                    other => other.clone(),
                })
                .collect::<Vec<_>>();
            Message { content, ..message.clone() }
        })
        .collect();
    let tools = flatten_tools(&request.tools);
    let request = if changed {
        request.update(crate::schema::LLMRequestPatch {
            messages: Some(messages),
            ..Default::default()
        })
    } else {
        request.clone()
    };
    FlattenedRequest { tools, request }
}

fn part_tool_call_id(part: &ContentPart) -> String {
    match part {
        ContentPart::ToolCall { id, .. } => id.clone(),
        _ => String::new(),
    }
}

fn part_tool_call_input(part: &ContentPart) -> Value {
    match part {
        ContentPart::ToolCall { input, .. } => input.clone(),
        _ => Value::Null,
    }
}

fn part_tool_result_id(part: &ContentPart) -> String {
    match part {
        ContentPart::ToolResult { id, .. } => id.clone(),
        _ => String::new(),
    }
}

fn part_tool_result(part: &ContentPart) -> crate::schema::ToolResultValue {
    match part {
        ContentPart::ToolResult { result, .. } => result.clone(),
        _ => crate::schema::ToolResultValue::Json { value: Value::Null },
    }
}

fn part_tool_executed(part: &ContentPart) -> Option<bool> {
    match part {
        ContentPart::ToolCall { provider_executed, .. }
        | ContentPart::ToolResult { provider_executed, .. } => *provider_executed,
        _ => None,
    }
}

fn part_metadata(part: &ContentPart) -> Option<BTreeMap<String, Value>> {
    match part {
        ContentPart::Text { metadata, .. }
        | ContentPart::Media { metadata, .. }
        | ContentPart::ToolCall { metadata, .. }
        | ContentPart::ToolResult { metadata, .. }
        | ContentPart::Reasoning { metadata, .. } => metadata.clone(),
        _ => None,
    }
}

fn part_provider_metadata(part: &ContentPart) -> Option<ProviderMetadata> {
    match part {
        ContentPart::Text { provider_metadata, .. }
        | ContentPart::Media { provider_metadata, .. }
        | ContentPart::ToolCall { provider_metadata, .. }
        | ContentPart::ToolResult { provider_metadata, .. }
        | ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
        _ => None,
    }
}

fn part_text(part: &ContentPart) -> Option<&str> {
    match part {
        ContentPart::Text { text, .. } => Some(text),
        _ => None,
    }
}

fn has_tool_history(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        if matches!(message.role, MessageRole::Tool) {
            return true;
        }
        matches!(message.role, MessageRole::Assistant)
            && message.content.iter().any(|part| matches!(part, ContentPart::ToolCall { .. }))
    })
}

/// Port of `detectMaxTokensField`: providers still requiring legacy
/// `max_tokens` when compatibility does not say.
fn detect_max_tokens_field(provider: &str, base_url: Option<&str>) -> MaxTokensFieldCompatibility {
    let p = provider.to_ascii_lowercase();
    let url = base_url.unwrap_or_default().to_ascii_lowercase();
    let legacy = [
        ("deepseek", ""),
        ("", "deepseek.com"),
        ("moonshotai", ""),
        ("", "api.moonshot.ai"),
        ("togetherai", ""),
        ("", "api.together."),
        ("zai", ""),
        ("zai-coding-plan", ""),
        ("zhipuai", ""),
        ("zhipuai-coding-plan", ""),
        ("", "api.z.ai"),
        ("", "open.bigmodel.cn"),
        ("nvidia", ""),
        ("", "integrate.api.nvidia.com"),
        ("cerebras", ""),
        ("", "cerebras.ai"),
        ("", "llm.chutes.ai"),
        ("chutes", ""),
        ("cloudflare-ai-gateway", ""),
        ("", "gateway.ai.cloudflare.com"),
        ("cloudflare-workers-ai", ""),
        ("", "api.cloudflare.com"),
    ];
    if legacy.iter().any(|(name, needle)| {
        (!name.is_empty() && p == *name) || (!needle.is_empty() && url.contains(*needle))
    }) {
        MaxTokensFieldCompatibility::MaxTokens
    } else {
        MaxTokensFieldCompatibility::MaxCompletionTokens
    }
}

fn detect_supports_store(provider: &str, base_url: Option<&str>) -> bool {
    let p = provider.to_ascii_lowercase();
    let url = base_url.unwrap_or_default().to_ascii_lowercase();
    let matches_any = |names: &[&str], needles: &[&str]| {
        names.contains(&p.as_str()) || needles.iter().any(|needle| url.contains(needle))
    };
    let non_standard = matches_any(&["nvidia"], &["integrate.api.nvidia.com"])
        || matches_any(&["moonshotai", "moonshotai-cn"], &["api.moonshot."])
        || matches_any(&["togetherai", "together"], &["api.together."])
        || matches_any(
            &["zai", "zai-coding-plan", "zhipuai", "zhipuai-coding-plan"],
            &["api.z.ai", "open.bigmodel.cn"],
        )
        || matches_any(&["deepseek"], &["deepseek.com"])
        || matches_any(&["cerebras"], &["cerebras.ai"])
        || matches_any(&["xai"], &["api.x.ai"])
        || matches_any(&["chutes"], &["chutes.ai"])
        || matches_any(&["cloudflare-workers-ai"], &["api.cloudflare.com"])
        || matches_any(&["cloudflare-ai-gateway"], &["gateway.ai.cloudflare.com"])
        || matches_any(&["vercel-ai-gateway"], &["ai-gateway.vercel.sh", "vercel.sh"])
        || matches_any(&["ant-ling"], &["api.ant-ling.com"])
        || matches_any(&["opencode"], &["opencode.ai"])
        || url.contains("generativelanguage.googleapis.com");
    !non_standard
}

fn detect_supports_strict_mode(provider: &str, base_url: Option<&str>) -> bool {
    let p = provider.to_ascii_lowercase();
    let url = base_url.unwrap_or_default().to_ascii_lowercase();
    let is_moonshot = p == "moonshotai" || p == "moonshotai-cn" || url.contains("api.moonshot.");
    let is_together = p == "togetherai" || p == "together" || url.contains("api.together.");
    let is_cloudflare_gateway =
        p == "cloudflare-ai-gateway" || url.contains("gateway.ai.cloudflare.com");
    let is_nvidia = p == "nvidia" || url.contains("integrate.api.nvidia.com");
    !(is_moonshot || is_together || is_cloudflare_gateway || is_nvidia)
}

fn detect_zai_tool_stream(provider: &str, base_url: Option<&str>, model_id: &str) -> bool {
    let p = provider.to_ascii_lowercase();
    let url = base_url.unwrap_or_default().to_ascii_lowercase();
    let is_zai = ["zai", "zai-coding-plan", "zhipuai", "zhipuai-coding-plan"].contains(&p.as_str())
        || url.contains("api.z.ai")
        || url.contains("open.bigmodel.cn");
    if !is_zai {
        return false;
    }
    let id = model_id.to_ascii_lowercase();
    !["glm-4.5", "glm-4.5-air", "glm-4.5-flash", "glm-4.5v"].contains(&id.as_str())
}

/// The two Chat-level provider options consumed by the base body (port of the
/// relevant slice of `OpenAIOptions.resolve`; malformed values are dropped).
struct ChatOptions {
    store: Option<bool>,
    reasoning_effort: Option<String>,
}

fn resolve_chat_options(request: &LLMRequest) -> ChatOptions {
    let options = request.provider_options.as_ref();
    ChatOptions {
        store: options.and_then(|options| options.get("store")).and_then(Value::as_bool),
        reasoning_effort: options
            .and_then(|options| options.get("reasoningEffort"))
            .and_then(Value::as_str)
            .filter(|effort| !effort.is_empty())
            .map(str::to_string),
    }
}

/// Lowering context threaded through message building.
struct Lowering<'a> {
    breakpoints: Option<Breakpoints>,
    provider_metadata_key: &'a str,
    model_id_lower: String,
    provider: &'a str,
    mistral: bool,
    assistant: Option<AssistantOverlay>,
}

impl Lowering<'_> {
    fn cache_control(&mut self, cache: Option<&CacheHint>) -> Option<Value> {
        self.breakpoints.as_mut()?.cache_control(cache)
    }

    /// Tool call id shaping per model family (port of `lowering.toolCallID`).
    fn tool_call_id(&self, id: &str) -> String {
        if self.mistral {
            let cleaned: String = id.chars().filter(char::is_ascii_alphanumeric).collect();
            let sliced: String = cleaned.chars().take(9).collect();
            return format!("{sliced:0<9}");
        }
        if self.model_id_lower.contains("claude") {
            return id
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
                .collect();
        }
        if self.provider == "openai"
            || self.provider == "azure"
            || self.model_id_lower.starts_with("openai/")
        {
            return id.chars().take(40).collect();
        }
        id.to_string()
    }
}

fn require_reasoning_flag(request: &LLMRequest) -> bool {
    let compatibility = request.model.compatibility.as_ref();
    compatibility.and_then(|c| c.require_reasoning).unwrap_or_else(|| {
        let base_url = request
            .model
            .route
            .endpoint
            .base_url
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let model_id = request.model.id.0.to_ascii_lowercase();
        compatibility.is_some_and(|c| c.reasoning_field.is_some())
            || request.model.provider.0 == "deepseek"
            || base_url.contains("deepseek.com")
            || model_id.contains("deepseek")
    })
}

/// Port of `lowerTool`.
fn lower_tool(tool: &ToolDefinition, lowering: &mut Lowering, supports_strict_mode: bool) -> Value {
    let mut function: Map<String, Value> = [
        ("name", Value::String(tool.name.clone())),
        ("description", Value::String(tool.description.clone())),
        ("parameters", tool.input_schema.clone()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    if supports_strict_mode {
        function.insert("strict".to_string(), Value::Bool(false));
    }
    let mut lowered: Map<String, Value> =
        [("type", Value::String("function".into())), ("function", Value::Object(function))]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
    if let Some(cache_control) = lowering.cache_control(tool.cache.as_ref()) {
        lowered.insert("cache_control".to_string(), cache_control);
    }
    Value::Object(lowered)
}

fn lower_tool_choice(tool_choice: &ToolChoice) -> Result<Value, AiError> {
    match tool_choice {
        ToolChoice::Auto { .. } => Ok(json!("auto")),
        ToolChoice::None { .. } => Ok(json!("none")),
        ToolChoice::Required { .. } => Ok(json!("required")),
        ToolChoice::Named { name, .. } => {
            if name.is_empty() {
                return Err(shared::invalid_request(
                    "OpenAI Chat tool choice requires a tool name",
                ));
            }
            Ok(json!({"type": "function", "function": {"name": name}}))
        }
    }
}

/// Port of `toolResultText`.
fn tool_result_text(result: &crate::schema::ToolResultValue) -> String {
    match result {
        crate::schema::ToolResultValue::Text { value } => js_string(value),
        crate::schema::ToolResultValue::Error { value } => {
            let structured = matches!(value, Value::Array(_) | Value::Object(_));
            if structured {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                js_string(value)
            }
        }
        crate::schema::ToolResultValue::Json { value } => {
            serde_json::to_string(value).unwrap_or_default()
        }
        crate::schema::ToolResultValue::Content { .. } => String::new(),
    }
}

/// JS `String(value)` for JSON scalars.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(bool) => bool.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// Port of `lowerMedia`: PDFs lower as inline `file` parts; images as
/// `image_url` (URL sources pass through, everything else inlines); other
/// media kinds are rejected.
fn lower_media(
    media: &crate::schema::media::Asset,
    filename: Option<&str>,
) -> Result<Value, AiError> {
    if media.media_type.eq_ignore_ascii_case("application/pdf") {
        let inline = media.inline().ok_or_else(|| {
            shared::invalid_request(format!(
                "OpenAI Chat requires inline media (bytes or base64); {} sources must be materialized first",
                media_kind_label(media)
            ))
        })?;
        return Ok(json!({
            "type": "file",
            "file": {
                "filename": filename.unwrap_or("document.pdf"),
                "file_data": inline.data_url,
            }
        }));
    }
    if media.kind() != crate::schema::media::Kind::Image {
        return Err(shared::invalid_request(format!(
            "OpenAI Chat does not support media type {}",
            media.media_type
        )));
    }
    let url = match &media.source {
        crate::schema::media::Source::Url { url, .. } => url.clone(),
        _ => media
            .inline()
            .ok_or_else(|| {
                shared::invalid_request(format!(
                    "OpenAI Chat requires inline media (bytes or base64); {} sources must be materialized first",
                    media_kind_label(media)
                ))
            })?
            .data_url,
    };
    Ok(json!({"type": "image_url", "image_url": {"url": url}}))
}

fn media_kind_label(media: &crate::schema::media::Asset) -> &'static str {
    match media.source {
        crate::schema::media::Source::Bytes { .. }
        | crate::schema::media::Source::Base64 { .. } => "inline",
        crate::schema::media::Source::Url { .. } => "url",
        crate::schema::media::Source::Ref { .. } => "ref",
    }
}

/// Port of `toolFileMedia`: tool files carry a data URL, an http(s) URL, or
/// raw base64; the declared mime wins over any data-URL prefix.
fn tool_file_media(item: &Value) -> (crate::schema::media::Asset, Option<String>) {
    let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
    let mime =
        item.get("mime").and_then(Value::as_str).unwrap_or("application/octet-stream").to_string();
    let name = item.get("name").and_then(Value::as_str).map(str::to_string);
    let asset = if let Some((media_type, data)) = parse_data_url(uri) {
        media_asset(
            crate::schema::media::Source::Base64 { data, media_type: mime.clone() },
            if media_type.is_empty() { mime.clone() } else { media_type },
        )
    } else if uri.starts_with("http://") || uri.starts_with("https://") {
        media_asset(
            crate::schema::media::Source::Url {
                url: uri.to_string(),
                media_type: Some(mime.clone()),
                expires_at: None,
            },
            mime.clone(),
        )
    } else {
        media_asset(
            crate::schema::media::Source::Base64 {
                data: uri.to_string(),
                media_type: mime.clone(),
            },
            mime.clone(),
        )
    };
    (asset, name)
}

fn media_asset(
    source: crate::schema::media::Source,
    media_type: String,
) -> crate::schema::media::Asset {
    crate::schema::media::Asset {
        source,
        media_type,
        info: None,
        expires_at: None,
        provider_metadata: None,
    }
}

fn parse_data_url(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    let media_type = header.strip_suffix(";base64").unwrap_or(header);
    Some((media_type.to_string(), data.to_string()))
}

/// Scalar reasoning carried by a `native.openaiCompatible` message.
fn native_reasoning_content(message: &Message) -> Option<String> {
    message
        .native
        .as_ref()?
        .get("openaiCompatible")?
        .get("reasoning_content")?
        .as_str()
        .map(str::to_string)
}

fn native_reasoning_details(message: &Message) -> Option<Vec<Value>> {
    message.native.as_ref()?.get("openaiCompatible")?.get("reasoning_details")?.as_array().cloned()
}

fn part_reasoning_details(part: &ContentPart, key: &str) -> Option<Vec<Value>> {
    part_provider_metadata(part)?.get(key)?.get("reasoningDetails")?.as_array().cloned()
}

fn part_reasoning_field(part: &ContentPart, key: &str) -> Option<String> {
    part_provider_metadata(part)?.get(key)?.get("reasoningField")?.as_str().map(str::to_string)
}

/// Port of `lowerReasoningDetail`: Kimi rejects its stream-only `index`.
fn lower_reasoning_detail(detail: &Value) -> Value {
    match detail_type(detail) {
        Some("summary") => json!({
            "type": "summary",
            "summary": detail.get("summary").cloned().unwrap_or(Value::Null),
        }),
        Some("encrypted") => json!({
            "type": "encrypted",
            "encrypted": detail.get("encrypted").cloned().unwrap_or(Value::Null),
        }),
        _ => detail.clone(),
    }
}

fn is_kimi_detail(detail: &Value) -> bool {
    matches!(detail_type(detail), Some("summary" | "encrypted"))
}

/// Port of `lowerAssistantMessage` (reasoning field selection included).
fn lower_assistant_message(
    message: &Message,
    configured_field: Option<&str>,
    require_reasoning: bool,
    lowering: &mut Lowering,
) -> Result<Value, AiError> {
    let mut content: Vec<Value> = Vec::new();
    let mut reasoning_parts: Vec<&ContentPart> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text, cache, .. } => {
                let mut lowered: Map<String, Value> =
                    [("type", Value::String("text".into())), ("text", Value::String(text.clone()))]
                        .into_iter()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect();
                if let Some(cache_control) = lowering.cache_control(cache.as_ref()) {
                    lowered.insert("cache_control".to_string(), cache_control);
                }
                content.push(Value::Object(lowered));
            }
            ContentPart::Reasoning { .. } => reasoning_parts.push(part),
            ContentPart::ToolCall { .. } => {
                tool_calls.push(lower_tool_call(part, lowering));
            }
            other => {
                return Err(shared::invalid_request(format!(
                    "OpenAI Chat assistant messages only support text, reasoning, and tool-call content for now (got {})",
                    other.kind()
                )));
            }
        }
    }
    let text: String = reasoning_parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Reasoning { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let key = lowering.provider_metadata_key;
    let any_details =
        reasoning_parts.iter().any(|part| part_reasoning_details(part, key).is_some());
    let details: Option<Vec<Value>> = if any_details {
        Some(
            reasoning_parts
                .iter()
                .filter_map(|part| part_reasoning_details(part, key))
                .flatten()
                .filter(valid_reasoning_detail)
                .map(|detail| lower_reasoning_detail(&detail))
                .collect(),
        )
    } else {
        native_reasoning_details(message).map(|details| {
            details
                .iter()
                .filter(|detail| valid_reasoning_detail(detail))
                .map(lower_reasoning_detail)
                .collect()
        })
    };
    let observed_field = reasoning_parts.iter().find_map(|part| part_reasoning_field(part, key));
    let native_reasoning = native_reasoning_content(message);
    let fully_structured =
        reasoning_parts.iter().all(|part| part_reasoning_details(part, key).is_some());
    let field: Option<String> = if configured_field.is_some()
        && (require_reasoning || !reasoning_parts.is_empty() || native_reasoning.is_some())
    {
        configured_field.map(str::to_string)
    } else if reasoning_parts.is_empty() {
        require_reasoning.then(|| "reasoning_content".to_string())
    } else if let Some(observed) = observed_field {
        Some(observed)
    } else if native_reasoning.is_some() || !fully_structured || require_reasoning {
        Some("reasoning_content".to_string())
    } else if details.as_ref().is_some_and(|details| details.iter().any(is_kimi_detail)) {
        // Kimi always expects `reasoning_content` on replayed assistant
        // messages, even when thinking arrived only through structured details.
        Some("reasoning_content".to_string())
    } else {
        None
    };
    let reasoning_text: Option<String> = if configured_field.is_some() {
        if reasoning_parts.is_empty() {
            native_reasoning.clone().or(if require_reasoning { Some(String::new()) } else { None })
        } else {
            Some(text.clone())
        }
    } else if reasoning_parts.is_empty() {
        native_reasoning.clone().or(if require_reasoning { Some(String::new()) } else { None })
    } else {
        Some(text.clone())
    };

    let content_value = if content.iter().any(|part| part.get("cache_control").is_some()) {
        Value::Array(content.clone())
    } else if content.is_empty() && !tool_calls.is_empty() {
        Value::Null
    } else {
        Value::String(
            content
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
        )
    };
    let mut lowered: Map<String, Value> =
        [("role", Value::String("assistant".into())), ("content", content_value)]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
    if !tool_calls.is_empty() {
        lowered.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    if let Some(details) = details {
        lowered.insert("reasoning_details".to_string(), Value::Array(details));
    }
    if let (Some(field), Some(reasoning_text)) = (&field, reasoning_text) {
        lowered.insert(field.clone(), Value::String(reasoning_text));
    }
    if let Some(assistant) = lowering.assistant {
        assistant(message, &mut lowered);
    }
    Ok(Value::Object(lowered))
}

fn lower_tool_call(part: &ContentPart, lowering: &Lowering) -> Value {
    let ContentPart::ToolCall { id, name, input, provider_metadata, .. } = part else {
        return Value::Null;
    };
    let mut lowered: Map<String, Value> = [
        ("id", Value::String(lowering.tool_call_id(id))),
        ("type", Value::String("function".into())),
        (
            "function",
            json!({"name": name, "arguments": serde_json::to_string(input).unwrap_or_default()}),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    let extra = provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.get(lowering.provider_metadata_key))
        .and_then(|inner| inner.get("extraContent"));
    if let Some(extra) = decode_extra_content(extra) {
        lowered.insert("extra_content".to_string(), extra);
    }
    Value::Object(lowered)
}

/// Port of `lowerUserMessage`.
fn lower_user_message(message: &Message, lowering: &mut Lowering) -> Result<Value, AiError> {
    let mut content: Vec<Value> = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text, cache, .. } => {
                let mut lowered: Map<String, Value> =
                    [("type", Value::String("text".into())), ("text", Value::String(text.clone()))]
                        .into_iter()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect();
                if let Some(cache_control) = lowering.cache_control(cache.as_ref()) {
                    lowered.insert("cache_control".to_string(), cache_control);
                }
                content.push(Value::Object(lowered));
            }
            ContentPart::Media { media, filename, .. } => {
                content.push(lower_media(media, filename.as_deref())?);
            }
            other => {
                return Err(shared::invalid_request(format!(
                    "OpenAI Chat user messages only support text and media content for now (got {})",
                    other.kind()
                )));
            }
        }
    }
    let all_plain_text = content.iter().all(|part| {
        part.get("type").and_then(Value::as_str) == Some("text")
            && part.get("cache_control").is_none()
    });
    if all_plain_text {
        return Ok(json!({
            "role": "user",
            "content": content
                .iter()
                .map(|part| part.get("text").and_then(Value::as_str).unwrap_or_default())
                .collect::<String>(),
        }));
    }
    Ok(json!({"role": "user", "content": content}))
}

/// Port of `toolMessage` (cache breakpoints belong on text content parts).
fn tool_message(tool_call_id: &str, text: &str, cache_control: Option<Value>) -> Value {
    match cache_control {
        None => json!({"role": "tool", "tool_call_id": tool_call_id, "content": text}),
        Some(cache_control) => json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": [{"type": "text", "text": text, "cache_control": cache_control}],
        }),
    }
}

struct LoweredToolMessages {
    messages: Vec<Value>,
    attachments: Vec<Value>,
}

/// Port of `lowerToolMessages`: content results keep their text; file items
/// become follow-up vision/document attachments.
fn lower_tool_messages(
    message: &Message,
    lowering: &mut Lowering,
) -> Result<LoweredToolMessages, AiError> {
    let mut lowered = LoweredToolMessages { messages: Vec::new(), attachments: Vec::new() };
    for part in &message.content {
        let ContentPart::ToolResult { id, result, cache, .. } = part else {
            return Err(shared::invalid_request(
                "OpenAI Chat tool messages only support tool-result content for now",
            ));
        };
        let cache_control = lowering.cache_control(cache.as_ref());
        if !matches!(result, crate::schema::ToolResultValue::Content { .. }) {
            lowered.messages.push(tool_message(
                &lowering.tool_call_id(id),
                &tool_result_text(result),
                cache_control,
            ));
            continue;
        }
        let crate::schema::ToolResultValue::Content { value } = result else {
            unreachable!("guarded above");
        };
        let text = value
            .0
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        lowered.messages.push(tool_message(&lowering.tool_call_id(id), &text, cache_control));
        for item in
            value.0.iter().filter(|item| item.get("type").and_then(Value::as_str) == Some("file"))
        {
            let (media, filename) = tool_file_media(item);
            lowered.attachments.push(lower_media(&media, filename.as_deref())?);
        }
    }
    Ok(lowered)
}

/// Port of `lowerMessages`: system array, chronological system-update
/// wrapping/merging, attachment flushing, and assistant bridging.
fn lower_messages(request: &LLMRequest, lowering: &mut Lowering) -> Result<Vec<Value>, AiError> {
    let mut messages: Vec<Value> = Vec::new();
    let has_cached_system = request.system.iter().any(|part| part.cache.is_some());
    if !request.system.is_empty() {
        if has_cached_system && lowering.breakpoints.is_some() {
            let content = request
                .system
                .iter()
                .map(|part| {
                    let mut lowered: Map<String, Value> = [
                        ("type", Value::String("text".into())),
                        ("text", Value::String(part.text.clone())),
                    ]
                    .into_iter()
                    .map(|(key, value)| (key.to_string(), value))
                    .collect();
                    if let Some(cache_control) = lowering.cache_control(part.cache.as_ref()) {
                        lowered.insert("cache_control".to_string(), cache_control);
                    }
                    Value::Object(lowered)
                })
                .collect::<Vec<_>>();
            messages.push(json!({"role": "system", "content": content}));
        } else {
            messages.push(json!({
                "role": "system",
                "content": shared::join_text(request.system.iter().map(|part| part.text.as_str())),
            }));
        }
    }
    let compatibility = request.model.compatibility.as_ref();
    let require_reasoning = require_reasoning_flag(request);
    let configured_field = compatibility.and_then(|c| c.reasoning_field.as_deref());
    let require_assistant_after_tool =
        compatibility.and_then(|c| c.require_assistant_after_tool).unwrap_or(lowering.mistral);

    let bridge_tools = |messages: &mut Vec<Value>| {
        if require_assistant_after_tool
            && messages.last().and_then(|message| message.get("role")).and_then(Value::as_str)
                == Some("tool")
        {
            messages.push(json!({"role": "assistant", "content": "Done."}));
        }
    };

    let mut pending_attachments: Vec<Value> = Vec::new();
    let flush_attachments = |messages: &mut Vec<Value>, pending: &mut Vec<Value>| {
        if pending.is_empty() {
            return;
        }
        bridge_tools(messages);
        messages.push(json!({"role": "user", "content": pending.clone()}));
        pending.clear();
    };

    for message in &request.messages {
        match message.role {
            MessageRole::User => bridge_tools(&mut messages),
            MessageRole::System => {
                let wrapped = wrapped_system_update(message);
                let cache_control = lowering.cache_control(last_cache_hint(message));
                if !pending_attachments.is_empty() {
                    let mut content = std::mem::take(&mut pending_attachments);
                    content.push(match &cache_control {
                        Some(cache_control) => json!({
                            "type": "text",
                            "text": wrapped,
                            "cache_control": cache_control,
                        }),
                        None => json!({"type": "text", "text": wrapped}),
                    });
                    messages.push(json!({"role": "user", "content": content}));
                    continue;
                }
                let previous = messages.last().cloned();
                match previous {
                    Some(previous)
                        if previous.get("role").and_then(Value::as_str) == Some("user")
                            && previous.get("content").is_some_and(Value::is_string) =>
                    {
                        let previous_text =
                            previous.get("content").and_then(Value::as_str).unwrap_or_default();
                        let merged = match &cache_control {
                            Some(cache_control) => json!({
                                "role": "user",
                                "content": [
                                    {"type": "text", "text": previous_text},
                                    {"type": "text", "text": wrapped, "cache_control": cache_control},
                                ],
                            }),
                            None => json!({
                                "role": "user",
                                "content": format!("{previous_text}\n{wrapped}"),
                            }),
                        };
                        let length = messages.len();
                        messages[length - 1] = merged;
                    }
                    Some(previous)
                        if previous.get("role").and_then(Value::as_str) == Some("user")
                            && previous.get("content").is_some_and(Value::is_array) =>
                    {
                        let mut content = previous
                            .get("content")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        content.push(match &cache_control {
                            Some(cache_control) => json!({
                                "type": "text",
                                "text": wrapped,
                                "cache_control": cache_control,
                            }),
                            None => json!({"type": "text", "text": wrapped}),
                        });
                        let length = messages.len();
                        messages[length - 1] = json!({"role": "user", "content": content});
                    }
                    _ => messages.push(match &cache_control {
                        Some(cache_control) => json!({
                            "role": "user",
                            "content": [{"type": "text", "text": wrapped, "cache_control": cache_control}],
                        }),
                        None => json!({"role": "user", "content": wrapped}),
                    }),
                }
                continue;
            }
            MessageRole::Assistant
                if message
                    .content
                    .iter()
                    .all(|part| part_text(part).is_some_and(|text| text.trim().is_empty())) =>
            {
                continue;
            }
            MessageRole::Tool => {
                let lowered = lower_tool_messages(message, lowering)?;
                messages.extend(lowered.messages);
                pending_attachments.extend(lowered.attachments);
                continue;
            }
            MessageRole::Assistant => {}
        }
        flush_attachments(&mut messages, &mut pending_attachments);
        match message.role {
            MessageRole::User => messages.push(lower_user_message(message, lowering)?),
            MessageRole::Assistant => messages.push(lower_assistant_message(
                message,
                configured_field,
                require_reasoning,
                lowering,
            )?),
            MessageRole::System | MessageRole::Tool => unreachable!("handled above"),
        }
    }
    flush_attachments(&mut messages, &mut pending_attachments);
    Ok(messages)
}

fn last_cache_hint(message: &Message) -> Option<&CacheHint> {
    message.content.last().and_then(|part| part.cache())
}

/// Port of `wrappedSystemUpdate` (text-only; last part carries the cache hint).
fn wrapped_system_update(message: &Message) -> String {
    let texts = message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    shared::wrap_system_update(texts.iter().map(String::as_str))
}

/// Port of `OpenAIChat.fromRequest`: build the provider-native Chat body from
/// a common request.
pub fn from_request(request: &LLMRequest, ext: &OpenAiChatExt) -> Result<Value, AiError> {
    let compatibility = request.model.compatibility.as_ref();
    if let Some(field) = compatibility.and_then(|c| c.reasoning_field.as_deref()) {
        if RESERVED_REASONING_FIELDS.contains(&field) {
            return Err(shared::invalid_request(format!(
                "OpenAI Chat reasoning field conflicts with reserved field {field}"
            )));
        }
    }
    let generation = request.generation.as_ref();
    let flattened = flatten_tool_request(request);
    let provider = request.model.provider.0.as_str();
    let base_url = request.model.route.endpoint.base_url.as_deref();
    let max_tokens_field = compatibility
        .and_then(|c| c.max_tokens_field)
        .unwrap_or_else(|| detect_max_tokens_field(provider, base_url));
    let supports_store = compatibility
        .and_then(|c| c.supports_store)
        .unwrap_or_else(|| detect_supports_store(provider, base_url));
    let supports_usage_in_streaming =
        compatibility.and_then(|c| c.supports_usage_in_streaming).unwrap_or(true);
    let supports_strict_mode = compatibility
        .and_then(|c| c.supports_strict_mode)
        .unwrap_or_else(|| detect_supports_strict_mode(provider, base_url));
    let zai_tool_stream = compatibility
        .and_then(|c| c.zai_tool_stream)
        .unwrap_or_else(|| detect_zai_tool_stream(provider, base_url, &request.model.id.0));
    let has_history = has_tool_history(&flattened.request.messages);
    let has_active_tools = !flattened.tools.is_empty();

    let model_id_lower = request.model.id.0.to_ascii_lowercase();
    let mistral = ["mistral", "devstral", "codestral", "pixtral", "mixtral"]
        .iter()
        .any(|family| model_id_lower.contains(family));
    let provider_metadata_key = request
        .model
        .route
        .provider_metadata_key
        .clone()
        .unwrap_or_else(|| request.model.provider.0.clone());
    let mut lowering = Lowering {
        breakpoints: ext.cache_control.then(Breakpoints::new),
        provider_metadata_key: &provider_metadata_key,
        model_id_lower,
        provider,
        mistral,
        assistant: ext.assistant,
    };
    let messages = lower_messages(&flattened.request, &mut lowering)?;

    let options = resolve_chat_options(request);
    let cache_key = compatibility
        .and_then(|c| c.supports_prompt_cache_key)
        .unwrap_or(false)
        .then(|| prompt_cache_key(request))
        .flatten();

    let mut body: Map<String, Value> = [
        ("model", Value::String(request.model.id.0.clone())),
        ("messages", Value::Array(messages)),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    if has_active_tools {
        let tools = flattened
            .tools
            .iter()
            .map(|tool| lower_tool(tool, &mut lowering, supports_strict_mode))
            .collect::<Vec<_>>();
        body.insert("tools".to_string(), Value::Array(tools));
        if let Some(tool_choice) = &request.tool_choice {
            body.insert("tool_choice".to_string(), lower_tool_choice(tool_choice)?);
        }
    } else if has_history {
        // Anthropic via LiteLLM and Bedrock require `tools` to be present
        // whenever history contains tool calls/results.
        body.insert("tools".to_string(), Value::Array(Vec::new()));
    }
    body.insert("stream".to_string(), Value::Bool(true));
    if supports_usage_in_streaming {
        body.insert("stream_options".to_string(), json!({"include_usage": true}));
    }
    if zai_tool_stream && has_active_tools {
        body.insert("tool_stream".to_string(), Value::Bool(true));
    }
    match max_tokens_field {
        MaxTokensFieldCompatibility::MaxCompletionTokens => {
            body.insert(
                "max_completion_tokens".to_string(),
                generation.and_then(|g| g.max_tokens).map_or(Value::Null, Value::from),
            );
        }
        MaxTokensFieldCompatibility::MaxTokens => {
            body.insert(
                "max_tokens".to_string(),
                generation.and_then(|g| g.max_tokens).map_or(Value::Null, Value::from),
            );
        }
    }
    for (key, value) in [
        ("temperature", generation.and_then(|g| g.temperature)),
        ("top_p", generation.and_then(|g| g.top_p)),
        ("frequency_penalty", generation.and_then(|g| g.frequency_penalty)),
        ("presence_penalty", generation.and_then(|g| g.presence_penalty)),
        ("seed", generation.and_then(|g| g.seed)),
    ] {
        if let Some(value) = value {
            body.insert(key.to_string(), Value::from(value));
        }
    }
    if let Some(stop) = generation.and_then(|g| g.stop.clone()) {
        body.insert(
            "stop".to_string(),
            Value::Array(stop.into_iter().map(Value::String).collect()),
        );
    }
    if supports_store {
        body.insert("store".to_string(), options.store.map_or(Value::Bool(false), Value::Bool));
    }
    if let Some(cache_key) = cache_key {
        body.insert("prompt_cache_key".to_string(), Value::String(cache_key));
    }
    if let Some(effort) = options.reasoning_effort {
        body.insert("reasoning_effort".to_string(), Value::String(effort));
    }
    if let Some(overlay) = ext.body {
        overlay(request, &mut body);
    }
    // `undefined`-valued spreads in TS collapse to absent keys.
    body.retain(|_, value| !value.is_null());
    Ok(Value::Object(body))
}

// =============================================================================
// Provider body overlays (ports of the provider `fromRequest` extensions)
// =============================================================================

fn provider_options(request: &LLMRequest) -> Option<&ProviderOptions> {
    request.provider_options.as_ref()
}

fn option_bool<'a>(options: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    options.get(key).filter(|value| value.is_boolean())
}

fn option_string(options: &Map<String, Value>, key: &str) -> Option<String> {
    options.get(key).and_then(Value::as_str).map(str::to_string)
}

fn option_object(options: &Map<String, Value>, key: &str) -> Option<Map<String, Value>> {
    options.get(key).and_then(Value::as_object).cloned()
}

fn option_array(options: &Map<String, Value>, key: &str) -> Option<Vec<Value>> {
    options.get(key).and_then(Value::as_array).cloned()
}

/// Groq (port of `providers/groq.ts` `fromRequest`): `reasoning_format:
/// "parsed"` except on GPT-OSS models, where `include_reasoning` applies.
fn groq_body_overlay(request: &LLMRequest, body: &mut Map<String, Value>) {
    let Some(options) = provider_options(request) else {
        body.insert("reasoning_format".into(), json!("parsed"));
        return;
    };
    let gpt_oss = request.model.id.0.starts_with("openai/gpt-oss-");
    if gpt_oss {
        if let Some(include) = option_bool(options, "includeReasoning") {
            body.insert("include_reasoning".into(), include.clone());
        }
    } else {
        body.insert("reasoning_format".into(), json!("parsed"));
    }
    if let Some(parallel) = option_bool(options, "parallelToolCalls") {
        body.insert("parallel_tool_calls".into(), parallel.clone());
    }
    if let Some(service_tier) = option_string(options, "serviceTier") {
        body.insert("service_tier".into(), Value::String(service_tier));
    }
    if let Some(user) = option_string(options, "user") {
        body.insert("user".into(), Value::String(user));
    }
}

/// OpenRouter (port of `providers/openrouter.ts`): replay reasoning through
/// the `reasoning` field only when structured details exist, and merge the
/// routing/usage provider options.
fn openrouter_body_overlay(request: &LLMRequest, body: &mut Map<String, Value>) {
    let source_assistants: Vec<&Message> = request
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .collect();
    if let Some(Value::Array(messages)) = body.get_mut("messages") {
        let mut assistant_index = 0usize;
        for message in messages.iter_mut() {
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let source = source_assistants.get(assistant_index).copied();
            assistant_index += 1;
            let reasoning = source
                .map(|source| {
                    source
                        .content
                        .iter()
                        .filter_map(|part| match part {
                            ContentPart::Reasoning { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<String>()
                })
                .unwrap_or_default();
            let reasoning_details =
                message.get("reasoning_details").and_then(Value::as_array).cloned();
            let Some(object) = message.as_object_mut() else { continue };
            object.remove("reasoning_content");
            object.remove("reasoning_text");
            let keep = !reasoning.is_empty()
                && reasoning_details.as_ref().is_some_and(|details| !details.is_empty());
            if keep {
                object.insert("reasoning".into(), Value::String(reasoning));
            } else {
                object.remove("reasoning");
            }
        }
    }
    let options = provider_options(request).cloned().unwrap_or_default();
    let handled = [
        "usage",
        "models",
        "provider",
        "plugins",
        "web_search_options",
        "debug",
        "user",
        "reasoning",
        "promptCacheKey",
    ];
    for (key, value) in &options {
        if !handled.contains(&key.as_str()) {
            body.insert(key.clone(), value.clone());
        }
    }
    match options.get("usage") {
        Some(Value::Bool(true)) | None => {
            body.insert("usage".into(), json!({"include": true}));
        }
        Some(Value::Bool(false)) => {
            body.insert("usage".into(), json!({"include": false}));
        }
        Some(usage) if usage.is_object() => {
            body.insert("usage".into(), usage.clone());
        }
        Some(_) => {}
    }
    if let Some(models) = option_array(&options, "models") {
        body.insert("models".into(), Value::Array(models));
    }
    if let Some(provider) = option_object(&options, "provider") {
        body.insert("provider".into(), Value::Object(provider));
    }
    if let Some(plugins) = option_array(&options, "plugins") {
        body.insert("plugins".into(), Value::Array(plugins));
    }
    if let Some(search) = option_object(&options, "web_search_options") {
        body.insert("web_search_options".into(), Value::Object(search));
    }
    if let Some(debug) = option_object(&options, "debug") {
        body.insert("debug".into(), Value::Object(debug));
    }
    if let Some(user) = option_string(&options, "user") {
        body.insert("user".into(), Value::String(user));
    }
    if let Some(reasoning) = option_object(&options, "reasoning") {
        // Upstreams such as Anthropic and Alibaba reject a thinking budget
        // that is not below the output limit; 1,024 is Anthropic's minimum.
        let reasoning = match reasoning.get("max_tokens").and_then(Value::as_f64) {
            Some(max_tokens) => {
                let max = request.generation.as_ref().and_then(|g| g.max_tokens);
                let mut fitted = reasoning.clone();
                fitted.insert(
                    "max_tokens".into(),
                    Value::from(fit_thinking_budget(max_tokens, max, 1024.0)),
                );
                fitted
            }
            None => reasoning,
        };
        body.insert("reasoning".into(), Value::Object(reasoning));
    }
}

/// Tool streaming arrived with GLM-4.6; later versions inherit support (port
/// of `zai-chat.ts` `supportsToolStreaming`).
fn supports_tool_streaming(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut major: Option<u32> = None;
    let mut minor: Option<u32> = None;
    while let Some(position) = rest.find("glm-") {
        let after = &rest[position + 4..];
        let is_boundary = position == 0 || rest.as_bytes()[position - 1] == b'/';
        if is_boundary {
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() {
                major = digits.parse().ok();
                let tail = &after[digits.len()..];
                if let Some(decimal) = tail.strip_prefix('.') {
                    let minor_digits: String =
                        decimal.chars().take_while(char::is_ascii_digit).collect();
                    minor = minor_digits.parse().ok();
                    if !minor_digits.is_empty() {
                        let after_minor = &decimal[minor_digits.len()..];
                        if after_minor.starts_with('-') || after_minor.is_empty() {
                            break;
                        }
                    }
                } else if tail.starts_with('-') || tail.is_empty() {
                    break;
                }
            }
        }
        rest = &rest[position + 1..];
    }
    match (major, minor) {
        (Some(major), minor) => major > 4 || (major == 4 && minor.unwrap_or(0) >= 6),
        (None, _) => false,
    }
}

/// Zai (port of `protocols/zai-chat.ts`): thinking toggle + tool streaming.
fn zai_body_overlay(request: &LLMRequest, body: &mut Map<String, Value>) {
    let options = provider_options(request).cloned().unwrap_or_default();
    if let Some(thinking) = option_object(&options, "thinking") {
        body.insert("thinking".into(), Value::Object(thinking));
    }
    let tool_stream = option_bool(&options, "toolStream").cloned().or(
        match body.get("tools").and_then(Value::as_array) {
            Some(tools) if !tools.is_empty() && supports_tool_streaming(&request.model.id.0) => {
                Some(Value::Bool(true))
            }
            _ => None,
        },
    );
    if let Some(tool_stream) = tool_stream {
        body.insert("tool_stream".into(), tool_stream);
    }
    if let Some(do_sample) = option_bool(&options, "doSample") {
        body.insert("do_sample".into(), do_sample.clone());
    }
    if let Some(response_format) = option_object(&options, "responseFormat") {
        body.insert("response_format".into(), Value::Object(response_format));
    }
    if let Some(request_id) = option_string(&options, "requestID") {
        body.insert("request_id".into(), Value::String(request_id));
    }
    if let Some(user_id) = option_string(&options, "userID") {
        body.insert("user_id".into(), Value::String(user_id));
    }
}

/// Alibaba (port of `protocols/alibaba-chat.ts`): DashScope thinking knobs,
/// search options, and `parallel_tool_calls` mirroring the tool choice.
fn alibaba_body_overlay(request: &LLMRequest, body: &mut Map<String, Value>) {
    let options = provider_options(request).cloned().unwrap_or_default();
    if let Some(enable) = option_bool(&options, "enableThinking") {
        body.insert("enable_thinking".into(), enable.clone());
    }
    if let Some(budget) = options.get("thinkingBudget").and_then(Value::as_f64) {
        let max = request.generation.as_ref().and_then(|g| g.max_tokens);
        body.insert("thinking_budget".into(), Value::from(fit_thinking_budget(budget, max, 1.0)));
    }
    if let Some(preserve) = option_bool(&options, "preserveThinking") {
        body.insert("preserve_thinking".into(), preserve.clone());
    }
    if let Some(clear) = option_bool(&options, "clearThinking") {
        body.insert("clear_thinking".into(), clear.clone());
    }
    if let Some(thinking) = option_object(&options, "thinking") {
        body.insert("thinking".into(), Value::Object(thinking));
    }
    if let Some(tool_stream) = option_bool(&options, "toolStream") {
        body.insert("tool_stream".into(), tool_stream.clone());
    }
    let parallel_tool_calls =
        option_bool(&options, "parallelToolCalls").cloned().or(match &request.tool_choice {
            Some(choice) => choice_disable_parallel(choice).map(|disable| Value::Bool(!disable)),
            None => None,
        });
    if let Some(parallel) = parallel_tool_calls {
        body.insert("parallel_tool_calls".into(), parallel);
    }
    if let Some(penalty) = options.get("repetitionPenalty").and_then(Value::as_f64) {
        body.insert("repetition_penalty".into(), Value::from(penalty));
    }
    if let Some(top_k) = request.generation.as_ref().and_then(|g| g.top_k) {
        body.insert("top_k".into(), Value::from(top_k));
    }
    if let Some(response_format) = option_object(&options, "responseFormat") {
        body.insert("response_format".into(), Value::Object(response_format));
    }
    if let Some(enable_search) = option_bool(&options, "enableSearch") {
        body.insert("enable_search".into(), enable_search.clone());
    }
    if let Some(search_options) = option_object(&options, "searchOptions") {
        body.insert("search_options".into(), Value::Object(search_options));
    }
}

fn choice_disable_parallel(choice: &ToolChoice) -> Option<bool> {
    match choice {
        ToolChoice::Auto { disable_parallel_tool_use }
        | ToolChoice::Required { disable_parallel_tool_use }
        | ToolChoice::Named { disable_parallel_tool_use, .. } => *disable_parallel_tool_use,
        ToolChoice::None {} => None,
    }
}

fn camel_to_snake(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for character in key.chars() {
        if character.is_ascii_uppercase() {
            out.push('_');
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// Venice assistant overlay (port of `venice-chat.ts` `assistant`): replay
/// message/tool thought signatures and encrypted reasoning.
fn venice_assistant_overlay(source: &Message, message: &mut Map<String, Value>) {
    let metadata = |part: &ContentPart| {
        part_provider_metadata(part).and_then(|metadata| metadata.get("venice").cloned())
    };
    let parts: Vec<&ContentPart> = source
        .content
        .iter()
        .filter(|part| matches!(part.kind(), "text" | "reasoning" | "tool-call"))
        .collect();
    let signature = parts
        .iter()
        .filter_map(|part| metadata(part))
        .find_map(|metadata| metadata.get("messageThoughtSignature").cloned());
    if let Some(Value::String(signature)) = signature {
        message.insert("thought_signature".into(), Value::String(signature));
    }
    let encrypted = parts
        .iter()
        .filter_map(|part| metadata(part))
        .find_map(|metadata| metadata.get("encryptedReasoningContent").cloned());
    if let Some(Value::String(encrypted)) = encrypted {
        message.insert("reasoning_content".into(), Value::String(encrypted));
    }
    let calls: Vec<&ContentPart> =
        source.content.iter().filter(|part| matches!(part, ContentPart::ToolCall { .. })).collect();
    if let Some(Value::Array(tool_calls)) = message.get_mut("tool_calls") {
        for (index, call) in tool_calls.iter_mut().enumerate() {
            let Some(signature) = calls
                .get(index)
                .and_then(|part| metadata(part))
                .and_then(|metadata| metadata.get("thoughtSignature").cloned())
            else {
                continue;
            };
            if let Some(Value::String(signature)) = Some(signature) {
                if let Some(object) = call.as_object_mut() {
                    object.insert("thought_signature".into(), Value::String(signature));
                }
            }
        }
    }
}

/// Venice body overlay (port of `venice-chat.ts` `fromRequest`).
fn venice_body_overlay(request: &LLMRequest, body: &mut Map<String, Value>) {
    let options = provider_options(request).cloned().unwrap_or_default();
    let max_completion = options
        .get("maxCompletionTokens")
        .and_then(Value::as_f64)
        .or(options.get("maxTokens").and_then(Value::as_f64))
        .or(request.generation.as_ref().and_then(|g| g.max_tokens));
    if let Some(max_completion) = max_completion {
        body.insert("max_completion_tokens".into(), Value::from(max_completion));
    }
    body.remove("reasoning_effort");
    let reasoning = match option_string(&options, "reasoningEffort") {
        Some(effort) => {
            let mut reasoning = option_object(&options, "reasoning").unwrap_or_default();
            reasoning.insert("effort".into(), Value::String(effort));
            reasoning
        }
        None => option_object(&options, "reasoning").unwrap_or_default(),
    };
    if !reasoning.is_empty() {
        body.insert("reasoning".into(), Value::Object(reasoning));
    }
    // Match the previous Venice SDK default, not the gateway's added prompt.
    let mut parameters = option_object(&options, "veniceParameters").unwrap_or_default();
    parameters.insert(
        "includeVeniceSystemPrompt".to_string(),
        parameters.get("includeVeniceSystemPrompt").cloned().unwrap_or(Value::Bool(false)),
    );
    let venice_parameters: Map<String, Value> = parameters
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| (camel_to_snake(&key), value))
        .collect();
    body.insert("venice_parameters".into(), Value::Object(venice_parameters));
    for (option, field) in [
        ("promptCacheRetention", "prompt_cache_retention"),
        ("repetitionPenalty", "repetition_penalty"),
        ("stopTokenIds", "stop_token_ids"),
        ("maxTemp", "max_temp"),
        ("minTemp", "min_temp"),
    ] {
        if let Some(value) = options.get(option).cloned() {
            if !value.is_null() {
                body.insert(field.into(), value);
            }
        }
    }
    if let Some(parallel) = option_bool(&options, "parallelToolCalls") {
        body.insert("parallel_tool_calls".into(), parallel.clone());
    }
    if let Some(min_p) = options.get("minP").and_then(Value::as_f64) {
        body.insert("min_p".into(), Value::from(min_p));
    }
    if let Some(top_k) = request.generation.as_ref().and_then(|g| g.top_k) {
        body.insert("top_k".into(), Value::from(top_k));
    }
    for (option, field) in [("logprobs", "logprobs"), ("topLogprobs", "top_logprobs")] {
        if let Some(value) = option_bool(&options, option) {
            body.insert(field.into(), value.clone());
        }
    }
    if let Some(response_format) = option_object(&options, "responseFormat") {
        body.insert("response_format".into(), Value::Object(response_format));
    }
    if let Some(user) = option_string(&options, "user") {
        body.insert("user".into(), Value::String(user));
    }
}

fn venice_ext() -> OpenAiChatExt {
    OpenAiChatExt {
        id: "venice-chat",
        cache_control: true,
        allow_string_error: true,
        assistant: Some(venice_assistant_overlay),
        body: Some(venice_body_overlay),
    }
}

// =============================================================================
// Route factories + per-provider compatibility presets
// =============================================================================

#[expect(clippy::too_many_arguments, reason = "port of Route.make's parameter set")]
fn chat_route(
    id: &str,
    provider: Option<&str>,
    provider_metadata_key: Option<&str>,
    base_url: Option<&str>,
    protocol: Arc<dyn Protocol>,
    framing: Framing,
    auth: Option<AuthDefinition>,
    transport: SharedTransport,
) -> Route {
    let mut endpoint = Endpoint::path(PATH);
    if let Some(base_url) = base_url {
        endpoint = endpoint.with_base_url(base_url);
    }
    Route::new(RouteInput {
        id: id.to_string(),
        provider: provider.map(str::to_string),
        provider_metadata_key: provider_metadata_key.map(str::to_string),
        protocol,
        endpoint,
        auth,
        framing,
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

/// The native OpenAI Chat route (`OpenAIChat.route`).
pub fn openai_chat_route(transport: SharedTransport) -> Route {
    chat_route(
        ADAPTER,
        Some("openai"),
        Some("openai"),
        Some(DEFAULT_BASE_URL),
        Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        Framing::sse_with_done(),
        None,
        transport,
    )
}

/// DeepSeek (`providers/deepseek.ts`): base protocol, `max_tokens`, no store.
pub fn deepseek_chat_route(transport: SharedTransport, auth: Option<AuthDefinition>) -> Route {
    chat_route(
        "deepseek-chat",
        Some("deepseek"),
        Some("deepseek"),
        Some("https://api.deepseek.com/v1"),
        Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        Framing::sse_with_done(),
        auth,
        transport,
    )
}

/// Groq (`providers/groq.ts`): parsed reasoning + service knobs overlay.
pub fn groq_chat_route(transport: SharedTransport, auth: Option<AuthDefinition>) -> Route {
    let ext = OpenAiChatExt {
        id: "groq-chat",
        cache_control: false,
        allow_string_error: false,
        assistant: None,
        body: Some(groq_body_overlay),
    };
    chat_route(
        "groq-chat",
        Some("groq"),
        Some("groq"),
        Some("https://api.groq.com/openai/v1"),
        Arc::new(OpenAiChat::new(ext)),
        Framing::sse_with_done(),
        auth,
        transport,
    )
}

/// OpenRouter (`providers/openrouter.ts`): reasoning replay + routing options.
pub fn openrouter_chat_route(transport: SharedTransport, auth: Option<AuthDefinition>) -> Route {
    let ext = OpenAiChatExt {
        id: "openrouter-chat",
        cache_control: true,
        allow_string_error: false,
        assistant: None,
        body: Some(openrouter_body_overlay),
    };
    chat_route(
        "openrouter",
        Some("openrouter"),
        Some("openrouter"),
        Some("https://openrouter.ai/api/v1"),
        Arc::new(OpenAiChat::new(ext)),
        Framing::sse_with_done(),
        auth,
        transport,
    )
}

/// Venice (`providers/venice.ts` + `protocols/venice-chat.ts`).
pub fn venice_chat_route(transport: SharedTransport, auth: Option<AuthDefinition>) -> Route {
    chat_route(
        "venice-chat",
        Some("venice"),
        Some("venice"),
        Some("https://api.venice.ai/api/v1"),
        Arc::new(VeniceChatProtocol),
        Framing::sse_with_done(),
        auth,
        transport,
    )
}

/// Zai (`providers/zai.ts` + `protocols/zai-chat.ts`).
pub fn zai_chat_route(transport: SharedTransport, auth: Option<AuthDefinition>) -> Route {
    let ext = OpenAiChatExt {
        id: "zai-chat",
        cache_control: false,
        allow_string_error: false,
        assistant: None,
        body: Some(zai_body_overlay),
    };
    chat_route(
        "zai-chat",
        Some("zai"),
        Some("zai"),
        Some("https://api.z.ai/api/paas/v4"),
        Arc::new(OpenAiChat::new(ext)),
        Framing::sse_with_done(),
        auth,
        transport,
    )
}

/// Alibaba DashScope compatible-mode chat (`providers/alibaba.ts`). Alibaba
/// uses plain `sse` framing — `[DONE]` never reaches the protocol.
pub fn alibaba_chat_route(transport: SharedTransport, auth: Option<AuthDefinition>) -> Route {
    let ext = OpenAiChatExt {
        id: "alibaba-chat",
        cache_control: true,
        allow_string_error: false,
        assistant: None,
        body: Some(alibaba_body_overlay),
    };
    chat_route(
        "alibaba-chat",
        Some("alibaba"),
        Some("alibaba"),
        Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        Arc::new(OpenAiChat::new(ext)),
        Framing::sse(),
        auth,
        transport,
    )
}

/// Compatibility preset for models on the native OpenAI Chat route.
pub fn openai_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility::default()
}

/// `deepseek.ts` model compatibility.
pub fn deepseek_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility {
        max_tokens_field: Some(MaxTokensFieldCompatibility::MaxTokens),
        supports_store: Some(false),
        ..Default::default()
    }
}

/// `groq.ts` model compatibility.
pub fn groq_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility {
        max_tokens_field: Some(MaxTokensFieldCompatibility::MaxCompletionTokens),
        reasoning_field: Some("reasoning".into()),
        require_reasoning: Some(false),
        supports_store: Some(false),
        supports_strict_mode: Some(false),
        ..Default::default()
    }
}

/// `openrouter.ts` model compatibility (plus the reasoning-reasoning tests'
/// `reasoningField: "reasoning"` overlay applied at model level).
pub fn openrouter_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility {
        supports_prompt_cache_key: Some(true),
        reasoning_field: Some("reasoning".into()),
        ..Default::default()
    }
}

/// `venice-chat.ts` `compatibility`.
pub fn venice_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility {
        max_tokens_field: Some(MaxTokensFieldCompatibility::MaxCompletionTokens),
        supports_store: Some(false),
        supports_prompt_cache_key: Some(true),
        ..Default::default()
    }
}

/// `zai-chat.ts` `compatibility`.
pub fn zai_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility {
        max_tokens_field: Some(MaxTokensFieldCompatibility::MaxTokens),
        supports_store: Some(false),
        supports_strict_mode: Some(false),
        reasoning_field: Some("reasoning_content".into()),
        zai_tool_stream: Some(false),
        ..Default::default()
    }
}

/// `alibaba-chat.ts` `compatibility`.
pub fn alibaba_chat_compatibility() -> LanguageModelCompatibility {
    LanguageModelCompatibility {
        max_tokens_field: Some(MaxTokensFieldCompatibility::MaxCompletionTokens),
        supports_store: Some(false),
        supports_strict_mode: Some(false),
        reasoning_field: Some("reasoning_content".into()),
        zai_tool_stream: Some(false),
        ..Default::default()
    }
}

// =============================================================================
// Tests: cassette parity against TS dumps + unit tests ported from
// packages/ai/test/provider/openai-chat.test.ts and the provider tests.
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare two serializable values after number normalization; accepts an
    /// optional trailing format note like `assert_eq!`.
    macro_rules! json_eq {
        ($actual:expr, $expected:expr) => {{
            let (actual, expected) = (
                normalize(::serde_json::to_value(&$actual).unwrap()),
                normalize(::serde_json::to_value(&$expected).unwrap()),
            );
            assert_eq!(actual, expected);
        }};
        ($actual:expr, $expected:expr, $($note:tt)*) => {{
            let (actual, expected) = (
                normalize(::serde_json::to_value(&$actual).unwrap()),
                normalize(::serde_json::to_value(&$expected).unwrap()),
            );
            assert_eq!(actual, expected, $($note)*);
        }};
    }

    use crate::protocols::openai_compatible::{
        openai_compatible_chat_configured, openai_compatible_chat_route,
    };
    use crate::schema::{CachePolicy, GenerationOptions, LanguageModel, ToolResultValue};
    use crate::test_support::{Cassette, MockTransport, RecordedInteraction};
    use futures::StreamExt;

    // ------------------------------------------------------------------
    // Parity replay (the acceptance): every openai-chat-family cassette,
    // replayed through the real route stack, must reproduce the LLMEvent
    // sequence dumped from the REAL v2 step machines.
    // ------------------------------------------------------------------

    fn expected_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-fixtures/expected/openai-chat")
    }

    fn recordings_path(group: &str, name: &str) -> std::path::PathBuf {
        let root = crate::test_support::recordings_dir();
        if group.starts_with("openrouter") {
            root.join(format!("{group}.json"))
        } else {
            root.join(group).join(format!("{name}.json"))
        }
    }

    fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut buffer = 0_u32;
        let mut bits = 0_u32;
        for byte in input.bytes() {
            if byte == b'=' || byte == b'\n' {
                continue;
            }
            let value = ALPHABET
                .iter()
                .position(|&candidate| candidate == byte)
                .ok_or_else(|| format!("invalid base64 byte {byte}"))?
                as u32;
            buffer = (buffer << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        Ok(out)
    }

    /// HttpRecorder decodes `bodyEncoding: "base64"` bodies on replay; the
    /// shared MockTransport does not know about the field, so decode here.
    fn decoded_cassette(path: &std::path::Path) -> Cassette {
        let raw =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let value: Value = serde_json::from_str(&raw).unwrap();
        let mut cassette: Cassette =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (interaction, raw_interaction) in cassette
            .interactions
            .iter_mut()
            .zip(value["interactions"].as_array().cloned().unwrap_or_default())
        {
            if raw_interaction["response"]["bodyEncoding"].as_str() == Some("base64") {
                if let Some(response) = interaction.response.as_mut() {
                    if let Some(body) = response.body.take() {
                        let decoded = base64_decode(&body)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        response.body = Some(String::from_utf8(decoded).expect("base64 body utf8"));
                    }
                }
            }
        }
        cassette
    }

    fn family_route(group: &str, provider: Option<&str>, transport: SharedTransport) -> Route {
        match group {
            "groq-chat" => groq_chat_route(transport, None),
            "venice-chat" => venice_chat_route(transport, None),
            "zai-chat" => zai_chat_route(transport, None),
            "alibaba-chat" => alibaba_chat_route(transport, None),
            "openrouter-reasoning" | "openrouter-reasoning-tool-loop" => {
                openrouter_chat_route(transport, None)
            }
            "openai-compatible-chat" => openai_compatible_chat_configured(
                transport,
                provider.expect("openai-compatible fixtures record their provider"),
                "https://replay.test/v1",
            ),
            other => panic!("unknown openai-chat family group {other}"),
        }
    }

    fn family_compatibility(group: &str) -> Option<LanguageModelCompatibility> {
        match group {
            "groq-chat" => Some(groq_chat_compatibility()),
            "venice-chat" => Some(venice_chat_compatibility()),
            "zai-chat" => Some(zai_chat_compatibility()),
            "alibaba-chat" => Some(alibaba_chat_compatibility()),
            "openrouter-reasoning" | "openrouter-reasoning-tool-loop" => {
                Some(openrouter_chat_compatibility())
            }
            _ => None,
        }
    }

    fn family_model_id(group: &str, provider: Option<&str>) -> &'static str {
        match group {
            "groq-chat" => "qwen/qwen3.6-27b",
            "venice-chat" => "deepseek-v4-flash",
            "zai-chat" => "glm-5.3",
            "alibaba-chat" => "qwen3-max",
            "openrouter-reasoning" | "openrouter-reasoning-tool-loop" => {
                "anthropic/claude-sonnet-4.6"
            }
            _ if provider == Some("togetherai") => "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            _ => "gemini-3.8-flash",
        }
    }

    /// Numbers compare as f64 on both sides: TS dumps print JS numbers
    /// (integers without decimal points) while serde prints f64 tokens. The
    /// `LLMEvent` tree serializes camelCase natively (oracle gate 3 finding 1),
    /// so no key renaming is needed for event comparisons.
    fn normalize(value: Value) -> Value {
        match value {
            Value::Number(number) => match number.as_f64() {
                Some(float) => {
                    serde_json::Number::from_f64(float).map_or(Value::Null, Value::Number)
                }
                None => Value::Null,
            },
            Value::Array(items) => Value::Array(items.into_iter().map(normalize).collect()),
            Value::Object(fields) => Value::Object(
                fields.into_iter().map(|(key, value)| (key, normalize(value))).collect(),
            ),
            other => other,
        }
    }

    fn first_mismatch(expected: &Value, actual: &Value, path: &str) -> Option<String> {
        match (expected, actual) {
            (Value::Object(a), Value::Object(b)) => {
                for key in a.keys().chain(b.keys()) {
                    match (a.get(key), b.get(key)) {
                        (Some(a_value), Some(b_value)) => {
                            if let Some(error) =
                                first_mismatch(a_value, b_value, &format!("{path}.{key}"))
                            {
                                return Some(error);
                            }
                        }
                        (missing, present) => {
                            return Some(format!(
                                "{path}.{key}: expected {missing:?}, actual {present:?}"
                            ));
                        }
                    }
                }
                None
            }
            (Value::Array(a), Value::Array(b)) => {
                for (index, (a_item, b_item)) in a.iter().zip(b.iter()).enumerate() {
                    if let Some(error) = first_mismatch(a_item, b_item, &format!("{path}[{index}]"))
                    {
                        return Some(error);
                    }
                }
                (a.len() != b.len()).then(|| format!("{path}: lengths {} vs {}", a.len(), b.len()))
            }
            (a, b) if a == b => None,
            (a, b) => Some(format!("{path}: expected {a}, actual {b}")),
        }
    }

    async fn run_parity_group(group: &str) {
        let dir = expected_root().join(group);
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no expected fixtures for {group}");
        let mut cassettes = 0_usize;
        for file in files {
            let fixture: Value = serde_json::from_str(
                &std::fs::read_to_string(&file)
                    .unwrap_or_else(|e| panic!("{}: {e}", file.display())),
            )
            .unwrap();
            let name = fixture["name"].as_str().unwrap_or_default().to_string();
            let provider = fixture["provider"].as_str().map(str::to_string);
            let cassette = decoded_cassette(&recordings_path(group, &name));
            let transport: SharedTransport = Arc::new(MockTransport::from_cassette(&cassette));
            let route = family_route(group, provider.as_deref(), Arc::clone(&transport));
            json_eq!(
                route.protocol.id(),
                fixture["protocol"].as_str().unwrap_or_default(),
                "{group}/{name}: protocol mismatch"
            );
            assert_eq!(
                route.provider_metadata_key.as_deref(),
                Some(fixture["providerMetadataKey"].as_str().unwrap_or_default()),
                "{group}/{name}: providerMetadataKey mismatch"
            );
            let mut model = route.model(family_model_id(group, provider.as_deref()), None).unwrap();
            if let Some(compatibility) = family_compatibility(group) {
                model = model.with_compatibility(compatibility);
            }
            let expected_interactions = fixture["interactions"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|interaction| {
                    // The request-body goldens (`body`/`bodyText`, oracle
                    // gate 3 finding 4) live in the same entries but are
                    // owned by tests/body_parity.rs; project them out here.
                    let mut projected = interaction;
                    if let Some(object) = projected.as_object_mut() {
                        object.remove("body");
                        object.remove("bodyText");
                    }
                    projected
                })
                .collect::<Vec<_>>();
            for (index, expected) in expected_interactions.iter().enumerate() {
                let mut request = LLMRequest::new(model.clone());
                request.messages = vec![Message::user("parity replay")];
                let items: Vec<Result<LLMEvent, AiError>> = match route.stream(request).await {
                    Ok(stream) => stream.collect().await,
                    Err(error) => vec![Err(error)],
                };
                let actual = match items.iter().find_map(|item| item.as_ref().err()) {
                    Some(error) => json!({
                        "error": {
                            "tag": error.tag(),
                            "message": error.message(),
                        }
                    }),
                    None => json!({
                        "events": items
                            .into_iter()
                            .map(Result::unwrap)
                            .map(|event| serde_json::to_value(&event).unwrap())
                            .collect::<Vec<_>>(),
                    }),
                };
                if let Some(mismatch) =
                    first_mismatch(&normalize(expected.clone()), &normalize(actual.clone()), "")
                {
                    panic!(
                        "{group}/{name} interaction {index}: {mismatch}\nexpected: {expected}\nactual:   {actual}"
                    );
                }
            }
            cassettes += 1;
        }
        assert!(cassettes > 0, "{group}: replayed at least one cassette");
    }

    macro_rules! parity_group_test {
        ($test_name:ident, $group:expr) => {
            #[tokio::test]
            async fn $test_name() {
                run_parity_group($group).await;
            }
        };
    }

    parity_group_test!(openai_chat_parity_groq_chat, "groq-chat");
    parity_group_test!(openai_chat_parity_venice_chat, "venice-chat");
    parity_group_test!(openai_chat_parity_zai_chat, "zai-chat");
    parity_group_test!(openai_chat_parity_alibaba_chat, "alibaba-chat");
    parity_group_test!(openai_chat_parity_openai_compatible_chat, "openai-compatible-chat");
    parity_group_test!(openai_chat_parity_openrouter_reasoning, "openrouter-reasoning");
    parity_group_test!(
        openai_chat_parity_openrouter_reasoning_tool_loop,
        "openrouter-reasoning-tool-loop"
    );

    #[test]
    fn openai_chat_parity_corpus_is_complete() {
        let groups = [
            "groq-chat",
            "venice-chat",
            "zai-chat",
            "alibaba-chat",
            "openai-compatible-chat",
            "openrouter-reasoning",
            "openrouter-reasoning-tool-loop",
        ];
        let mut total = 0;
        for group in groups {
            let dir = expected_root().join(group);
            let count = std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
                .count();
            let recorded = if group.starts_with("openrouter") {
                usize::from(recordings_path(group, group).exists())
            } else {
                std::fs::read_dir(crate::test_support::recordings_dir().join(group)).map_or(
                    0,
                    |entries| {
                        entries
                            .flatten()
                            .filter(|entry| {
                                entry.path().extension().is_some_and(|ext| ext == "json")
                            })
                            .count()
                    },
                )
            };
            assert_eq!(count, recorded, "{group}: dumped every cassette");
            total += count;
        }
        assert_eq!(total, 50, "openai-chat family cassette count");
    }

    // ------------------------------------------------------------------
    // Unit-test helpers (ports of test/lib/{sse,openai-chunks}.ts)
    // ------------------------------------------------------------------

    fn sse(chunks: &[Value]) -> String {
        let mut body = String::new();
        for chunk in chunks {
            body.push_str(&format!("data: {chunk}\n\n"));
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    fn delta_chunk(delta: Value, finish: Option<&str>) -> Value {
        json!({"choices": [{"delta": delta, "finish_reason": finish}]})
    }

    fn usage_chunk(usage: Value) -> Value {
        json!({"choices": [], "usage": usage})
    }

    fn scripted_route(bodies: &[String]) -> (Route, Arc<MockTransport>) {
        let transport = Arc::new(MockTransport::from_interactions(
            bodies
                .iter()
                .map(|body| RecordedInteraction {
                    method: "POST".into(),
                    url: "https://api.openai.test/v1/chat/completions".into(),
                    status: 200,
                    body: body.clone(),
                    request_body: None,
                })
                .collect(),
        ));
        let route = openai_chat_route(Arc::clone(&transport) as SharedTransport).with(
            crate::route::RoutePatch {
                endpoint: Some(crate::route::EndpointPatch {
                    base_url: Some("https://api.openai.test/v1/".into()),
                    path: None,
                    query: None,
                }),
                ..Default::default()
            },
        );
        (route, transport)
    }

    fn chat_model(route: &Route) -> LanguageModel {
        route.model("gpt-4o-mini", None).unwrap()
    }

    fn compat_model(route: &Route, compatibility: LanguageModelCompatibility) -> LanguageModel {
        route.model("gpt-4o-mini", None).unwrap().with_compatibility(compatibility)
    }

    async fn generate(route: &Route, request: LLMRequest) -> Result<Vec<LLMEvent>, AiError> {
        let stream = route.stream(request).await?;
        let items: Vec<Result<LLMEvent, AiError>> = stream.collect().await;
        items.into_iter().collect()
    }

    async fn generate_ok(route: &Route, request: LLMRequest) -> Vec<LLMEvent> {
        generate(route, request).await.unwrap_or_else(|error| panic!("stream failed: {error}"))
    }

    fn event_values(events: Vec<LLMEvent>) -> Value {
        Value::Array(
            events
                .into_iter()
                .map(|event| normalize(serde_json::to_value(&event).unwrap()))
                .collect(),
        )
    }

    fn replay_body(route: &Route, message: &Message) -> Value {
        let mut request = LLMRequest::new(chat_model(route));
        request.messages = vec![message.clone()];
        compiled_body(route, &request)
    }

    fn compiled_body(route: &Route, request: &LLMRequest) -> Value {
        route.build_body(&route.prepare_request(request)).expect("body builds")
    }

    fn base_request(model: &LanguageModel) -> LLMRequest {
        let mut request = LLMRequest::new(model.clone());
        request.system = vec![SystemPart::new("You are concise.")];
        request.messages = vec![Message::user("Say hello.")];
        request.generation = Some(GenerationOptions {
            max_tokens: Some(20.0),
            temperature: Some(0.0),
            ..Default::default()
        });
        request
    }

    use crate::schema::SystemPart;

    // ------------------------------------------------------------------
    // Lowering unit tests (ported from openai-chat.test.ts)
    // ------------------------------------------------------------------

    #[test]
    fn openai_chat_prepares_payload() {
        let (route, _transport) = scripted_route(&[]);
        let body = compiled_body(&route, &base_request(&chat_model(&route)));
        json_eq!(
            body,
            json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role": "system", "content": "You are concise."},
                    {"role": "user", "content": "Say hello."},
                ],
                "stream": true,
                "stream_options": {"include_usage": true},
                "store": false,
                "max_completion_tokens": 20,
                "temperature": 0,
            })
        );
    }

    #[test]
    fn openai_chat_wraps_chronological_system_updates_in_order() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::user("Before."),
            Message::system("Treat <admin> & data literally."),
            Message::assistant(vec![ContentPart::text("After.")]),
        ];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([
                {
                    "role": "user",
                    "content": "Before.\n<system-update>\nTreat &lt;admin&gt; &amp; data literally.\n</system-update>"
                },
                {"role": "assistant", "content": "After."},
            ])
        );
    }

    #[test]
    fn openai_chat_omits_empty_assistant_messages() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::user("Before."),
            Message::assistant(vec![]),
            Message::assistant(vec![ContentPart::text("")]),
            Message::assistant(vec![ContentPart::text(" \n\t ")]),
            Message::assistant(vec![ContentPart::text("After.")]),
        ];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([
                {"role": "user", "content": "Before."},
                {"role": "assistant", "content": "After."},
            ])
        );
    }

    #[test]
    fn openai_chat_replays_canonical_reasoning_as_reasoning_content() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::assistant(vec![
            ContentPart::Reasoning {
                text: "thinking".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            },
            ContentPart::text("Hello"),
        ])];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{ "role": "assistant", "content": "Hello", "reasoning_content": "thinking" }])
        );

        // reasoning-only history
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::assistant(vec![ContentPart::Reasoning {
            text: "hidden".into(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }])];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{ "role": "assistant", "content": "", "reasoning_content": "hidden" }])
        );

        // concatenated text parts
        let mut request = LLMRequest::new(model.clone());
        request.messages =
            vec![Message::assistant(vec![ContentPart::text("Hello"), ContentPart::text(" world")])];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{ "role": "assistant", "content": "Hello world" }])
        );
    }

    #[test]
    fn openai_chat_writes_configured_reasoning_field_on_every_assistant_message() {
        let (route, _transport) = scripted_route(&[]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility {
                reasoning_field: Some("vendor_reasoning".into()),
                ..Default::default()
            },
        );
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::assistant(vec![
                ContentPart::Reasoning {
                    text: "thinking".into(),
                    encrypted: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: Some(BTreeMap::from([(
                        "openai".to_string(),
                        BTreeMap::from([(
                            "reasoningField".to_string(),
                            Value::String("reasoning".into()),
                        )]),
                    )])),
                },
                ContentPart::text("Hello"),
            ]),
            Message::assistant(vec![ContentPart::text("Done")]),
        ];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([
                { "role": "assistant", "content": "Hello", "vendor_reasoning": "thinking" },
                { "role": "assistant", "content": "Done", "vendor_reasoning": "" },
            ])
        );
    }

    #[test]
    fn openai_chat_preserves_observed_reasoning_fields_when_required() {
        let (route, _transport) = scripted_route(&[]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility { require_reasoning: Some(true), ..Default::default() },
        );
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::assistant(vec![
                ContentPart::Reasoning {
                    text: "thinking".into(),
                    encrypted: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: Some(BTreeMap::from([(
                        "openai".to_string(),
                        BTreeMap::from([(
                            "reasoningField".to_string(),
                            Value::String("reasoning_text".into()),
                        )]),
                    )])),
                },
                ContentPart::text("Hello"),
            ]),
            Message::assistant(vec![ContentPart::text("Done")]),
        ];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([
                { "role": "assistant", "content": "Hello", "reasoning_text": "thinking" },
                { "role": "assistant", "content": "Done", "reasoning_content": "" },
            ])
        );
    }

    #[test]
    fn openai_chat_omits_empty_configured_reasoning_when_optional() {
        let (route, _transport) = scripted_route(&[]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility {
                reasoning_field: Some("reasoning_text".into()),
                require_reasoning: Some(false),
                ..Default::default()
            },
        );
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::assistant(vec![
                ContentPart::Reasoning {
                    text: "thinking".into(),
                    encrypted: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::text("Hello"),
            ]),
            Message::assistant(vec![ContentPart::text("Done")]),
        ];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([
                { "role": "assistant", "content": "Hello", "reasoning_text": "thinking" },
                { "role": "assistant", "content": "Done" },
            ])
        );
    }

    #[test]
    fn openai_chat_rejects_reserved_reasoning_fields() {
        let (route, _transport) = scripted_route(&[]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility {
                reasoning_field: Some("content".into()),
                ..Default::default()
            },
        );
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::assistant(vec![ContentPart::Reasoning {
            text: "thinking".into(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }])];
        let error = route.build_body(&route.prepare_request(&request)).unwrap_err();
        assert!(error.message().contains("reserved field content"), "got {}", error.message());
    }

    #[test]
    fn openai_chat_maps_provider_options_and_drops_malformed_siblings() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("think")];
        request.provider_options = Some(
            serde_json::from_value(
                json!({"store": true, "reasoningEffort": "max", "topLogprobs": 25}),
            )
            .unwrap(),
        );
        let body = compiled_body(&route, &request);
        json_eq!(body["store"], json!(true));
        json_eq!(body["reasoning_effort"], json!("max"));

        // custom effort strings pass through
        let mut request = LLMRequest::new(chat_model(&route));
        request.messages = vec![Message::user("think")];
        request.provider_options =
            Some(serde_json::from_value(json!({"reasoningEffort": "experimental"})).unwrap());
        json_eq!(compiled_body(&route, &request)["reasoning_effort"], json!("experimental"));
    }

    #[test]
    fn openai_chat_gates_the_prompt_cache_key_on_compatibility() {
        let (route, _transport) = scripted_route(&[]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility {
                supports_prompt_cache_key: Some(true),
                ..Default::default()
            },
        );
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("Hello")];
        request.prompt_cache_key = Some("session_123".into());
        json_eq!(compiled_body(&route, &request)["prompt_cache_key"], json!("session_123"));

        // without the flag the key is omitted
        let mut request = LLMRequest::new(chat_model(&route));
        request.messages = vec![Message::user("Hello")];
        request.prompt_cache_key = Some("session_123".into());
        let body = compiled_body(&route, &request);
        assert!(body.get("prompt_cache_key").is_none());

        // cache: none opts out even with the flag
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("Hello")];
        request.prompt_cache_key = Some("session_123".into());
        request.cache = Some(CachePolicy::None);
        let body = compiled_body(&route, &request);
        assert!(body.get("prompt_cache_key").is_none());
    }

    #[test]
    fn openai_chat_prepares_tool_messages_and_id_clamps() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let long_id = format!("call_{}", "a".repeat(48));
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::user("What is the weather?"),
            Message::tool_call("call_1", "lookup", json!({"query": "weather"})),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!({"forecast": "sunny"}) },
            ),
        ];
        json_eq!(
            compiled_body(&route, &request),
            json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role": "user", "content": "What is the weather?"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "lookup", "arguments": "{\"query\":\"weather\"}"},
                        }],
                    },
                    {"role": "tool", "tool_call_id": "call_1", "content": "{\"forecast\":\"sunny\"}"},
                ],
                "tools": [],
                "stream": true,
                "stream_options": {"include_usage": true},
                "store": false,
            })
        );

        // 40-char clamp for openai models
        let mut request = LLMRequest::new(chat_model(&route));
        request.messages = vec![
            Message::tool_call(long_id.clone(), "lookup", json!({})),
            Message::tool_result(
                long_id.clone(),
                "lookup",
                ToolResultValue::Json { value: json!("Sunny") },
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].clone();
        let clamped: String = long_id.chars().take(40).collect();
        json_eq!(messages[0]["tool_calls"][0]["id"], json!(clamped));
        json_eq!(messages[1]["tool_call_id"], json!(clamped));
    }

    #[test]
    fn openai_chat_replays_gemini_thought_signatures_as_extra_content() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::user("Weather in Paris and Tokyo?"),
            Message::assistant(vec![
                ContentPart::ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    input: json!({"city": "Paris"}),
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: Some(BTreeMap::from([(
                        "openai".to_string(),
                        BTreeMap::from([(
                            "extraContent".to_string(),
                            json!({"google": {"thought_signature": "sig_1"}}),
                        )]),
                    )])),
                },
                ContentPart::ToolCall {
                    id: "call_2".into(),
                    name: "lookup".into(),
                    namespace: None,
                    input: json!({"city": "Tokyo"}),
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ]),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!("Sunny") },
            ),
            Message::tool_result(
                "call_2",
                "lookup",
                ToolResultValue::Json { value: json!("Rainy") },
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].clone();
        json_eq!(
            messages[1]["tool_calls"],
            json!([
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "lookup", "arguments": "{\"city\":\"Paris\"}"},
                    "extra_content": {"google": {"thought_signature": "sig_1"}},
                },
                {
                    "id": "call_2",
                    "type": "function",
                    "function": {"name": "lookup", "arguments": "{\"city\":\"Tokyo\"}"},
                },
            ])
        );
    }

    #[test]
    fn openai_chat_preserves_structured_tool_errors() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let error_value =
            json!({"error": {"type": "unknown", "message": "Tool execution interrupted"}});
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::tool_call("call_1", "bash", json!({})),
            Message::tool_result(
                "call_1",
                "bash",
                ToolResultValue::Error { value: error_value.clone() },
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].as_array().unwrap().clone();
        json_eq!(
            messages.last().unwrap(),
            &json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": serde_json::to_string(&error_value).unwrap(),
            })
        );
    }

    fn content_result(items: Value) -> ToolResultValue {
        ToolResultValue::Content {
            value: crate::schema::ToolContentItems(items.as_array().cloned().unwrap_or_default()),
        }
    }

    #[test]
    fn openai_chat_continues_image_tool_results_as_vision_input() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::tool_call("call_image", "read", json!({"path": "pixel.png"})),
            Message::tool_result(
                "call_image",
                "read",
                content_result(json!([
                    {"type": "text", "text": "Image read successfully"},
                    {"type": "file", "uri": "data:image/png;base64,AAECAw==", "mime": "image/png"},
                ])),
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].clone();
        json_eq!(
            messages,
            json!([
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_image",
                        "type": "function",
                        "function": {"name": "read", "arguments": "{\"path\":\"pixel.png\"}"},
                    }],
                },
                {"role": "tool", "tool_call_id": "call_image", "content": "Image read successfully"},
                {
                    "role": "user",
                    "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AAECAw=="}}],
                },
            ])
        );
        assert!(!messages.to_string().contains("\"content\":\"AAECAw==\""));

        // remote tool URLs pass through
        let mut request = LLMRequest::new(chat_model(&route));
        let url = "https://example.com/tool-image.png?version=2";
        request.messages = vec![
            Message::user("Describe the image."),
            Message::tool_call("call_image", "read_image", json!({})),
            Message::tool_result(
                "call_image",
                "read_image",
                content_result(json!([
                    {"type": "text", "text": "Image attached."},
                    {"type": "file", "mime": "image/png", "uri": url},
                ])),
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].as_array().unwrap().clone();
        json_eq!(
            messages.last().unwrap(),
            &json!({"role": "user", "content": [{"type": "image_url", "image_url": {"url": url}}]})
        );
    }

    #[test]
    fn openai_chat_orders_parallel_tool_images_into_one_vision_message() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::assistant(vec![
                ContentPart::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    namespace: None,
                    input: json!({}),
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::ToolCall {
                    id: "call_2".into(),
                    name: "read".into(),
                    namespace: None,
                    input: json!({}),
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ]),
            Message::tool(ContentPart::ToolResult {
                id: "call_1".into(),
                name: "read".into(),
                namespace: None,
                result: content_result(json!([
                    {"type": "file", "uri": "data:image/png;base64,AAEC", "mime": "image/png"},
                ])),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }),
            Message::tool(ContentPart::ToolResult {
                id: "call_2".into(),
                name: "read".into(),
                namespace: None,
                result: content_result(json!([
                    {"type": "file", "uri": "data:image/jpeg;base64,/9j/", "mime": "image/jpeg"},
                ])),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }),
        ];
        let messages = compiled_body(&route, &request)["messages"].clone();
        json_eq!(
            messages.as_array().unwrap().split_at(1).1,
            vec![
                json!({"role": "tool", "tool_call_id": "call_1", "content": ""}),
                json!({"role": "tool", "tool_call_id": "call_2", "content": ""}),
                json!({"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAEC"}},
                    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,/9j/"}},
                ]}),
            ]
            .as_slice()
        );
    }

    #[test]
    fn openai_chat_aggregates_tool_images_with_a_following_system_update() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::tool_result(
                "call_1",
                "read",
                content_result(
                    json!([{"type": "file", "uri": "data:image/png;base64,AAEC", "mime": "image/png"}]),
                ),
            ),
            Message::tool_result(
                "call_2",
                "read",
                content_result(
                    json!([{"type": "file", "uri": "data:image/webp;base64,UklG", "mime": "image/webp"}]),
                ),
            ),
            Message::system("Inspect both images."),
        ];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([
                {"role": "tool", "tool_call_id": "call_1", "content": ""},
                {"role": "tool", "tool_call_id": "call_2", "content": ""},
                {"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAEC"}},
                    {"type": "image_url", "image_url": {"url": "data:image/webp;base64,UklG"}},
                    {"type": "text", "text": "<system-update>\nInspect both images.\n</system-update>"},
                ]},
            ])
        );
    }

    #[test]
    fn openai_chat_passes_encoded_images_and_urls_through() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let media = |media_type: &str, data: &str| ContentPart::Media {
            media: crate::schema::media::base64(data, media_type),
            filename: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        };
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::new(
            MessageRole::User,
            vec![media("image/png", "not-base64"), media("image/svg+xml", "PHN2Zz4=")],
        )];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,not-base64"}},
                    {"type": "image_url", "image_url": {"url": "data:image/svg+xml;base64,PHN2Zz4="}},
                ],
            }])
        );

        let url = "https://example.com/image.png?size=64#preview";
        let mut request = LLMRequest::new(chat_model(&route));
        request.messages = vec![Message::new(
            MessageRole::User,
            vec![ContentPart::Media {
                media: crate::schema::media::Asset {
                    source: crate::schema::media::Source::Url {
                        url: url.to_string(),
                        media_type: Some("image/png".into()),
                        expires_at: None,
                    },
                    media_type: "image/png".into(),
                    info: None,
                    expires_at: None,
                    provider_metadata: None,
                },
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        )];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{"role": "user", "content": [{"type": "image_url", "image_url": {"url": url}}]}])
        );
    }

    #[test]
    fn openai_chat_rejects_non_image_media_and_remote_pdfs() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::new(
            MessageRole::User,
            vec![ContentPart::Media {
                media: crate::schema::media::base64("AAECAw==", "audio/mpeg"),
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        )];
        let error = route.build_body(&route.prepare_request(&request)).unwrap_err();
        assert!(
            error.message().contains("OpenAI Chat does not support media type audio/mpeg"),
            "got {}",
            error.message()
        );

        let mut request = LLMRequest::new(chat_model(&route));
        request.messages = vec![Message::new(
            MessageRole::User,
            vec![ContentPart::Media {
                media: crate::schema::media::Asset {
                    source: crate::schema::media::Source::Url {
                        url: "https://example.com/report.pdf".into(),
                        media_type: Some("application/pdf".into()),
                        expires_at: None,
                    },
                    media_type: "application/pdf".into(),
                    info: None,
                    expires_at: None,
                    provider_metadata: None,
                },
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        )];
        let error = route.build_body(&route.prepare_request(&request)).unwrap_err();
        assert!(
            error.message().contains("OpenAI Chat requires inline media"),
            "got {}",
            error.message()
        );
    }

    #[test]
    fn openai_chat_lowers_inline_pdfs_as_file_parts() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::new(
            MessageRole::User,
            vec![
                ContentPart::text("Summarize these."),
                ContentPart::Media {
                    media: crate::schema::media::base64("JVBERi0=", "application/pdf"),
                    filename: Some("report.pdf".into()),
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::Media {
                    media: crate::schema::media::Asset {
                        source: crate::schema::media::Source::Base64 {
                            data: "JVBERi0=".into(),
                            media_type: "application/pdf".into(),
                        },
                        media_type: "application/pdf".into(),
                        info: None,
                        expires_at: None,
                        provider_metadata: None,
                    },
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ],
        )];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Summarize these."},
                    {"type": "file", "file": {"filename": "report.pdf", "file_data": "data:application/pdf;base64,JVBERi0="}},
                    {"type": "file", "file": {"filename": "document.pdf", "file_data": "data:application/pdf;base64,JVBERi0="}},
                ],
            }])
        );
    }

    #[test]
    fn openai_chat_bridges_assistant_after_tool_when_required() {
        let (route, _transport) = scripted_route(&[]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility {
                require_assistant_after_tool: Some(true),
                ..Default::default()
            },
        );
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::tool_call("call_image", "read", json!({})),
            Message::tool_result(
                "call_image",
                "read",
                content_result(
                    json!([{"type": "file", "uri": "data:image/png;base64,AAECAw==", "mime": "image/png"}]),
                ),
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].clone();
        let roles: Vec<&str> = messages
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        json_eq!(roles, ["assistant", "tool", "assistant", "user"]);
        json_eq!(messages[2], json!({"role": "assistant", "content": "Done."}));
    }

    #[test]
    fn openai_chat_replays_native_openai_compatible_reasoning() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let details = json!([{ "type": "reasoning.encrypted", "data": "opaque", "index": 0 }]);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message {
            native: Some(BTreeMap::from([(
                "openaiCompatible".to_string(),
                json!({"reasoning_content": "thinking", "reasoning_details": details}),
            )])),
            ..Message::assistant(vec![ContentPart::Reasoning {
                text: "thinking".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }])
        }];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{
                "role": "assistant",
                "content": "",
                "reasoning_content": "thinking",
                "reasoning_details": details,
            }])
        );
    }

    #[test]
    fn openai_chat_strips_kimi_streaming_index_on_replay() {
        let (route, _transport) = scripted_route(&[]);
        let model = chat_model(&route);
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::assistant(vec![ContentPart::Reasoning {
            text: "thinking".into(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: Some(BTreeMap::from([(
                "openai".to_string(),
                BTreeMap::from([(
                    "reasoningDetails".to_string(),
                    json!([
                        {"index": 0, "type": "summary", "summary": "thinking"},
                        {"index": 1, "type": "encrypted", "encrypted": "opaque"},
                    ]),
                )]),
            )])),
        }])];
        json_eq!(
            compiled_body(&route, &request)["messages"],
            json!([{
                "role": "assistant",
                "content": "",
                "reasoning_content": "thinking",
                "reasoning_details": [
                    {"type": "summary", "summary": "thinking"},
                    {"type": "encrypted", "encrypted": "opaque"},
                ],
            }])
        );
    }

    #[test]
    fn openai_chat_mistral_tool_ids_and_max_tokens_detection() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let deepseek = deepseek_chat_route(Arc::clone(&failing), None);
        let model = deepseek
            .model("deepseek-chat", None)
            .unwrap()
            .with_compatibility(deepseek_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("hi")];
        request.generation =
            Some(GenerationOptions { max_tokens: Some(20.0), ..Default::default() });
        let body = compiled_body(&deepseek, &request);
        json_eq!(body["max_tokens"], json!(20));
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("store").is_none());

        // mistral families clamp tool ids to 9 alphanumeric chars
        let route = openai_chat_route(Arc::clone(&failing));
        let model = route.model("mistral-small-latest", None).unwrap();
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![
            Message::tool_call("call_ABC-123456789xyz", "lookup", json!({})),
            Message::tool_result(
                "call_ABC-123456789xyz",
                "lookup",
                ToolResultValue::Json { value: json!("ok") },
            ),
        ];
        let messages = compiled_body(&route, &request)["messages"].clone();
        json_eq!(messages[0]["tool_calls"][0]["id"], json!("callABC12"));
    }

    #[test]
    fn openai_chat_groq_body_overlay() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = groq_chat_route(failing, None);
        let model = route
            .model("qwen/qwen3.6-27b", None)
            .unwrap()
            .with_compatibility(groq_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("hi")];
        request.generation =
            Some(GenerationOptions { max_tokens: Some(512.0), ..Default::default() });
        let body = compiled_body(&route, &request);
        json_eq!(body["reasoning_format"], json!("parsed"));
        json_eq!(body["max_completion_tokens"], json!(512));
        assert!(body.get("store").is_none());
        assert!(body.get("include_reasoning").is_none());

        let model = route
            .model("openai/gpt-oss-20b", None)
            .unwrap()
            .with_compatibility(groq_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("hi")];
        request.provider_options = Some(
            serde_json::from_value(json!({
                "includeReasoning": false,
                "parallelToolCalls": true,
                "serviceTier": "on_demand",
                "user": "recorded-test",
            }))
            .unwrap(),
        );
        let body = compiled_body(&route, &request);
        assert!(body.get("reasoning_format").is_none());
        json_eq!(body["include_reasoning"], json!(false));
        json_eq!(body["parallel_tool_calls"], json!(true));
        json_eq!(body["service_tier"], json!("on_demand"));
        json_eq!(body["user"], json!("recorded-test"));
    }

    #[test]
    fn openai_chat_openrouter_body_overlay() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = openrouter_chat_route(failing, None);
        let model = route
            .model("anthropic/claude-sonnet-4.6", None)
            .unwrap()
            .with_compatibility(openrouter_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::assistant(vec![
            ContentPart::Reasoning {
                text: "thinking".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: Some(BTreeMap::from([(
                    "openrouter".to_string(),
                    BTreeMap::from([(
                        "reasoningDetails".to_string(),
                        json!([{ "type": "reasoning.text", "text": "thinking", "signature": "s1" }]),
                    )]),
                )])),
            },
            ContentPart::text("answer"),
        ])];
        request.generation =
            Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
        request.provider_options = Some(
            serde_json::from_value(json!({
                "reasoning": {"max_tokens": 1900},
                "models": ["a/b"],
                "usage": false,
            }))
            .unwrap(),
        );
        let body = compiled_body(&route, &request);
        let assistant = &body["messages"][0];
        json_eq!(assistant["reasoning"], json!("thinking"));
        assert!(assistant.get("reasoning_content").is_none());
        assert!(assistant.get("reasoning_text").is_none());
        json_eq!(assistant["reasoning_details"][0]["signature"], json!("s1"));
        // usage: false → {include: false}; budget fit below max_tokens/2
        json_eq!(body["usage"], json!({"include": false}));
        json_eq!(body["models"], json!(["a/b"]));
        json_eq!(body["reasoning"]["max_tokens"], json!(1024.0));
    }

    #[test]
    fn openai_chat_zai_body_overlay_tool_stream_detection() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = zai_chat_route(failing, None);
        let weather = ToolDefinition::new(
            "get_weather",
            "Get weather",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
        );
        for (model_id, expect_tool_stream) in
            [("glm-4.5", false), ("glm-4.6", true), ("glm-5.3", true)]
        {
            let model =
                route.model(model_id, None).unwrap().with_compatibility(zai_chat_compatibility());
            let mut request = LLMRequest::new(model.clone());
            request.messages = vec![Message::user("hi")];
            request.tools = vec![crate::schema::ToolEntry::Definition(weather.clone())];
            request.provider_options = Some(
                serde_json::from_value(
                    json!({"thinking": {"type": "enabled", "clear_thinking": false}}),
                )
                .unwrap(),
            );
            let body = compiled_body(&route, &request);
            json_eq!(
                body.get("tool_stream").and_then(Value::as_bool),
                if expect_tool_stream { Some(true) } else { None },
                "{model_id}"
            );
            json_eq!(
                body["thinking"],
                json!({"type": "enabled", "clear_thinking": false}),
                "{model_id}"
            );
            assert!(body.get("max_tokens").is_none());
        }

        // explicit toolStream option wins; legacy glm-4.5 tool call without
        // streamed arguments still works
        let model =
            route.model("glm-4.5", None).unwrap().with_compatibility(zai_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("hi")];
        request.provider_options =
            Some(serde_json::from_value(json!({"toolStream": true})).unwrap());
        json_eq!(compiled_body(&route, &request)["tool_stream"], json!(true));
    }

    #[test]
    fn openai_chat_alibaba_body_overlay() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = alibaba_chat_route(failing, None);
        let model = route
            .model("qwen3-max", None)
            .unwrap()
            .with_compatibility(alibaba_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("hi")];
        request.generation = Some(GenerationOptions {
            max_tokens: Some(1024.0),
            top_k: Some(8.0),
            ..Default::default()
        });
        request.tool_choice =
            Some(crate::schema::ToolChoice::Required { disable_parallel_tool_use: Some(true) });
        request.provider_options = Some(
            serde_json::from_value(json!({
                "enableThinking": true,
                "thinkingBudget": 1900,
                "enableSearch": true,
            }))
            .unwrap(),
        );
        let body = compiled_body(&route, &request);
        json_eq!(body["enable_thinking"], json!(true));
        json_eq!(body["thinking_budget"], json!(512.0)); // fit below max_tokens/2
        json_eq!(body["top_k"], json!(8.0));
        json_eq!(body["enable_search"], json!(true));
        json_eq!(body["parallel_tool_calls"], json!(false));
        json_eq!(body["max_completion_tokens"], json!(1024));
        assert!(body.get("store").is_none());
    }

    #[test]
    fn openai_chat_venice_body_overlay_replays_signatures() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = venice_chat_route(failing, None);
        let model = route
            .model("gemini-3-8-flash", None)
            .unwrap()
            .with_compatibility(venice_chat_compatibility());
        let venice_metadata = |inner: Value| {
            Some(BTreeMap::from([("venice".to_string(), {
                let map: BTreeMap<String, Value> = serde_json::from_value(inner).unwrap();
                map
            })]))
        };
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::assistant(vec![
            ContentPart::Reasoning {
                text: "thinking".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: venice_metadata(json!({"reasoningDetails": [
                    {"type": "reasoning.text", "text": "thinking", "signature": "sig"}
                ]})),
            },
            ContentPart::ToolCall {
                id: "call_1".into(),
                name: "lookup_rate".into(),
                namespace: None,
                input: json!({"code": "ZEBRA"}),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: venice_metadata(json!({"thoughtSignature": "tool-sig"})),
            },
        ])];
        request.generation =
            Some(GenerationOptions { max_tokens: Some(4096.0), ..Default::default() });
        request.provider_options = Some(
            serde_json::from_value(json!({
                "reasoningEffort": "high",
                "parallelToolCalls": true,
                "veniceParameters": {"enableWebSearch": "auto"},
            }))
            .unwrap(),
        );
        let body = compiled_body(&route, &request);
        let assistant = &body["messages"][0];
        json_eq!(assistant["tool_calls"][0]["thought_signature"], json!("tool-sig"));
        assert!(body.get("reasoning_effort").is_none());
        json_eq!(body["reasoning"]["effort"], json!("high"));
        json_eq!(body["parallel_tool_calls"], json!(true));
        json_eq!(
            body["venice_parameters"],
            json!({"enable_web_search": "auto", "include_venice_system_prompt": false})
        );
        json_eq!(body["max_completion_tokens"], json!(4096));
    }

    // ------------------------------------------------------------------
    // Stream machine tests (ported from openai-chat.test.ts)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn openai_chat_parses_text_and_usage_stream() {
        let body = sse(&[
            delta_chunk(json!({"role": "assistant", "content": "Hello"}), None),
            delta_chunk(json!({"content": "!"}), None),
            delta_chunk(json!({}), Some("stop")),
            usage_chunk(json!({
                "prompt_tokens": 5,
                "completion_tokens": 2,
                "total_tokens": 7,
                "prompt_tokens_details": {"cached_tokens": 1, "cache_write_tokens": 2},
                "completion_tokens_details": {"reasoning_tokens": 0},
            })),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        let usage = json!({
            "inputTokens": 5,
            "outputTokens": 2,
            "nonCachedInputTokens": 2,
            "cacheReadInputTokens": 1,
            "cacheWriteInputTokens": 2,
            "reasoningTokens": 0,
            "totalTokens": 7,
            "providerMetadata": {"openai": {
                "prompt_tokens": 5,
                "completion_tokens": 2,
                "total_tokens": 7,
                "prompt_tokens_details": {"cached_tokens": 1, "cache_write_tokens": 2},
                "completion_tokens_details": {"reasoning_tokens": 0},
            }},
        });
        json_eq!(
            event_values(events),
            normalize(json!([
                {"type": "step-start", "index": 0},
                {"type": "text-start", "id": "text-0"},
                {"type": "text-delta", "id": "text-0", "text": "Hello"},
                {"type": "text-delta", "id": "text-0", "text": "!"},
                {"type": "text-end", "id": "text-0"},
                {
                    "type": "step-finish",
                    "index": 0,
                    "reason": {"normalized": "stop", "raw": "stop"},
                    "usage": usage,
                },
                {"type": "finish", "reason": {"normalized": "stop", "raw": "stop"}, "usage": usage},
            ]))
        );
    }

    #[tokio::test]
    async fn openai_chat_finishes_at_the_done_sentinel() {
        // No [DONE] wait: the body ends right after the finish chunk.
        let body = format!(
            "data: {}\n\ndata: {}\n\n",
            serde_json::to_string(&delta_chunk(json!({"content": "Hello"}), None)).unwrap(),
            serde_json::to_string(&delta_chunk(json!({}), Some("stop"))).unwrap(),
        );
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        assert_eq!(events.last().map(LLMEvent::kind), Some("finish"));
    }

    #[tokio::test]
    async fn openai_chat_refusals_and_reasoning_fields_stream_and_replay() {
        // refusal joins ordinary text
        let body = sse(&[
            delta_chunk(json!({"role": "assistant", "refusal": "I can't"}), None),
            delta_chunk(json!({"refusal": " help with that."}), None),
            delta_chunk(json!({}), Some("stop")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        assert_eq!(response.text(), "I can't help with that.");
        let replay = replay_body(&route, &response.message);
        json_eq!(
            replay["messages"],
            json!([{"role": "assistant", "content": "I can't help with that."}])
        );

        // each compatible reasoning field parses and replays under its own name
        for field in ["reasoning_content", "reasoning", "reasoning_text"] {
            let body = sse(&[
                delta_chunk(json!({field: "thinking"}), None),
                delta_chunk(json!({"content": "Hello"}), None),
                delta_chunk(json!({}), Some("stop")),
            ]);
            let (route, _transport) = scripted_route(&[body]);
            let events = generate_ok(&route, base_request(&chat_model(&route))).await;
            let response = crate::schema::LLMResponse::from_events(events).unwrap();
            json_eq!(response.reasoning(), "thinking", "{field}");
            let reasoning_metadata = response
                .message
                .content
                .iter()
                .find_map(|part| match part {
                    ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                    _ => None,
                })
                .unwrap();
            json_eq!(reasoning_metadata["openai"]["reasoningField"], json!(field), "{field}");
            let replay = replay_body(&route, &response.message);
            json_eq!(replay["messages"][0][field], json!("thinking"), "{field}");
        }
    }

    #[tokio::test]
    async fn openai_chat_reasoning_details_merge_and_replay() {
        // OpenRouter dialect: consecutive summary deltas merge; replay keeps
        // every original field (index included).
        let details = json!([
            {"type": "reasoning.summary", "summary": "Plan tools", "format": "openai-responses-v1", "index": 0},
            {"type": "reasoning.encrypted", "data": "opaque", "format": "openai-responses-v1", "index": 0},
        ]);
        let body = sse(&[
            delta_chunk(
                json!({"reasoning_details": [
                    {"type": "reasoning.summary", "summary": "Plan", "format": "openai-responses-v1", "index": 0},
                ]}),
                None,
            ),
            delta_chunk(
                json!({"reasoning_details": [
                    {"type": "reasoning.summary", "summary": " tools", "format": "openai-responses-v1", "index": 0},
                ]}),
                None,
            ),
            delta_chunk(json!({"reasoning_details": [details[1].clone()]}), None),
            delta_chunk(json!({"content": "Hello"}), None),
            delta_chunk(json!({}), Some("stop")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        json_eq!(response.reasoning(), "Plan tools");
        let metadata = response
            .message
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                _ => None,
            })
            .unwrap();
        assert_eq!(metadata["openai"]["reasoningDetails"], details);
        let replay = replay_body(&route, &response.message);
        json_eq!(
            replay["messages"],
            json!([{"role": "assistant", "content": "Hello", "reasoning_details": details}])
        );
    }

    #[tokio::test]
    async fn openai_chat_kimi_details_display_and_merge_by_index() {
        let stored = json!([
            {"index": 0, "type": "summary", "summary": "Plan tools"},
            {"index": 1, "type": "encrypted", "encrypted": "opaque"},
        ]);
        let body = sse(&[
            delta_chunk(json!({"reasoning_content": "Let me"}), None),
            delta_chunk(json!({"reasoning_content": " think"}), None),
            delta_chunk(
                json!({"reasoning_details": [{"index": 0, "type": "summary", "summary": "Plan"}]}),
                None,
            ),
            delta_chunk(
                json!({"reasoning_details": [{"index": 0, "type": "summary", "summary": " tools"}]}),
                None,
            ),
            delta_chunk(
                json!({"reasoning_details": [{"index": 1, "type": "encrypted", "encrypted": "opaque"}]}),
                None,
            ),
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "get_time", "arguments": "{}"}}]}),
                None,
            ),
            delta_chunk(json!({}), Some("tool_calls")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        json_eq!(response.reasoning(), "Let me think");
        let metadata = response
            .message
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                _ => None,
            })
            .unwrap();
        json_eq!(metadata["openai"]["reasoningField"], json!("reasoning_content"));
        json_eq!(metadata["openai"]["reasoningDetails"], stored);
        let replay = replay_body(&route, &response.message);
        json_eq!(
            replay["messages"][0],
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "get_time", "arguments": "{}"}}],
                "reasoning_content": "Let me think",
                "reasoning_details": [
                    {"type": "summary", "summary": "Plan tools"},
                    {"type": "encrypted", "encrypted": "opaque"},
                ],
            })
        );
    }

    #[tokio::test]
    async fn openai_chat_drops_unknown_reasoning_details() {
        let body = sse(&[
            delta_chunk(
                json!({
                    "reasoning": "thinking",
                    "reasoning_details": [{"type": "reasoning.future", "format": "provider-v2"}],
                }),
                None,
            ),
            delta_chunk(json!({"content": "Hello"}), None),
            delta_chunk(json!({}), Some("stop")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        json_eq!(response.reasoning(), "thinking");
        let metadata = response
            .message
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                _ => None,
            })
            .unwrap();
        json_eq!(metadata["openai"]["reasoningField"], json!("reasoning"));
        json_eq!(metadata["openai"]["reasoningDetails"], json!([]));
    }

    #[tokio::test]
    async fn openai_chat_assembles_streamed_tool_input() {
        let body = sse(&[
            delta_chunk(
                json!({"role": "assistant", "tool_calls": [
                    {"index": 0, "id": "call_1", "function": {"name": "lookup", "arguments": "{\"query\""}},
                ]}),
                None,
            ),
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "function": {"arguments": ":\"weather\"}"}}]}),
                None,
            ),
            delta_chunk(json!({}), Some("tool_calls")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let mut request = base_request(&chat_model(&route));
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        let events = generate_ok(&route, request).await;
        json_eq!(
            event_values(events),
            normalize(json!([
                {"type": "step-start", "index": 0},
                {"type": "tool-input-start", "id": "call_1", "name": "lookup"},
                {"type": "tool-input-delta", "id": "call_1", "name": "lookup", "text": "{\"query\"", "input": {}},
                {"type": "tool-input-delta", "id": "call_1", "name": "lookup", "text": ":\"weather\"}", "input": {"query": "weather"}},
                {"type": "tool-input-end", "id": "call_1", "name": "lookup"},
                {"type": "tool-call", "id": "call_1", "name": "lookup", "input": {"query": "weather"}},
                {
                    "type": "step-finish",
                    "index": 0,
                    "reason": {"normalized": "tool-calls", "raw": "tool_calls"},
                },
                {"type": "finish", "reason": {"normalized": "tool-calls", "raw": "tool_calls"}},
            ]))
        );
    }

    #[tokio::test]
    async fn openai_chat_gemini_parallel_signatures_without_indexes() {
        let body = sse(&[
            delta_chunk(
                json!({"role": "assistant", "tool_calls": [
                    {
                        "extra_content": {"google": {"thought_signature": "sig_1"}},
                        "id": "call_1",
                        "function": {"name": "lookup", "arguments": "{\"city\":\"Paris\"}"},
                    },
                ]}),
                None,
            ),
            delta_chunk(
                json!({"tool_calls": [
                    {"id": "call_2", "function": {"name": "lookup", "arguments": "{\"city\":\"Tokyo\"}"}},
                ]}),
                None,
            ),
            delta_chunk(json!({}), Some("stop")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let mut request = base_request(&chat_model(&route));
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        let events = generate_ok(&route, request).await;
        let calls: Vec<&LLMEvent> =
            events.iter().filter(|event| matches!(event, LLMEvent::ToolCall { .. })).collect();
        json_eq!(calls.len(), 2);
        json_eq!(
            serde_json::to_value(calls[0]).unwrap()["providerMetadata"],
            json!({"openai": {"extraContent": {"google": {"thought_signature": "sig_1"}}}}),
        );
        assert!(serde_json::to_value(calls[1]).unwrap().get("providerMetadata").is_none());
    }

    #[tokio::test]
    async fn openai_chat_does_not_finalize_tools_on_filter_or_length() {
        for finish in ["content_filter", "length"] {
            let body = sse(&[
                delta_chunk(
                    json!({"tool_calls": [
                        {"index": 0, "id": "call_1", "function": {"name": "lookup", "arguments": "{\"query\":\"weather\""}},
                    ]}),
                    None,
                ),
                delta_chunk(json!({}), Some(finish)),
            ]);
            let (route, _transport) = scripted_route(&[body]);
            let mut request = base_request(&chat_model(&route));
            request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
                "lookup",
                "Lookup data",
                json!({"type": "object"}),
            ))];
            let events = generate_ok(&route, request).await;
            let response = crate::schema::LLMResponse::from_events(events).unwrap();
            assert!(response.tool_calls().is_empty(), "{finish}");
            json_eq!(response.finish_reason.normalized, finish_reason_for(finish), "{finish}");
        }
    }

    fn finish_reason_for(finish: &str) -> FinishReason {
        match finish {
            "content_filter" => FinishReason::ContentFilter,
            "length" => FinishReason::Length,
            _ => FinishReason::Stop,
        }
    }

    #[tokio::test]
    async fn openai_chat_rejects_unknown_finish_reasons() {
        let body = sse(&[
            delta_chunk(
                json!({"tool_calls": [
                    {"index": 0, "id": "call_1", "function": {"name": "lookup", "arguments": "{\"query\":\"weather\""}},
                ]}),
                None,
            ),
            delta_chunk(json!({}), Some("future_reason")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let mut request = base_request(&chat_model(&route));
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        let error = generate(&route, request).await.unwrap_err();
        json_eq!(error.tag(), "UnknownProvider");
        assert_eq!(error.message(), "Provider finish_reason: future_reason");
    }

    #[tokio::test]
    async fn openai_chat_finalizes_tools_without_a_finish_reason_when_not_required() {
        let body = format!(
            "data: {}\n\ndata: {}\n\n",
            serde_json::to_string(&delta_chunk(
                json!({"role": "assistant", "tool_calls": [
                    {"index": 0, "id": "call_1", "function": {"name": "lookup", "arguments": "{\"query\""}},
                ]}),
                None,
            ))
            .unwrap(),
            serde_json::to_string(&delta_chunk(
                json!({"tool_calls": [{"index": 0, "function": {"arguments": ":\"weather\"}"}}]}),
                None,
            ))
            .unwrap(),
        );
        let (route, _transport) = scripted_route(&[body]);
        let model = compat_model(
            &route,
            LanguageModelCompatibility { require_finish_reason: Some(false), ..Default::default() },
        );
        let mut request = base_request(&model);
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        let events = generate_ok(&route, request).await;
        json_eq!(
            event_values(
                events
                    .iter()
                    .filter(|event| *event != &LLMEvent::StepStart { index: 0.0 })
                    .cloned()
                    .collect()
            ),
            normalize(json!([
                {"type": "tool-input-start", "id": "call_1", "name": "lookup"},
                {"type": "tool-input-delta", "id": "call_1", "name": "lookup", "text": "{\"query\"", "input": {}},
                {"type": "tool-input-delta", "id": "call_1", "name": "lookup", "text": ":\"weather\"}", "input": {"query": "weather"}},
                {"type": "tool-input-end", "id": "call_1", "name": "lookup"},
                {"type": "tool-call", "id": "call_1", "name": "lookup", "input": {"query": "weather"}},
                {"type": "step-finish", "index": 0, "reason": {"normalized": "tool-calls"}},
                {"type": "finish", "reason": {"normalized": "tool-calls"}},
            ]))
        );
    }

    #[tokio::test]
    async fn openai_chat_fails_on_malformed_stream_events() {
        let body = sse(&[delta_chunk(json!({"content": 123}), None)]);
        let (route, _transport) = scripted_route(&[body]);
        let error = generate(&route, base_request(&chat_model(&route))).await.unwrap_err();
        assert!(
            error.message().contains("Invalid openai/openai-chat stream event"),
            "got {}",
            error.message()
        );
    }

    #[tokio::test]
    async fn openai_chat_fails_when_a_buffered_tool_never_receives_a_name() {
        let body = sse(&[
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_1", "function": {"arguments": "{}"}}]}),
                None,
            ),
            delta_chunk(json!({}), Some("tool_calls")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let mut request = base_request(&chat_model(&route));
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        let error = generate(&route, request).await.unwrap_err();
        json_eq!(error.tag(), "InvalidProviderOutput");
        assert!(
            error.message().contains("OpenAI Chat tool call delta is missing id or name"),
            "got {}",
            error.message()
        );
    }

    #[tokio::test]
    async fn openai_chat_ignores_empty_identity_on_later_deltas_and_buffers_names() {
        let body = sse(&[
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "lookup", "arguments": "{"}}]}),
                None,
            ),
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "id": "", "function": {"name": "", "arguments": "\"query\":\"weather\"}"}}]}),
                None,
            ),
            delta_chunk(json!({}), Some("tool_calls")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let mut request = base_request(&chat_model(&route));
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        let events = generate_ok(&route, request.clone()).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        json_eq!(response.tool_calls().len(), 1);

        // identity arrives on the second delta
        let body = sse(&[
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_1", "function": {"arguments": "{"}}]}),
                None,
            ),
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "function": {"name": "lookup", "arguments": "\"query\":"}}]}),
                None,
            ),
            delta_chunk(
                json!({"tool_calls": [{"index": 0, "function": {"arguments": "\"weather\"}"}}]}),
                None,
            ),
            delta_chunk(json!({}), Some("tool_calls")),
        ]);
        let (route, _transport) = scripted_route(&[body]);
        let events = generate_ok(&route, request).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        assert_eq!(response.tool_calls().len(), 1);
        let call = serde_json::to_value(response.tool_calls()[0]).unwrap();
        json_eq!(call["input"], json!({"query": "weather"}));
    }

    #[tokio::test]
    async fn openai_chat_keeps_one_reasoning_lifecycle_across_content() {
        let details = json!([{"type": "reasoning.text", "text": "thinking", "format": "anthropic-claude-v1", "index": 0}]);
        let mut chunks = vec![delta_chunk(
            json!({"reasoning": "thinking", "reasoning_details": details.clone()}),
            None,
        )];
        for index in 0..25 {
            chunks.push(delta_chunk(json!({"content": format!("chunk-{index} ")}), None));
        }
        chunks.push(delta_chunk(json!({}), Some("stop")));
        let (route, _transport) = scripted_route(&[sse(&chunks)]);
        let events = generate_ok(&route, base_request(&chat_model(&route))).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        json_eq!(response.reasoning(), "thinking");
        assert_eq!(
            response.text(),
            (0..25).fold(String::new(), |acc, i| acc + &format!("chunk-{i} "))
        );
        assert_eq!(
            response.events.iter().filter(|event| event.kind() == "reasoning-start").count(),
            1
        );
        assert_eq!(
            response.events.iter().filter(|event| event.kind() == "reasoning-end").count(),
            1
        );
    }

    #[tokio::test]
    async fn openai_chat_provider_metadata_namespace_follows_the_route() {
        // vendor namespace + configured custom field
        let (route, _transport) = scripted_route(&[]);
        let route = route.with(crate::route::RoutePatch {
            provider_metadata_key: Some("vendor".into()),
            ..Default::default()
        });
        let model = compat_model(
            &route,
            LanguageModelCompatibility {
                reasoning_field: Some("vendor_reasoning".into()),
                ..Default::default()
            },
        );
        let details =
            json!([{ "type": "reasoning.text", "text": "thinking", "signature": "signed" }]);
        let body = sse(&[
            delta_chunk(
                json!({"vendor_reasoning": "thinking", "reasoning_details": details.clone()}),
                None,
            ),
            delta_chunk(json!({"content": "Hello"}), None),
            delta_chunk(json!({}), Some("stop")),
            usage_chunk(json!({"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7})),
        ]);
        let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://api.openai.test/v1/chat/completions".into(),
            status: 200,
            body,
            request_body: None,
        }]));
        let route = route.with(crate::route::RoutePatch {
            transport: Some(Arc::clone(&transport) as SharedTransport),
            ..Default::default()
        });
        let events = generate_ok(&route, base_request(&model)).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        let metadata = response
            .message
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                _ => None,
            })
            .unwrap();
        json_eq!(
            serde_json::to_value(&metadata).unwrap(),
            json!({"vendor": {"reasoningField": "vendor_reasoning", "reasoningDetails": details}}),
        );
        let usage = response.usage.unwrap();
        json_eq!(
            serde_json::to_value(usage.provider_metadata.unwrap()).unwrap()["vendor"],
            json!({"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}),
        );
    }

    #[tokio::test]
    async fn openai_chat_http_errors_fail_before_stream_parsing() {
        let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://api.openai.test/v1/chat/completions".into(),
            status: 400,
            body: "{\"error\":{\"message\":\"Bad request\",\"type\":\"invalid_request_error\"}}"
                .into(),
            request_body: None,
        }]));
        let (mut route, _transport) = scripted_route(&[]);
        route = route.with(crate::route::RoutePatch {
            transport: Some(Arc::clone(&transport) as SharedTransport),
            ..Default::default()
        });
        let error = generate(&route, base_request(&chat_model(&route))).await.unwrap_err();
        json_eq!(error.tag(), "InvalidRequest");
        assert_eq!(error.message(), "Bad request");
    }

    #[tokio::test]
    async fn openai_chat_venice_stream_hides_the_encrypted_marker() {
        // reasoning_content streams the marker; visible deltas stop at it and
        // the encrypted content replays through metadata.
        let mut chunks =
            vec![delta_chunk(json!({"reasoning_content": "Visible__ENCRYPTED"}), None)];
        chunks.push(delta_chunk(json!({"reasoning_content": "_REASONING__payload"}), None));
        chunks.push(delta_chunk(json!({"content": "Done."}), None));
        chunks.push(delta_chunk(json!({}), Some("stop")));
        let body = sse(&chunks);
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = venice_chat_route(failing, None);
        let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://api.venice.ai/api/v1/chat/completions".into(),
            status: 200,
            body,
            request_body: None,
        }]));
        let route = route.with(crate::route::RoutePatch {
            transport: Some(Arc::clone(&transport) as SharedTransport),
            ..Default::default()
        });
        let model = route
            .model("deepseek-v4-flash", None)
            .unwrap()
            .with_compatibility(venice_chat_compatibility());
        let mut request = LLMRequest::new(model.clone());
        request.messages = vec![Message::user("hi")];
        let events = generate_ok(&route, request).await;
        let response = crate::schema::LLMResponse::from_events(events).unwrap();
        assert_eq!(response.reasoning(), "Visible");
        assert!(reasoning_metadata_of(&response)
            .get("venice")
            .and_then(|inner| inner.get("encryptedReasoningContent"))
            .is_some());
        assert_eq!(response.text(), "Done.");
    }

    fn reasoning_metadata_of(response: &crate::schema::LLMResponse) -> Value {
        let metadata = response
            .message
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                _ => None,
            })
            .unwrap_or_default();
        serde_json::to_value(metadata).unwrap()
    }

    // ------------------------------------------------------------------
    // openai-compatible re-badging (ported from
    // openai-compatible-chat.test.ts basics)
    // ------------------------------------------------------------------

    #[test]
    fn openai_chat_compatible_rebadges_the_route_identity() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route = openai_compatible_chat_route(Arc::clone(&failing));
        assert_eq!(route.id, "openai-compatible-chat");
        assert_eq!(route.protocol.id(), "openai-chat");
        assert_eq!(route.provider_metadata_key.as_deref(), Some("openai"));
        assert!(route.endpoint.base_url.is_none());

        let configured =
            openai_compatible_chat_configured(failing, "deepseek", "https://api.deepseek.test/v1/");
        assert_eq!(configured.provider.as_ref().map(|p| p.0.as_str()), Some("deepseek"));
        // the metadata key follows a changed provider
        assert_eq!(configured.provider_metadata_key.as_deref(), Some("deepseek"));
    }

    #[test]
    fn openai_chat_compatible_prepares_a_generic_chat_target() {
        let failing: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport" });
        let route =
            openai_compatible_chat_configured(failing, "deepseek", "https://api.deepseek.test/v1/");
        let model = route.model("deepseek-chat", None).unwrap();
        let mut request = LLMRequest::new(model.clone());
        request.system = vec![SystemPart::new("You are concise.")];
        request.messages = vec![Message::user("Say hello.")];
        request.generation = Some(GenerationOptions {
            max_tokens: Some(20.0),
            temperature: Some(0.0),
            ..Default::default()
        });
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({"type": "object"}),
        ))];
        request.tool_choice = Some(crate::schema::ToolChoice::required());
        let body = compiled_body(&route, &request);
        // detected legacy max_tokens + no store (deepseek.com base URL)
        json_eq!(body["model"], json!("deepseek-chat"));
        json_eq!(body["tools"][0]["function"]["strict"], json!(false));
        json_eq!(body["tool_choice"], json!("required"));
        json_eq!(body["stream_options"], json!({"include_usage": true}));
        json_eq!(body["max_tokens"], json!(20));
        assert!(body.get("store").is_none());
    }
}
