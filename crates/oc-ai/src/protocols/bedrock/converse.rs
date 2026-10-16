//! The Bedrock Converse protocol (port of `bedrock-converse.ts`): request
//! body lowering (`modelId`/`messages` content blocks/`system`/
//! `inferenceConfig`/`toolConfig`/`additionalModelRequestFields`) and the
//! streaming state machine over AWS event-stream frames re-wrapped as
//! `{"<event-type>": payload}` JSON (see [`super::event_stream`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{
    self, decode_base64, encode_base64, fit_thinking_budget, parse_json,
    sum_non_cached_to_inclusive, total_tokens,
};
use crate::protocols::utils::{lifecycle, tool_stream};
use crate::route::client::{Route, RouteInput, RoutePatch};
use crate::route::endpoint::{encode_segment, Endpoint, EndpointPart};
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::{Framing, SharedTransport};
use crate::schema::media::Asset;
use crate::schema::{
    AiError, CacheHint, ContentPart, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest,
    MessageRole, ProviderMetadata, ToolChoice, ToolDefinition, ToolResultValue, Usage,
};

use super::sigv4::{self, BedrockAuthOptions};

const ADAPTER: &str = "bedrock-converse";
/// Claude on Bedrock requires the thinking budget below `maxTokens`, with a
/// minimum of 1,024 (port of `MIN_THINKING_BUDGET`).
const MIN_THINKING_BUDGET: f64 = 1024.0;
/// Bedrock accepts at most four explicit `cachePoint` breakpoints per
/// request (port of `BedrockCache.BEDROCK_BREAKPOINT_CAP`).
const BEDROCK_BREAKPOINT_CAP: usize = 4;

// =============================================================================
// Cache breakpoints (port of utils/bedrock-cache.ts)
// =============================================================================

const LEGACY_CLAUDE: [&str; 4] = [
    "anthropic.claude-instant",
    "anthropic.claude-v1",
    "anthropic.claude-v2",
    "anthropic.claude-3-",
];
const CLAUDE_5M: [&str; 6] = [
    "anthropic.claude-3-5-sonnet-20241022-v2:0",
    "anthropic.claude-3-5-haiku-20241022-v1:0",
    "anthropic.claude-3-7-sonnet-20250219-v1:0",
    "anthropic.claude-sonnet-4-20250514-v1:0",
    "anthropic.claude-opus-4-20250514-v1:0",
    "anthropic.claude-opus-4-1-20250805-v1:0",
];

/// Shared four-breakpoint budget (port of `BedrockCache.breakpoints`).
/// Substring matching also handles regional prefixes (`eu.`/`us.`) and
/// model-bearing ARNs.
struct Breakpoints {
    remaining: usize,
    dropped: usize,
    supported: bool,
    ttl_1h: bool,
}

fn breakpoints(model_id: &str) -> Breakpoints {
    let short = CLAUDE_5M.iter().any(|id| model_id.contains(id));
    Breakpoints {
        remaining: BEDROCK_BREAKPOINT_CAP,
        dropped: 0,
        supported: model_id.contains("anthropic.claude-")
            && (short || !LEGACY_CLAUDE.iter().any(|id| model_id.contains(id))),
        ttl_1h: !short,
    }
}

fn cache_point(breakpoints: &mut Breakpoints, cache: Option<&CacheHint>) -> Option<Value> {
    let cache = cache?;
    if !breakpoints.supported {
        return None;
    }
    if breakpoints.remaining == 0 {
        breakpoints.dropped += 1;
        return None;
    }
    breakpoints.remaining -= 1;
    let ttl_1h = breakpoints.ttl_1h && cache.ttl_seconds.is_some_and(|ttl| ttl >= 3600.0);
    Some(if ttl_1h {
        json!({ "cachePoint": { "type": "default", "ttl": "1h" } })
    } else {
        json!({ "cachePoint": { "type": "default" } })
    })
}

// =============================================================================
// Media lowering (port of utils/bedrock-media.ts — image + document blocks)
// =============================================================================

