//! The Gemini generateContent protocol (port of
//! `src/protocols/gemini.ts`).
//!
//! Request side: `GeminiBody` (`contents`, `systemInstruction`,
//! `generationConfig` with `thinkingConfig`, `tools[{functionDeclarations}]`
//! whose `parametersJsonSchema` carry the sanitized input schema, and
//! `toolConfig`). Response side: a pure state machine over
//! `Framing::sse` frames — `candidates[0].content.parts` (`text`, `thought`
//! → reasoning, inline `functionCall{name, args}`), `usageMetadata`
//! (inclusive input; mappers subtract down), and `finishReason` mapping.
//! Gemini never emits a terminal event inside the stream; the finish pair is
//! synthesized by [`Protocol::on_halt`] (port of `onHalt`).

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{
    self, encode_json, fit_thinking_budget, lenient, subtract_tokens, total_tokens,
};
use crate::protocols::utils::gemini_generate_content as generate_content;
use crate::protocols::utils::lifecycle;
use crate::route::auth::{self, Secret};
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::{Endpoint, Framing, Route, RouteInput, RoutePatch, SharedTransport};
use crate::schema::media;
use crate::schema::{
    AiError, AiErrorReason, ContentPart, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest,
    Message, MessageRole, ProviderMetadata, ReasonFields, SanitizerCompatibility, ToolChoice,
    ToolDefinition, ToolResultValue, Usage,
};

use super::sanitizer;

const ADAPTER: &str = "gemini";
/// Google documents this sentinel for replaying Gemini 3 function calls
/// after their original signature was lost.
const SKIP_THOUGHT_SIGNATURE_VALIDATOR: &str = "skip_thought_signature_validator";
/// Gemini 2.5 rejects a budget under the model's minimum: 512 on Flash-Lite,
/// the highest, and 128 on Pro.
const MIN_THINKING_BUDGET: f64 = 512.0;
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

// =============================================================================
// Model gating (regex ports of the TS helpers)
// =============================================================================

/// Gemini 3 rejects replayed function calls without a thought signature.
/// Google's SDKs avoid that by retaining complete model responses, but the
/// durable history may carry an unsigned call from an older or external
/// session. Model IDs are open-ended, so unknown Gemini aliases inherit
/// current behavior.
fn requires_thought_signature_fallback(model_id: &str) -> bool {
    let starts_gemini =
        |prefix: &str| model_id.split('/').any(|segment| segment.starts_with(prefix));
    if !starts_gemini("gemini-") {
        return false;
    }
    // gemini-1.x / gemini-2.x never validated signatures
    if starts_gemini("gemini-1") || starts_gemini("gemini-2") {
        // but only when the version digit is followed by `.` or `-` or ends there
        let legacy = regex::Regex::new(r"(^|/)gemini-(?:1|2)(?:[.-]|$)").unwrap();
        if legacy.is_match(model_id) {
            return false;
        }
    }
    let exact = regex::Regex::new(r"(^|/)gemini-pro(?:-vision)?$").unwrap();
    if exact.is_match(model_id) {
        return false;
    }
    let robotics = regex::Regex::new(r"(^|/)gemini-robotics-er-1\.5(?:[.-]|$)").unwrap();
    !robotics.is_match(model_id)
}

/// Gemini 2.5 accepts media nested inside function responses; matched Gemini
/// 2.5 variants reject it, so their tool-result attachments lower as a
/// separate user turn instead.
fn routes_legacy_tool_media(model_id: &str) -> bool {
    regex::Regex::new(r"(?i)gemini-2[.-]5(?:[.-]|$)").unwrap().is_match(model_id)
}

/// Blacklist: Gemini 1.x/2.x ignore or reject explicit function call ids.
/// Every other model id (Gemini 3+, gemma, anything unrecognized) gets them.
fn omits_function_call_ids(model_id: &str) -> bool {
    let pattern = regex::Regex::new(r"(?i)^gemini(?:-live)?-(\d+)").unwrap();
    pattern
        .captures(model_id)
        .and_then(|captures| captures.get(1))
        .and_then(|digits| digits.as_str().parse::<u32>().ok())
        .is_some_and(|major| major < 3)
}

// =============================================================================
// Provider options (lenient decode — malformed fields are dropped)
// =============================================================================

#[derive(Debug, Clone, Default, PartialEq)]
struct ThinkingConfigInput {
    thinking_budget: Option<f64>,
    include_thoughts: Option<bool>,
    thinking_level: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct GeminiOptions {
    cached_content: Option<String>,
    safety_settings: Option<Vec<(String, String)>>,
    service_tier: Option<String>,
    thinking_config: Option<ThinkingConfigInput>,
}

fn decode_options(options: &crate::schema::ProviderOptions) -> GeminiOptions {
    let value = Value::Object(options.clone());
    let thinking_config = lenient::lenient(value.get("thinkingConfig"), |input| {
        let budget = lenient::lenient(input.get("thinkingBudget"), Value::as_f64);
        let include = lenient::lenient(input.get("includeThoughts"), Value::as_bool);
        let level = lenient::lenient(input.get("thinkingLevel"), |value| {
            value.as_str().map(str::to_string)
        });
        Some(ThinkingConfigInput {
            thinking_budget: budget,
            include_thoughts: include,
            thinking_level: level,
        })
    });
    GeminiOptions {
        cached_content: lenient::lenient(value.get("cachedContent"), |value| {
            value.as_str().map(str::to_string)
        }),
        safety_settings: lenient::lenient(value.get("safetySettings"), |input| {
            let items = input.as_array()?;
            let mut settings = Vec::new();
            for item in items {
                let category = item.get("category")?.as_str()?;
                let threshold = item.get("threshold")?.as_str()?;
                settings.push((category.to_string(), threshold.to_string()));
            }
            Some(settings)
        }),
        service_tier: lenient::lenient(value.get("serviceTier"), |value| {
            value.as_str().map(str::to_string)
        }),
        thinking_config,
    }
}

// =============================================================================
// Response frame decoding (Effect-Schema-parity lenient decode)
// =============================================================================

/// Gemini is known to send explicit `null` for optional streaming fields
/// (usage counts, flags, whole subtrees); every response-side optional
/// therefore accepts null (port of `optionalNull`).
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_field_names)] // the provider wire fields all end in TokenCount
struct DecodedUsage {
    cached_content_token_count: Option<f64>,
    thoughts_token_count: Option<f64>,
    prompt_token_count: Option<f64>,
    candidates_token_count: Option<f64>,
    total_token_count: Option<f64>,
}

