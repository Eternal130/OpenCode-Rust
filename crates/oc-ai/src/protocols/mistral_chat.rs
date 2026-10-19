//! The Mistral chat-completions protocol (port of
//! `src/protocols/mistral-chat.ts` + `utils/mistral-tool-id.ts`).
//!
//! Request side: OpenAI-shaped chat bodies with Mistral's extensions
//! (structured `thinking` content, `document_url` media, `prefix`
//! continuation, `prompt_cache_key`, strict:false tools). Stream side:
//! OpenAI-style deltas where tool identity is fragmentary — ids may be
//! absent, partial, or repeated — normalized through the Mistral tool-id
//! hasher; content switches between text and native thinking blocks.
//! `[DONE]` is the terminal frame (`sseWithDone` framing).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{self, encode_json, subtract_tokens, total_tokens};
use crate::protocols::utils::{lifecycle, tool_stream};
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::{Endpoint, Framing, Route, RouteInput, SharedTransport};
use crate::schema::{
    AiError, AiErrorReason, ContentPart, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest,
    Message, MessageRole, ReasonFields, ToolChoice, ToolResultValue, Usage,
};

const ADAPTER: &str = "mistral-chat";
pub const DEFAULT_BASE_URL: &str = "https://api.mistral.ai/v1";
pub const PATH: &str = "/chat/completions";

// =============================================================================
// Mistral tool ids (port of `utils/mistral-tool-id.ts`)
// =============================================================================

/// Mistral only accepts 9-character alphanumeric tool-call ids.
fn tool_id_valid(id: &str) -> bool {
    id.len() == 9 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn to_base36(mut value: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(DIGITS[(value % 36) as usize] as char);
        value /= 36;
    }
    digits.into_iter().rev().collect()
}

/// Deterministic 9-char id derived from two FNV-1a 32-bit passes (port of
/// `MistralToolID.hash`: `Math.imul(result ^ charCode, 16777619)`).
fn tool_id_hash(value: &str) -> String {
    fn fnv1a(seed: u32, value: &str) -> u32 {
        let mut result = seed;
        for unit in value.chars() {
            result ^= unit as u32;
            result = result.wrapping_mul(16_777_619);
        }
        result
    }
    let combined = format!(
        "{:0>7}{:0>7}",
        to_base36(fnv1a(2_166_136_261, value)),
        to_base36(fnv1a(2_246_822_519, value))
    );
    if combined.len() >= 9 {
        combined[combined.len() - 9..].to_string()
    } else {
        combined
    }
}

/// Per-request id normalizer (port of `MistralToolID.normalizer`): valid ids
/// pass through (and are reserved, including ids seen later in the history);
/// invalid ids map deterministically to fresh valid ones.
fn tool_id_normalizer(request: &LLMRequest) -> impl Fn(&str) -> String + '_ {
    let mut reserved: BTreeSet<String> = BTreeSet::new();
    for message in &request.messages {
        for part in &message.content {
            let id = match part {
                ContentPart::ToolCall { id, .. } | ContentPart::ToolResult { id, .. } => Some(id),
                _ => None,
            };
            if let Some(id) = id.filter(|id| tool_id_valid(id)) {
                reserved.insert(id.clone());
            }
        }
    }
    // body building is single-threaded; interior mutability keeps the
    // normalizer a plain `Fn` for the lowering helpers
    let state = std::cell::RefCell::new((reserved, BTreeMap::<String, String>::new()));
    move |id: &str| {
        if tool_id_valid(id) {
            return id.to_string();
        }
        let mut guard = state.borrow_mut();
        let (used, normalized) = &mut *guard;
        if let Some(previous) = normalized.get(id) {
            return previous.clone();
        }
        let mut attempt = 0u32;
        let mut candidate = tool_id_hash(id);
        while used.contains(&candidate) {
            attempt += 1;
            candidate = tool_id_hash(&format!("{id}:{attempt}"));
        }
        used.insert(candidate.clone());
        normalized.insert(id.to_string(), candidate.clone());
        candidate
    }
}

// =============================================================================
// Provider options (strict decode — malformed values fail the request)
// =============================================================================

#[derive(Debug, Clone, Default)]
struct MistralOptions {
    safe_prompt: Option<bool>,
    document_image_limit: Option<f64>,
    document_page_limit: Option<f64>,
    parallel_tool_calls: Option<bool>,
    reasoning_effort: Option<String>,
    prompt_mode: Option<String>,
    prompt_cache_key: Option<String>,
}

