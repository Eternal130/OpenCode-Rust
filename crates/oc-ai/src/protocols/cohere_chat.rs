//! The Cohere v2 chat protocol (port of `src/protocols/cohere-chat.ts`).
//!
//! Request side: OpenAI-shaped messages with Cohere's native extensions
//! (`thinking` blocks, `tool_plan`, per-message `tool_call_id`). Stream
//! side: typed SSE events (`message-start`, `content-delta`, `tool-plan-
//! delta`, `tool-call-*`, `message-end`) accumulated through the shared
//! tool stream; `message-end` is the terminal event and a stream that ends
//! without it fails typed (incomplete stream).

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{self, fit_thinking_budget, subtract_tokens, total_tokens};
use crate::protocols::utils::{lifecycle, tool_stream};
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::{Framing, Route, RouteInput, SharedTransport};
use crate::schema::{
    AiError, ContentPart, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest, MessageRole,
    ProviderMetadata, ToolChoice, ToolResultValue, Usage,
};

const ADAPTER: &str = "cohere-chat";
pub const DEFAULT_BASE_URL: &str = "https://api.cohere.com/v2";

// =============================================================================
// Provider options (strict decode — malformed values fail the request)
// =============================================================================

#[derive(Debug, Clone, Default)]
struct ThinkingOptions {
    present: bool,
    kind: Option<String>,
    token_budget: Option<f64>,
}

fn decode_options(options: &crate::schema::ProviderOptions) -> Result<ThinkingOptions, AiError> {
    let Some(thinking) = options.get("thinking") else {
        return Ok(ThinkingOptions::default());
    };
    let present = true;
    let Some(record) = thinking.as_object() else {
        return Err(shared::invalid_request("Expected object, but got something else"));
    };
    let kind = match record.get("type") {
        None => None,
        Some(Value::String(kind)) if kind == "enabled" || kind == "disabled" => Some(kind.clone()),
        Some(_) => {
            return Err(shared::invalid_request(
                "Expected \"enabled\" | \"disabled\", but got something else",
            ));
        }
    };
    let token_budget = match record.get("tokenBudget") {
        None => None,
        Some(Value::Number(number)) => {
            let value = number.as_f64().unwrap_or_default();
            if number.as_i64().is_none() || value <= 0.0 {
                return Err(shared::invalid_request(
                    "Expected a positive integer, but got something else",
                ));
            }
            Some(value)
        }
        Some(_) => {
            return Err(shared::invalid_request(
                "Expected a positive integer, but got something else",
            ));
        }
    };
    Ok(ThinkingOptions { present, kind, token_budget })
}

// =============================================================================
// Request lowering
// =============================================================================

fn unsupported_content(role: &str, types: &[&str]) -> AiError {
    let types = match types.len() {
        0 => String::new(),
        1 => types[0].to_string(),
        2 => format!("{} and {}", types[0], types[1]),
        _ => format!("{}, and {}", types[..types.len() - 1].join(", "), types[types.len() - 1]),
    };
    shared::invalid_request(format!(
        "Cohere Chat {role} messages only support {types} content for now"
    ))
}

/// `String(value)` for tool-result payloads (JS string coercion).
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

/// Render a tool result into response text (port of `toolResultText`).
fn tool_result_text(result: &ToolResultValue) -> String {
    match result {
        ToolResultValue::Text { value } | ToolResultValue::Error { value } => {
            if value.is_array() || value.is_object() {
                shared::encode_json(value)
            } else {
                stringify_value(value)
            }
        }
        ToolResultValue::Json { value } => shared::encode_json(value),
        ToolResultValue::Content { .. } => String::new(),
    }
}