impl DecodedUsage {
    /// The decoded usage record for `providerMetadata` — unknown provider
    /// fields are dropped, absent counts omitted.
    fn to_metadata(&self) -> Value {
        let mut out = Map::new();
        if let Some(count) = self.cached_content_token_count {
            out.insert("cachedContentTokenCount".into(), json!(count));
        }
        if let Some(count) = self.thoughts_token_count {
            out.insert("thoughtsTokenCount".into(), json!(count));
        }
        if let Some(count) = self.prompt_token_count {
            out.insert("promptTokenCount".into(), json!(count));
        }
        if let Some(count) = self.candidates_token_count {
            out.insert("candidatesTokenCount".into(), json!(count));
        }
        if let Some(count) = self.total_token_count {
            out.insert("totalTokenCount".into(), json!(count));
        }
        Value::Object(out)
    }
}

#[derive(Debug, Clone)]
struct ResponseContent {
    #[allow(dead_code)]
    role: Option<String>,
    parts: Option<Vec<Value>>,
}

#[derive(Debug, Clone)]
struct Candidate {
    content: Option<ResponseContent>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct GeminiEventFrame {
    /// The `error` key's value when present (any JSON value, including null).
    error: Option<Value>,
    candidates: Option<Vec<Candidate>>,
    /// `promptFeedback` keeps its full object shape (rest keys preserved).
    prompt_feedback: Option<Value>,
    usage_metadata: Option<DecodedUsage>,
}

impl GeminiEventFrame {
    /// Re-encode the decoded event (port of `ProviderShared.encodeJson` on
    /// the decoded struct: unknown top-level keys are dropped).
    fn to_json(&self) -> Value {
        let mut out = Map::new();
        if let Some(error) = &self.error {
            out.insert("error".into(), error.clone());
        }
        if let Some(candidates) = &self.candidates {
            out.insert(
                "candidates".into(),
                Value::Array(
                    candidates
                        .iter()
                        .map(|candidate| {
                            let mut entry = Map::new();
                            if let Some(content) = &candidate.content {
                                let mut content_json = Map::new();
                                if let Some(role) = &content.role {
                                    content_json.insert("role".into(), json!(role));
                                }
                                if let Some(parts) = &content.parts {
                                    content_json
                                        .insert("parts".into(), Value::Array(parts.clone()));
                                }
                                entry.insert("content".into(), Value::Object(content_json));
                            }
                            if let Some(reason) = &candidate.finish_reason {
                                entry.insert("finishReason".into(), json!(reason));
                            }
                            Value::Object(entry)
                        })
                        .collect(),
                ),
            );
        }
        if let Some(feedback) = &self.prompt_feedback {
            out.insert("promptFeedback".into(), feedback.clone());
        }
        if let Some(usage) = &self.usage_metadata {
            out.insert("usageMetadata".into(), usage.to_metadata());
        }
        Value::Object(out)
    }
}

fn optional_null_number(record: &Map<String, Value>, key: &str) -> Result<Option<f64>, ()> {
    match record.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(number.as_f64()),
        Some(_) => Err(()),
    }
}

fn optional_null_string(record: &Map<String, Value>, key: &str) -> Result<Option<String>, ()> {
    match record.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(()),
    }
}

/// Decode one framed SSE payload into the event shape (port of
/// `Protocol.jsonEvent(GeminiEvent)` semantics: struct decode failures fail
/// the whole event).
fn decode_event(frame: &str) -> Result<GeminiEventFrame, ()> {
    let value: Value = serde_json::from_str(frame).map_err(|_| ())?;
    let record = value.as_object().ok_or(())?;

    let error = record.get("error").cloned();

    let mut candidates = None;
    match record.get("candidates") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            let mut decoded = Vec::with_capacity(items.len());
            for item in items {
                decoded.push(decode_candidate(item)?);
            }
            candidates = Some(decoded);
        }
        Some(_) => return Err(()),
    }

    let prompt_feedback = match record.get("promptFeedback") {
        None | Some(Value::Null) => None,
        Some(feedback @ Value::Object(_)) => Some(feedback.clone()),
        Some(_) => return Err(()),
    };

    let usage_metadata = match record.get("usageMetadata") {
        None | Some(Value::Null) => None,
        Some(Value::Object(usage)) => Some(DecodedUsage {
            cached_content_token_count: optional_null_number(usage, "cachedContentTokenCount")?,
            thoughts_token_count: optional_null_number(usage, "thoughtsTokenCount")?,
            prompt_token_count: optional_null_number(usage, "promptTokenCount")?,
            candidates_token_count: optional_null_number(usage, "candidatesTokenCount")?,
            total_token_count: optional_null_number(usage, "totalTokenCount")?,
        }),
        Some(_) => return Err(()),
    };

    Ok(GeminiEventFrame { error, candidates, prompt_feedback, usage_metadata })
}

fn decode_candidate(item: &Value) -> Result<Candidate, ()> {
    let record = item.as_object().ok_or(())?;
    let finish_reason = optional_null_string(record, "finishReason")?;
    let content = match record.get("content") {
        None | Some(Value::Null) => None,
        Some(Value::Object(content)) => {
            let role = match content.get("role") {
                None | Some(Value::Null) => None,
                Some(Value::String(role)) if role == "user" || role == "model" => {
                    Some(role.clone())
                }
                Some(_) => return Err(()),
            };
            let parts = match content.get("parts") {
                None | Some(Value::Null) => None,
                Some(Value::Array(parts)) => Some(parts.clone()),
                Some(_) => return Err(()),
            };
            Some(ResponseContent { role, parts })
        }
        Some(_) => return Err(()),
    };
    Ok(Candidate { content, finish_reason })
}

/// One decoded candidate content part (the response-side
/// `GeminiContentPart` union). Only the shapes the step machine consumes
/// carry data; the rest exist so union decoding accepts them.
#[derive(Debug, Clone)]
enum ContentPartFrame {
    Text {
        text: String,
        thought: Option<bool>,
        signature: Option<String>,
    },
    InlineData {
        mime_type: String,
        data: String,
        signature: Option<String>,
    },
    FileData,
    FunctionCall {
        id: Option<String>,
        name: String,
        args: Option<Value>,
        signature: Option<String>,
    },
    FunctionResponse,
}

impl ContentPartFrame {
    /// A non-empty `thoughtSignature` (empty strings are falsy in TS).
    fn signature(part: &Map<String, Value>) -> Option<String> {
        part.get("thoughtSignature")
            .and_then(Value::as_str)
            .filter(|signature| !signature.is_empty())
            .map(str::to_string)
    }
}

fn decode_inline_data(record: &Map<String, Value>, data_key: &str) -> Option<(String, String)> {
    let mime_type = record.get("mimeType")?.as_str()?;
    let data = record.get(data_key)?.as_str()?;
    Some((mime_type.to_string(), data.to_string()))
}