fn decode_options(options: &crate::schema::ProviderOptions) -> Result<MistralOptions, AiError> {
    let take_bool = |value: Option<&Value>, field: &str| -> Result<Option<bool>, AiError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            Some(_) => {
                Err(shared::invalid_request(format!("Expected boolean, but received {field}")))
            }
        }
    };
    let take_number = |value: Option<&Value>, field: &str| -> Result<Option<f64>, AiError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(number)) => Ok(number.as_f64()),
            Some(_) => {
                Err(shared::invalid_request(format!("Expected number, but received {field}")))
            }
        }
    };
    let take_string = |value: Option<&Value>, field: &str| -> Result<Option<String>, AiError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => {
                Err(shared::invalid_request(format!("Expected string, but received {field}")))
            }
        }
    };
    Ok(MistralOptions {
        safe_prompt: take_bool(options.get("safePrompt"), "safePrompt")?,
        document_image_limit: take_number(options.get("documentImageLimit"), "documentImageLimit")?,
        document_page_limit: take_number(options.get("documentPageLimit"), "documentPageLimit")?,
        parallel_tool_calls: take_bool(options.get("parallelToolCalls"), "parallelToolCalls")?,
        reasoning_effort: take_string(options.get("reasoningEffort"), "reasoningEffort")?,
        prompt_mode: match options.get("promptMode") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) if text == "reasoning" => Some(text.clone()),
            Some(_) => {
                return Err(shared::invalid_request(
                    "Expected \"reasoning\", but received promptMode",
                ));
            }
        },
        prompt_cache_key: take_string(options.get("promptCacheKey"), "promptCacheKey")?,
    })
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
        "Mistral Chat {role} messages only support {types} content for now"
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

fn tool_result_text(result: &ToolResultValue) -> String {
    match result {
        ToolResultValue::Text { value } | ToolResultValue::Error { value } => {
            if value.is_array() || value.is_object() {
                encode_json(value)
            } else {
                stringify_value(value)
            }
        }
        ToolResultValue::Json { value } => encode_json(value),
        ToolResultValue::Content { .. } => String::new(),
    }
}

/// A native `thinking` content object: `{type:"thinking", thinking:[units]}`
/// (port of `isMistralThinkingContent`).
fn is_thinking_content(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Object(record)) => {
            record.get("type").and_then(Value::as_str) == Some("thinking")
                && record.get("thinking").is_some_and(Value::is_array)
        }
        _ => false,
    }
}

fn media_url(media: &crate::schema::media::Asset) -> Result<String, AiError> {
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
                "Mistral Chat requires inline media (bytes or base64); {source} sources must be materialized first"
            )))
        },
        |inline| Ok(inline.data_url),
    )
}

fn lower_media(part: &ContentPart) -> Result<Value, AiError> {
    let ContentPart::Media { media, .. } = part else {
        unreachable!("lower_media receives media parts");
    };
    let mime = media.media_type.to_ascii_lowercase();
    if mime.starts_with("image/") {
        return Ok(json!({ "type": "image_url", "image_url": media_url(media)? }));
    }
    if mime == "application/pdf" {
        return Ok(json!({ "type": "document_url", "document_url": media_url(media)? }));
    }
    Err(shared::invalid_request(format!(
        "Mistral Chat does not support media type {}",
        media.media_type
    )))
}

fn lower_user(message: &Message) -> Result<Value, AiError> {
    let mut content: Vec<Value> = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text, .. } => content.push(json!({ "type": "text", "text": text })),
            ContentPart::Media { .. } => content.push(lower_media(part)?),
            _ => return Err(unsupported_content("user", &["text", "media"])),
        }
    }
    if content.iter().all(|part| part.get("type").and_then(Value::as_str) == Some("text")) {
        let joined: String =
            content.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect();
        return Ok(json!({ "role": "user", "content": joined }));
    }
    Ok(json!({ "role": "user", "content": content }))
}

fn lower_assistant(
    message: &Message,
    normalize_id: &dyn Fn(&str) -> String,
    prefix: bool,
) -> Result<Value, AiError> {
    let structured = message.content.iter().any(|part| match part {
        ContentPart::Reasoning { provider_metadata, .. } => is_thinking_content(
            provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mistral"))
                .and_then(|mistral| mistral.get("thinking")),
        ),
        _ => false,
    });
    let mut content: Vec<Value> = Vec::new();
    let mut text: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text: part_text, .. } => {
                if structured {
                    content.push(json!({ "type": "text", "text": part_text }));
                } else {
                    text.push(part_text.clone());
                }
            }
            ContentPart::Reasoning { text: part_text, provider_metadata, .. } => {
                let native = provider_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("mistral"))
                    .and_then(|mistral| mistral.get("thinking"));
                if structured && is_thinking_content(native) {
                    content.push(native.cloned().unwrap_or(Value::Null));
                } else if structured {
                    content.push(json!({ "type": "text", "text": part_text }));
                } else {
                    text.push(part_text.clone());
                }
            }
            ContentPart::ToolCall { id, name, input, .. } => {
                tool_calls.push(json!({
                    "id": normalize_id(id),
                    "type": "function",
                    "function": { "name": name, "arguments": encode_json(input) }
                }));
            }
            _ => {
                return Err(unsupported_content("assistant", &["text", "reasoning", "tool-call"]));
            }
        }
    }
    let mut entry = Map::new();
    entry.insert("role".into(), json!("assistant"));
    if structured {
        entry.insert("content".into(), Value::Array(content));
    } else {
        entry.insert("content".into(), json!(text.concat()));
    }
    if !tool_calls.is_empty() {
        entry.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    if prefix {
        entry.insert("prefix".into(), json!(true));
    }
    Ok(Value::Object(entry))
}

