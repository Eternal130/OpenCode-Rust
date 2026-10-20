//! `google-interactions` (port of
//! `packages/ai/src/protocols/google-interactions.ts`). The Gemini
//! Interactions lane: stateless thought-signature replay,
//! `previous_interaction_id` continuation, and native `function_call` /
//! `function_result` steps over the `/interactions` SSE stream.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{
    self, event_error, flatten_tools, invalid_request, join_text, subtract_tokens,
    wrap_system_update,
};
use crate::protocols::utils::lifecycle::{self};
use crate::protocols::utils::tool_stream::{self, PendingTool};
use crate::provider_error::{classify_provider_failure, provider_error_message, ProviderFailure};
use crate::route::auth::{self, Definition as AuthDefinition, Secret};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{Route, RoutePatch};
use crate::schema::media::Asset;
use crate::schema::{
    AiError, ContentPart, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest, Message,
    MessageRole, ProviderId, ProviderMetadata, SystemPart, ToolChoice, ToolDefinition, Usage,
};

pub const ADAPTER: &str = "google-interactions";
pub const NAME: &str = "Google Interactions";
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

// =============================================================================
// Provider options (google-interactions.ts:30-40)
// =============================================================================

/// The `google-interactions` provider options
/// (`previousInteractionId` / `store` / `thinkingLevel` /
/// `thinkingSummaries` / `serviceTier`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub previous_interaction_id: Option<String>,
    pub store: Option<bool>,
    pub thinking_level: Option<String>,
    pub thinking_summaries: Option<String>,
    pub service_tier: Option<String>,
}

const THINKING_LEVELS: [&str; 4] = ["minimal", "low", "medium", "high"];
const THINKING_SUMMARIES: [&str; 2] = ["auto", "none"];
const SERVICE_TIERS: [&str; 3] = ["standard", "flex", "priority"];

/// `decodeOptions` (lenient validation of the five known keys).
pub fn decode_options(request: &LLMRequest) -> Result<Options, AiError> {
    let Some(options) = request.provider_options.as_ref() else { return Ok(Options::default()) };
    let mut decoded = Options::default();
    if let Some(value) = options.get("previousInteractionId") {
        let Some(text) = value.as_str() else {
            return Err(invalid_request(
                "Google Interactions previousInteractionId must be a string",
            ));
        };
        decoded.previous_interaction_id = Some(text.to_string());
    }
    if let Some(value) = options.get("store") {
        let Some(flag) = value.as_bool() else {
            return Err(invalid_request("Google Interactions store must be a boolean"));
        };
        decoded.store = Some(flag);
    }
    if let Some(value) = options.get("thinkingLevel") {
        let Some(text) = value.as_str() else {
            return Err(invalid_request("Google Interactions thinkingLevel must be a string"));
        };
        if !THINKING_LEVELS.contains(&text) {
            return Err(invalid_request(format!(
                "Google Interactions thinkingLevel must be one of {}",
                THINKING_LEVELS.join(", ")
            )));
        }
        decoded.thinking_level = Some(text.to_string());
    }
    if let Some(value) = options.get("thinkingSummaries") {
        let Some(text) = value.as_str() else {
            return Err(invalid_request("Google Interactions thinkingSummaries must be a string"));
        };
        if !THINKING_SUMMARIES.contains(&text) {
            return Err(invalid_request(format!(
                "Google Interactions thinkingSummaries must be one of {}",
                THINKING_SUMMARIES.join(", ")
            )));
        }
        decoded.thinking_summaries = Some(text.to_string());
    }
    if let Some(value) = options.get("serviceTier") {
        let Some(text) = value.as_str() else {
            return Err(invalid_request("Google Interactions serviceTier must be a string"));
        };
        if !SERVICE_TIERS.contains(&text) {
            return Err(invalid_request(format!(
                "Google Interactions serviceTier must be one of {}",
                SERVICE_TIERS.join(", ")
            )));
        }
        decoded.service_tier = Some(text.to_string());
    }
    Ok(decoded)
}

// =============================================================================
// Request body construction (google-interactions.ts:185-338)
// =============================================================================