/// Decode one part through the union (text → inlineData → fileData →
/// functionCall → functionResponse); `None` means the part matched none of
/// the recognized shapes (port of `decodeGeminiContentPart` returning None).
fn decode_content_part(part: &Value) -> Option<ContentPartFrame> {
    let record = part.as_object()?;
    // GeminiTextPart — `text` is required
    if let Some(Value::String(text)) = record.get("text") {
        let thought = match record.get("thought") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(flag)) => Some(*flag),
            Some(_) => return None,
        };
        let signature = match record.get("thoughtSignature") {
            None | Some(Value::Null) => None,
            Some(Value::String(signature)) => Some(signature.clone()),
            Some(_) => return None,
        };
        return Some(ContentPartFrame::Text { text: text.clone(), thought, signature });
    }
    let signature = ContentPartFrame::signature(record);
    if let Some(Value::Object(inline)) = record.get("inlineData") {
        let (mime_type, data) = decode_inline_data(inline, "data")?;
        return Some(ContentPartFrame::InlineData { mime_type, data, signature });
    }
    if let Some(Value::Object(file)) = record.get("fileData") {
        decode_inline_data(file, "fileUri")?;
        return Some(ContentPartFrame::FileData);
    }
    if let Some(Value::Object(call)) = record.get("functionCall") {
        let id = match call.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) => Some(id.clone()),
            Some(_) => return None,
        };
        let name = call.get("name")?.as_str()?.to_string();
        let args = call.get("args").cloned();
        return Some(ContentPartFrame::FunctionCall { id, name, args, signature });
    }
    if let Some(Value::Object(response)) = record.get("functionResponse") {
        match response.get("id") {
            None => {}
            Some(Value::String(_)) => {}
            _ => return None,
        }
        response.get("name")?.as_str()?;
        response.get("response")?;
        if let Some(Value::Array(parts)) = response.get("parts") {
            for item in parts {
                let inline = item.as_object()?;
                decode_inline_data(inline, "data")?;
            }
        }
        return Some(ContentPartFrame::FunctionResponse);
    }
    None
}

// =============================================================================
// Stream parsing
// =============================================================================

fn provider_metadata(key: &str, metadata: Value) -> ProviderMetadata {
    // The inner record is a JSON object keyed by string — matching the
    // ProviderMetadata shape (BTreeMap of BTreeMaps).
    let inner: BTreeMetadata = match metadata {
        Value::Object(map) => map.into_iter().collect(),
        other => [("value".to_string(), other)].into_iter().collect(),
    };
    std::iter::once((key.to_string(), inner)).collect()
}

type BTreeMetadata = std::collections::BTreeMap<String, Value>;

fn thought_signature_metadata(key: &str, signature: &str) -> ProviderMetadata {
    provider_metadata(key, json!({ "thoughtSignature": signature }))
}

/// Gemini reports `promptTokenCount` (inclusive total) with a
/// `cachedContentTokenCount` subset. `candidatesTokenCount` is *exclusive*
/// of `thoughtsTokenCount` — visible-only — so the two are summed into the
/// inclusive `outputTokens` the contract expects (mappers subtract down).
fn map_usage(usage: &DecodedUsage, metadata_key: &str) -> Usage {
    let prompt = usage.prompt_token_count;
    let cached = usage.cached_content_token_count;
    let thoughts = usage.thoughts_token_count;
    let visible = usage.candidates_token_count;
    let non_cached = subtract_tokens(prompt, cached);
    let output_tokens = visible.map(|visible| visible + thoughts.unwrap_or(0.0));
    Usage {
        input_tokens: prompt,
        output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cached,
        reasoning_tokens: thoughts,
        total_tokens: total_tokens(prompt, output_tokens, usage.total_token_count),
        provider_metadata: Some(provider_metadata(metadata_key, usage.to_metadata())),
        context_tokens: None,
        cache_write_input_tokens: None,
    }
}

fn map_finish_reason(finish_reason: Option<&str>, has_tool_calls: bool) -> FinishReason {
    let Some(finish_reason) = finish_reason else {
        return if has_tool_calls { FinishReason::ToolCalls } else { FinishReason::Unknown };
    };
    match finish_reason {
        "STOP" => {
            if has_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        "MAX_TOKENS" => FinishReason::Length,
        reason if generate_content::content_filtered(Some(reason)) => FinishReason::ContentFilter,
        "MALFORMED_FUNCTION_CALL"
        | "UNEXPECTED_TOOL_CALL"
        | "NO_IMAGE"
        | "TOO_MANY_TOOL_CALLS"
        | "MISSING_THOUGHT_SIGNATURE"
        | "MALFORMED_RESPONSE" => FinishReason::Error,
        _ => FinishReason::Unknown,
    }
}

/// Parser state threaded through the step machine (port of `ParserState`).
#[derive(Debug, Clone)]
pub struct ParserState {
    route: String,
    provider_metadata_key: String,
    finish_reason: Option<String>,
    has_tool_calls: bool,
    prompt_feedback: Option<Value>,
    usage: Option<Usage>,
    lifecycle: lifecycle::State,
    reasoning_signature: Option<String>,
    text_signature: Option<String>,
    reasoning_id: Option<String>,
    text_id: Option<String>,
    next_reasoning_id: usize,
    next_text_id: usize,
    seen_call_ids: HashSet<String>,
}

/// Process-global fallback entropy when `/dev/urandom` is unavailable: the
/// wall clock mixed with an atomic counter through an xorshift scramble.
fn fallback_random_bytes() -> [u8; 16] {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut state = nanos ^ count.rotate_left(32) ^ 0x9e37_79b9_7f4a_7c15;
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state >> 24) as u8;
    }
    bytes
}

fn random_bytes() -> [u8; 16] {
    use std::io::Read as _;
    let mut bytes = [0u8; 16];
    let filled = std::fs::File::open("/dev/urandom")
        .is_ok_and(|mut file| file.read_exact(&mut bytes).is_ok());
    if filled {
        bytes
    } else {
        fallback_random_bytes()
    }
}

/// A globally unique fallback id for function calls that arrive without one
/// (Gemini 1.5): `tool_` + a UUIDv4-shaped hex string, mirroring
/// `crypto.randomUUID().replaceAll("-", "")`.
fn fresh_tool_id() -> String {
    let mut bytes = random_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut id = String::from("tool_");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

fn invalid_stream_event(route_field: &str, route_label: &str, body: String) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields {
            message: format!("Invalid {route_label} stream event"),
            body: Some(body),
            ..Default::default()
        },
        classification: None,
        route: Some(route_field.to_string()),
    })
}