const IMAGE_FORMATS: [(&str, &str); 5] = [
    ("image/png", "png"),
    ("image/jpeg", "jpeg"),
    ("image/jpg", "jpeg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];
const DOCUMENT_FORMATS: [(&str, &str); 9] = [
    ("application/pdf", "pdf"),
    ("text/csv", "csv"),
    ("application/msword", "doc"),
    ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", "docx"),
    ("application/vnd.ms-excel", "xls"),
    ("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "xlsx"),
    ("text/html", "html"),
    ("text/plain", "txt"),
    ("text/markdown", "md"),
];

/// Lower one media part into Bedrock image/document blocks; documents get a
/// user-facing name unique within the request.
fn lower_media(
    asset: &Asset,
    filename: Option<&str>,
    names: &mut Vec<String>,
) -> Result<Vec<Value>, AiError> {
    let inline = asset.inline().ok_or_else(|| {
        shared::invalid_request(
            "Bedrock Converse only accepts inline image and document media (URL/ref assets are not supported)",
        )
    })?;
    let mime = inline.mime.as_str();
    if let Some((_, format)) = IMAGE_FORMATS.iter().find(|(known, _)| *known == mime) {
        return Ok(vec![
            json!({ "image": { "format": format, "source": { "bytes": inline.base64 } } }),
        ]);
    }
    if let Some((_, format)) = DOCUMENT_FORMATS.iter().find(|(known, _)| *known == mime) {
        let base = filename.map_or_else(|| format!("document-{}", names.len() + 1), str::to_string);
        let mut name = base.clone();
        let mut attempt = 1;
        while names.contains(&name) {
            attempt += 1;
            name = format!("{base}-{attempt}");
        }
        names.push(name.clone());
        return Ok(vec![json!({
            "document": { "format": format, "name": name, "source": { "bytes": inline.base64 } }
        })]);
    }
    Err(shared::invalid_request(format!("Bedrock Converse does not support {mime} media")))
}

// =============================================================================
// Mistral tool-id normalization (port of utils/mistral-tool-id.ts)
// =============================================================================

mod mistral_tool_id {
    /// `[A-Za-z0-9]{9}` (port of `MistralToolID.valid`).
    pub fn valid(id: &str) -> bool {
        id.len() == 9 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }

    fn fnv(seed: u32, value: &str) -> u32 {
        let mut result = seed;
        for char in value.chars() {
            result = (result ^ (char as u32 & 0xFF)).wrapping_mul(16_777_619);
        }
        result
    }

    fn to_base36(value: u32) -> String {
        let digits: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
        let mut out = Vec::new();
        let mut remaining = value;
        for _ in 0..7 {
            out.push(digits[(remaining % 36) as usize]);
            remaining /= 36;
        }
        out.into_iter().rev().collect()
    }

    /// Deterministic 9-char `[A-Za-z0-9]` projection (port of
    /// `MistralToolID.hash`).
    pub fn hash(value: &str) -> String {
        let left = to_base36(fnv(2_166_136_261, value));
        let right = to_base36(fnv(2_246_822_519, value));
        let combined: String = format!("{left}{right}");
        combined.chars().rev().take(9).collect::<String>().chars().rev().collect()
    }
}

/// Mistral can reject replay IDs even when they satisfy Converse's broader
/// ID syntax, so every tool id projects to a valid one (port of
/// `MistralToolID.normalizer`).
struct ToolIdNormalizer {
    used: Vec<String>,
    normalized: Vec<(String, String)>,
}

impl ToolIdNormalizer {
    fn collect(request: &LLMRequest) -> Self {
        let mut used = Vec::new();
        for message in &request.messages {
            for part in &message.content {
                let id = match part {
                    ContentPart::ToolCall { id, .. } | ContentPart::ToolResult { id, .. } => {
                        Some(id)
                    }
                    _ => None,
                };
                if let Some(id) = id.filter(|id| mistral_tool_id::valid(id)) {
                    if !used.contains(id) {
                        used.push(id.clone());
                    }
                }
            }
        }
        Self { used, normalized: Vec::new() }
    }

    fn normalize(&mut self, id: &str) -> String {
        if mistral_tool_id::valid(id) {
            return id.to_string();
        }
        if let Some((_, previous)) = self.normalized.iter().find(|(from, _)| from == id) {
            return previous.clone();
        }
        let mut attempt = 0;
        let mut candidate = mistral_tool_id::hash(id);
        while self.used.contains(&candidate) {
            attempt += 1;
            candidate = mistral_tool_id::hash(&format!("{id}:{attempt}"));
        }
        self.used.push(candidate.clone());
        self.normalized.push((id.to_string(), candidate.clone()));
        candidate
    }
}

// =============================================================================
// Request lowering (port of `fromRequest` + lower* helpers)
// =============================================================================

fn lower_tool_spec(tool: &ToolDefinition) -> Value {
    let mut spec = Map::new();
    spec.insert("name".into(), json!(tool.name));
    if !tool.description.trim().is_empty() {
        spec.insert("description".into(), json!(tool.description));
    }
    spec.insert("inputSchema".into(), json!({ "json": tool.input_schema }));
    json!({ "toolSpec": spec })
}

fn lower_tool_choice(choice: &ToolChoice) -> Option<Value> {
    match choice {
        ToolChoice::Auto { .. } => Some(json!({ "auto": {} })),
        ToolChoice::None {} => None,
        ToolChoice::Required { .. } => Some(json!({ "any": {} })),
        ToolChoice::Named { name, .. } => Some(json!({ "tool": { "name": name } })),
    }
}

/// Models can emit names Converse rejects when replayed in history
/// (non-alphanumerics collapse, capped at 64 chars).
fn sanitize_tool_name(name: &str) -> String {
    let cleaned: String =
        name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    let cleaned: String = cleaned.chars().take(64).collect();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

fn remove_empty_tool_input_keys(input: &Value) -> Value {
    match input {
        Value::Array(items) => {
            Value::Array(items.iter().map(remove_empty_tool_input_keys).collect())
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !key.is_empty())
                .map(|(key, value)| (key.clone(), remove_empty_tool_input_keys(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn text_with_cache(
    breakpoints: &mut Breakpoints,
    text: &str,
    cache: Option<&CacheHint>,
) -> Vec<Value> {
    let mut blocks = vec![json!({ "text": text })];
    if let Some(point) = cache_point(breakpoints, cache) {
        blocks.push(point);
    }
    blocks
}

fn tool_result_text(result: &ToolResultValue) -> String {
    match result {
        ToolResultValue::Text { value } | ToolResultValue::Error { value } => {
            value.as_str().map_or_else(|| value.to_string(), str::to_string)
        }
        ToolResultValue::Json { value } => value.to_string(),
        ToolResultValue::Content { .. } => String::new(),
    }
}

/// Keep Claude and Nova tool-result images inline; other models' images are
/// hoisted beside the result (port of `keepToolImagesInline`).
fn keep_tool_images_inline(id: &str) -> bool {
    id.contains("anthropic.claude-") || id.contains("amazon.nova-")
}

/// The reasoning signature: `encrypted` first, then provider metadata
/// (port of `reasoningSignature`).
fn reasoning_signature(part: &ContentPart, key: &str) -> Option<String> {
    let ContentPart::Reasoning { encrypted, provider_metadata, .. } = part else {
        return None;
    };
    if let Some(encrypted) = encrypted.as_ref().filter(|value| !value.is_empty()) {
        return Some(encrypted.clone());
    }
    provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|record| record.get("signature"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn reasoning_redacted_data(part: &ContentPart, key: &str) -> Option<String> {
    let ContentPart::Reasoning { provider_metadata, .. } = part else {
        return None;
    };
    provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|record| record.get("redactedData"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Wrap one per-provider metadata record: `{key: {…record}}`.
fn provider_metadata(key: &str, metadata: Value) -> ProviderMetadata {
    let record: BTreeMap<String, Value> = serde_json::from_value(metadata).unwrap_or_default();
    BTreeMap::from([(key.to_string(), record)])
}

/// Append `content` onto the last message when it is a user turn, else push
/// a new one (Converse only alternates user/assistant; v2 merges the same
/// way — `lowerMessages`'s `previous?.role === "user"` branches).
fn push_user(messages: &mut Vec<Value>, content: Vec<Value>) {
    if let Some(Value::Object(previous)) =
        messages.last_mut().filter(|last| last.get("role").and_then(Value::as_str) == Some("user"))
    {
        if let Some(previous_content) = previous.get_mut("content").and_then(Value::as_array_mut) {
            previous_content.extend(content);
            return;
        }
    }
    messages.push(json!({ "role": "user", "content": content }));
}

fn flush_images(messages: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if pending.is_empty() {
        return;
    }
    let images = std::mem::take(pending);
    push_user(messages, images);
}

/// Lower the message history into Converse roles (port of `lowerMessages`):
/// system messages merge into the next user turn, tool messages land as
/// user `toolResult` blocks (images hoisted beside the result for models
/// that reject inline ones), consecutive same-role turns merge.
fn lower_messages(
    request: &LLMRequest,
    breakpoints: &mut Breakpoints,
) -> Result<Vec<Value>, AiError> {
    let mut messages: Vec<Value> = Vec::new();
    let mut document_names: Vec<String> = Vec::new();
    let mut mistral_ids = (request.model.id.as_str().contains("mistral."))
        .then(|| ToolIdNormalizer::collect(request));
    let mut identity = |id: &str| -> String {
        match mistral_ids.as_mut() {
            Some(normalizer) => normalizer.normalize(id),
            None => id.to_string(),
        }
    };
    let hoist_images = !keep_tool_images_inline(request.model.id.as_str());
    let mut pending_images: Vec<Value> = Vec::new();

    for message in &request.messages {
        if message.role != MessageRole::Tool {
            flush_images(&mut messages, &mut pending_images);
        }
        match message.role {
            MessageRole::System => {
                let texts: Vec<&str> = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let mut content = Vec::new();
                if texts.len() == message.content.len() && !texts.is_empty() {
                    for text in texts {
                        content.extend(text_with_cache(breakpoints, text, None));
                    }
                } else {
                    content.extend(text_with_cache(
                        breakpoints,
                        &shared::wrap_system_update(texts),
                        None,
                    ));
                }
                push_user(&mut messages, content);
            }
            MessageRole::User => {
                let mut content: Vec<Value> = Vec::new();
                for part in &message.content {
                    match part {
                        ContentPart::Text { text, cache, .. } => {
                            content.extend(text_with_cache(breakpoints, text, cache.as_ref()));
                        }
                        ContentPart::Media { media, filename, .. } => {
                            content.extend(lower_media(
                                media,
                                filename.as_deref(),
                                &mut document_names,
                            )?);
                        }
                        _ => {
                            return Err(shared::invalid_request(
                                "Bedrock Converse only supports text and media user content",
                            ))
                        }
                    }
                }
                push_user(&mut messages, content);
            }
            MessageRole::Assistant => {
                let mut content: Vec<Value> = Vec::new();
                for part in &message.content {
                    match part {
                        ContentPart::Text { text, cache, .. } => {
                            content.extend(text_with_cache(breakpoints, text, cache.as_ref()));
                        }
                        ContentPart::Reasoning { text, .. } => {
                            let signature = reasoning_signature(part, "bedrock");
                            let redacted = reasoning_redacted_data(part, "bedrock");
                            if signature.is_none() && redacted.is_some() {
                                content.push(json!({ "reasoningContent": { "redactedContent": redacted } }));
                                continue;
                            }
                            match signature {
                                Some(signature) if !signature.trim().is_empty() => {
                                    content.push(json!({ "reasoningContent": { "reasoningText": { "text": text, "signature": signature } } }));
                                }
                                _ => {
                                    // Interrupted streams and model switches can
                                    // leave unsigned reasoning: preserve readable
                                    // history as text rather than replay invalid
                                    // reasoningContent.
                                    if !text.trim().is_empty() {
                                        content.extend(text_with_cache(breakpoints, text, None));
                                    }
                                }
                            }
                        }
                        ContentPart::ToolCall { id, name, input, .. } => {
                            content.push(json!({ "toolUse": {
                                "toolUseId": identity(id),
                                "name": sanitize_tool_name(name),
                                "input": remove_empty_tool_input_keys(input),
                            } }));
                        }
                        _ => {
                            return Err(shared::invalid_request(
                                "Bedrock Converse only supports text, reasoning, and tool-call assistant content",
                            ))
                        }
                    }
                }
                if !content.is_empty() {
                    messages.push(json!({ "role": "assistant", "content": content }));
                }
            }
            MessageRole::Tool => {
                let mut content: Vec<Value> = Vec::new();
                for part in &message.content {
                    let ContentPart::ToolResult { id, result, cache, .. } = part else {
                        return Err(shared::invalid_request(
                            "Bedrock Converse tool messages must carry tool-result content",
                        ));
                    };
                    let lowered: Vec<Value> = match result {
                        ToolResultValue::Text { .. } | ToolResultValue::Error { .. } => {
                            vec![json!({ "text": tool_result_text(result) })]
                        }
                        ToolResultValue::Json { value } => vec![json!({ "json": value })],
                        ToolResultValue::Content { value } => {
                            let mut items = Vec::new();
                            for item in &value.0 {
                                let kind =
                                    item.get("type").and_then(Value::as_str).unwrap_or_default();
                                match kind {
                                    "text" => {
                                        items.push(json!({ "text": item.get("text").cloned().unwrap_or(Value::Null) }));
                                    }
                                    "image" => {
                                        let media: Asset = serde_json::from_value(item.get("media").cloned().unwrap_or(Value::Null)).map_err(|error| {
                                            shared::invalid_request(format!(
                                                "Bedrock Converse tool-result image failed to decode: {error}"
                                            ))
                                        })?;
                                        items.extend(lower_media(&media, None, &mut document_names)?);
                                    }
                                    _ => {
                                        return Err(shared::invalid_request(
                                            "Bedrock Converse tool-result content only supports text and image items",
                                        ))
                                    }
                                }
                            }
                            items
                        }
                    };
                    let status = if matches!(result, ToolResultValue::Error { .. }) {
                        "error"
                    } else {
                        "success"
                    };
                    let images: Vec<Value> = if hoist_images {
                        lowered.iter().filter(|item| item.get("image").is_some()).cloned().collect()
                    } else {
                        Vec::new()
                    };
                    let non_image: Vec<Value> =
                        lowered.into_iter().filter(|item| item.get("image").is_none()).collect();
                    let block_content = if non_image.is_empty() && !images.is_empty() {
                        vec![json!({ "text": "See attached image." })]
                    } else {
                        non_image
                    };
                    content.push(json!({ "toolResult": {
                        "toolUseId": identity(id),
                        "content": block_content,
                        "status": status,
                    } }));
                    pending_images.extend(images);
                    if let Some(point) = cache_point(breakpoints, cache.as_ref()) {
                        content.push(point);
                    }
                }
                push_user(&mut messages, content);
            }
        }
    }
    flush_images(&mut messages, &mut pending_images);
    Ok(messages)
}

/// System prompts share the cache-point convention (port of `lowerSystem`).
fn lower_system(request: &LLMRequest, breakpoints: &mut Breakpoints) -> Option<Value> {
    let content: Vec<Value> = request
        .system
        .iter()
        .filter(|part| !part.text.is_empty())
        .flat_map(|part| text_with_cache(breakpoints, &part.text, part.cache.as_ref()))
        .collect();
    (!content.is_empty()).then_some(Value::Array(content))
}

/// Nova 2 rejects `maxTokens` at high reasoning effort, where its output can
/// exceed the field's maximum (port of `isNova2` + `isHighReasoningEffort`).
fn nova2_max_tokens(request: &LLMRequest, max_tokens: Option<f64>) -> Option<f64> {
    if !request.model.id.as_str().contains("amazon.nova-2-") {
        return max_tokens;
    }
    let high = request
        .http
        .as_ref()
        .and_then(|http| http.body.as_ref())
        .and_then(|body| body.get("additionalModelRequestFields"))
        .and_then(|fields| fields.get("reasoningConfig"))
        .and_then(|config| config.get("maxReasoningEffort"))
        .and_then(Value::as_str)
        .is_some_and(|effort| effort == "high");
    if high {
        None
    } else {
        max_tokens
    }
}

/// The Converse request body (port of `BedrockConverseBody` /
/// `fromRequest`). Cache points are positional: emitted right after the
/// content they mark, tools → system → messages order for the budget.
pub fn body_from(request: &LLMRequest) -> Result<Value, AiError> {
    let generation = request.generation.as_ref();
    let tools = shared::flatten_tools(&request.tools);
    let tool_choice = request.tool_choice.as_ref().and_then(lower_tool_choice);
    let mut breakpoints = breakpoints(request.model.id.as_str());
    let tool_config = (!tools.is_empty()).then(|| {
        let mut entries: Vec<Value> = Vec::with_capacity(tools.len() * 2);
        for tool in &tools {
            entries.push(lower_tool_spec(tool));
            if let Some(point) = cache_point(&mut breakpoints, tool.cache.as_ref()) {
                entries.push(point);
            }
        }
        let mut config = Map::new();
        config.insert("tools".into(), Value::Array(entries));
        if let Some(choice) = tool_choice {
            config.insert("toolChoice".into(), choice);
        }
        Value::Object(config)
    });

    let max_tokens = nova2_max_tokens(request, generation.and_then(|options| options.max_tokens));

    // Thinking option: providerOptions root `thinking` (v2 decodeOptions).
    let thinking = request.provider_options.as_ref().and_then(|options| options.get("thinking"))
        .and_then(|thinking| {
            let enabled = thinking.get("type").and_then(Value::as_str) == Some("enabled");
            let budget = thinking.get("budgetTokens").and_then(Value::as_f64);
            budget.filter(|_| enabled)
        })
        .map(|budget_tokens| {
            json!({
                "type": "enabled",
                "budget_tokens": fit_thinking_budget(budget_tokens, max_tokens, MIN_THINKING_BUDGET),
            })
        });

    let system = lower_system(request, &mut breakpoints);
    let messages = lower_messages(request, &mut breakpoints)?;
    let inference_config = {
        let stop_empty = generation
            .and_then(|options| options.stop.as_ref())
            .is_none_or(std::vec::Vec::is_empty);
        if max_tokens.is_none()
            && generation.and_then(|options| options.temperature).is_none()
            && generation.and_then(|options| options.top_p).is_none()
            && stop_empty
        {
            None
        } else {
            let mut config = Map::new();
            if let Some(max_tokens) = max_tokens {
                config.insert("maxTokens".into(), json!(max_tokens));
            }
            if let Some(temperature) = generation.and_then(|options| options.temperature) {
                config.insert("temperature".into(), json!(temperature));
            }
            if let Some(top_p) = generation.and_then(|options| options.top_p) {
                config.insert("topP".into(), json!(top_p));
            }
            if let Some(stop) =
                generation.and_then(|options| options.stop.as_ref()).filter(|stop| !stop.is_empty())
            {
                config.insert("stopSequences".into(), json!(stop));
            }
            Some(Value::Object(config))
        }
    };
    // Converse's base inferenceConfig has no topK or thinking; models accept
    // them as model-specific fields through additionalModelRequestFields.
    let additional = match (generation.and_then(|options| options.top_k), thinking) {
        (None, None) => None,
        (top_k, thinking) => {
            let mut fields = Map::new();
            if let Some(top_k) = top_k {
                fields.insert("top_k".into(), json!(top_k));
            }
            if let Some(thinking) = thinking {
                fields.insert("thinking".into(), thinking);
            }
            Some(Value::Object(fields))
        }
    };
    let mut body = Map::new();
    body.insert("modelId".into(), json!(request.model.id.as_str()));
    body.insert("messages".into(), Value::Array(messages));
    if let Some(system) = system {
        body.insert("system".into(), system);
    }
    if let Some(inference_config) = inference_config {
        body.insert("inferenceConfig".into(), inference_config);
    }
    if let Some(tool_config) = tool_config {
        body.insert("toolConfig".into(), tool_config);
    }
    if let Some(additional) = additional {
        body.insert("additionalModelRequestFields".into(), additional);
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream parsing (port of `step` + `onHalt`)
// =============================================================================

fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" | "model_context_window_exceeded" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        "content_filtered" | "guardrail_intervened" => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

#[derive(Default)]
struct ParserState {
    provider_metadata_key: String,
    tools: tool_stream::State,
    finish_reason: Option<FinishReasonDetails>,
    usage: Option<Usage>,
    has_tool_calls: bool,
    lifecycle: lifecycle::State,
    reasoning_signatures: BTreeMap<usize, String>,
    reasoning_redacted: BTreeMap<usize, Vec<Vec<u8>>>,
}

/// AWS reports inputTokens separately from cache reads and writes; Bedrock
/// does not break reasoning out of outputTokens (port of `mapUsage`).
fn map_usage(usage: &Value, key: &str) -> Usage {
    let field = |name: &str| usage.get(name).and_then(Value::as_f64);
    let input_tokens = sum_non_cached_to_inclusive(
        field("inputTokens"),
        field("cacheReadInputTokens"),
        field("cacheWriteInputTokens"),
    );
    Usage {
        input_tokens,
        output_tokens: field("outputTokens"),
        non_cached_input_tokens: field("inputTokens"),
        cache_read_input_tokens: field("cacheReadInputTokens"),
        cache_write_input_tokens: field("cacheWriteInputTokens"),
        total_tokens: total_tokens(input_tokens, field("outputTokens"), field("totalTokens")),
        provider_metadata: Some(provider_metadata(key, usage.clone())),
        ..Default::default()
    }
}

fn step(mut state: ParserState, frame: &str) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let event = parse_json(ADAPTER, frame, "Failed to parse Bedrock Converse event")?;
    let key = state.provider_metadata_key.clone();
    let mut events: Vec<LLMEvent> = Vec::new();

    if let Some(start) = event.get("contentBlockStart") {
        if let Some(tool_use) = start.get("start").and_then(|start| start.get("toolUse")) {
            let index =
                start.get("contentBlockIndex").and_then(Value::as_u64).unwrap_or_default() as usize;
            let id =
                tool_use.get("toolUseId").and_then(Value::as_str).unwrap_or_default().to_string();
            let name = tool_use.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            state.lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
            state.tools = tool_stream::start(
                &state.tools,
                &index.to_string(),
                tool_stream::PendingTool {
                    id: id.clone(),
                    name: name.clone(),
                    namespace: None,
                    input: String::new(),
                    provider_executed: None,
                    provider_metadata: None,
                },
            );
            events.push(LLMEvent::ToolInputStart {
                id,
                name,
                namespace: None,
                provider_executed: None,
                provider_metadata: None,
            });
        }
        return Ok((state, events));
    }

    if let Some(delta_event) = event.get("contentBlockDelta") {
        let index = delta_event.get("contentBlockIndex").and_then(Value::as_u64).unwrap_or_default()
            as usize;
        let delta = delta_event.get("delta").cloned().unwrap_or(Value::Null);
        if let Some(text) = delta.get("text").and_then(Value::as_str) {
            let id = format!("text-{index}");
            state.lifecycle = lifecycle::text_delta(&state.lifecycle, &mut events, &id, text, None);
            return Ok((state, events));
        }
        if let Some(reasoning) = delta.get("reasoningContent") {
            let text = reasoning.get("text").and_then(Value::as_str);
            let signature = reasoning.get("signature").and_then(Value::as_str);
            let data = reasoning.get("data").and_then(Value::as_str);
            let redacted = reasoning.get("redactedContent").and_then(Value::as_str);
            let redacted_chunk = match redacted {
                Some(redacted) => Some(decode_base64(redacted).map_err(|error| {
                    AiError::event_error(
                        ADAPTER,
                        format!(
                            "Bedrock Converse reasoningContent.redactedContent contains invalid base64 data: {error}"
                        ),
                    )
                })?),
                None => None,
            };
            let has_redacted_chunk = redacted_chunk.is_some();
            let metadata = if let Some(signature) = signature {
                Some(provider_metadata(&key, json!({ "signature": signature })))
            } else if !has_redacted_chunk {
                data.map(|data| provider_metadata(&key, json!({ "redactedData": data })))
            } else {
                None
            };
            let id = format!("reasoning-{index}");
            if text.is_some() || metadata.is_some() {
                state.lifecycle = lifecycle::reasoning_delta(
                    &state.lifecycle,
                    &mut events,
                    &id,
                    text.unwrap_or(""),
                    metadata.clone(),
                );
            }
            if let Some(chunk) = redacted_chunk {
                state.reasoning_redacted.entry(index).or_default().push(chunk);
                if text.is_none() && metadata.is_none() {
                    state.lifecycle =
                        lifecycle::reasoning_start(&state.lifecycle, &mut events, &id, None);
                }
            }
            if let Some(signature) = signature {
                state.reasoning_signatures.insert(index, signature.to_string());
            }
            if !has_redacted_chunk && data.is_some() {
                state.reasoning_redacted.remove(&index);
            }
            return Ok((state, events));
        }
        if let Some(input) =
            delta.get("toolUse").and_then(|tool| tool.get("input")).and_then(Value::as_str)
        {
            // A delta for a block that is not open — stopped or never
            // started — has nothing to attach to and is dropped.
            if let Some(outcome) = tool_stream::append(&state.tools, &index.to_string(), input) {
                if !outcome.events.is_empty() {
                    state.lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
                }
                events.extend(outcome.events);
                state.tools = outcome.tools;
            }
            return Ok((state, events));
        }
        return Ok((state, events));
    }

    if let Some(stop) = event.get("contentBlockStop") {
        let index =
            stop.get("contentBlockIndex").and_then(Value::as_u64).unwrap_or_default() as usize;
        let result = tool_stream::finish(ADAPTER, &state.tools, &index.to_string())?;
        if result.events.is_empty() {
            let metadata = state
                .reasoning_signatures
                .get(&index)
                .map(|signature| provider_metadata(&key, json!({ "signature": signature })))
                .or_else(|| {
                    state.reasoning_redacted.get(&index).map(|chunks| {
                        provider_metadata(
                            &key,
                            json!({ "redactedData": encode_base64(&chunks.concat()) }),
                        )
                    })
                });
            let text_id = format!("text-{index}");
            state.lifecycle =
                lifecycle::text_end(&state.lifecycle, &mut events, &text_id, None, None);
            let reasoning_id = format!("reasoning-{index}");
            state.lifecycle = lifecycle::reasoning_end(
                &state.lifecycle,
                &mut events,
                &reasoning_id,
                metadata,
                None,
            );
        } else {
            state.lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        }
        let has_tool_events = result.events.iter().any(|event| {
            matches!(event, LLMEvent::ToolCall { .. } | LLMEvent::ToolInputError { .. })
        });
        events.extend(result.events);
        if has_tool_events {
            state.has_tool_calls = true;
        }
        state.tools = result.tools;
        state.reasoning_signatures.remove(&index);
        state.reasoning_redacted.remove(&index);
        return Ok((state, events));
    }

    if let Some(stop) = event.get("messageStop") {
        let reason = stop.get("stopReason").and_then(Value::as_str).unwrap_or_default();
        if reason == "malformed_model_output" || reason == "malformed_tool_use" {
            return Err(AiError::event_error(
                ADAPTER,
                format!("Bedrock Converse stopped with {reason}"),
            ));
        }
        state.finish_reason = Some(FinishReasonDetails {
            normalized: map_finish_reason(reason),
            raw: Some(reason.to_string()),
            category: None,
            explanation: None,
        });
        return Ok((state, events));
    }

    if let Some(metadata) = event.get("metadata") {
        if let Some(usage) = metadata.get("usage") {
            state.usage = Some(map_usage(usage, &key)).or(state.usage);
        }
        return Ok((state, events));
    }

    if let Some(exception) = event.get("exception") {
        let details = exception.get("details").cloned().unwrap_or(Value::Null);
        let message = details
            .get("message")
            .or_else(|| details.get("originalMessage"))
            .and_then(Value::as_str)
            .unwrap_or("Bedrock Converse stream error");
        return Err(AiError::event_error(ADAPTER, message));
    }

    Ok((state, events))
}

/// The finish pair is synthesized after the stream ends: `messageStop`
/// carries the stop reason, `metadata` the usage — both held in state so
/// exactly one finish emits once both chunks had their chance (port of
/// `onHalt`).
fn on_halt(state: &ParserState) -> Vec<LLMEvent> {
    let Some(finish_reason) = &state.finish_reason else {
        return Vec::new();
    };
    let normalized = if finish_reason.normalized == FinishReason::Stop && state.has_tool_calls {
        FinishReason::ToolCalls
    } else {
        finish_reason.normalized
    };
    let mut events = Vec::new();
    let mut lifecycle_state = state.lifecycle.clone();
    for (index, chunks) in &state.reasoning_redacted {
        let id = format!("reasoning-{index}");
        let metadata = state.reasoning_signatures.get(index).map_or_else(
            || {
                provider_metadata(
                    &state.provider_metadata_key,
                    json!({ "redactedData": encode_base64(&chunks.concat()) }),
                )
            },
            |signature| {
                provider_metadata(&state.provider_metadata_key, json!({ "signature": signature }))
            },
        );
        lifecycle_state =
            lifecycle::reasoning_end(&lifecycle_state, &mut events, &id, Some(metadata), None);
    }
    lifecycle::finish(
        &lifecycle_state,
        &mut events,
        lifecycle::FinishInput {
            reason: FinishReasonDetails {
                normalized,
                raw: finish_reason.raw.clone(),
                category: None,
                explanation: None,
            },
            usage: state.usage.clone(),
            provider_metadata: None,
        },
    );
    events
}

/// The Bedrock Converse protocol.
pub struct BedrockConverseProtocol;

impl Protocol for BedrockConverseProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        body_from(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState {
            provider_metadata_key: request
                .model
                .route
                .provider_metadata_key
                .clone()
                .unwrap_or_else(|| "bedrock".to_string()),
            ..Default::default()
        })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("BedrockConverseProtocol state");
        let (next, events) = step(state, frame)?;
        Ok((Box::new(next), events))
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("BedrockConverseProtocol state");
        let events = on_halt(&state);
        Ok((Box::new(state), events))
    }
}

// =============================================================================
// Routes
// =============================================================================

/// The Bedrock Converse route (port of `BedrockConverse.route` +
/// `amazon-bedrock.ts`'s `configuredRoute`): the validated model id embeds
/// in the path so the URL matches the body that gets signed, and the region
/// embeds in the endpoint host.
pub fn bedrock_converse_route(transport: SharedTransport, options: BedrockAuthOptions) -> Route {
    let auth = sigv4::resolve_auth(&options, "bedrock", "Bedrock Converse");
    let region = sigv4::resolve_region(options.region.as_deref(), options.credentials.as_ref());
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("amazon-bedrock".to_string()),
        provider_metadata_key: Some("bedrock".to_string()),
        protocol: Arc::new(BedrockConverseProtocol),
        endpoint: Endpoint::dynamic_path(|input| {
            let model = input.request.model.id.as_str();
            format!("/model/{}/converse-stream", encode_segment(model))
        })
        .with_base_url(format!("https://bedrock-runtime.{region}.amazonaws.com")),
        auth: Some(auth),
        framing: Framing::aws_event_stream(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

/// Anthropic-on-Bedrock: the Anthropic Messages protocol over Bedrock's
/// `invoke-stream` endpoint (the pre-Converse invoke shape — the
/// `/model/{id}/invoke-stream` endpoint AWS exposes for the native
/// anthropic messages body).
pub fn bedrock_invoke_route(transport: SharedTransport, options: BedrockAuthOptions) -> Route {
    let auth = sigv4::resolve_auth(&options, "bedrock", "Bedrock Invoke");
    let region = sigv4::resolve_region(options.region.as_deref(), options.credentials.as_ref());
    crate::protocols::anthropic_messages::anthropic_messages_route(transport).with(RoutePatch {
        id: Some("bedrock-invoke".to_string()),
        provider: Some("amazon-bedrock".to_string()),
        provider_metadata_key: Some("bedrock".to_string()),
        auth: Some(auth),
        endpoint: Some(crate::route::endpoint::EndpointPatch {
            base_url: Some(format!("https://bedrock-runtime.{region}.amazonaws.com")),
            path: Some(EndpointPart::Dynamic(Arc::new(
                |input: &crate::route::endpoint::EndpointInput<'_>| {
                    let model = input.request.model.id.as_str();
                    format!("/model/{}/invoke-stream", encode_segment(model))
                },
            ))),
            query: None,
        }),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests;