/// `mediaContent` (google-interactions.ts:185-202): image/audio/video map
/// to their kind, `application/pdf` and `text/csv` to documents; a
/// google-owned Files reference forwards its uri, everything else goes
/// inline.
fn media_content(asset: &Asset) -> Result<Value, AiError> {
    let kind = asset.kind();
    let kind_name = match kind {
        crate::schema::media::Kind::Image => "image",
        crate::schema::media::Kind::Audio => "audio",
        crate::schema::media::Kind::Video => "video",
        _ => "document",
    };
    let allowed = matches!(
        kind,
        crate::schema::media::Kind::Image
            | crate::schema::media::Kind::Audio
            | crate::schema::media::Kind::Video
    ) || asset.media_type == "application/pdf"
        || asset.media_type == "text/csv";
    if !allowed {
        return Err(invalid_request(format!(
            "Google Interactions does not support {} document input",
            asset.media_type
        )));
    }
    if let crate::schema::media::Source::Ref { provider, id, .. } = &asset.source {
        if *provider == ProviderId::new("google") {
            return Ok(json!({ "type": kind_name, "uri": id, "mime_type": asset.media_type }));
        }
    }
    let inline = asset.inline().ok_or_else(|| {
        invalid_request(format!(
            "Google Interactions requires inline media; {} sources must be materialized first",
            match &asset.source {
                crate::schema::media::Source::Url { .. } => "url",
                crate::schema::media::Source::Ref { .. } => "ref",
                _ => "unknown",
            }
        ))
    })?;
    Ok(json!({ "type": kind_name, "data": inline.base64, "mime_type": inline.mime }))
}

/// The replayable `interactionSignature` a prior assistant part carried in
/// its provider metadata (google-interactions.ts:204-209).
fn signature(metadata: Option<&ProviderMetadata>, key: &str) -> Option<String> {
    metadata?.get(key)?.get("interactionSignature")?.as_str().map(str::to_string)
}