fn tool_file_asset(item: &Value) -> crate::schema::media::Asset {
    let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
    let mime = item.get("mime").and_then(Value::as_str).unwrap_or("application/octet-stream");
    if let Some(mut asset) = crate::schema::media::parse_data_url(uri) {
        asset.media_type = mime.to_string();
        if let crate::schema::media::Source::Base64 { media_type, .. } = &mut asset.source {
            *media_type = mime.to_string();
        }
        return asset;
    }
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return crate::schema::media::url(uri, Some(mime.to_string()), None);
    }
    crate::schema::media::base64(uri, mime)
}

fn lower_tool_results(
    message: &Message,
    normalize_id: &dyn Fn(&str) -> String,
) -> Result<Vec<Value>, AiError> {
    let mut output: Vec<Value> = Vec::new();
    for part in &message.content {
        let ContentPart::ToolResult { id, name, result, .. } = part else {
            return Err(unsupported_content("tool", &["tool-result"]));
        };
        let ToolResultValue::Content { value } = result else {
            output.push(json!({
                "role": "tool",
                "tool_call_id": normalize_id(id),
                "name": name,
                "content": tool_result_text(result),
            }));
            continue;
        };
        let mut content: Vec<Value> = Vec::new();
        for item in &value.0 {
            if item.get("type").and_then(Value::as_str) == Some("text") {
                content.push(json!({
                    "type": "text",
                    "text": item.get("text").and_then(Value::as_str).unwrap_or_default()
                }));
                continue;
            }
            let asset = tool_file_asset(item);
            let media_part = ContentPart::Media {
                media: asset,
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            };
            content.push(lower_media(&media_part)?);
        }
        let joined: String = content
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect();
        output.push(json!({
            "role": "tool",
            "tool_call_id": normalize_id(id),
            "name": name,
            "content": if content.iter().any(|item| item.get("type").and_then(Value::as_str) != Some("text")) {
                Value::Array(content)
            } else {
                json!(joined)
            },
        }));
    }
    Ok(output)
}

fn lower_messages(request: &LLMRequest) -> Result<Vec<Value>, AiError> {
    let normalize_id = tool_id_normalizer(request);
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
    let last_index = request.messages.len().saturating_sub(1);
    for (index, message) in request.messages.iter().enumerate() {
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
            MessageRole::User => messages.push(lower_user(message)?),
            MessageRole::Assistant => {
                let has_tool_calls =
                    message.content.iter().any(|part| matches!(part, ContentPart::ToolCall { .. }));
                let has_native_thinking = message.content.iter().any(|part| match part {
                    ContentPart::Reasoning { provider_metadata, .. } => is_thinking_content(
                        provider_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.get("mistral"))
                            .and_then(|mistral| mistral.get("thinking")),
                    ),
                    _ => false,
                });
                let text: String = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text, .. } | ContentPart::Reasoning { text, .. } => {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .concat();
                if !has_tool_calls && !has_native_thinking && text.trim().is_empty() {
                    continue;
                }
                messages.push(lower_assistant(
                    message,
                    &normalize_id,
                    !has_tool_calls && index == last_index,
                )?);
            }
            MessageRole::Tool => messages.extend(lower_tool_results(message, &normalize_id)?),
        }
    }
    Ok(messages)
}