/// The image URL Cohere accepts: the remote URL for `url` assets, the data
/// URL for inline ones (port of the media lowering in `fromRequest`).
fn image_url(media: &crate::schema::media::Asset) -> Result<String, AiError> {
    if let Some(url) = media.url() {
        return Ok(url.to_string());
    }
    media.inline().map_or_else(
        || {
            let source = match &media.source {
                crate::schema::media::Source::Url { .. } => "url",
                crate::schema::media::Source::Ref { .. } => "ref",
                _ => "other",
            };
            Err(shared::invalid_request(format!(
                "Cohere Chat requires inline media (bytes or base64); {source} sources must be materialized first"
            )))
        },
        |inline| Ok(inline.data_url),
    )
}

/// `String(index)` — Cohere keys content blocks by their numeric index.
fn content_id(index: f64) -> String {
    index.to_string()
}

fn body_from(request: &LLMRequest) -> Result<Value, AiError> {
    let options = decode_options(request.provider_options.as_ref().unwrap_or(&Map::new()))?;
    let projected =
        crate::protocols::sanitizer::project_tools(request.tools.clone(), &request.model);
    let flattened = crate::protocols::shared::flatten_tools(&projected);
    let mut messages: Vec<Value> = Vec::new();
    if !request.system.is_empty() {
        let content = if request.system.len() == 1 {
            json!(request.system[0].text)
        } else {
            Value::Array(
                request
                    .system
                    .iter()
                    .map(|part| json!({ "type": "text", "text": part.text }))
                    .collect(),
            )
        };
        messages.push(json!({ "role": "system", "content": content }));
    }

    for message in &request.messages {
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
                messages.push(json!({
                    "role": "user",
                    "content": shared::wrap_system_update([text.as_str()])
                }));
            }
            MessageRole::Tool => {
                for part in &message.content {
                    let ContentPart::ToolResult { id, name, result, .. } = part else {
                        return Err(unsupported_content("tool", &["tool-result"]));
                    };
                    if let ToolResultValue::Content { value } = result {
                        if value
                            .0
                            .iter()
                            .any(|item| item.get("type").and_then(Value::as_str) == Some("file"))
                        {
                            return Err(shared::invalid_request(
                                "Cohere Chat does not support file content in tool results",
                            ));
                        }
                    }
                    let _ = name;
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": tool_result_text(result),
                    }));
                }
            }
            role @ (MessageRole::User | MessageRole::Assistant) => {
                let mut content: Vec<Value> = Vec::new();
                let mut calls: Vec<Value> = Vec::new();
                let mut plans: Vec<String> = Vec::new();
                for part in &message.content {
                    match part {
                        ContentPart::Text { text, .. } => {
                            content.push(json!({ "type": "text", "text": text }));
                        }
                        ContentPart::Reasoning { text, provider_metadata, .. }
                            if role == MessageRole::Assistant =>
                        {
                            let tool_plan = provider_metadata
                                .as_ref()
                                .and_then(|metadata| metadata.get("cohere"))
                                .and_then(|cohere| cohere.get("toolPlan"))
                                == Some(&Value::Bool(true));
                            if tool_plan {
                                plans.push(text.clone());
                            } else {
                                content.push(json!({ "type": "thinking", "thinking": text }));
                            }
                        }
                        ContentPart::ToolCall { id, name, input, .. }
                            if role == MessageRole::Assistant =>
                        {
                            calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": { "name": name, "arguments": shared::encode_json(input) }
                            }));
                        }
                        ContentPart::Media { media, .. }
                            if role == MessageRole::User
                                && media.media_type.starts_with("image/") =>
                        {
                            let url = image_url(media)?;
                            content
                                .push(json!({ "type": "image_url", "image_url": { "url": url } }));
                        }
                        _ => {
                            return Err(unsupported_content(
                                if role == MessageRole::User { "user" } else { "assistant" },
                                if role == MessageRole::User {
                                    &["text", "media"]
                                } else {
                                    &["text", "reasoning", "tool-call"]
                                },
                            ));
                        }
                    }
                }
                let mut entry = Map::new();
                entry.insert(
                    "role".into(),
                    json!(if role == MessageRole::User { "user" } else { "assistant" }),
                );
                if !content.is_empty() {
                    entry.insert("content".into(), Value::Array(content));
                }
                if !calls.is_empty() {
                    entry.insert("tool_calls".into(), Value::Array(calls));
                }
                if !plans.is_empty() {
                    entry.insert("tool_plan".into(), json!(plans.concat()));
                }
                messages.push(Value::Object(entry));
            }
        }
    }

    let selected = match &request.tool_choice {
        Some(ToolChoice::Named { name, .. }) => Some(name.clone()),
        _ => None,
    };
    let tools: Vec<_> = match &selected {
        None => flattened.clone(),
        Some(name) => flattened.iter().filter(|tool| &tool.name == name).cloned().collect(),
    };
    if selected.is_some() && tools.is_empty() {
        return Err(shared::invalid_request("Cohere Chat tool choice must name an available tool"));
    }
    if tools.iter().any(|tool| tool.native.is_some()) {
        return Err(shared::invalid_request("Cohere Chat does not support provider-defined tools"));
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(request.model.id.as_str()));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), json!(true));
    if !tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    // TOOL_CHOICE = { auto: undefined, none: "NONE", required: "REQUIRED", tool: "REQUIRED" }
    let tool_choice = match &request.tool_choice {
        None | Some(ToolChoice::Auto { .. }) => None,
        Some(ToolChoice::None { .. }) => Some("NONE"),
        Some(ToolChoice::Required { .. } | ToolChoice::Named { .. }) => Some("REQUIRED"),
    };
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".into(), json!(tool_choice));
    }
    if options.present {
        let kind = options.kind.clone().unwrap_or_else(|| "enabled".to_string());
        let token_budget = options.token_budget;
        let mut entry = Map::new();
        entry.insert("type".into(), json!(kind));
        if let Some(budget) = token_budget {
            // Cohere rejects budgets above max_tokens; fitting also leaves
            // room for the answer.
            entry.insert(
                "token_budget".into(),
                shared::number(fit_thinking_budget(
                    budget,
                    request.generation.as_ref().and_then(|g| g.max_tokens),
                    1.0,
                )),
            );
        }
        body.insert("thinking".into(), Value::Object(entry));
    }
    let generation = request.generation.clone().unwrap_or_default();
    if let Some(max_tokens) = generation.max_tokens {
        body.insert("max_tokens".into(), shared::number(max_tokens));
    }
    if let Some(temperature) = generation.temperature {
        body.insert("temperature".into(), shared::number(temperature));
    }
    if let Some(top_p) = generation.top_p {
        body.insert("p".into(), shared::number(top_p));
    }
    if let Some(top_k) = generation.top_k {
        body.insert("k".into(), shared::number(top_k));
    }
    if let Some(seed) = generation.seed {
        body.insert("seed".into(), shared::number(seed));
    }
    if let Some(stop) = &generation.stop {
        body.insert("stop_sequences".into(), json!(stop));
    }
    if let Some(frequency_penalty) = generation.frequency_penalty {
        body.insert("frequency_penalty".into(), shared::number(frequency_penalty));
    }
    if let Some(presence_penalty) = generation.presence_penalty {
        body.insert("presence_penalty".into(), shared::number(presence_penalty));
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream parsing
// =============================================================================

fn finish_reason(raw: &str) -> FinishReasonDetails {
    let normalized = match raw {
        "COMPLETE" | "STOP_SEQUENCE" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::Length,
        "TOOL_CALL" => FinishReason::ToolCalls,
        "ERROR" | "TIMEOUT" => FinishReason::Error,
        _ => FinishReason::Unknown,
    };
    FinishReasonDetails {
        normalized,
        raw: Some(raw.to_string()),
        category: None,
        explanation: None,
    }
}

/// The decoded Cohere usage record (unknown fields dropped by the schema
/// decode; kept as the provider metadata payload).
#[derive(Debug, Clone, Default)]
struct NativeUsage {
    tokens: Option<Value>,
    billed_units: Option<Value>,
    cached_tokens: Option<f64>,
}

impl NativeUsage {
    fn decode(usage: &Value) -> Option<Self> {
        let record = usage.as_object()?;
        let counts = |value: Option<&Value>| -> Option<Option<Value>> {
            match value {
                None | Some(Value::Null) => Some(None),
                Some(inner @ Value::Object(_)) => Some(Some(inner.clone())),
                Some(_) => None,
            }
        };
        let cached_tokens = match record.get("cached_tokens") {
            None | Some(Value::Null) => None,
            Some(Value::Number(number)) => Some(number.as_f64()?),
            Some(_) => return None,
        };
        Some(NativeUsage {
            tokens: counts(record.get("tokens"))?,
            billed_units: counts(record.get("billed_units"))?,
            cached_tokens,
        })
    }

    fn to_metadata(&self) -> Value {
        let mut out = Map::new();
        if let Some(tokens) = &self.tokens {
            out.insert("tokens".into(), tokens.clone());
        }
        if let Some(billed) = &self.billed_units {
            out.insert("billed_units".into(), billed.clone());
        }
        if let Some(cached) = self.cached_tokens {
            out.insert("cached_tokens".into(), json!(cached));
        }
        Value::Object(out)
    }

    fn field(&self, key: &str) -> Option<f64> {
        self.tokens.as_ref().and_then(|tokens| tokens.get(key)).and_then(Value::as_f64)
    }
}

fn map_usage(usage: &NativeUsage) -> Usage {
    let input = usage.field("input_tokens");
    let output = usage.field("output_tokens");
    Usage {
        input_tokens: input,
        output_tokens: output,
        non_cached_input_tokens: subtract_tokens(input, usage.cached_tokens),
        cache_read_input_tokens: usage.cached_tokens,
        reasoning_tokens: usage.field("reasoning_tokens"),
        total_tokens: total_tokens(input, output, None),
        provider_metadata: Some(cohere_metadata(usage.to_metadata())),
        context_tokens: None,
        cache_write_input_tokens: None,
    }
}

fn cohere_metadata(value: Value) -> ProviderMetadata {
    let inner = value.as_object().map(|map| map.clone().into_iter().collect()).unwrap_or_default();
    std::iter::once(("cohere".to_string(), inner)).collect()
}

/// Parser state (port of `State`).
#[derive(Debug, Clone)]
pub struct ParserState {
    lifecycle: lifecycle::State,
    tools: tool_stream::State,
    finished: bool,
}

/// One decoded stream event (port of the `Event` union; unknown event types
/// fail the frame).
#[derive(Debug, Clone)]
enum CohereEvent {
    MessageStart,
    ContentStart,
    ContentDelta {
        index: f64,
        thinking: Option<String>,
        text: Option<String>,
    },
    ContentEnd {
        index: f64,
    },
    ToolPlanDelta {
        tool_plan: String,
    },
    ToolCallDelta {
        index: f64,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    ToolCallEnd {
        index: f64,
    },
    MessageEnd {
        finish_reason: String,
        usage: Option<NativeUsage>,
    },
    Citation,
}

fn optional_string(record: &Map<String, Value>, key: &str) -> Result<Option<String>, ()> {
    match record.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(()),
    }
}

fn required_number(record: &Map<String, Value>, key: &str) -> Result<f64, ()> {
    record.get(key).and_then(Value::as_f64).ok_or(())
}

fn event_type(value: &Value) -> Result<&str, ()> {
    value.get("type").and_then(Value::as_str).ok_or(())
}

fn message_of(record: &Map<String, Value>) -> Result<&Map<String, Value>, ()> {
    record
        .get("delta")
        .and_then(Value::as_object)
        .and_then(|delta| delta.get("message"))
        .and_then(Value::as_object)
        .ok_or(())
}

fn decode_event(frame: &str) -> Result<CohereEvent, ()> {
    let value: Value = serde_json::from_str(frame).map_err(|_| ())?;
    let record = value.as_object().ok_or(())?;
    match event_type(&value)? {
        "message-start" => Ok(CohereEvent::MessageStart),
        // Lifecycle deltas open blocks on demand and ends are no-ops for
        // closed blocks, so content-start needs no handling of its own —
        // but the frame must still validate against the same schema.
        "content-start" => {
            required_number(record, "index")?;
            let content = message_of(record)?.get("content").and_then(Value::as_object).ok_or(())?;
            optional_string(content, "thinking")?;
            optional_string(content, "text")?;
            Ok(CohereEvent::ContentStart)
        }
        "content-delta" => {
            let index = required_number(record, "index")?;
            let message = message_of(record)?;
            let content = message.get("content").and_then(Value::as_object).ok_or(())?;
            Ok(CohereEvent::ContentDelta {
                index,
                thinking: optional_string(content, "thinking")?,
                text: optional_string(content, "text")?,
            })
        }
        "content-end" => Ok(CohereEvent::ContentEnd { index: required_number(record, "index")? }),
        "tool-plan-delta" => {
            let plan = message_of(record)?.get("tool_plan").and_then(Value::as_str).ok_or(())?;
            Ok(CohereEvent::ToolPlanDelta { tool_plan: plan.to_string() })
        }
        "tool-call-start" | "tool-call-delta" => {
            let index = required_number(record, "index")?;
            let call = message_of(record)?.get("tool_calls").and_then(Value::as_object).ok_or(())?;
            let function = call.get("function").and_then(Value::as_object).ok_or(())?;
            Ok(CohereEvent::ToolCallDelta {
                index,
                id: optional_string(call, "id")?,
                name: optional_string(function, "name")?,
                arguments: optional_string(function, "arguments")?,
            })
        }
        "tool-call-end" => {
            Ok(CohereEvent::ToolCallEnd { index: required_number(record, "index")? })
        }
        "message-end" => {
            let delta = record.get("delta").and_then(Value::as_object).ok_or(())?;
            let finish_reason = delta.get("finish_reason").and_then(Value::as_str).ok_or(())?;
            let usage = match delta.get("usage") {
                None | Some(Value::Null) => None,
                Some(usage) => Some(NativeUsage::decode(usage).ok_or(())?),
            };
            Ok(CohereEvent::MessageEnd { finish_reason: finish_reason.to_string(), usage })
        }
        "citation-start" | "citation-end" => Ok(CohereEvent::Citation),
        _ => Err(()),
    }
}

fn invalid_stream_event(route_label: &str, frame: &str) -> AiError {
    AiError::new(crate::schema::AiErrorReason::InvalidProviderOutput {
        fields: crate::schema::ReasonFields {
            message: format!("Invalid {route_label} stream event"),
            body: Some(frame.to_string()),
            ..Default::default()
        },
        classification: None,
        route: Some(ADAPTER.to_string()),
    })
}

/// Lifecycle deltas open blocks on demand and ends are no-ops for closed
/// blocks, so `content-start` needs no handling of its own.
fn step(
    state: ParserState,
    frame: &str,
    route_label: &str,
) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let event = decode_event(frame).map_err(|()| invalid_stream_event(route_label, frame))?;
    let mut events = Vec::new();
    match event {
        CohereEvent::MessageStart => {
            let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
            Ok((ParserState { lifecycle, ..state }, events))
        }
        CohereEvent::ContentStart => Ok((state, events)),
        CohereEvent::ContentDelta { index, thinking, text } => {
            let id = content_id(index);
            let lifecycle = if let Some(thinking) = thinking {
                lifecycle::reasoning_delta(&state.lifecycle, &mut events, &id, &thinking, None)
            } else {
                lifecycle::text_delta(
                    &state.lifecycle,
                    &mut events,
                    &id,
                    &text.unwrap_or_default(),
                    None,
                )
            };
            Ok((ParserState { lifecycle, ..state }, events))
        }
        CohereEvent::ContentEnd { index } => {
            let id = content_id(index);
            let lifecycle = lifecycle::text_end(
                &lifecycle::reasoning_end(&state.lifecycle, &mut events, &id, None, None),
                &mut events,
                &id,
                None,
                None,
            );
            Ok((ParserState { lifecycle, ..state }, events))
        }
        CohereEvent::ToolPlanDelta { tool_plan } => {
            let lifecycle = lifecycle::reasoning_delta(
                &state.lifecycle,
                &mut events,
                "tool-plan",
                &tool_plan,
                Some(cohere_metadata(json!({ "toolPlan": true }))),
            );
            Ok((ParserState { lifecycle, ..state }, events))
        }
        CohereEvent::ToolCallDelta { index, id, name, arguments } => {
            let key = content_id(index);
            let outcome = tool_stream::append_or_start(
                ADAPTER,
                &state.tools,
                &key,
                tool_stream::Delta {
                    id: id.as_deref(),
                    name: name.as_deref(),
                    text: arguments.as_deref().unwrap_or_default(),
                    provider_metadata: None,
                },
                "Cohere tool call is missing id or name",
            )?;
            Ok((ParserState { tools: outcome.tools, ..state }, outcome.events))
        }
        CohereEvent::ToolCallEnd { index } => {
            let key = content_id(index);
            let outcome = tool_stream::finish(ADAPTER, &state.tools, &key)?;
            Ok((ParserState { tools: outcome.tools, ..state }, outcome.events))
        }
        CohereEvent::MessageEnd { finish_reason: raw, usage } => {
            let pending = tool_stream::finish_all(ADAPTER, &state.tools)?;
            events.extend(pending.events);
            let lifecycle = lifecycle::finish(
                &state.lifecycle,
                &mut events,
                lifecycle::FinishInput {
                    reason: finish_reason(&raw),
                    usage: usage.as_ref().map(map_usage),
                    provider_metadata: None,
                },
            );
            Ok((ParserState { lifecycle, tools: pending.tools, finished: true }, events))
        }
        CohereEvent::Citation => Ok((state, events)),
    }
}