fn text_part(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

/// One media asset's tool-file shape (`ProviderShared.toolFileMedia` —
/// media assets pass through, anything else renders as JSON text).
fn tool_item_content(item: &Value) -> Result<Value, AiError> {
    if item.get("type").and_then(Value::as_str) == Some("file") {
        return media_content(&file_item_asset(item));
    }
    Ok(text_part(item.get("text").and_then(Value::as_str).unwrap_or_default()))
}

/// Rebuild the media asset of a tool-content `file` item (`{uri, mime}` —
/// data URIs stay inline, remote URIs become url sources).
fn file_item_asset(item: &Value) -> Asset {
    let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
    let mime = item.get("mime").and_then(Value::as_str).map(str::to_string);
    if let Some(data) = uri.strip_prefix("data:") {
        let (media_type, base64) = data.split_once(',').unwrap_or(("application/octet-stream", ""));
        let media_type = media_type.strip_suffix(";base64").unwrap_or(media_type);
        return crate::schema::media::base64(base64, media_type);
    }
    crate::schema::media::url(uri, mime.or(Some("application/octet-stream".to_string())), None)
}

/// `toolResultText` (shared.ts): non-object results render as text.
fn tool_result_text(value: &Value) -> String {
    if value.is_string() {
        return value.as_str().unwrap_or_default().to_string();
    }
    serde_json::to_string(value).unwrap_or_default()
}

/// `lowerToolResult` (google-interactions.ts:278-286): a JSON object
/// result passes through; content results lower text and media parts.
fn lower_tool_result(part: &ContentPart) -> Result<Value, AiError> {
    let ContentPart::ToolResult { result, .. } = part else {
        return Ok(Value::Null);
    };
    match result {
        crate::schema::ToolResultValue::Json { value } if value.is_object() => Ok(value.clone()),
        crate::schema::ToolResultValue::Json { value }
        | crate::schema::ToolResultValue::Text { value }
        | crate::schema::ToolResultValue::Error { value } => Ok(json!(tool_result_text(value))),
        crate::schema::ToolResultValue::Content { value } => {
            let items: Vec<Value> =
                value.0.iter().map(tool_item_content).collect::<Result<_, _>>()?;
            Ok(Value::Array(items))
        }
    }
}

/// `lowerMessages` (google-interactions.ts:211-276): consecutive ordinary
/// content stays one native step; tools and thoughts keep their
/// chronology.
fn lower_messages(request: &LLMRequest) -> Result<Vec<Value>, AiError> {
    let key = request
        .model
        .route
        .provider_metadata_key
        .clone()
        .unwrap_or_else(|| request.model.provider.as_str().to_string());
    let mut steps: Vec<Value> = Vec::new();
    for message in &request.messages {
        if message.role == MessageRole::System {
            let part = wrapped_system_message(message);
            steps.push(json!({
                "type": "user_input",
                "content": [text_part(&part)],
            }));
            continue;
        }
        let start = steps.len();
        let append = |steps: &mut Vec<Value>, content: Value| {
            let step_type =
                if message.role == MessageRole::Assistant { "model_output" } else { "user_input" };
            if steps.len() > start {
                if let Some(last) = steps.last_mut() {
                    if last.get("type").and_then(Value::as_str) == Some(step_type) {
                        if let Some(content_list) =
                            last.get_mut("content").and_then(Value::as_array_mut)
                        {
                            content_list.push(content);
                            return;
                        }
                    }
                }
            }
            steps.push(json!({ "type": step_type, "content": [content] }));
        };
        for part in &message.content {
            match part {
                ContentPart::Text { text, .. } => append(&mut steps, text_part(text)),
                ContentPart::Media { media, .. } => {
                    append(&mut steps, media_content(media)?);
                }
                ContentPart::Reasoning { text, provider_metadata, .. }
                    if message.role == MessageRole::Assistant =>
                {
                    // A thought always replays; the summary rides only when
                    // the prior response actually streamed one
                    // (google-interactions.ts:250-256).
                    let mut step = Map::new();
                    step.insert("type".to_string(), json!("thought"));
                    if let Some(sig) = signature(provider_metadata.as_ref(), &key) {
                        step.insert("signature".to_string(), json!(sig));
                    }
                    if !text.is_empty() {
                        step.insert("summary".to_string(), json!([text_part(text)]));
                    }
                    steps.push(Value::Object(step));
                }
                ContentPart::ToolCall { id, name, input, provider_metadata, .. }
                    if message.role == MessageRole::Assistant =>
                {
                    let mut step = Map::new();
                    step.insert("type".to_string(), json!("function_call"));
                    step.insert("id".to_string(), json!(id));
                    step.insert("name".to_string(), json!(name));
                    step.insert("arguments".to_string(), input.clone());
                    if let Some(sig) = signature(provider_metadata.as_ref(), &key) {
                        step.insert("signature".to_string(), json!(sig));
                    }
                    steps.push(Value::Object(step));
                }
                ContentPart::ToolResult { .. } if message.role == MessageRole::Tool => {
                    let ContentPart::ToolResult { id, name, result, .. } = part else {
                        unreachable!();
                    };
                    let mut step = Map::new();
                    step.insert("type".to_string(), json!("function_result"));
                    step.insert("call_id".to_string(), json!(id));
                    step.insert("name".to_string(), json!(name));
                    step.insert("result".to_string(), lower_tool_result(part)?);
                    let is_error = matches!(result, crate::schema::ToolResultValue::Error { .. });
                    if is_error {
                        step.insert("is_error".to_string(), json!(true));
                    }
                    steps.push(Value::Object(step));
                }
                other => {
                    let supported = match message.role {
                        MessageRole::User => "text, media",
                        _ => "text, media, reasoning, tool-call",
                    };
                    let kind = match other {
                        ContentPart::Compaction { .. } => "compaction",
                        ContentPart::Effort { .. } => "effort",
                        _ => "content",
                    };
                    let role = match message.role {
                        MessageRole::System => "system",
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::Tool => "tool",
                    };
                    return Err(invalid_request(format!(
                        "{ADAPTER} does not support {kind} content in a {role} message; supported: {supported}"
                    )));
                }
            }
        }
    }
    Ok(steps)
}

/// `wrappedSystemUpdate` over a mid-conversation system message: each
/// system text part wraps in `<system-update>` markers
/// (shared.ts:170-176).
fn wrapped_system_message(message: &Message) -> String {
    let parts: Vec<&str> = message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    wrap_system_update(parts)
}

/// The request-level system prompt joins unwrapped
/// (`system_instruction`, google-interactions.ts:317).
fn system_instruction(request: &LLMRequest) -> Option<String> {
    if request.system.is_empty() {
        return None;
    }
    let texts: Vec<&str> =
        request.system.iter().map(|part: &SystemPart| part.text.as_str()).collect();
    Some(join_text(texts))
}

/// `matchToolChoice` (google-interactions.ts:301-309).
fn lower_tool_choice(choice: &ToolChoice) -> Result<Value, AiError> {
    match choice {
        ToolChoice::Auto { .. } => Ok(json!("auto")),
        ToolChoice::None { .. } => Ok(json!("none")),
        ToolChoice::Required { .. } => Ok(json!("any")),
        ToolChoice::Named { name, .. } if name.is_empty() => {
            Err(invalid_request(format!("{ADAPTER} tool choice requires a tool name")))
        }
        ToolChoice::Named { name, .. } => {
            Ok(json!({ "allowed_tools": { "mode": "any", "tools": [name] } }))
        }
    }
}

/// `fromRequest` (google-interactions.ts:288-338).
pub fn body_from(request: &LLMRequest) -> Result<Value, AiError> {
    let options = decode_options(request)?;
    let tools = flatten_tools(&request.tools);
    if tools.iter().any(|tool| tool.native.as_ref().is_some_and(|native| !native.is_empty())) {
        return Err(invalid_request("Google Interactions hosted tools are not supported"));
    }
    if let Some(generation) = &request.generation {
        if generation.top_k.is_some()
            || generation.frequency_penalty.is_some()
            || generation.presence_penalty.is_some()
        {
            return Err(invalid_request(
                "Google Interactions does not support topK, frequencyPenalty, or presencePenalty",
            ));
        }
    }
    let choice = match &request.tool_choice {
        Some(choice) => Some(lower_tool_choice(choice)?),
        None => None,
    };
    let generation = request.generation.as_ref();
    let mut body = Map::new();
    body.insert("model".to_string(), json!(request.model.id.as_str()));
    body.insert("input".to_string(), Value::Array(lower_messages(request)?));
    body.insert("stream".to_string(), json!(true));
    // Full-history replay need not create retained provider resources;
    // continuation callers opt in to storage.
    body.insert("store".to_string(), json!(options.store.unwrap_or(false)));
    if let Some(id) = &options.previous_interaction_id {
        body.insert("previous_interaction_id".to_string(), json!(id));
    }
    if let Some(instruction) = system_instruction(request) {
        body.insert("system_instruction".to_string(), json!(instruction));
    }
    if let Some(tier) = &options.service_tier {
        body.insert("service_tier".to_string(), json!(tier));
    }
    if !tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(tools.iter().map(lower_tool_definition).collect()),
        );
    }
    let mut config = Map::new();
    if let Some(max_tokens) = generation.and_then(|g| g.max_tokens) {
        config.insert("max_output_tokens".to_string(), shared::number(max_tokens));
    }
    if let Some(temperature) = generation.and_then(|g| g.temperature) {
        config.insert("temperature".to_string(), shared::number(temperature));
    }
    if let Some(top_p) = generation.and_then(|g| g.top_p) {
        config.insert("top_p".to_string(), shared::number(top_p));
    }
    if let Some(seed) = generation.and_then(|g| g.seed) {
        config.insert("seed".to_string(), shared::number(seed));
    }
    if let Some(stop) = generation.and_then(|g| g.stop.clone()) {
        config.insert("stop_sequences".to_string(), json!(stop));
    }
    if let Some(level) = &options.thinking_level {
        config.insert("thinking_level".to_string(), json!(level));
    }
    if let Some(summaries) = &options.thinking_summaries {
        config.insert("thinking_summaries".to_string(), json!(summaries));
    }
    if let Some(choice) = choice {
        config.insert("tool_choice".to_string(), choice);
    }
    body.insert("generation_config".to_string(), Value::Object(config));
    Ok(Value::Object(body))
}