/// The finish flush emitted when the framed stream ends (port of `finish`,
/// wired as `onHalt`). An explicit `null` blockReason normalizes back to
/// absent; with neither a finish reason nor usage there is nothing to flush.
fn finish_events(state: &ParserState) -> Vec<LLMEvent> {
    let prompt_block_reason = if state.finish_reason.is_none() {
        state
            .prompt_feedback
            .as_ref()
            .and_then(|feedback| feedback.get("blockReason"))
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    };
    let finish_reason = state.finish_reason.clone().or_else(|| prompt_block_reason.clone());
    if finish_reason.is_none() && state.usage.is_none() {
        return Vec::new();
    }

    let key = &state.provider_metadata_key;
    let mut events = Vec::new();
    let mut lifecycle_state = state.lifecycle.clone();
    if let Some(reasoning_id) = &state.reasoning_id {
        lifecycle_state = lifecycle::reasoning_end(
            &lifecycle_state,
            &mut events,
            reasoning_id,
            state
                .reasoning_signature
                .as_deref()
                .map(|signature| thought_signature_metadata(key, signature)),
            None,
        );
    }
    if let Some(text_id) = &state.text_id {
        lifecycle_state = lifecycle::text_end(
            &lifecycle_state,
            &mut events,
            text_id,
            state
                .text_signature
                .as_deref()
                .map(|signature| thought_signature_metadata(key, signature)),
            None,
        );
    }
    lifecycle::finish(
        &lifecycle_state,
        &mut events,
        lifecycle::FinishInput {
            reason: FinishReasonDetails {
                normalized: if prompt_block_reason.is_some() {
                    FinishReason::ContentFilter
                } else {
                    map_finish_reason(finish_reason.as_deref(), state.has_tool_calls)
                },
                raw: finish_reason,
                category: None,
                explanation: None,
            },
            usage: state.usage.clone(),
            provider_metadata: state
                .prompt_feedback
                .as_ref()
                .map(|feedback| provider_metadata(key, json!({ "promptFeedback": feedback }))),
        },
    );
    events
}