fn body_from(request: &LLMRequest) -> Result<Value, AiError> {
    let options = decode_options(request.provider_options.as_ref().unwrap_or(&Map::new()))?;
    let selected = match &request.tool_choice {
        Some(ToolChoice::Named { name, .. }) => Some(name.clone()),
        _ => None,
    };
    if selected.as_deref().is_some_and(str::is_empty) {
        return Err(shared::invalid_request("Mistral Chat tool choice requires a tool name"));
    }
    if options.reasoning_effort.is_some() && options.prompt_mode.is_some() {
        return Err(shared::invalid_request(
            "Mistral Chat reasoningEffort and promptMode provider options are mutually exclusive",
        ));
    }
    let tool_choice = match &request.tool_choice {
        None => None,
        Some(ToolChoice::Auto { .. }) => Some(json!("auto")),
        Some(ToolChoice::None { .. }) => Some(json!("none")),
        Some(ToolChoice::Required { .. }) => Some(json!("any")),
        Some(ToolChoice::Named { name, .. }) => {
            Some(json!({ "type": "function", "function": { "name": name } }))
        }
    };
    let projected =
        crate::protocols::sanitizer::project_tools(request.tools.clone(), &request.model);
    let flattened = shared::flatten_tools(&projected);
    let messages = {
        let mut lowered = request.clone();
        lowered.messages = flatten_namespaced_messages(&request.messages);
        lower_messages(&lowered)?
    };
    let generation = request.generation.clone().unwrap_or_default();
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model.id.as_str()));
    body.insert("messages".into(), Value::Array(messages));
    if !flattened.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                flattened
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                                "strict": false,
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".into(), tool_choice);
    }
    body.insert("stream".into(), json!(true));
    if let Some(max_tokens) = generation.max_tokens {
        body.insert("max_tokens".into(), shared::number(max_tokens));
    }
    if let Some(seed) = generation.seed {
        body.insert("random_seed".into(), shared::number(seed));
    }
    if let Some(temperature) = generation.temperature {
        body.insert("temperature".into(), shared::number(temperature));
    }
    if let Some(top_p) = generation.top_p {
        body.insert("top_p".into(), shared::number(top_p));
    }
    if let Some(frequency_penalty) = generation.frequency_penalty {
        body.insert("frequency_penalty".into(), shared::number(frequency_penalty));
    }
    if let Some(presence_penalty) = generation.presence_penalty {
        body.insert("presence_penalty".into(), shared::number(presence_penalty));
    }
    if let Some(stop) = &generation.stop {
        body.insert("stop".into(), json!(stop));
    }
    if !matches!(request.cache, Some(crate::schema::CachePolicy::None)) {
        let cache_key =
            options.prompt_cache_key.clone().or_else(|| request.prompt_cache_key.clone());
        if let Some(cache_key) = cache_key {
            body.insert("prompt_cache_key".into(), json!(cache_key));
        }
    }
    if let Some(safe_prompt) = options.safe_prompt {
        body.insert("safe_prompt".into(), json!(safe_prompt));
    }
    if let Some(limit) = options.document_image_limit {
        body.insert("document_image_limit".into(), shared::number(limit));
    }
    if let Some(limit) = options.document_page_limit {
        body.insert("document_page_limit".into(), shared::number(limit));
    }
    let parallel_tool_calls = options.parallel_tool_calls.or_else(|| {
        matches!(
            &request.tool_choice,
            Some(
                ToolChoice::Auto { disable_parallel_tool_use: Some(true), .. }
                    | ToolChoice::Required { disable_parallel_tool_use: Some(true), .. }
                    | ToolChoice::Named { disable_parallel_tool_use: Some(true), .. }
            )
        )
        .then_some(false)
    });
    if let Some(parallel_tool_calls) = parallel_tool_calls {
        body.insert("parallel_tool_calls".into(), json!(parallel_tool_calls));
    }
    if let Some(effort) = options.reasoning_effort {
        body.insert("reasoning_effort".into(), json!(effort));
    }
    if let Some(mode) = options.prompt_mode {
        body.insert("prompt_mode".into(), json!(mode));
    }
    Ok(Value::Object(body))
}