fn lower_tool_definition(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.input_schema,
    })
}

// =============================================================================
// Stream parsing (google-interactions.ts:343-521)
// =============================================================================

/// Parser state (google-interactions.ts:172-179).
#[derive(Debug, Clone, Default)]
pub struct ParserState {
    pub route: String,
    pub metadata_key: String,
    pub lifecycle: lifecycle::State,
    /// The `step.start` outputs seen so far, keyed by index.
    pub steps: BTreeMap<String, Value>,
    pub tools: tool_stream::State,
    pub completed: bool,
}

impl ParserState {
    fn new(request: &LLMRequest) -> Self {
        Self {
            route: format!("{}/{}", request.model.provider.as_str(), ADAPTER),
            metadata_key: request
                .model
                .route
                .provider_metadata_key
                .clone()
                .unwrap_or_else(|| request.model.provider.as_str().to_string()),
            lifecycle: lifecycle::State::initial(),
            steps: BTreeMap::new(),
            tools: tool_stream::State::new(),
            completed: false,
        }
    }

    /// `{metadataKey: {interactionSignature}}` for one output step.
    fn signature_metadata(&self, step: &Value) -> Option<ProviderMetadata> {
        let sig = step.get("signature").and_then(Value::as_str)?;
        let mut namespace = BTreeMap::new();
        namespace.insert("interactionSignature".to_string(), json!(sig));
        let mut metadata = ProviderMetadata::new();
        metadata.insert(self.metadata_key.clone(), namespace);
        Some(metadata)
    }
}

/// `mapUsage` (google-interactions.ts:346-365).
fn map_usage(usage: Option<&Value>, key: &str) -> Option<Usage> {
    let usage = usage?;
    let number = |field: &str| usage.get(field).and_then(Value::as_f64);
    let input = number("total_input_tokens");
    let cached = number("total_cached_tokens");
    let reasoning = number("total_thought_tokens");
    let output = number("total_output_tokens").map(|output| output + reasoning.unwrap_or(0.0));
    let mut provider_metadata = ProviderMetadata::new();
    provider_metadata
        .insert(key.to_string(), serde_json::from_value(usage.clone()).unwrap_or_default());
    Some(Usage {
        context_tokens: number("raw_prompt_token").or(input),
        input_tokens: input,
        output_tokens: output,
        non_cached_input_tokens: subtract_tokens(input, cached),
        cache_read_input_tokens: cached,
        cache_write_input_tokens: None,
        reasoning_tokens: reasoning,
        total_tokens: number("total_tokens"),
        provider_metadata: Some(provider_metadata),
    })
}