/// Translate one decoded frame into emitted events plus the next state
/// (port of the gemini `step` machine; the frame string is decoded with
/// [`decode_event`] first — a decode failure is the route-level
/// `Invalid google/gemini stream event` error carrying the raw frame).
fn step(state: ParserState, frame: &str) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let event = decode_event(frame)
        .map_err(|()| invalid_stream_event(ADAPTER, &state.route, frame.to_string()))?;

    if let Some(error) = event.error.as_ref() {
        if error.is_object() {
            let record = error.as_object().unwrap();
            let message = record
                .get("message")
                .and_then(Value::as_str)
                .filter(|message| !message.is_empty())
                .or_else(|| {
                    record.get("status").and_then(Value::as_str).filter(|status| !status.is_empty())
                })
                .unwrap_or("Gemini provider error")
                .to_string();
            let status = record.get("code").and_then(Value::as_u64).map(|code| code as u16);
            let reason = crate::provider_error::classify_provider_failure(
                crate::provider_error::ProviderFailure {
                    message: &message,
                    status,
                    raw_body: Some(&encode_json(&event.to_json())),
                    data: None,
                    http: None,
                    retry_after_ms: None,
                    rate_limit: None,
                },
            );
            return Err(AiError::new(reason));
        }
        // a present-but-non-record `error` key is an invalid event
        return Err(invalid_stream_event(
            &state.route,
            &state.route,
            encode_json(&event.to_json()),
        ));
    }

    let mut next = state.clone();
    if let Some(feedback) = event.prompt_feedback.clone() {
        next.prompt_feedback = Some(feedback);
    }
    if let Some(usage) = event.usage_metadata.as_ref() {
        next.usage = Some(map_usage(usage, &state.provider_metadata_key));
    }

    let candidate = event.candidates.as_ref().and_then(|candidates| candidates.first());
    if let Some(finish_reason) = candidate.and_then(|candidate| candidate.finish_reason.as_deref())
    {
        if map_finish_reason(Some(finish_reason), state.has_tool_calls) == FinishReason::Error {
            return Err(AiError::new(AiErrorReason::InvalidProviderOutput {
                fields: ReasonFields {
                    message: format!("Gemini stopped with {finish_reason}"),
                    body: Some(encode_json(&event.to_json())),
                    ..Default::default()
                },
                classification: None,
                route: Some(state.route.clone()),
            }));
        }
    }
    let Some(candidate) = candidate else {
        return Ok((next, Vec::new()));
    };
    let Some(content) = candidate.content.as_ref() else {
        if candidate.finish_reason.is_some() {
            next.finish_reason = candidate.finish_reason.clone();
        }
        return Ok((next, Vec::new()));
    };

    let mut events: Vec<LLMEvent> = Vec::new();
    let key = state.provider_metadata_key.clone();
    let mut reasoning_signature = next.reasoning_signature.clone();
    let mut text_signature = next.text_signature.clone();
    let mut reasoning_id = next.reasoning_id.clone();
    let mut text_id = next.text_id.clone();
    let mut next_reasoning_id = next.next_reasoning_id;
    let mut next_text_id = next.next_text_id;
    let mut lifecycle_state = next.lifecycle.clone();

    for input in content.parts.iter().flatten() {
        // Unknown part shapes (executableCode, codeExecutionResult, …) are
        // skipped; recognized-but-malformed parts fail the event.
        if input.is_object() {
            let record = input.as_object().unwrap();
            let known = ["text", "inlineData", "functionCall", "functionResponse"]
                .iter()
                .any(|key| record.contains_key(*key));
            if !known {
                continue;
            }
        }
        let Some(part) = decode_content_part(input) else {
            return Err(invalid_stream_event(ADAPTER, &state.route, encode_json(&event.to_json())));
        };
        let signature = match &part {
            ContentPartFrame::Text { signature, .. }
            | ContentPartFrame::InlineData { signature, .. }
            | ContentPartFrame::FunctionCall { signature, .. } => signature.clone(),
            ContentPartFrame::FileData | ContentPartFrame::FunctionResponse => None,
        };
        // Gemini attaches replay signatures to thought parts, visible text,
        // or function calls; each block kind retains the signature attached
        // to its own parts.
        if let ContentPartFrame::Text { thought: Some(true), .. } = &part {
            if let Some(signature) = signature.clone() {
                reasoning_signature = Some(signature);
            }
        } else if let ContentPartFrame::Text { .. } = &part {
            if let Some(signature) = signature.clone() {
                text_signature = Some(signature);
            }
        }
        match &part {
            // Image-capable Gemini models return generated images as inline
            // data parts; surface them as first-class output.
            ContentPartFrame::InlineData { mime_type, data, .. } => {
                lifecycle_state = lifecycle::step_start(&lifecycle_state, &mut events);
                events.push(LLMEvent::Media {
                    media: media::base64(data.clone(), mime_type.clone()),
                    provider_metadata: signature
                        .as_deref()
                        .map(|signature| thought_signature_metadata(&key, signature)),
                });
            }
            ContentPartFrame::Text { text, .. } if !text.is_empty() => {
                if matches!(&part, ContentPartFrame::Text { thought: Some(true), .. }) {
                    if let Some(open_text) = text_id.take() {
                        lifecycle_state = lifecycle::text_end(
                            &lifecycle_state,
                            &mut events,
                            &open_text,
                            text_signature
                                .as_deref()
                                .map(|signature| thought_signature_metadata(&key, signature)),
                            None,
                        );
                        text_signature = None;
                    }
                    if reasoning_id.is_none() {
                        reasoning_id = Some(format!("reasoning-{next_reasoning_id}"));
                        next_reasoning_id += 1;
                    }
                    // TS `Lifecycle.reasoningDelta` forwards the delta's
                    // providerMetadata into `reasoningStart`, so a signature
                    // on the first thought part also lands on the block's
                    // start event (unlike text deltas, which start
                    // metadata-free). Mirror that asymmetry exactly instead
                    // of going through `lifecycle::reasoning_delta`.
                    let signature_metadata = signature
                        .as_deref()
                        .map(|signature| thought_signature_metadata(&key, signature));
                    lifecycle_state = lifecycle::reasoning_start(
                        &lifecycle_state,
                        &mut events,
                        reasoning_id.as_deref().unwrap_or_default(),
                        signature_metadata.clone(),
                    );
                    events.push(LLMEvent::ReasoningDelta {
                        id: reasoning_id.clone().expect("assigned above"),
                        text: text.clone(),
                        provider_metadata: signature_metadata,
                    });
                } else {
                    if let Some(open_reasoning) = reasoning_id.take() {
                        lifecycle_state = lifecycle::reasoning_end(
                            &lifecycle_state,
                            &mut events,
                            &open_reasoning,
                            reasoning_signature
                                .as_deref()
                                .map(|signature| thought_signature_metadata(&key, signature)),
                            None,
                        );
                        reasoning_signature = None;
                    }
                    if text_id.is_none() {
                        text_id = Some(format!("text-{next_text_id}"));
                        next_text_id += 1;
                    }
                    lifecycle_state = lifecycle::text_delta(
                        &lifecycle_state,
                        &mut events,
                        text_id.as_deref().unwrap_or_default(),
                        text,
                        text_signature
                            .as_deref()
                            .map(|signature| thought_signature_metadata(&key, signature)),
                    );
                    text_signature = None;
                }
            }
            ContentPartFrame::FunctionCall { id, name, args, .. } => {
                let input = args.clone().unwrap_or_else(|| json!({}));
                // Gemini 2.0+ supplies a unique function call id; when
                // omitted (Gemini 1.5) a globally unique id is generated. A
                // repeated supplier id would replay as two identical calls,
                // so only its first occurrence keeps it; `null` normalizes
                // to absent so the fallback applies.
                let supplied = id.clone();
                let duplicate = supplied.as_ref().is_some_and(|id| next.seen_call_ids.contains(id));
                if let Some(id) = supplied.as_ref() {
                    next.seen_call_ids.insert(id.clone());
                }
                let id = match &supplied {
                    Some(id) if !duplicate => id.clone(),
                    _ => fresh_tool_id(),
                };
                if let Some(open_reasoning) = reasoning_id.take() {
                    lifecycle_state = lifecycle::reasoning_end(
                        &lifecycle_state,
                        &mut events,
                        &open_reasoning,
                        reasoning_signature
                            .as_deref()
                            .map(|signature| thought_signature_metadata(&key, signature)),
                        None,
                    );
                    reasoning_signature = None;
                }
                if let Some(open_text) = text_id.take() {
                    lifecycle_state = lifecycle::text_end(
                        &lifecycle_state,
                        &mut events,
                        &open_text,
                        text_signature
                            .as_deref()
                            .map(|signature| thought_signature_metadata(&key, signature)),
                        None,
                    );
                    text_signature = None;
                }
                lifecycle_state = lifecycle::step_start(&lifecycle_state, &mut events);
                events.push(LLMEvent::ToolCall {
                    id,
                    name: name.clone(),
                    namespace: None,
                    input,
                    provider_executed: None,
                    provider_metadata: signature
                        .as_deref()
                        .map(|signature| thought_signature_metadata(&key, signature)),
                });
                next.has_tool_calls = true;
            }
            _ => {}
        }
    }

    next.lifecycle = lifecycle_state;
    next.reasoning_signature = reasoning_signature;
    next.text_signature = text_signature;
    next.reasoning_id = reasoning_id;
    next.text_id = text_id;
    next.next_reasoning_id = next_reasoning_id;
    next.next_text_id = next_text_id;
    if let Some(finish_reason) = candidate.finish_reason.clone() {
        next.finish_reason = Some(finish_reason);
    }
    Ok((next, events))
}

/// The first candidate's finish reason when the frame carries candidates at
/// all (mirrors the TS `candidate?.finishReason ?? nextState.finishReason`
/// early-return path).
#[allow(dead_code)]
fn candidate_finish_reason(event: &GeminiEventFrame) -> Option<String> {
    event
        .candidates
        .as_ref()
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.finish_reason.clone())
}

// =============================================================================
// Request lowering
// =============================================================================

fn format_content_types(types: &[&str]) -> String {
    match types.len() {
        0 => String::new(),
        1 => types[0].to_string(),
        2 => format!("{} and {}", types[0], types[1]),
        _ => format!("{}, and {}", types[..types.len() - 1].join(", "), types[types.len() - 1]),
    }
}

fn unsupported_content(role: &str, types: &[&str]) -> AiError {
    shared::invalid_request(format!(
        "Gemini {role} messages only support {} content for now",
        format_content_types(types)
    ))
}

/// `String(value)` for tool-result payloads: strings as-is, numbers and
/// booleans via their JSON literal, plain objects as JSON (see
/// [`tool_result_text`]).
fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => items.iter().map(stringify_value).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// Render a tool result into response text (port of `toolResultText`): text
/// results as their string, structured errors and json results as JSON.
fn tool_result_text(result: &ToolResultValue) -> String {
    match result {
        ToolResultValue::Text { value } | ToolResultValue::Error { value } => {
            let structured = value.is_array() || value.is_object();
            if structured {
                encode_json(value)
            } else {
                stringify_value(value)
            }
        }
        ToolResultValue::Json { value } => encode_json(value),
        ToolResultValue::Content { .. } => String::new(),
    }
}