/// The Cohere v2 chat protocol (port of `CohereChat.protocol`).
pub struct CohereChatProtocol;

impl Protocol for CohereChatProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        body_from(request)
    }

    fn initial(&self, _request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState {
            lifecycle: lifecycle::State::initial(),
            tools: tool_stream::State::new(),
            finished: false,
        })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("CohereChatProtocol state");
        let route_label = "cohere/cohere-chat";
        let (next, events) = step(state, frame, route_label)?;
        Ok((Box::new(next), events))
    }

    /// `message-end` is the terminal event (checked on the framed payload).
    fn terminal(&self, frame: &str) -> bool {
        event_type(&serde_json::from_str::<Value>(frame).unwrap_or(Value::Null))
            .is_ok_and(|kind| kind == "message-end")
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("CohereChatProtocol state");
        if state.finished {
            return Ok((Box::new(state), Vec::new()));
        }
        Err(shared::event_error(ADAPTER, "Cohere stream ended without message-end"))
    }
}

/// The Cohere v2 chat route (port of `CohereChat.route`).
pub fn cohere_route(transport: SharedTransport) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("cohere".to_string()),
        provider_metadata_key: Some("cohere".to_string()),
        protocol: Arc::new(CohereChatProtocol),
        endpoint: crate::route::Endpoint::path("/chat").with_base_url(DEFAULT_BASE_URL),
        auth: None,
        framing: Framing::sse(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

#[cfg(test)]
mod tests;