/// `onStart` (google-interactions.ts:367-403).
fn on_start(
    state: &ParserState,
    index: f64,
    step: &Value,
) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let mut events = Vec::new();
    let mut next = state.clone();
    next.lifecycle = lifecycle::step_start(&next.lifecycle, &mut events);
    let id = index.to_string();
    let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
    match step_type {
        "thought" => {
            let metadata = state.signature_metadata(step);
            next.lifecycle =
                lifecycle::reasoning_start(&next.lifecycle, &mut events, &id, metadata);
            for part in step.get("summary").and_then(Value::as_array).cloned().unwrap_or_default() {
                let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                next.lifecycle =
                    lifecycle::reasoning_delta(&next.lifecycle, &mut events, &id, text, None);
            }
        }
        "model_output" => {
            next.lifecycle = lifecycle::text_start(&next.lifecycle, &mut events, &id, None);
            for part in step.get("content").and_then(Value::as_array).cloned().unwrap_or_default() {
                if part.get("type").and_then(Value::as_str).is_some_and(|kind| kind != "text") {
                    return Err(event_error(
                        ADAPTER,
                        format!(
                            "Unsupported Interactions output: {}",
                            part.get("type").and_then(Value::as_str).unwrap_or_default()
                        ),
                    ));
                }
                if let Some(text) =
                    part.get("text").and_then(Value::as_str).filter(|t| !t.is_empty())
                {
                    next.lifecycle =
                        lifecycle::text_delta(&next.lifecycle, &mut events, &id, text, None);
                }
            }
        }
        "function_call" => {
            let (Some(id), Some(name)) =
                (step.get("id").and_then(Value::as_str), step.get("name").and_then(Value::as_str))
            else {
                return Err(event_error(ADAPTER, "Interactions function call lacks id or name"));
            };
            let metadata = state.signature_metadata(step);
            let input = step
                .get("arguments")
                .and_then(Value::as_object)
                .filter(|arguments| !arguments.is_empty())
                .map(|arguments| serde_json::to_string(arguments).unwrap_or_default())
                .unwrap_or_default();
            // The tool streams under its step index; identity lives on
            // the step (google-interactions.ts:393-399).
            next.tools = tool_stream::start(
                &next.tools,
                &index.to_string(),
                PendingTool {
                    id: id.to_string(),
                    name: name.to_string(),
                    namespace: None,
                    input,
                    provider_executed: None,
                    provider_metadata: metadata.clone(),
                },
            );
            events.push(LLMEvent::ToolInputStart {
                id: id.to_string(),
                name: name.to_string(),
                namespace: None,
                provider_executed: None,
                provider_metadata: metadata,
            });
        }
        other => {
            return Err(event_error(ADAPTER, format!("Unsupported Interactions step: {other}")));
        }
    }
    next.steps.insert(index.to_string(), step.clone());
    Ok((next, events))
}

/// `onDelta` (google-interactions.ts:405-448).
fn on_delta(
    state: &ParserState,
    index: f64,
    delta: &Value,
) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let Some(step) = state.steps.get(&index.to_string()).cloned() else {
        return Err(event_error(ADAPTER, "Interactions delta without step.start"));
    };
    let mut events = Vec::new();
    let mut next = state.clone();
    let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or_default();
    match delta_type {
        "text" if step.get("type").and_then(Value::as_str) == Some("model_output") => {
            let text = delta.get("text").and_then(Value::as_str).unwrap_or_default();
            next.lifecycle =
                lifecycle::text_delta(&next.lifecycle, &mut events, &index.to_string(), text, None);
            Ok((next, events))
        }
        "thought_summary" if step.get("type").and_then(Value::as_str) == Some("thought") => {
            let text = delta.pointer("/content/text").and_then(Value::as_str).unwrap_or_default();
            next.lifecycle = lifecycle::reasoning_delta(
                &next.lifecycle,
                &mut events,
                &index.to_string(),
                text,
                None,
            );
            Ok((next, events))
        }
        "thought_signature" => {
            let signature = delta.get("signature").and_then(Value::as_str);
            let mut updated = step.clone();
            if let Some(object) = updated.as_object_mut() {
                match signature {
                    Some(sig) => object.insert("signature".to_string(), json!(sig)),
                    None => object.remove("signature"),
                };
            }
            next.steps.insert(index.to_string(), updated.clone());
            // an open tool call's replayable signature updates with the step
            let key = index.to_string();
            if let Some(tool) = state.tools.get(&key).cloned() {
                let mut updated_tool = tool;
                updated_tool.provider_metadata = next.signature_metadata(&updated);
                next.tools = tool_stream::start(&next.tools, &key, updated_tool);
            }
            Ok((next, events))
        }
        "arguments_delta" if step.get("type").and_then(Value::as_str) == Some("function_call") => {
            let arguments = delta.get("arguments").and_then(Value::as_str).unwrap_or_default();
            let outcome = tool_stream::append_existing(
                ADAPTER,
                &state.tools,
                &index.to_string(),
                arguments,
                "Interactions arguments without function call",
            )?;
            next.tools = outcome.tools;
            events.extend(outcome.events);
            Ok((next, events))
        }
        other => Err(event_error(ADAPTER, format!("Unsupported Interactions delta: {other}"))),
    }
}