/// The `thoughtSignature` carried in a part's providerMetadata.
fn part_thought_signature(metadata: Option<&ProviderMetadata>, key: &str) -> Option<String> {
    metadata?.get(key)?.get("thoughtSignature").and_then(Value::as_str).map(str::to_string)
}

/// Tool schemas go in `parametersJsonSchema`, which accepts (sanitized)
/// standard JSON Schema.
fn lower_tool(tool: &ToolDefinition) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "parametersJsonSchema": tool.input_schema,
    })
}

fn lower_tool_config(tool_choice: &ToolChoice) -> Result<Value, AiError> {
    match tool_choice {
        ToolChoice::Auto { .. } => Ok(json!({ "functionCallingConfig": { "mode": "AUTO" } })),
        ToolChoice::None { .. } => Ok(json!({ "functionCallingConfig": { "mode": "NONE" } })),
        ToolChoice::Required { .. } => Ok(json!({ "functionCallingConfig": { "mode": "ANY" } })),
        ToolChoice::Named { name, .. } => {
            if name.is_empty() {
                Err(shared::invalid_request("Gemini tool choice requires a tool name"))
            } else {
                Ok(
                    json!({ "functionCallingConfig": { "mode": "ANY", "allowedFunctionNames": [name] } }),
                )
            }
        }
    }
}

fn lower_content_part(part: &ContentPart) -> Result<Value, AiError> {
    match part {
        ContentPart::Text { text, .. } => Ok(json!({ "text": text })),
        ContentPart::Media { media, .. } => generate_content::media_part("Gemini", media),
        _ => unreachable!("lower_content_part receives pre-validated parts"),
    }
}

/// A text part with an optional replay signature.
fn text_part(text: &str, signature: Option<&str>) -> Value {
    let mut part = Map::new();
    part.insert("text".into(), json!(text));
    if let Some(signature) = signature {
        part.insert("thoughtSignature".into(), json!(signature));
    }
    Value::Object(part)
}

/// Whether any part in a lowered turn is a `functionResponse` part.
fn has_function_response(parts: &[Value]) -> bool {
    parts.iter().any(|part| part.get("functionResponse").is_some())
}