/// Flatten namespaces off tool calls/results (port of the
/// `flattenToolRequest` message rewrite).
fn flatten_namespaced_messages(messages: &[Message]) -> Vec<Message> {
    let mut next = Vec::with_capacity(messages.len());
    for message in messages {
        let mut changed = false;
        let mut content: Vec<ContentPart> = Vec::with_capacity(message.content.len());
        for part in &message.content {
            match part {
                ContentPart::ToolCall { namespace: Some(namespace), .. }
                | ContentPart::ToolResult { namespace: Some(namespace), .. } => {
                    let renamed = rename_namespaced(part, namespace);
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

// =============================================================================
// Stream parsing
// =============================================================================

fn map_usage(usage: &Value) -> Option<Usage> {
    let record = usage.as_object()?;
    let number = |key: &str| -> Option<f64> {
        match record.get(key) {
            Some(Value::Number(number)) => number.as_f64(),
            _ => None,
        }
    };
    let input = number("prompt_tokens");
    let detail_cached = |key: &str| -> Option<f64> {
        record.get(key).and_then(Value::as_object)?.get("cached_tokens").and_then(Value::as_f64)
    };
    let reported = number("num_cached_tokens")
        .or_else(|| detail_cached("prompt_tokens_details"))
        .or_else(|| detail_cached("prompt_token_details"));
    let cached = match (input, reported) {
        (Some(input), Some(reported)) => Some(input.min(reported).max(0.0)),
        _ => None,
    };
    let output = number("completion_tokens");
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        non_cached_input_tokens: subtract_tokens(input, cached),
        cache_read_input_tokens: cached,
        total_tokens: total_tokens(input, output, number("total_tokens")),
        provider_metadata: Some(mistral_metadata(Value::Object(record.clone()))),
        context_tokens: None,
        cache_write_input_tokens: None,
        reasoning_tokens: None,
    })
}

fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" | "model_length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        "error" | "network_error" => FinishReason::Error,
        _ => FinishReason::Unknown,
    }
}

/// Units of a native `thinking` array (port of `thinkingUnits`): a plain
/// string becomes one text unit; non-conforming array members drop.
fn thinking_units(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::String(text)) => vec![json!({ "type": "text", "text": text })],
        Some(Value::Array(items)) => items
            .iter()
            .filter(|unit| {
                unit.as_object().is_some_and(|unit| {
                    unit.get("type").is_none_or(Value::is_string)
                        && unit.get("text").is_none_or(Value::is_string)
                })
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn thinking_text(units: &[Value]) -> String {
    units
        .iter()
        .filter_map(|unit| unit.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .concat()
}

#[derive(Debug, Clone)]
enum ActiveContent {
    Text { id: String },
    Reasoning { id: String, thinking: Option<Value>, thinking_units: Vec<Value> },
}

impl ActiveContent {
    fn id(&self) -> &str {
        match self {
            ActiveContent::Text { id } | ActiveContent::Reasoning { id, .. } => id,
        }
    }
}

/// Parser state (port of `ParserState`).
#[derive(Debug, Clone, Default)]
pub struct ParserState {
    tools: tool_stream::State,
    pending_tools: BTreeMap<String, PendingFragment>,
    tool_ids: BTreeMap<String, String>,
    used_tool_ids: BTreeSet<String>,
    completed_tools: Vec<LLMEvent>,
    latest_tool_key: Option<String>,
    generated_tools: usize,
    lifecycle: lifecycle::State,
    active: Option<ActiveContent>,
    next_content: usize,
    usage: Option<Usage>,
    finish_reason: Option<FinishReasonDetails>,
}

#[derive(Debug, Clone)]
struct PendingFragment {
    id: String,
    input: String,
}

fn close_active(state: &ParserState, events: &mut Vec<LLMEvent>) -> ParserState {
    let Some(active) = &state.active else { return state.clone() };
    let lifecycle = match active {
        ActiveContent::Text { id } => lifecycle::text_end(&state.lifecycle, events, id, None, None),
        ActiveContent::Reasoning { id, thinking, thinking_units } => {
            let mut native = clone_object(thinking.as_ref());
            native.insert("type".into(), json!("thinking"));
            native.insert("thinking".into(), Value::Array(thinking_units.clone()));
            lifecycle::reasoning_end(
                &state.lifecycle,
                events,
                id,
                Some(mistral_metadata(json!({ "thinking": Value::Object(native) }))),
                Some(thinking_text(thinking_units)),
            )
        }
    };
    ParserState { lifecycle, active: None, ..state.clone() }
}

fn clone_object(value: Option<&Value>) -> Map<String, Value> {
    value.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn mistral_metadata(value: Value) -> crate::schema::ProviderMetadata {
    let inner = value.as_object().map(|map| map.clone().into_iter().collect()).unwrap_or_default();
    std::iter::once(("mistral".to_string(), inner)).collect()
}

fn append_text(state: &ParserState, events: &mut Vec<LLMEvent>, text: &str) -> ParserState {
    if text.is_empty() {
        return state.clone();
    }
    let current = if matches!(&state.active, Some(ActiveContent::Text { .. })) {
        state.clone()
    } else {
        close_active(state, events)
    };
    let (active, next_content) = match &current.active {
        Some(active) => (ActiveContent::Text { id: active.id().to_string() }, current.next_content),
        None => (
            ActiveContent::Text { id: format!("text-{}", current.next_content) },
            current.next_content + 1,
        ),
    };
    let lifecycle = lifecycle::text_delta(&current.lifecycle, events, active.id(), text, None);
    ParserState { lifecycle, active: Some(active), next_content, ..current }
}

fn append_thinking(state: &ParserState, events: &mut Vec<LLMEvent>, part: &Value) -> ParserState {
    let current = if matches!(&state.active, Some(ActiveContent::Reasoning { .. })) {
        state.clone()
    } else {
        close_active(state, events)
    };
    let units = thinking_units(part.get("thinking"));
    let (mut active, next_content) = match &current.active {
        Some(active) => match active {
            ActiveContent::Reasoning { id, thinking, thinking_units } => (
                ActiveContent::Reasoning {
                    id: id.clone(),
                    thinking: thinking.clone(),
                    thinking_units: thinking_units.clone(),
                },
                current.next_content,
            ),
            ActiveContent::Text { .. } => unreachable!("text closed above"),
        },
        None => (
            ActiveContent::Reasoning {
                id: format!("reasoning-{}", current.next_content),
                thinking: None,
                thinking_units: Vec::new(),
            },
            current.next_content + 1,
        ),
    };
    if let ActiveContent::Reasoning { thinking_units: accumulated, .. } = &mut active {
        accumulated.extend(units.iter().cloned());
    }
    let ActiveContent::Reasoning { thinking, thinking_units: accumulated, id, .. } = &active else {
        unreachable!()
    };
    // Keep native units out of streamed events until the block completes.
    let mut merged = clone_object(thinking.as_ref());
    if let Some(part_object) = part.as_object() {
        merged.extend(part_object.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    merged.insert("type".into(), json!("thinking"));
    merged.insert("thinking".into(), json!([]));
    let text = thinking_text(&units);
    let lifecycle = if text.is_empty() {
        lifecycle::reasoning_start(&current.lifecycle, events, id, None)
    } else {
        lifecycle::reasoning_delta(&current.lifecycle, events, id, &text, None)
    };
    let active = ActiveContent::Reasoning {
        id: id.clone(),
        thinking: Some(Value::Object(merged)),
        thinking_units: accumulated.clone(),
    };
    ParserState { lifecycle, active: Some(active), next_content, ..current }
}

fn append_content(state: &ParserState, events: &mut Vec<LLMEvent>, content: &Value) -> ParserState {
    match content {
        Value::String(text) => append_text(state, events, text),
        Value::Array(parts) => parts.iter().fold(state.clone(), |current, part| {
            match part.get("type").and_then(Value::as_str) {
                Some("text") => append_text(
                    &current,
                    events,
                    part.get("text").and_then(Value::as_str).unwrap_or_default(),
                ),
                Some("thinking") => append_thinking(&current, events, part),
                _ => close_active(&current, events),
            }
        }),
        _ => state.clone(),
    }
}

fn normalize_stream_tool_id(state: &mut ParserState, source: &str) -> String {
    if tool_id_valid(source) {
        state.used_tool_ids.insert(source.to_string());
        return source.to_string();
    }
    if let Some(previous) = state.tool_ids.get(source) {
        return previous.clone();
    }
    let mut attempt = 0u32;
    let mut id = tool_id_hash(source);
    while state.used_tool_ids.contains(&id) {
        attempt += 1;
        id = tool_id_hash(&format!("{source}:{attempt}"));
    }
    state.tool_ids.insert(source.to_string(), id.clone());
    state.used_tool_ids.insert(id.clone());
    id
}

fn tool_text(delta: &Value) -> String {
    let value = delta.get("function").and_then(|function| function.get("arguments"));
    match value {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(other) => encode_json(other),
    }
}

fn append_tools(
    initial: &ParserState,
    events: &mut Vec<LLMEvent>,
    deltas: &[Value],
) -> Result<ParserState, AiError> {
    if deltas.is_empty() {
        return Ok(initial.clone());
    }
    let mut state = close_active(initial, events);
    for (position, delta) in deltas.iter().enumerate() {
        let wire_id = delta
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        let provided_id = if wire_id.as_deref() == Some("null") { None } else { wire_id };
        let key = delta
            .get("index")
            .and_then(Value::as_f64)
            .map(|index| index.to_string())
            .or_else(|| {
                provided_id.clone().map(|id| format!("id:{id}")).or_else(|| {
                    if deltas.len() > 1 {
                        Some(format!("position:{position}"))
                    } else {
                        state
                            .latest_tool_key
                            .clone()
                            .or_else(|| Some(format!("missing:{}", state.generated_tools)))
                    }
                })
            })
            .unwrap_or_default();
        let existing = state.tools.get(&key).cloned();
        let pending = state.pending_tools.get(&key).cloned();
        let source = provided_id.clone().unwrap_or_else(|| format!("generated:{key}"));
        let normalized_id = if existing.is_some() || pending.is_some() {
            existing
                .as_ref()
                .map(|tool| tool.id.clone())
                .or_else(|| pending.as_ref().map(|tool| tool.id.clone()))
                .unwrap_or_default()
        } else {
            normalize_stream_tool_id(&mut state, &source)
        };
        let name = existing.as_ref().map(|tool| tool.name.clone()).or_else(|| {
            delta
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        });
        let text = format!(
            "{}{}",
            pending.as_ref().map(|tool| tool.input.as_str()).unwrap_or_default(),
            tool_text(delta)
        );
        let Some(name) = name else {
            state
                .pending_tools
                .insert(key.clone(), PendingFragment { id: normalized_id, input: text });
            state.latest_tool_key = Some(key);
            if provided_id.is_none() && pending.is_none() {
                state.generated_tools += 1;
            }
            continue;
        };
        let outcome = tool_stream::append_or_start(
            ADAPTER,
            &state.tools,
            &key,
            tool_stream::Delta {
                id: Some(normalized_id.as_str()),
                name: Some(name.as_str()),
                text: &text,
                provider_metadata: None,
            },
            "Mistral Chat tool call delta is missing a name",
        )?;
        if !outcome.events.is_empty() {
            state.lifecycle = lifecycle::step_start(&state.lifecycle, events);
        }
        events.extend(outcome.events);
        state.pending_tools.remove(&key);
        state.tools = outcome.tools;
        state.latest_tool_key = Some(key);
        if provided_id.is_none() && existing.is_none() && pending.is_none() {
            state.generated_tools += 1;
        }
    }
    Ok(state)
}

fn has_late_content(event: &Value) -> bool {
    let delta = event.pointer("/choices/0/delta").and_then(Value::as_object);
    let Some(delta) = delta else { return false };
    match delta.get("content") {
        Some(Value::String(text)) if !text.is_empty() => return true,
        Some(Value::Array(items)) if !items.is_empty() => return true,
        _ => {}
    }
    delta.get("tool_calls").and_then(Value::as_array).is_some_and(|tools| {
        tools.iter().any(|tool| {
            tool.get("id").is_some_and(Value::is_string)
                || tool.get("function").and_then(|f| f.get("name")).is_some_and(Value::is_string)
                || tool.get("function").and_then(|f| f.get("arguments")).is_some()
        })
    })
}

fn invalid_stream_event(frame: &str) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields {
            message: "Invalid mistral/mistral-chat stream event".to_string(),
            body: Some(frame.to_string()),
            ..Default::default()
        },
        classification: None,
        route: Some(ADAPTER.to_string()),
    })
}

fn step(mut state: ParserState, frame: &str) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let event: Value = serde_json::from_str(frame).map_err(|_| invalid_stream_event(frame))?;
    let Some(record) = event.as_object() else {
        return Err(invalid_stream_event(frame));
    };
    if let Some(error) = record.get("error").filter(|error| !error.is_null()) {
        let Some(error_record) = error.as_object() else {
            return Err(invalid_stream_event(frame));
        };
        let message = error_record.get("message").and_then(Value::as_str).unwrap_or_default();
        let status = error_record.get("code").and_then(Value::as_u64).map(|code| code as u16);
        let reason = crate::provider_error::classify_provider_failure(
            crate::provider_error::ProviderFailure {
                message,
                status,
                raw_body: Some(&encode_json(&event)),
                data: None,
                http: None,
                retry_after_ms: None,
                rate_limit: None,
            },
        );
        return Err(AiError::new(reason));
    }
    let usage = record.get("usage").filter(|usage| !usage.is_null()).and_then(map_usage);
    let usage = usage.or_else(|| state.usage.clone());
    if state.finish_reason.is_some() {
        if has_late_content(&event) {
            return Err(shared::event_error(
                ADAPTER,
                "Mistral Chat received content after the finish reason",
            ));
        }
        state.usage = usage;
        return Ok((state, Vec::new()));
    }
    let choice =
        record.get("choices").and_then(Value::as_array).and_then(|choices| choices.first());
    let mut events: Vec<LLMEvent> = Vec::new();
    let delta = choice.and_then(|choice| choice.get("delta"));
    let with_content = match delta.and_then(|delta| delta.get("content")) {
        Some(content) if !content.is_null() => append_content(&state, &mut events, content),
        _ => state.clone(),
    };
    let tool_deltas: Vec<Value> = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let with_tools = append_tools(&with_content, &mut events, &tool_deltas)?;
    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(finish_reason) = finish_reason else {
        let mut next = with_tools;
        next.usage = usage;
        return Ok((next, events));
    };
    let reason = FinishReasonDetails {
        normalized: map_finish_reason(&finish_reason),
        raw: Some(finish_reason.clone()),
        category: None,
        explanation: None,
    };
    if reason.normalized == FinishReason::Error {
        let details = ReasonFields {
            message: format!("Mistral Chat stopped with {finish_reason}"),
            body: Some(encode_json(&event)),
            ..Default::default()
        };
        let reason = if finish_reason == "network_error" {
            AiErrorReason::ProviderInternal { fields: details, retry_after_ms: None }
        } else {
            AiErrorReason::UnknownProvider { fields: details }
        };
        return Err(AiError::new(reason));
    }
    let incomplete = reason.normalized == FinishReason::Length
        || reason.normalized == FinishReason::ContentFilter;
    if !incomplete && !with_tools.pending_tools.is_empty() {
        return Err(shared::event_error(ADAPTER, "Mistral Chat tool call delta is missing a name"));
    }
    let mut next = with_tools;
    if !incomplete && !next.tools.is_empty() {
        let finished = tool_stream::finish_all(ADAPTER, &next.tools)?;
        next.tools = finished.tools;
        next.completed_tools = finished.events;
    }
    next.usage = usage;
    next.finish_reason = Some(reason);
    Ok((next, events))
}

/// The finish flush emitted when the framed stream ends (port of
/// `finishEvents`).
fn finish_events(state: &ParserState) -> Result<Vec<LLMEvent>, AiError> {
    let Some(reason) = &state.finish_reason else {
        return Err(AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new("Mistral Chat stream ended without finish_reason"),
            classification: Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM.into()),
            route: Some(ADAPTER.to_string()),
        }));
    };
    let mut events = Vec::new();
    let closed = close_active(state, &mut events);
    let lifecycle = if closed.completed_tools.is_empty() {
        closed.lifecycle.clone()
    } else {
        lifecycle::step_start(&closed.lifecycle, &mut events)
    };
    events.extend(closed.completed_tools.iter().cloned());
    let reason = if reason.normalized == FinishReason::Stop
        && closed.completed_tools.iter().any(|event| matches!(event, LLMEvent::ToolCall { .. }))
    {
        FinishReasonDetails { normalized: FinishReason::ToolCalls, ..reason.clone() }
    } else {
        reason.clone()
    };
    lifecycle::finish(
        &lifecycle,
        &mut events,
        lifecycle::FinishInput { reason, usage: closed.usage.clone(), provider_metadata: None },
    );
    Ok(events)
}