/// `onStop` (google-interactions.ts:450-466).
fn on_stop(state: &ParserState, index: f64) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let Some(step) = state.steps.get(&index.to_string()).cloned() else {
        return Err(event_error(ADAPTER, "Interactions step.stop without step.start"));
    };
    let mut events = Vec::new();
    let mut next = state.clone();
    let id = index.to_string();
    match step.get("type").and_then(Value::as_str).unwrap_or_default() {
        "thought" => {
            next.lifecycle = lifecycle::reasoning_end(
                &next.lifecycle,
                &mut events,
                &id,
                next.signature_metadata(&step),
                None,
            );
            Ok((next, events))
        }
        "model_output" => {
            next.lifecycle = lifecycle::text_end(&next.lifecycle, &mut events, &id, None, None);
            Ok((next, events))
        }
        _ => {
            let outcome = tool_stream::finish(ADAPTER, &state.tools, &id)?;
            next.tools = outcome.tools;
            Ok((next, outcome.events))
        }
    }
}

/// `step` over one decoded event (google-interactions.ts:468-521).
pub fn step(state: ParserState, event: &Value) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let event_type = event.get("event_type").and_then(Value::as_str).unwrap_or_default();
    match event_type {
        "step.start" => {
            let index = event.get("index").and_then(Value::as_f64).unwrap_or_default();
            let step = event.get("step").cloned().unwrap_or(Value::Null);
            on_start(&state, index, &step)
        }
        "step.delta" => {
            let index = event.get("index").and_then(Value::as_f64).unwrap_or_default();
            let delta = event.get("delta").cloned().unwrap_or(Value::Null);
            on_delta(&state, index, &delta)
        }
        "step.stop" => {
            let index = event.get("index").and_then(Value::as_f64).unwrap_or_default();
            on_stop(&state, index)
        }
        "interaction.created" | "interaction.status_update" => Ok((state, Vec::new())),
        "error" => {
            let body = serde_json::to_string(event).unwrap_or_default();
            let message = provider_error_message(&body)
                .unwrap_or_else(|| "Google Interactions stream error".to_string());
            Err(AiError::new(classify_provider_failure(ProviderFailure {
                message: &message,
                status: None,
                raw_body: Some(&body),
                data: event.get("error"),
                http: None,
                retry_after_ms: None,
                rate_limit: None,
            })))
        }
        "interaction.completed" => {
            let interaction = event.get("interaction").cloned().unwrap_or(Value::Null);
            let status = interaction.get("status").and_then(Value::as_str).unwrap_or_default();
            if status == "failed" || status == "cancelled" {
                let body = serde_json::to_string(event).unwrap_or_default();
                let message = format!("Google Interactions {status}");
                return Err(AiError::new(classify_provider_failure(ProviderFailure {
                    message: &message,
                    status: None,
                    raw_body: Some(&body),
                    data: Some(&interaction),
                    http: None,
                    retry_after_ms: None,
                    rate_limit: None,
                })));
            }
            if !matches!(status, "completed" | "requires_action" | "incomplete") {
                return Err(event_error(
                    ADAPTER,
                    format!("Unexpected terminal Interactions status: {status}"),
                ));
            }
            let pending = tool_stream::finish_all(ADAPTER, &state.tools)?;
            let mut events = pending.events;
            let normalized = match status {
                "requires_action" => FinishReason::ToolCalls,
                "incomplete" => FinishReason::Length,
                _ => FinishReason::Stop,
            };
            let mut namespace = BTreeMap::new();
            if let Some(id) = interaction.get("id").and_then(Value::as_str) {
                namespace.insert("interactionId".to_string(), json!(id));
            }
            let mut provider_metadata = ProviderMetadata::new();
            provider_metadata.insert(state.metadata_key.clone(), namespace);
            let mut next = state.clone();
            next.lifecycle = lifecycle::finish(
                &next.lifecycle,
                &mut events,
                lifecycle::FinishInput {
                    reason: FinishReasonDetails {
                        normalized,
                        raw: Some(status.to_string()),
                        category: None,
                        explanation: None,
                    },
                    usage: map_usage(interaction.get("usage"), &state.metadata_key),
                    provider_metadata: Some(provider_metadata),
                },
            );
            next.tools = pending.tools;
            next.completed = true;
            Ok((next, events))
        }
        other => Err(event_error(ADAPTER, format!("Unsupported Interactions event: {other}"))),
    }
}