/// Lower one message list into `contents` (port of `lowerMessages`).
fn lower_messages(request: &LLMRequest) -> Result<Vec<Value>, AiError> {
    let mut contents: Vec<Value> = Vec::new();
    let metadata_key = request
        .model
        .route
        .provider_metadata_key
        .clone()
        .unwrap_or_else(|| request.model.provider.as_str().to_string());
    let omit_call_ids = omits_function_call_ids(request.model.id.as_str());
    let legacy_tool_media = routes_legacy_tool_media(request.model.id.as_str());
    let mut pending_media: Vec<Value> = Vec::new();

    for message in &request.messages {
        if message.role != MessageRole::Tool && !pending_media.is_empty() {
            let mut parts = vec![json!({ "text": "Attached media from tool result:" })];
            parts.append(&mut pending_media);
            contents.push(json!({ "role": "user", "parts": parts }));
        }
        match message.role {
            MessageRole::System => {
                let mut text = String::new();
                for part in &message.content {
                    if !matches!(part, ContentPart::Text { .. }) {
                        return Err(unsupported_content("system", &["text"]));
                    }
                    let ContentPart::Text { text: part_text, .. } = part else {
                        unreachable!();
                    };
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(part_text);
                }
                let wrapped = shared::wrap_system_update([text.as_str()]);
                let previous = contents.last().cloned();
                match previous {
                    Some(previous)
                        if previous.get("role").and_then(Value::as_str) == Some("user")
                            && !has_function_response(
                                previous
                                    .get("parts")
                                    .and_then(Value::as_array)
                                    .map(Vec::as_slice)
                                    .unwrap_or_default(),
                            ) =>
                    {
                        let mut parts = previous
                            .get("parts")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        parts.push(json!({ "text": wrapped }));
                        let mut next = Map::new();
                        next.insert("role".into(), json!("user"));
                        next.insert("parts".into(), Value::Array(parts));
                        *contents.last_mut().expect("previous exists") = Value::Object(next);
                    }
                    _ => contents.push(json!({ "role": "user", "parts": [{ "text": wrapped }] })),
                }
            }
            MessageRole::User => {
                let mut parts: Vec<Value> = Vec::new();
                for part in &message.content {
                    match part {
                        ContentPart::Text { .. } | ContentPart::Media { .. } => {
                            parts.push(lower_content_part(part)?);
                        }
                        _ => return Err(unsupported_content("user", &["text", "media"])),
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            MessageRole::Assistant => {
                let mut parts: Vec<Value> = Vec::new();
                // Parallel Gemini 3 calls may carry one signature on the
                // first call; unsigned sibling calls are valid.
                let mut has_signed_tool_call = false;
                for part in &message.content {
                    match part {
                        ContentPart::Text { text, provider_metadata, .. } => {
                            parts.push(text_part(
                                text,
                                part_thought_signature(provider_metadata.as_ref(), &metadata_key)
                                    .as_deref(),
                            ));
                        }
                        ContentPart::Media { media: _, provider_metadata, .. } => {
                            let mut lowered = lower_content_part(part)?;
                            if let Some(signature) =
                                part_thought_signature(provider_metadata.as_ref(), &metadata_key)
                            {
                                lowered
                                    .as_object_mut()
                                    .expect("media part is an object")
                                    .insert("thoughtSignature".into(), json!(signature));
                            }
                            parts.push(lowered);
                        }
                        ContentPart::Reasoning { text, provider_metadata, .. } => {
                            let mut entry = Map::new();
                            entry.insert("text".into(), json!(text));
                            entry.insert("thought".into(), json!(true));
                            if let Some(signature) =
                                part_thought_signature(provider_metadata.as_ref(), &metadata_key)
                            {
                                entry.insert("thoughtSignature".into(), json!(signature));
                            }
                            parts.push(Value::Object(entry));
                        }
                        ContentPart::ToolCall { id, name, input, provider_metadata, .. } => {
                            let signature =
                                part_thought_signature(provider_metadata.as_ref(), &metadata_key);
                            let effective = signature.clone().or_else(|| {
                                (requires_thought_signature_fallback(request.model.id.as_str())
                                    && !has_signed_tool_call)
                                    .then(|| SKIP_THOUGHT_SIGNATURE_VALIDATOR.to_string())
                            });
                            let mut call = Map::new();
                            let mut function_call = Map::new();
                            if !omit_call_ids {
                                function_call.insert("id".into(), json!(id));
                            }
                            function_call.insert("name".into(), json!(name));
                            function_call.insert("args".into(), input.clone());
                            call.insert("functionCall".into(), Value::Object(function_call));
                            if let Some(signature) = &effective {
                                call.insert("thoughtSignature".into(), json!(signature));
                            }
                            // Only a genuine provider signature marks the
                            // batch as signed — the bypass sentinel does not.
                            if signature.is_some() {
                                has_signed_tool_call = true;
                            }
                            parts.push(Value::Object(call));
                        }
                        _ => {
                            return Err(unsupported_content(
                                "assistant",
                                &["text", "reasoning", "tool-call", "media"],
                            ));
                        }
                    }
                }
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            MessageRole::Tool => {
                let mut parts: Vec<Value> = Vec::new();
                for part in &message.content {
                    let ContentPart::ToolResult { id, name, result, .. } = part else {
                        return Err(unsupported_content("tool", &["tool-result"]));
                    };
                    let mut function_response = Map::new();
                    if !omit_call_ids {
                        function_response.insert("id".into(), json!(id));
                    }
                    function_response.insert("name".into(), json!(name));
                    let ToolResultValue::Content { value } = result else {
                        function_response.insert(
                            "response".into(),
                            json!({ "name": name, "content": tool_result_text(result) }),
                        );
                        parts.push(json!({ "functionResponse": Value::Object(function_response) }));
                        continue;
                    };
                    let mut text: Vec<String> = Vec::new();
                    let mut media_parts: Vec<Value> = Vec::new();
                    for item in &value.0 {
                        if item.get("type").and_then(Value::as_str) == Some("text") {
                            text.push(
                                item.get("text")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                            );
                            continue;
                        }
                        // Tool files carry a data URL, an http(s) URL, or raw
                        // base64 in `uri`; the declared `mime` wins over any
                        // data-URL prefix so tool authors control the type
                        // the model sees (port of `toolFileMedia`).
                        let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
                        let mime = item
                            .get("mime")
                            .and_then(Value::as_str)
                            .unwrap_or("application/octet-stream");
                        let asset = match media::parse_data_url(uri) {
                            Some(mut asset) => {
                                asset.media_type = mime.to_string();
                                if let crate::schema::media::Source::Base64 { media_type, .. } =
                                    &mut asset.source
                                {
                                    *media_type = mime.to_string();
                                }
                                asset
                            }
                            None => {
                                if uri.starts_with("http://") || uri.starts_with("https://") {
                                    media::url(uri, Some(mime.to_string()), None)
                                } else {
                                    media::base64(uri, mime)
                                }
                            }
                        };
                        let inline = asset.inline().ok_or_else(|| {
                            shared::invalid_request(format!(
                                "Gemini requires inline media (bytes or base64); {} sources must be materialized first",
                                match &asset.source {
                                    crate::schema::media::Source::Url { .. } => "url",
                                    crate::schema::media::Source::Ref { .. } => "ref",
                                    _ => "other",
                                }
                            ))
                        })?;
                        media_parts.push(json!({
                            "inlineData": { "mimeType": inline.mime, "data": inline.base64 }
                        }));
                    }
                    if legacy_tool_media && !media_parts.is_empty() {
                        pending_media.append(&mut media_parts);
                    }
                    function_response.insert(
                        "response".into(),
                        json!({ "name": name, "content": text.join("\n") }),
                    );
                    if !legacy_tool_media && !media_parts.is_empty() {
                        function_response.insert("parts".into(), Value::Array(media_parts));
                    }
                    parts.push(json!({ "functionResponse": Value::Object(function_response) }));
                }
                // Gemini requires every response to a parallel call batch
                // in one user turn: consecutive tool results join the open
                // function-response turn.
                let previous = contents.last().cloned();
                match previous {
                    Some(previous)
                        if previous.get("role").and_then(Value::as_str) == Some("user")
                            && has_function_response(
                                previous
                                    .get("parts")
                                    .and_then(Value::as_array)
                                    .map(Vec::as_slice)
                                    .unwrap_or_default(),
                            ) =>
                    {
                        let mut existing = previous
                            .get("parts")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        existing.append(&mut parts);
                        let mut next = Map::new();
                        next.insert("role".into(), json!("user"));
                        next.insert("parts".into(), Value::Array(existing));
                        *contents.last_mut().expect("previous exists") = Value::Object(next);
                    }
                    _ => {
                        let mut next = Map::new();
                        next.insert("role".into(), json!("user"));
                        next.insert("parts".into(), Value::Array(parts));
                        contents.push(Value::Object(next));
                    }
                }
            }
        }
    }

    if !pending_media.is_empty() {
        let mut parts = vec![json!({ "text": "Attached media from tool result:" })];
        parts.append(&mut pending_media);
        contents.push(json!({ "role": "user", "parts": parts }));
    }
    Ok(contents)
}

/// Flatten namespaces off tool calls/results in the message list (port of
/// `flattenToolRequest`'s message rewrite: `ns_name`, namespace dropped).
fn flatten_namespaced_messages(messages: &[Message]) -> Vec<Message> {
    let mut next = Vec::with_capacity(messages.len());
    for message in messages {
        let mut changed = false;
        let mut content: Vec<ContentPart> = Vec::with_capacity(message.content.len());
        for part in &message.content {
            match part {
                ContentPart::ToolCall { namespace: Some(namespace), .. }
                | ContentPart::ToolResult { namespace: Some(namespace), .. } => {
                    let namespace = namespace.clone();
                    let renamed = rename_namespaced(part, &namespace);
                    content.push(renamed);
                    changed = true;
                }
                other => content.push(other.clone()),
            }
        }
        if changed {
            let mut rewritten = message.clone();
            rewritten.content = content;
            next.push(rewritten);
        } else {
            next.push(message.clone());
        }
    }
    next
}

fn rename_namespaced(part: &ContentPart, namespace: &str) -> ContentPart {
    match part {
        ContentPart::ToolCall { id, name, input, provider_metadata, .. } => ContentPart::ToolCall {
            id: id.clone(),
            name: format!("{namespace}_{name}"),
            namespace: None,
            input: input.clone(),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: provider_metadata.clone(),
        },
        ContentPart::ToolResult { id, name, result, provider_metadata, .. } => {
            ContentPart::ToolResult {
                id: id.clone(),
                name: format!("{namespace}_{name}"),
                namespace: None,
                result: result.clone(),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: provider_metadata.clone(),
            }
        }
        other => other.clone(),
    }
}

/// Build the Gemini body from a common request (port of `fromRequest`).
/// Tool schemas are projected through the resolved sanitizer here — TS runs
/// `ToolSchemaProjection.tools` in `prepareRequest` before any protocol
/// builds its body; the Rust route exposes the plumbing and the protocol
/// applies it, which lowers to identical bodies.
fn body_from(request: &LLMRequest) -> Result<Value, AiError> {
    let projected = sanitizer::project_tools(request.tools.clone(), &request.model);
    let flattened = shared::flatten_tools(&projected);
    let has_tools = !flattened.is_empty();
    let generation = request.generation.clone().unwrap_or_default();
    let options = decode_options(request.provider_options.as_ref().unwrap_or(&Map::new()));

    let thinking_config = options.thinking_config.as_ref().map(|config| {
        let mut entry = Map::new();
        if let Some(budget) = config.thinking_budget {
            entry.insert(
                "thinkingBudget".into(),
                json!(fit_thinking_budget(budget, generation.max_tokens, MIN_THINKING_BUDGET)),
            );
        }
        entry.insert("includeThoughts".into(), json!(config.include_thoughts.unwrap_or(true)));
        if let Some(level) = &config.thinking_level {
            entry.insert("thinkingLevel".into(), json!(level));
        }
        Value::Object(entry)
    });

    let mut generation_config = Map::new();
    if let Some(max_tokens) = generation.max_tokens {
        generation_config.insert("maxOutputTokens".into(), json!(max_tokens));
    }
    if let Some(temperature) = generation.temperature {
        generation_config.insert("temperature".into(), json!(temperature));
    }
    if let Some(top_p) = generation.top_p {
        generation_config.insert("topP".into(), json!(top_p));
    }
    if let Some(top_k) = generation.top_k {
        generation_config.insert("topK".into(), json!(top_k));
    }
    if let Some(frequency_penalty) = generation.frequency_penalty {
        generation_config.insert("frequencyPenalty".into(), json!(frequency_penalty));
    }
    if let Some(presence_penalty) = generation.presence_penalty {
        generation_config.insert("presencePenalty".into(), json!(presence_penalty));
    }
    if let Some(seed) = generation.seed {
        generation_config.insert("seed".into(), json!(seed));
    }
    if let Some(stop) = &generation.stop {
        generation_config.insert("stopSequences".into(), json!(stop));
    }
    if let Some(thinking_config) = thinking_config {
        generation_config.insert("thinkingConfig".into(), thinking_config);
    }

    let messages = flatten_namespaced_messages(&request.messages);
    let mut lowered = request.clone();
    lowered.messages = messages;

    let mut body = Map::new();
    if let Some(cached_content) = &options.cached_content {
        body.insert("cachedContent".into(), json!(cached_content));
    }
    body.insert("contents".into(), Value::Array(lower_messages(&lowered)?));
    if let Some(safety_settings) = &options.safety_settings {
        body.insert(
            "safetySettings".into(),
            Value::Array(
                safety_settings
                    .iter()
                    .map(|(category, threshold)| {
                        json!({ "category": category, "threshold": threshold })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(service_tier) = &options.service_tier {
        body.insert("serviceTier".into(), json!(service_tier));
    }
    if !request.system.is_empty() {
        body.insert(
            "systemInstruction".into(),
            json!({ "parts": request.system.iter().map(|part| json!({ "text": part.text })).collect::<Vec<_>>() }),
        );
    }
    if has_tools {
        body.insert(
            "tools".into(),
            json!([{ "functionDeclarations": flattened.iter().map(lower_tool).collect::<Vec<_>>() }]),
        );
    }
    if has_tools {
        if let Some(tool_choice) = &request.tool_choice {
            body.insert("toolConfig".into(), lower_tool_config(tool_choice)?);
        }
    }
    if !generation_config.is_empty() {
        body.insert("generationConfig".into(), Value::Object(generation_config));
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Protocol and route
// =============================================================================

/// The Gemini protocol (port of `Gemini.protocol`) — request body
/// construction and the streaming state machine shared by Google AI Studio
/// and Vertex Gemini.
pub struct GeminiProtocol;

impl Protocol for GeminiProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        body_from(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState {
            route: format!("{}/{}", request.model.provider.as_str(), request.model.route.id),
            provider_metadata_key: request
                .model
                .route
                .provider_metadata_key
                .clone()
                .unwrap_or_else(|| request.model.provider.as_str().to_string()),
            finish_reason: None,
            has_tool_calls: false,
            prompt_feedback: None,
            usage: None,
            lifecycle: lifecycle::State::initial(),
            reasoning_signature: None,
            text_signature: None,
            reasoning_id: None,
            text_id: None,
            next_reasoning_id: 0,
            next_text_id: 0,
            seen_call_ids: HashSet::new(),
        })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("GeminiProtocol state");
        let (next, events) = step(state, frame)?;
        Ok((Box::new(next), events))
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("GeminiProtocol state");
        let events = finish_events(&state);
        Ok((Box::new(state), events))
    }

    /// Gemini's schema rules are this API's default, including for tuned
    /// endpoints whose IDs do not name Gemini.
    fn sanitizer(&self) -> Option<SanitizerCompatibility> {
        Some(SanitizerCompatibility::Gemini)
    }
}

/// The Gemini route (port of `Gemini.route`): the model id is embedded in
/// the path and SSE framing is pinned at the URL level (`?alt=sse`).
/// Auth is [`auth::Definition::none`] — the Google facade wraps it with the
/// `x-goog-api-key` header (see [`google_route`]).
pub fn gemini_route(transport: SharedTransport) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("google".to_string()),
        provider_metadata_key: Some("google".to_string()),
        protocol: Arc::new(GeminiProtocol),
        endpoint: Endpoint::dynamic_path(|input| {
            generate_content::path(input.request.model.id.as_str(), true)
        })
        .with_base_url(DEFAULT_BASE_URL),
        auth: None,
        framing: Framing::sse(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

/// Google provider auth (port of the `auth` helper in
/// `src/providers/google.ts`): explicit auth wins, else the configured API
/// key, else `GOOGLE_GENERATIVE_AI_API_KEY`, applied as the
/// `x-goog-api-key` header.
#[must_use]
pub fn google_auth(api_key: Option<Secret>) -> auth::Definition {
    auth::or_else(&auth::optional(api_key), &auth::config("GOOGLE_GENERATIVE_AI_API_KEY"))
        .header("x-goog-api-key")
}

/// Google-facade configuration for [`google_route`].
pub struct GoogleConfig {
    pub api_key: Option<Secret>,
    pub base_url: Option<String>,
}

/// The configured Google Gemini route (port of `Google.configure`'s
/// `configuredRoute`: base URL override + the api-key auth chain).
#[must_use]
pub fn google_route(transport: SharedTransport, config: GoogleConfig) -> Route {
    gemini_route(transport).with(RoutePatch {
        endpoint: config.base_url.map(|base_url| crate::route::EndpointPatch {
            base_url: Some(base_url),
            path: None,
            query: None,
        }),
        auth: Some(google_auth(config.api_key)),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests;