/// The Mistral chat-completions protocol (port of `MistralChat.protocol`).
pub struct MistralChatProtocol;

impl Protocol for MistralChatProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        body_from(request)
    }

    fn initial(&self, _request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState::default())
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        if frame == "[DONE]" {
            return Ok((state, Vec::new()));
        }
        let state = *state.downcast::<ParserState>().expect("MistralChatProtocol state");
        let (next, events) = step(state, frame)?;
        Ok((Box::new(next), events))
    }

    /// `[DONE]` is the terminal frame.
    fn terminal(&self, frame: &str) -> bool {
        frame == "[DONE]"
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("MistralChatProtocol state");
        let events = finish_events(&state)?;
        Ok((Box::new(state), events))
    }
}

/// The Mistral chat-completions route (port of `MistralChat.route`).
pub fn mistral_route(transport: SharedTransport) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("mistral".to_string()),
        provider_metadata_key: Some("mistral".to_string()),
        protocol: Arc::new(MistralChatProtocol),
        endpoint: Endpoint::path(PATH).with_base_url(DEFAULT_BASE_URL),
        auth: None,
        framing: Framing::sse_with_done(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

// =============================================================================
// Provider configuration (port of `providers/mistral.ts` configure)
// =============================================================================

/// The native Mistral package (the v2 `/v1/chat/completions` deployment).
pub const NATIVE_PACKAGE: &str = "@opencode/ai/providers/mistral";

/// Mistral provider configuration (port of the `configure` input): the
/// explicit key over the `MISTRAL_API_KEY` fallback, and an optional base
/// URL override (auth is optional).
#[derive(Default)]
pub struct MistralConfig {
    pub api_key: Option<crate::route::auth::Secret>,
    pub base_url: Option<String>,
}

/// The configured native Mistral route: `POST {base}/chat/completions`
/// with optional bearer auth (`AuthOptions.bearer(input, "MISTRAL_API_KEY")`,
/// providers/mistral.ts:32-38). The protocol route itself is auth-free
/// (mistral-chat.ts `Auth.none`); the provider binding adds the bearer.
#[must_use]
pub fn mistral_configured(transport: SharedTransport, config: MistralConfig) -> Route {
    use crate::route::auth;
    let auth =
        auth::or_else(&auth::optional(config.api_key), &auth::config("MISTRAL_API_KEY")).bearer();
    mistral_route(transport).with(crate::route::RoutePatch {
        auth: Some(auth),
        endpoint: Some(crate::route::endpoint::EndpointPatch {
            base_url: Some(config.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string())),
            path: None,
            query: None,
        }),
        ..Default::default()
    })
}

/// Map a Mistral package onto its route (the oc-ai half of the facade's
/// `family_of` selection — the native package runs this protocol).
/// `None` → not a Mistral package.
#[must_use]
pub fn mistral_route_for_package(
    package: &str,
    transport: SharedTransport,
    config: MistralConfig,
) -> Option<Route> {
    match package {
        NATIVE_PACKAGE => Some(mistral_configured(transport, config)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