/// Whether one decoded event terminates the stream
/// (`terminal`, google-interactions.ts:541).
#[must_use]
pub fn terminal(event: &Value) -> bool {
    event.get("event_type").and_then(Value::as_str) == Some("interaction.completed")
}

// =============================================================================
// Protocol and route (google-interactions.ts:526-559)
// =============================================================================

/// The `google-interactions` protocol.
pub struct GoogleInteractionsProtocol;

impl Protocol for GoogleInteractionsProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        body_from(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState::new(request))
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("GoogleInteractions state");
        let event: Value =
            serde_json::from_str(frame).map_err(|cause| frame_decode_error(frame, cause))?;
        let (next, events) = step(state, &event)?;
        Ok((Box::new(next), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        serde_json::from_str::<Value>(frame).is_ok_and(|event| terminal(&event))
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("GoogleInteractions state");
        if state.completed {
            return Ok((Box::new(state), Vec::new()));
        }
        Err(event_error(ADAPTER, "Google Interactions stream ended before interaction.completed"))
    }

    /// Gemini's schema rules (google-interactions.ts:528).
    fn sanitizer(&self) -> Option<crate::schema::SanitizerCompatibility> {
        Some(crate::schema::SanitizerCompatibility::Gemini)
    }
}

/// The base `google-interactions` route (port of `GoogleInteractions.route`):
/// auth is [`auth::Definition::none`] — the Google facade wraps it with the
/// `x-goog-api-key` header (see [`google_interactions_route`]).
#[must_use]
pub fn google_interactions_base_route(transport: SharedTransport) -> Route {
    Route::new(crate::route::RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("google".to_string()),
        provider_metadata_key: Some("google".to_string()),
        protocol: Arc::new(GoogleInteractionsProtocol),
        endpoint: Endpoint::path("/interactions").with_base_url(DEFAULT_BASE_URL),
        auth: None,
        framing: Framing::sse(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

/// Google-facade configuration for [`google_interactions_route`].
pub struct GoogleInteractionsConfig {
    pub api_key: Option<Secret>,
    pub base_url: Option<String>,
}

/// The configured Google Interactions route (port of
/// `Google.configure(...).interactions(modelID)`): base URL override + the
/// `x-goog-api-key` auth chain (providers/google.ts:50-53).
#[must_use]
pub fn google_interactions_route(
    transport: SharedTransport,
    config: GoogleInteractionsConfig,
) -> Route {
    google_interactions_base_route(transport).with(RoutePatch {
        endpoint: config.base_url.map(|base_url| crate::route::EndpointPatch {
            base_url: Some(base_url),
            path: None,
            query: None,
        }),
        auth: Some(google_interactions_auth(config.api_key)),
        ..Default::default()
    })
}

/// Google provider auth (providers/google.ts:38-42).
#[must_use]
pub fn google_interactions_auth(api_key: Option<Secret>) -> AuthDefinition {
    auth::or_else(&auth::optional(api_key), &auth::config("GOOGLE_GENERATIVE_AI_API_KEY"))
        .header("x-goog-api-key")
}

/// Event-frame decode errors keep the raw frame on `reason.body`
/// (`decodeFrame`, google-interactions.ts:287-293).
fn frame_decode_error(frame: &str, cause: serde_json::Error) -> AiError {
    use crate::schema::AiErrorReason;
    let mut error = event_error(ADAPTER, format!("{NAME} sent an invalid stream event: {cause}"));
    if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
        fields.body = Some(frame.to_string());
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{LLMRequest, Message, ToolResultValue};

    fn base_request() -> LLMRequest {
        crate::test_support::request_for_tests(vec![Message::user(
            "Reply with exactly one word: hello",
        )])
    }

    #[test]
    fn lowers_the_minimal_body_with_store_default_false() {
        let request = base_request();
        let body = body_from(&request).unwrap();
        assert_eq!(body["model"], json!("test-model"));
        assert_eq!(
            body["input"],
            json!([{ "type": "user_input", "content": [{ "type": "text", "text": "Reply with exactly one word: hello" }] }])
        );
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["store"], json!(false));
        assert!(body.get("previous_interaction_id").is_none());
        assert!(body.get("system_instruction").is_none());
        assert_eq!(body["generation_config"], json!({}));
    }

    #[test]
    fn options_reject_topk_and_penalties_and_hosted_tools() {
        let mut request = base_request();
        request.generation =
            Some(crate::schema::GenerationOptions { top_k: Some(4.0), ..Default::default() });
        let error = body_from(&request).unwrap_err();
        assert!(error.message().contains("does not support topK"));

        let mut hosted = base_request();
        hosted.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition {
            name: "native_tool".into(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: None,
            cache: None,
            metadata: None,
            native: Some(
                [("google".to_string(), json!({ "type": "googleSearch" }))].into_iter().collect(),
            ),
        })];
        let error = body_from(&hosted).unwrap_err();
        assert!(error.message().contains("hosted tools are not supported"));
    }

    #[test]
    fn tool_choice_maps_to_the_interactions_shapes() {
        let mut request = base_request();
        request.tool_choice = Some(ToolChoice::Required { disable_parallel_tool_use: None });
        assert_eq!(body_from(&request).unwrap()["generation_config"]["tool_choice"], json!("any"));
        request.tool_choice =
            Some(ToolChoice::Named { name: "get_weather".into(), disable_parallel_tool_use: None });
        assert_eq!(
            body_from(&request).unwrap()["generation_config"]["tool_choice"],
            json!({ "allowed_tools": { "mode": "any", "tools": ["get_weather"] } })
        );
    }

    #[test]
    fn assistant_history_replays_statelessly_with_signatures() {
        let mut request = base_request();
        let mut thought_metadata = ProviderMetadata::new();
        let mut namespace = BTreeMap::new();
        namespace.insert("interactionSignature".to_string(), json!("sig-thought"));
        thought_metadata.insert("test-provider".to_string(), namespace);
        let mut tool_metadata = ProviderMetadata::new();
        let mut namespace = BTreeMap::new();
        namespace.insert("interactionSignature".to_string(), json!("sig-tool"));
        tool_metadata.insert("test-provider".to_string(), namespace);
        request.messages = vec![
            Message::user("What is the weather in Paris?"),
            Message {
                role: MessageRole::Assistant,
                content: vec![
                    ContentPart::Reasoning {
                        text: "thinking".to_string(),
                        encrypted: None,
                        cache: None,
                        metadata: None,
                        provider_metadata: Some(thought_metadata),
                    },
                    ContentPart::ToolCall {
                        id: "call_1".into(),
                        name: "get_weather".into(),
                        namespace: None,
                        input: json!({ "city": "Paris" }),
                        provider_executed: None,
                        cache: None,
                        metadata: None,
                        provider_metadata: Some(tool_metadata),
                    },
                ],
                ..Message::user("")
            },
            Message {
                role: MessageRole::Tool,
                content: vec![ContentPart::ToolResult {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    namespace: None,
                    result: ToolResultValue::Json { value: json!({ "temperature": 22 }) },
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                }],
                ..Message::user("")
            },
        ];
        let body = body_from(&request).unwrap();
        assert_eq!(
            body["input"],
            json!([
                { "type": "user_input", "content": [{ "type": "text", "text": "What is the weather in Paris?" }] },
                { "type": "thought", "signature": "sig-thought", "summary": [{ "type": "text", "text": "thinking" }] },
                { "type": "function_call", "id": "call_1", "name": "get_weather", "arguments": { "city": "Paris" }, "signature": "sig-tool" },
                { "type": "function_result", "call_id": "call_1", "name": "get_weather", "result": { "temperature": 22 } }
            ])
        );
    }

    #[test]
    fn consecutive_text_merges_into_one_step_by_role() {
        let mut request = base_request();
        request.messages = vec![
            Message::user("one"),
            Message { role: MessageRole::User, content: vec![], ..Message::user("two") },
        ];
        request.messages[1].content = vec![crate::schema::ContentPart::Text {
            text: "two".into(),
            cache: None,
            metadata: None,
            provider_metadata: None,
        }];
        // simulate merge: two consecutive user messages lower to two
        // user_input steps; consecutive parts *within* one message merge.
        let body = body_from(&request).unwrap();
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], json!("user_input"));
        assert_eq!(input[1]["type"], json!("user_input"));
    }
}
