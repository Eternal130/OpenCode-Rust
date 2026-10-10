//! The provider-neutral Open Responses protocol (port of
//! `src/protocols/open-responses.ts`, HTTP/SSE path).
//!
//! This is the baseline Responses implementation that ~8 providers build on:
//! request lowering (message history → `input` items incl. function calls,
//! reasoning with encrypted-content passthrough, hosted tool replay) and the
//! pure SSE step machine (`response.output_item.*`, `*.delta` streams,
//! `response.completed/failed/incomplete`) normalized into common
//! [`LLMEvent`]s with item-id-keyed state.
//!
//! Deferred (per ai-map §10): the WebSocket channel/continuation transport —
//! SSE is the parity path.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::shared::{
    self, encode_json, event_error, flatten_tools, invalid_request, join_text, prompt_cache_key,
    subtract_tokens, sum_tokens, total_tokens,
};
use crate::protocols::utils::lifecycle;
use crate::protocols::utils::open_responses_options as options;
use crate::protocols::utils::open_responses_options::Resolved;
use crate::protocols::utils::tool_stream::{self, PendingTool};
use crate::route::auth::Definition as AuthDefinition;
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{Route, RouteDefaults, RouteInput};
use crate::schema::media::{self, Asset};
use crate::schema::{
    AiError, AiErrorReason, ContentPart, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest,
    Message, MessageRole, ProviderId, ProviderMetadata, ReasonFields, ToolChoice, ToolDefinition,
    Usage,
};

pub const ADAPTER: &str = "open-responses";
pub const NAME: &str = "Open Responses";
pub const PATH: &str = "/responses";
pub const DEFAULT_PROVIDER_METADATA_KEY: &str = "openresponses";

// =============================================================================
// Request body wire types
// =============================================================================

/// `input_text` content item.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "input_text")]
pub struct InputText {
    pub text: String,
}

/// `input_image` content item.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "input_image")]
pub struct InputImage {
    pub image_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `input_file` content item (`file_url` or `file_data`, not both).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "input_file")]
pub struct InputFile {
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_url: Option<String>,
}

/// `input_video` content item (tool results only).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "input_video")]
pub struct InputVideo {
    pub video_url: String,
}

/// `output_text` content item (assistant replay).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "output_text")]
pub struct OutputText {
    pub text: String,
}

/// `summary_text` reasoning summary part.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "summary_text")]
pub struct SummaryText {
    pub text: String,
}

/// A lowered function tool definition.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// The `message` phase marker (`commentary` / `final_answer`; JSON `null` is
/// a distinct wire value from absence).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MessagePhase {
    #[serde(rename = "commentary")]
    Commentary,
    #[serde(rename = "final_answer")]
    FinalAnswer,
}

/// Phase field: `None` = absent, `Some(None)` = JSON `null`,
/// `Some(Some(phase))` = a known phase.
pub type PhaseField = Option<Option<MessagePhase>>;

/// Read a phase exactly like the TS `messagePhase` helper: `null` and the two
/// known literals pass through, anything else reads as absent.
#[must_use]
pub fn message_phase(value: Option<&Value>) -> PhaseField {
    match value {
        Some(Value::Null) => Some(None),
        Some(Value::String(text)) if text == "commentary" => Some(Some(MessagePhase::Commentary)),
        Some(Value::String(text)) if text == "final_answer" => {
            Some(Some(MessagePhase::FinalAnswer))
        }
        _ => None,
    }
}

fn phase_json(phase: PhaseField) -> Option<Value> {
    phase.map(|phase| serde_json::to_value(phase).unwrap_or(Value::Null))
}

/// One `input` array item (port of `InputItem`). Optionality of `null`-able
/// fields is controlled by hand-building the JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum InputItem {
    Compaction {
        id: Option<String>,
        encrypted_content: String,
    },
    ConfigurationUpdate {
        effort: String,
    },
    Message {
        id: Option<String>,
        role: &'static str,
        status: Option<String>,
        phase: PhaseField,
        content: Vec<Value>,
    },
    /// `content` as a bare string (system/developer messages).
    MessageText {
        role: &'static str,
        content: String,
    },
    Reasoning {
        id: Option<String>,
        summary: Vec<SummaryText>,
        /// `None` = absent, `Some(None)` = JSON `null`.
        encrypted_content: Option<Option<String>>,
    },
    FunctionCall {
        id: Option<String>,
        call_id: String,
        name: String,
        namespace: Option<String>,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: Value,
    },
    /// Hosted tool item or replay, passed through verbatim.
    Passthrough(Value),
}

impl InputItem {
    /// The exact wire JSON for this item.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            InputItem::Compaction { id, encrypted_content } => {
                let mut object = Map::new();
                object.insert("type".into(), json!("compaction"));
                if let Some(id) = id {
                    object.insert("id".into(), json!(id));
                }
                object.insert("encrypted_content".into(), json!(encrypted_content));
                Value::Object(object)
            }
            InputItem::ConfigurationUpdate { effort } => {
                json!({ "type": "configuration_update", "reasoning": { "effort": effort } })
            }
            InputItem::Message { id, role, status, phase, content } => {
                let mut object = Map::new();
                object.insert("type".into(), json!("message"));
                if let Some(id) = id {
                    object.insert("id".into(), json!(id));
                }
                object.insert("role".into(), json!(role));
                if let Some(status) = status {
                    object.insert("status".into(), json!(status));
                }
                object.insert("content".into(), Value::Array(content.clone()));
                if let Some(phase) = phase_json(*phase) {
                    object.insert("phase".into(), phase);
                }
                Value::Object(object)
            }
            InputItem::MessageText { role, content } => {
                json!({ "type": "message", "role": role, "content": content })
            }
            InputItem::Reasoning { id, summary, encrypted_content } => {
                let mut object = Map::new();
                object.insert("type".into(), json!("reasoning"));
                if let Some(id) = id {
                    object.insert("id".into(), json!(id));
                }
                object.insert(
                    "summary".into(),
                    Value::Array(
                        summary
                            .iter()
                            .map(|part| serde_json::to_value(part).unwrap_or(Value::Null))
                            .collect(),
                    ),
                );
                if let Some(encrypted) = encrypted_content {
                    object.insert(
                        "encrypted_content".into(),
                        encrypted.clone().map_or(Value::Null, |text| json!(text)),
                    );
                }
                Value::Object(object)
            }
            InputItem::FunctionCall { id, call_id, name, namespace, arguments } => {
                let mut object = Map::new();
                object.insert("type".into(), json!("function_call"));
                if let Some(id) = id {
                    object.insert("id".into(), json!(id));
                }
                object.insert("call_id".into(), json!(call_id));
                object.insert("name".into(), json!(name));
                if let Some(namespace) = namespace {
                    object.insert("namespace".into(), json!(namespace));
                }
                object.insert("arguments".into(), json!(arguments));
                Value::Object(object)
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                json!({ "type": "function_call_output", "call_id": call_id, "output": output })
            }
            InputItem::Passthrough(value) => value.clone(),
        }
    }

    /// The item's `type` discriminator.
    #[must_use]
    pub fn kind(&self) -> &str {
        match self {
            InputItem::Compaction { .. } => "compaction",
            InputItem::ConfigurationUpdate { .. } => "configuration_update",
            InputItem::Message { .. } | InputItem::MessageText { .. } => "message",
            InputItem::Reasoning { .. } => "reasoning",
            InputItem::FunctionCall { .. } => "function_call",
            InputItem::FunctionCallOutput { .. } => "function_call_output",
            InputItem::Passthrough(value) => {
                value.get("type").and_then(Value::as_str).unwrap_or("item")
            }
        }
    }
}

// =============================================================================
// Provider adapter seam
// =============================================================================

/// Per-provider extension points over the baseline protocol (port of
/// `ProviderAdapter`): hosted-tool lowering, provider media rules, and
/// hosted-item replay restoration.
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    /// Lower a provider-native tool config (`ToolDefinition.native.<id>`);
    /// `None` means the adapter has no native tools and the baseline error
    /// applies.
    fn native_tool(&self, _native: &BTreeMap<String, Value>) -> Option<Result<Value, AiError>> {
        None
    }
    /// Provider-specific media lowering; `None` falls through to the baseline.
    fn lower_media(
        &self,
        _part: &ContentPart,
        _media: &Asset,
        _request: &LLMRequest,
    ) -> Option<Value> {
        None
    }
    /// Restore a replayable hosted item from a provider-executed tool result.
    fn restore_hosted_tool_item(&self, _item: &Value) -> Option<Value> {
        None
    }
}

/// The provider-neutral adapter (`BASE_ADAPTER`).
pub struct BaseAdapter;

impl ProviderAdapter for BaseAdapter {
    fn id(&self) -> &str {
        ADAPTER
    }
    fn name(&self) -> &str {
        NAME
    }
}

// =============================================================================
// Request lowering
// =============================================================================

/// Serialize integral floats as JSON integers so wire bytes match the TS
/// encoder (`20` stays `20`, not `20.0`) — [`crate::protocols::shared::number`].
fn number(value: f64) -> Value {
    crate::protocols::shared::number(value)
}

/// Lower one tool definition (port of `lowerTool`): the baseline rejects
/// provider-native tools.
pub fn lower_tool(protocol_name: &str, tool: &ToolDefinition) -> Result<Value, AiError> {
    if tool.native.is_some() {
        return Err(invalid_request(format!(
            "{protocol_name} does not support provider-native tool {}",
            tool.name
        )));
    }
    Ok(serde_json::to_value(Tool {
        kind: "function",
        name: tool.name.clone(),
        description: tool.description.clone(),
        parameters: tool.input_schema.clone(),
        strict: Some(false),
    })
    .unwrap_or(Value::Null))
}

/// Lower the (already flattened) tool list, honoring adapter native tools.
pub fn lower_tools(
    tools: &[ToolDefinition],
    adapter: &dyn ProviderAdapter,
) -> Result<Vec<Value>, AiError> {
    tools
        .iter()
        .map(|tool| match tool.native.as_ref() {
            Some(native) => adapter.native_tool(native).unwrap_or_else(|| {
                Err(invalid_request(format!(
                    "{} does not support provider-native tool {}",
                    adapter.name(),
                    tool.name
                )))
            }),
            None => lower_tool(adapter.name(), tool),
        })
        .collect()
}

/// Lower the common tool choice (port of `lowerToolChoice` +
/// `matchToolChoice`).
pub fn lower_tool_choice(protocol_name: &str, tool_choice: &ToolChoice) -> Result<Value, AiError> {
    match tool_choice {
        ToolChoice::Auto { .. } => Ok(json!("auto")),
        ToolChoice::None { .. } => Ok(json!("none")),
        ToolChoice::Required { .. } => Ok(json!("required")),
        ToolChoice::Named { name, .. } if name.is_empty() => {
            Err(invalid_request(format!("{protocol_name} tool choice requires a tool name")))
        }
        ToolChoice::Named { name, .. } => Ok(json!({ "type": "function", "name": name })),
    }
}

/// Server-issued item ids need a nonempty prefix and suffix (port of
/// `itemID`): `prefix_suffix` with a real `_` separator.
fn item_id(provider_metadata: Option<&ProviderMetadata>, key: &str) -> Option<String> {
    let text = provider_metadata?.get(key)?.get("itemId")?.as_str()?;
    let separator = text.find('_')?;
    (separator > 0 && separator < text.len() - 1).then(|| text.to_string())
}

/// The provider metadata key for a request's model route (port of
/// `metadataKey`).
#[must_use]
pub fn metadata_key(request: &LLMRequest) -> String {
    request
        .model
        .route
        .provider_metadata_key
        .clone()
        .unwrap_or_else(|| DEFAULT_PROVIDER_METADATA_KEY.to_string())
}

/// Where a message's parts sit in the item stream (port of `MessageMetadata`):
/// `{itemId?, status?, phase?}`.
#[derive(Debug, Clone, Default)]
struct MessageMetadata {
    item_id: Option<String>,
    status: Option<String>,
    phase: PhaseField,
}

fn decode_message_metadata(raw: Option<&BTreeMap<String, Value>>) -> Option<MessageMetadata> {
    let object = raw?;
    Some(MessageMetadata {
        item_id: object.get("itemId").and_then(Value::as_str).map(str::to_string),
        status: object.get("status").and_then(Value::as_str).map(str::to_string),
        phase: message_phase(object.get("phase")),
    })
}

/// `part.providerMetadata` across every part variant.
fn part_provider_metadata(part: &ContentPart) -> Option<&ProviderMetadata> {
    match part {
        ContentPart::Text { provider_metadata, .. }
        | ContentPart::Media { provider_metadata, .. }
        | ContentPart::ToolCall { provider_metadata, .. }
        | ContentPart::ToolResult { provider_metadata, .. }
        | ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.as_ref(),
        ContentPart::Compaction { .. } | ContentPart::Effort { .. } => None,
    }
}

fn media_of(part: &ContentPart) -> Option<&Asset> {
    match part {
        ContentPart::Media { media, .. } => Some(media),
        _ => None,
    }
}

/// Lowering target for media parts (user message vs tool result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTarget {
    Message,
    ToolResult,
}

fn source_type(asset: &Asset) -> &'static str {
    match asset.source {
        crate::schema::media::Source::Bytes { .. }
        | crate::schema::media::Source::Base64 { .. } => "inline",
        crate::schema::media::Source::Url { .. } => "url",
        crate::schema::media::Source::Ref { .. } => "ref",
    }
}

/// Baseline media lowering (port of `lowerMedia`).
///
/// `target` mirrors the TS distinction: tool-result videos lower to
/// `input_video` while user-message videos are rejected by the caller.
pub fn lower_media(
    part: &ContentPart,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
    target: MediaTarget,
) -> Result<Value, AiError> {
    let media = media_of(part)
        .ok_or_else(|| invalid_request(format!("{} expected a media part", adapter.name())))?;
    if let Some(provider_media) = adapter.lower_media(part, media, request) {
        return Ok(provider_media);
    }
    let key = metadata_key(request);
    let detail = match part_provider_metadata(part)
        .and_then(|metadata| metadata.get(&key))
        .and_then(|metadata| metadata.get("detail"))
    {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| invalid_request("Expected a string, but got a non-string detail"))?
                .to_string(),
        ),
    };
    let mime = media.media_type.to_lowercase();
    let url = media.url().map(str::to_string);
    let location = match url.clone() {
        Some(url) => url,
        None => {
            media
                .inline()
                .ok_or_else(|| {
                    invalid_request(format!(
                "{} requires inline media (bytes or base64); {} sources must be materialized first",
                adapter.name(),
                source_type(media)
            ))
                })?
                .data_url
        }
    };
    let filename = match part {
        ContentPart::Media { filename, .. } => filename.clone(),
        _ => None,
    };
    if media.kind() != media::Kind::Image {
        if target == MediaTarget::ToolResult && media.kind() == media::Kind::Video {
            return Ok(
                serde_json::to_value(InputVideo { video_url: location }).unwrap_or(Value::Null)
            );
        }
        let file = InputFile {
            filename: filename
                .or_else(|| (mime == "application/pdf").then(|| "document.pdf".to_string()))
                .unwrap_or_else(|| "file".to_string()),
            detail,
            file_data: url.is_none().then_some(location.clone()),
            file_url: url,
        };
        return Ok(serde_json::to_value(file).unwrap_or(Value::Null));
    }
    Ok(serde_json::to_value(InputImage { image_url: location, detail }).unwrap_or(Value::Null))
}

fn lower_message_media(
    part: &ContentPart,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Value, AiError> {
    let lowered = lower_media(part, request, adapter, MediaTarget::Message)?;
    if lowered.get("type").and_then(Value::as_str) == Some("input_video") {
        return Err(invalid_request(format!(
            "{} user messages do not support input_video",
            adapter.name()
        )));
    }
    Ok(lowered)
}

fn unsupported_content(adapter_name: &str, role: &str, types: &[&str]) -> AiError {
    let listed = match types.len() {
        0 => String::new(),
        1 => types[0].to_string(),
        2 => format!("{} and {}", types[0], types[1]),
        _ => format!("{}, and {}", types[..types.len() - 1].join(", "), types[types.len() - 1]),
    };
    invalid_request(format!("{adapter_name} {role} messages only support {listed} content for now"))
}

fn lower_user_content(
    part: &ContentPart,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Value, AiError> {
    match part {
        ContentPart::Text { text, .. } => {
            Ok(serde_json::to_value(InputText { text: text.clone() }).unwrap_or(Value::Null))
        }
        ContentPart::Media { .. } => lower_message_media(part, request, adapter),
        _ => Err(unsupported_content(adapter.name(), "user", &["text", "media"])),
    }
}

/// Lift a tool-result file content item into a media part (port of
/// `toolFileMedia`).
fn tool_file_media(item: &Value) -> Option<ContentPart> {
    let uri = item.get("uri")?.as_str()?;
    let mime = item.get("mime").and_then(Value::as_str).unwrap_or("application/octet-stream");
    let mut asset = media::parse_data_url(uri).unwrap_or_else(|| {
        if uri.starts_with("http://") || uri.starts_with("https://") {
            media::url(uri, Some(mime.to_string()), None)
        } else {
            media::base64(uri, mime)
        }
    });
    // The declared mime wins over any data-URL prefix.
    asset.media_type = mime.to_string();
    Some(ContentPart::Media {
        media: asset,
        filename: item.get("name").and_then(Value::as_str).map(str::to_string),
        cache: None,
        metadata: None,
        provider_metadata: None,
    })
}

/// JS `String(value)` for the JSON values tool results carry.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        other => other.to_string(),
    }
}

/// Text form of a tool result (port of `toolResultText`).
#[must_use]
pub fn tool_result_text(part: &ContentPart) -> String {
    let ContentPart::ToolResult { result, .. } = part else {
        return String::new();
    };
    match result {
        crate::schema::ToolResultValue::Text { value } => js_string(value),
        crate::schema::ToolResultValue::Error { value } => {
            let structured = value.is_array() || value.as_object().is_some();
            if structured {
                encode_json(value)
            } else {
                js_string(value)
            }
        }
        crate::schema::ToolResultValue::Json { value } => encode_json(value),
        crate::schema::ToolResultValue::Content { .. } => String::new(),
    }
}

fn input_text_value(text: &str) -> Value {
    serde_json::to_value(InputText { text: text.to_string() }).unwrap_or(Value::Null)
}

fn lower_tool_result_content_item(
    item: &Value,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Value, AiError> {
    if item.get("type").and_then(Value::as_str) == Some("text") {
        return Ok(input_text_value(item.get("text").and_then(Value::as_str).unwrap_or_default()));
    }
    let media = tool_file_media(item)
        .ok_or_else(|| invalid_request("tool result file items require uri and mime"))?;
    lower_media(&media, request, adapter, MediaTarget::ToolResult)
}

fn lower_tool_result_output(
    part: &ContentPart,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Value, AiError> {
    let ContentPart::ToolResult { result, .. } = part else {
        return Err(unsupported_content(adapter.name(), "tool", &["tool-result"]));
    };
    match result {
        // Text/json/error results encode as a plain string for backward
        // compatibility with existing cassettes and provider expectations.
        crate::schema::ToolResultValue::Content { value } => {
            let mut items = Vec::new();
            for item in &value.0 {
                items.push(lower_tool_result_content_item(item, request, adapter)?);
            }
            Ok(Value::Array(items))
        }
        _ => Ok(Value::String(tool_result_text(part))),
    }
}

const DEFAULT_EFFORT: &str = "medium";

/// Hosted tool item types accepted by the baseline replay check (port of
/// `HostedToolItem`): the raw JSON is replayed verbatim.
pub const HOSTED_TOOL_ITEM_TYPES: [&str; 4] =
    ["web_search_call", "file_search_call", "code_interpreter_call", "mcp_call"];

/// Whether a JSON value is a replayable hosted tool item (light validation of
/// the TS `Schema.is(HostedToolItem)` guard: type + string id).
#[must_use]
pub fn is_hosted_tool_item(value: &Value) -> bool {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    HOSTED_TOOL_ITEM_TYPES.contains(&kind)
        && value.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
}

/// Draft input entries: reasoning items stay mutable until the assistant
/// message is fully folded (later parts append summaries to earlier items).
enum InputDraft {
    Item(InputItem),
    ReasoningRef(String),
}

/// Lower the message history into `input` items (port of `lowerMessages`).
pub fn lower_messages(
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Vec<Value>, AiError> {
    let key = metadata_key(request);
    let mut input: Vec<InputDraft> = Vec::new();
    // Reasoning items by id, mutated by later parts before materialization.
    let mut reasoning_items: BTreeMap<String, InputItem> = BTreeMap::new();

    for message in &request.messages {
        let metadata = decode_message_metadata(
            message
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get(&key))
                .map(|value| value as &BTreeMap<String, Value>),
        );
        match message.role {
            MessageRole::System => {
                if let Some(update) = crate::effort_updates::effort_update(message) {
                    // Consecutive updates are rejected, so a newer one
                    // replaces its predecessor.
                    if matches!(input.last(), Some(InputDraft::Item(item)) if item.kind() == "configuration_update")
                    {
                        input.pop();
                    }
                    input.push(InputDraft::Item(InputItem::ConfigurationUpdate {
                        effort: update.effort.unwrap_or_else(|| DEFAULT_EFFORT.to_string()),
                    }));
                    continue;
                }
                let mut parts = Vec::new();
                for part in &message.content {
                    let ContentPart::Text { text, .. } = part else {
                        return Err(unsupported_content(adapter.name(), "system", &["text"]));
                    };
                    parts.push(text.clone());
                }
                input.push(InputDraft::Item(InputItem::MessageText {
                    role: "developer",
                    content: join_text(parts.iter().map(String::as_str)),
                }));
            }
            MessageRole::User => {
                let mut content = Vec::new();
                for part in &message.content {
                    content.push(lower_user_content(part, request, adapter)?);
                }
                if !content.is_empty() {
                    input.push(InputDraft::Item(InputItem::Message {
                        id: metadata.as_ref().and_then(|m| m.item_id.clone()),
                        role: "user",
                        status: metadata.as_ref().and_then(|m| m.status.clone()),
                        phase: metadata.as_ref().and_then(|m| m.phase),
                        content,
                    }));
                }
            }
            MessageRole::Assistant => {
                lower_assistant_message(
                    message,
                    request,
                    adapter,
                    &key,
                    metadata.as_ref(),
                    &mut input,
                    &mut reasoning_items,
                )?;
            }
            MessageRole::Tool => {
                for part in &message.content {
                    let ContentPart::ToolResult { id, .. } = part else {
                        return Err(unsupported_content(adapter.name(), "tool", &["tool-result"]));
                    };
                    input.push(InputDraft::Item(InputItem::FunctionCallOutput {
                        call_id: id.clone(),
                        output: lower_tool_result_output(part, request, adapter)?,
                    }));
                }
            }
        }
    }

    Ok(input
        .into_iter()
        .map(|draft| match draft {
            InputDraft::Item(item) => item.to_json(),
            InputDraft::ReasoningRef(id) => {
                reasoning_items.get(&id).map_or(Value::Null, InputItem::to_json)
            }
        })
        .collect())
}

/// The assistant replay: text grouping, reasoning folding, compaction
/// checkpoints, tool calls, and hosted-tool results (port of the assistant
/// branch of `lowerMessages`).
fn lower_assistant_message(
    message: &Message,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
    key: &str,
    metadata: Option<&MessageMetadata>,
    input: &mut Vec<InputDraft>,
    reasoning_items: &mut BTreeMap<String, InputItem>,
) -> Result<(), AiError> {
    fn flush_text(
        text_parts: &mut Vec<(&str, Option<ProviderMetadata>)>,
        metadata: Option<&MessageMetadata>,
        key: &str,
        input: &mut Vec<InputDraft>,
    ) {
        if text_parts.is_empty() {
            return;
        }
        // Group consecutive parts by (item id, phase).
        let mut groups: Vec<(Option<String>, PhaseField, Vec<String>)> = Vec::new();
        for (text, provider_metadata) in text_parts.iter() {
            let part_metadata = provider_metadata.as_ref().and_then(|m| m.get(key));
            let id = item_id(provider_metadata.as_ref(), key)
                .or_else(|| metadata.and_then(|m| m.item_id.clone()));
            let phase = message_phase(part_metadata.and_then(|value| value.get("phase")))
                .or_else(|| metadata.and_then(|m| m.phase));
            match groups.last_mut() {
                Some(group) if group.0 == id && group.1 == phase => group.2.push(text.to_string()),
                _ => groups.push((id, phase, vec![text.to_string()])),
            }
        }
        text_parts.clear();
        for (id, phase, parts) in groups {
            input.push(InputDraft::Item(InputItem::Message {
                id,
                role: "assistant",
                // Replayed text is a finished input item, even if generation
                // was cut short.
                status: Some("completed".to_string()),
                phase,
                content: parts
                    .into_iter()
                    .map(|text| serde_json::to_value(OutputText { text }).unwrap_or(Value::Null))
                    .collect(),
            }));
        }
    }

    let mut text_parts: Vec<(&str, Option<ProviderMetadata>)> = Vec::new();
    let mut hosted_tool_items: BTreeSet<String> = BTreeSet::new();

    for part in &message.content {
        match part {
            ContentPart::Compaction { provider, id, encrypted, .. } => {
                flush_text(&mut text_parts, metadata, key, input);
                if *provider != request.model.provider || encrypted.is_none() {
                    return Err(invalid_request(
                        "Compaction state must be replayed to its originating provider and API",
                    ));
                }
                input.push(InputDraft::Item(InputItem::Compaction {
                    id: id.clone(),
                    encrypted_content: encrypted.clone().unwrap_or_default(),
                }));
            }
            ContentPart::Text { text, provider_metadata, .. } => {
                text_parts.push((text, provider_metadata.clone()));
            }
            ContentPart::Reasoning { text, provider_metadata, .. } => {
                flush_text(&mut text_parts, metadata, key, input);
                let Some(record) = provider_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get(key))
                    .filter(|record| !record.is_empty())
                else {
                    continue;
                };
                let id = item_id(provider_metadata.as_ref(), key);
                let encrypted_content = match record.get("reasoningEncryptedContent") {
                    Some(Value::String(text)) => Some(Some(text.clone())),
                    Some(Value::Null) => Some(None),
                    _ => None,
                };
                let summary = if text.is_empty() {
                    Vec::new()
                } else {
                    vec![SummaryText { text: text.clone() }]
                };
                if let Some(existing) = id.clone().and_then(|id| reasoning_items.get_mut(&id)) {
                    if let InputItem::Reasoning {
                        summary: existing_summary,
                        encrypted_content: existing_encrypted,
                        ..
                    } = existing
                    {
                        existing_summary.extend(summary);
                        if matches!(encrypted_content, Some(Some(_))) {
                            *existing_encrypted = encrypted_content;
                        }
                    }
                } else {
                    let draft = InputItem::Reasoning { id: id.clone(), summary, encrypted_content };
                    if let Some(id) = id {
                        reasoning_items.insert(id.clone(), draft.clone());
                        input.push(InputDraft::ReasoningRef(id));
                    } else {
                        input.push(InputDraft::Item(draft));
                    }
                }
            }
            ContentPart::ToolCall {
                id,
                name,
                namespace,
                input: tool_input,
                provider_executed,
                provider_metadata,
                ..
            } => {
                flush_text(&mut text_parts, metadata, key, input);
                if *provider_executed == Some(true) {
                    continue;
                }
                input.push(InputDraft::Item(InputItem::FunctionCall {
                    id: item_id(provider_metadata.as_ref(), key),
                    call_id: id.clone(),
                    name: name.clone(),
                    namespace: namespace.clone(),
                    arguments: encode_json(tool_input),
                }));
            }
            ContentPart::ToolResult {
                provider_executed: Some(true),
                provider_metadata,
                result,
                ..
            } => {
                flush_text(&mut text_parts, metadata, key, input);
                let item = item_id(provider_metadata.as_ref(), key);
                let hosted = match result {
                    crate::schema::ToolResultValue::Json { value } => {
                        if is_hosted_tool_item(value) {
                            Some(value.clone())
                        } else {
                            adapter.restore_hosted_tool_item(value)
                        }
                    }
                    _ => None,
                };
                if let (Some(item), Some(hosted)) = (&item, &hosted) {
                    if hosted.get("id").and_then(Value::as_str) == Some(item.as_str()) {
                        if hosted_tool_items.insert(item.clone()) {
                            input.push(InputDraft::Item(InputItem::Passthrough(hosted.clone())));
                        }
                        continue;
                    }
                }
                let content: Vec<Value> = match result {
                    crate::schema::ToolResultValue::Content { value } => value.0.clone(),
                    _ => vec![json!({ "type": "text", "text": tool_result_text(part) })],
                };
                let mut lowered = Vec::new();
                for content_item in &content {
                    if content_item.get("type").and_then(Value::as_str) == Some("text") {
                        lowered.push(input_text_value(
                            content_item.get("text").and_then(Value::as_str).unwrap_or_default(),
                        ));
                    } else {
                        let media = tool_file_media(content_item).ok_or_else(|| {
                            invalid_request("tool result file items require uri and mime")
                        })?;
                        lowered.push(lower_message_media(&media, request, adapter)?);
                    }
                }
                input.push(InputDraft::Item(InputItem::Message {
                    id: None,
                    role: "user",
                    status: None,
                    phase: None,
                    content: lowered,
                }));
            }
            ContentPart::Media { .. } => {
                flush_text(&mut text_parts, metadata, key, input);
                // Responses has no assistant-authored image item; replay
                // generated media (e.g. from Gemini) as user input.
                input.push(InputDraft::Item(InputItem::Message {
                    id: None,
                    role: "user",
                    status: None,
                    phase: None,
                    content: vec![lower_message_media(part, request, adapter)?],
                }));
            }
            _ => {
                return Err(unsupported_content(
                    adapter.name(),
                    "assistant",
                    &["text", "reasoning", "tool-call", "tool-result", "media"],
                ));
            }
        }
    }
    flush_text(&mut text_parts, metadata, key, input);
    Ok(())
}

/// `{model, input, instructions?}` (port of `lowerConversation`).
pub fn lower_conversation(
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Map<String, Value>, AiError> {
    let instructions = join_text(request.system.iter().map(|part| part.text.as_str()));
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model.id.as_str()));
    body.insert("input".into(), Value::Array(lower_messages(request, adapter)?));
    if !instructions.is_empty() {
        body.insert("instructions".into(), json!(instructions));
    }
    Ok(body)
}

/// Generation + resolved provider options (port of `lowerGeneration`).
#[must_use]
pub fn lower_generation(request: &LLMRequest, options: &Resolved) -> Map<String, Value> {
    let generation = request.generation.as_ref();
    let cache_key = prompt_cache_key(request);
    let parallel_tool_calls = resolve_parallel_tool_calls(request);
    let mut body = Map::new();
    body.insert("stream".into(), json!(true));
    if let Some(value) = generation.and_then(|g| g.max_tokens) {
        body.insert("max_output_tokens".into(), number(value));
    }
    if let Some(value) = generation.and_then(|g| g.temperature) {
        body.insert("temperature".into(), number(value));
    }
    if let Some(value) = generation.and_then(|g| g.top_p) {
        body.insert("top_p".into(), number(value));
    }
    if let Some(value) = generation.and_then(|g| g.presence_penalty) {
        body.insert("presence_penalty".into(), number(value));
    }
    if let Some(value) = generation.and_then(|g| g.frequency_penalty) {
        body.insert("frequency_penalty".into(), number(value));
    }
    if let Some(store) = options.store {
        body.insert("store".into(), json!(store));
    }
    if let Some(metadata) = options.metadata.clone() {
        body.insert(
            "metadata".into(),
            Value::Object(metadata.into_iter().map(|(k, v)| (k, json!(v))).collect()),
        );
    }
    if let Some(safety_identifier) = options.safety_identifier.clone() {
        body.insert("safety_identifier".into(), json!(safety_identifier));
    }
    if let Some(include_obfuscation) = options
        .stream_options
        .as_ref()
        .and_then(|stream_options| stream_options.include_obfuscation)
    {
        body.insert("stream_options".into(), json!({ "include_obfuscation": include_obfuscation }));
    }
    if let Some(top_logprobs) = options.top_logprobs {
        body.insert("top_logprobs".into(), json!(top_logprobs));
    }
    if let Some(cache_key) = cache_key {
        body.insert("prompt_cache_key".into(), json!(cache_key));
    }
    if let Some(include) = options.include.clone() {
        body.insert(
            "include".into(),
            Value::Array(include.into_iter().map(|item| json!(item)).collect()),
        );
    }
    if options.reasoning_effort.is_some() || options.reasoning_summary.is_some() {
        let mut reasoning = Map::new();
        if let Some(effort) = options.reasoning_effort.clone() {
            reasoning.insert("effort".into(), json!(effort));
        }
        if let Some(summary) = options.reasoning_summary.clone() {
            reasoning.insert("summary".into(), json!(summary));
        }
        body.insert("reasoning".into(), Value::Object(reasoning));
    }
    if let Some(text_verbosity) = options.text_verbosity.clone() {
        body.insert("text".into(), json!({ "verbosity": text_verbosity }));
    }
    if let Some(service_tier) = options.service_tier.clone() {
        body.insert("service_tier".into(), json!(service_tier));
    }
    if let Some(max_tool_calls) = options.max_tool_calls {
        body.insert("max_tool_calls".into(), json!(max_tool_calls));
    }
    if let Some(parallel_tool_calls) = parallel_tool_calls {
        body.insert("parallel_tool_calls".into(), json!(parallel_tool_calls));
    }
    if let Some(truncation) = options.truncation.clone() {
        body.insert("truncation".into(), json!(truncation));
    }
    body
}

/// `lowerGeneration` with the request's own resolved options.
#[must_use]
pub fn lower_generation_for(request: &LLMRequest) -> Map<String, Value> {
    lower_generation(request, &options::resolve(request))
}

/// Provider option wins over the canonical tool-choice setting (port of
/// `resolveParallelToolCalls`).
#[must_use]
pub fn resolve_parallel_tool_calls(request: &LLMRequest) -> Option<bool> {
    if let Some(configured) = options::resolve(request).parallel_tool_calls {
        return Some(configured);
    }
    let disabled = match request.tool_choice.as_ref() {
        Some(
            ToolChoice::Auto { disable_parallel_tool_use }
            | ToolChoice::Required { disable_parallel_tool_use }
            | ToolChoice::Named { disable_parallel_tool_use, .. },
        ) => *disable_parallel_tool_use,
        Some(ToolChoice::None { .. }) | None => None,
    };
    disabled.map(|disabled| !disabled)
}

/// `allowed_tools` tool choice from provider options (port of
/// `allowedToolChoice`).
#[must_use]
pub fn allowed_tool_choice(request: &LLMRequest) -> Option<Value> {
    let allowed = options::resolve(request).allowed_tools?;
    Some(json!({
        "type": "allowed_tools",
        "mode": allowed.mode,
        "tools": allowed
            .tool_names
            .iter()
            .map(|name| json!({ "type": "function", "name": name }))
            .collect::<Vec<_>>(),
    }))
}

/// Flatten namespaces and namespace-qualified part names (port of
/// `flattenToolRequest`).
#[must_use]
pub fn flatten_tool_request(request: &LLMRequest) -> LLMRequest {
    let mut messages = Vec::with_capacity(request.messages.len());
    for message in &request.messages {
        let mut content = Vec::with_capacity(message.content.len());
        for part in &message.content {
            content.push(match part {
                ContentPart::ToolCall { namespace: Some(namespace), name, .. } => {
                    let joined = format!("{namespace}_{name}");
                    let mut cloned = part.clone();
                    if let ContentPart::ToolCall { name: cloned_name, namespace, .. } = &mut cloned
                    {
                        *cloned_name = joined;
                        *namespace = None;
                    }
                    cloned
                }
                ContentPart::ToolResult { namespace: Some(namespace), name, .. } => {
                    let joined = format!("{namespace}_{name}");
                    let mut cloned = part.clone();
                    if let ContentPart::ToolResult { name: cloned_name, namespace, .. } =
                        &mut cloned
                    {
                        *cloned_name = joined;
                        *namespace = None;
                    }
                    cloned
                }
                other => other.clone(),
            });
        }
        messages.push(Message {
            id: message.id.clone(),
            role: message.role,
            content,
            metadata: message.metadata.clone(),
            provider_metadata: message.provider_metadata.clone(),
            native: message.native.clone(),
        });
    }
    request
        .clone()
        .update(crate::schema::LLMRequestPatch { messages: Some(messages), ..Default::default() })
}

/// Build the provider-native body (port of `fromRequestWithAdapter`).
pub fn from_request_with_adapter(
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
) -> Result<Value, AiError> {
    let projected = flatten_tool_request(request);
    let mut body = lower_conversation(&projected, adapter)?;
    for (key, value) in lower_generation_for(request) {
        body.insert(key, value);
    }
    let tools = flatten_tools(&request.tools);
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(lower_tools(&tools, adapter)?));
    }
    let tool_choice = allowed_tool_choice(request).or(match request.tool_choice.as_ref() {
        Some(choice) => Some(lower_tool_choice(adapter.name(), choice)?),
        None => None,
    });
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".into(), tool_choice);
    }
    Ok(Value::Object(body))
}

/// Build the body with the baseline adapter (port of `fromRequest`).
pub fn from_request(request: &LLMRequest) -> Result<Value, AiError> {
    from_request_with_adapter(request, &BaseAdapter)
}

// =============================================================================
// Stream parsing
// =============================================================================

/// A decoded output item (port of `StreamItem`): typed accessors over the
/// raw JSON, with the full value preserved for hosted replay and error
/// encoding.
#[derive(Debug, Clone)]
pub struct StreamItem {
    pub raw: Value,
    pub kind: String,
    pub id: Option<String>,
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub arguments: Option<String>,
    pub encrypted_content: Option<String>,
}

impl StreamItem {
    fn field(&self, name: &str) -> Option<&Value> {
        self.raw.get(name)
    }

    pub fn content_array(&self) -> Vec<&Value> {
        self.field("content")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .collect()
    }

    pub fn summary_array(&self) -> Vec<&Value> {
        self.field("summary")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .collect()
    }

    #[must_use]
    pub fn phase(&self) -> PhaseField {
        message_phase(self.field("phase"))
    }
}

/// A decoded stream frame (port of `Event`): known fields as typed accessors,
/// the full frame preserved in [`Event::raw`].
#[derive(Debug, Clone)]
pub struct Event {
    pub raw: Value,
    pub kind: String,
}

impl Event {
    pub fn delta(&self) -> Option<&str> {
        self.raw.get("delta").and_then(Value::as_str)
    }
    pub fn arguments(&self) -> Option<&str> {
        self.raw.get("arguments").and_then(Value::as_str)
    }
    pub fn text(&self) -> Option<&str> {
        self.raw.get("text").and_then(Value::as_str)
    }
    /// `refusal` reaches the parser through the schema's rest passthrough.
    pub fn refusal(&self) -> Option<&str> {
        self.raw.get("refusal").and_then(Value::as_str)
    }
    pub fn item_id(&self) -> Option<String> {
        self.raw.get("item_id").and_then(Value::as_str).map(str::to_string)
    }
    pub fn output_index(&self) -> Option<u64> {
        self.raw.get("output_index").and_then(Value::as_u64)
    }
    pub fn summary_index(&self) -> Option<u64> {
        self.raw.get("summary_index").and_then(Value::as_u64)
    }
    #[must_use]
    pub fn item(&self) -> Option<StreamItem> {
        let item = self.raw.get("item")?;
        if item.is_null() {
            return None;
        }
        decode_stream_item(item)
    }
    #[must_use]
    pub fn response(&self) -> Option<&Value> {
        self.raw.get("response").filter(|value| !value.is_null())
    }
    fn response_field(&self, name: &str) -> Option<&Value> {
        self.response()?.get(name)
    }
    pub fn response_id(&self) -> Option<&str> {
        self.response_field("id").and_then(Value::as_str)
    }
    #[must_use]
    pub fn response_service_tier(&self) -> Option<&Value> {
        self.response()?.get("service_tier")
    }
    pub fn response_output(&self) -> Vec<Value> {
        self.response_field("output").and_then(Value::as_array).cloned().unwrap_or_default()
    }
    #[must_use]
    pub fn response_usage(&self) -> Option<&Value> {
        self.response()?.get("usage").filter(|value| !value.is_null())
    }
    pub fn incomplete_reason(&self) -> Option<String> {
        self.response()?
            .get("incomplete_details")
            .filter(|value| !value.is_null())?
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string)
    }
}

fn decode_stream_item(item: &Value) -> Option<StreamItem> {
    if !item.is_object() {
        return None;
    }
    let str_field = |name: &str| item.get(name).and_then(Value::as_str).map(str::to_string);
    Some(StreamItem {
        raw: item.clone(),
        kind: str_field("type")?,
        id: str_field("id"),
        call_id: str_field("call_id"),
        name: str_field("name"),
        namespace: str_field("namespace"),
        arguments: str_field("arguments"),
        encrypted_content: str_field("encrypted_content"),
    })
}

/// Decode one SSE frame (port of `Protocol.jsonEvent(Event)`): the frame must
/// be a JSON object carrying a string `type`.
pub fn decode_event(frame: &str) -> Result<Event, AiError> {
    let raw = shared::decode_json(frame).map_err(|cause| {
        AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields {
                message: format!("Failed to decode {ADAPTER} event"),
                body: Some(frame.to_string()),
                cause: Some(cause),
                http: None,
            },
            classification: None,
            route: Some(ADAPTER.to_string()),
        })
    })?;
    if !raw.is_object() {
        return Err(event_error(ADAPTER, format!("Failed to decode {ADAPTER} event")));
    }
    let kind = raw
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| event_error(ADAPTER, format!("Failed to decode {ADAPTER} event")))?
        .to_string();
    Ok(Event { raw, kind })
}

/// Per-summary-part lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryStatus {
    Active,
    CanConclude,
    Concluded,
}

/// One streamed reasoning item's parser bookkeeping.
#[derive(Debug, Clone)]
pub struct ReasoningStreamItem {
    pub encrypted_content: Option<String>,
    /// Keyed by the wire `summary_index` (numeric ordering matters).
    pub summary_parts: BTreeMap<u64, SummaryStatus>,
    pub delta_indexes: BTreeSet<u64>,
}

/// The currently-open assistant message.
#[derive(Debug, Clone)]
pub struct ActiveMessage {
    pub id: String,
    pub phase: PhaseField,
}

/// Parser state (port of `ParserState`).
#[derive(Debug, Clone)]
pub struct ParserState {
    pub provider: ProviderId,
    pub completed_compactions: BTreeSet<String>,
    pub id: String,
    pub name: String,
    pub provider_metadata_key: String,
    pub tools: tool_stream::State,
    pub has_function_call: bool,
    pub lifecycle: lifecycle::State,
    pub output_items: BTreeMap<u64, String>,
    pub message: Option<ActiveMessage>,
    pub reasoning_items: BTreeMap<String, ReasoningStreamItem>,
    /// Mirror of `Lifecycle.State.text` (private upstream): the open text
    /// block ids in insertion order, used to close earlier messages.
    pub open_text: Vec<String>,
    /// Counter for locally minted item ids (deterministic; TS mints UUIDs).
    pub minted: u64,
}

impl ParserState {
    #[must_use]
    pub fn new(request: &LLMRequest, adapter: &dyn ProviderAdapter) -> Self {
        Self {
            provider: request.model.provider.clone(),
            completed_compactions: BTreeSet::new(),
            id: adapter.id().to_string(),
            name: adapter.name().to_string(),
            provider_metadata_key: metadata_key(request),
            tools: tool_stream::State::new(),
            has_function_call: false,
            lifecycle: lifecycle::State::initial(),
            output_items: BTreeMap::new(),
            message: None,
            reasoning_items: BTreeMap::new(),
            open_text: Vec::new(),
            minted: 0,
        }
    }

    /// `Lifecycle.State.stepStarted` (private upstream) observed by probing:
    /// `step_start` emits exactly when no step has started yet.
    fn step_started(&self) -> bool {
        let mut probe = Vec::new();
        lifecycle::step_start(&self.lifecycle, &mut probe);
        probe.is_empty()
    }

    fn open_text_has(&self, id: &str) -> bool {
        self.open_text.iter().any(|open| open == id)
    }

    fn open_text_insert(&mut self, id: &str) {
        if !self.open_text_has(id) {
            self.open_text.push(id.to_string());
        }
    }

    fn open_text_remove(&mut self, id: &str) {
        self.open_text.retain(|open| open != id);
    }

    /// `Lifecycle.State.reasoning.size > 0`, derived from the summary-part
    /// statuses: a part that is not `concluded` has an open lifecycle block.
    fn open_reasoning_blocks(&self) -> bool {
        self.reasoning_items.values().any(|item| {
            item.summary_parts.values().any(|status| *status != SummaryStatus::Concluded)
        })
    }
}

/// Minted-id prefixes per item type (port of `ITEM_ID_PREFIX`).
fn item_id_prefix(kind: &str) -> &'static str {
    match kind {
        "message" => "msg",
        "reasoning" => "rs",
        "function_call" => "fc",
        "compaction" => "cmp",
        _ => "item",
    }
}

/// An item without an id adopts the id already open in its output slot,
/// otherwise a locally minted one (port of `resolveItem`). Minted ids are
/// deterministic 32-char hex shapes standing in for the TS UUID mint; each
/// mint advances the parser's counter.
fn resolve_item(state: &mut ParserState, item: &StreamItem, index: Option<u64>) -> StreamItem {
    let mut resolved = item.clone();
    let id =
        item.id.clone().or_else(|| index.and_then(|index| state.output_items.get(&index).cloned()));
    resolved.id = Some(if let Some(id) = id {
        id
    } else {
        state.minted += 1;
        format!("{}_{}{:012x}", item_id_prefix(&item.kind), "0".repeat(20), state.minted)
    });
    resolved
}

/// The normalized event: `item_id` routed through the registered output
/// slots, items resolved with stable ids (port of `normalize`). Minting
/// mutates the parser counter, exactly where TS draws a fresh UUID.
pub fn normalize(state: &mut ParserState, event: Event) -> NormalizedEvent {
    let item_id = event.item_id().map(|item_id| match event.output_index() {
        Some(index) => state.output_items.get(&index).cloned().unwrap_or(item_id),
        None => item_id,
    });
    let item = event.item().map(|item| resolve_item(state, &item, event.output_index()));
    NormalizedEvent { event, item_id, item }
}

/// The normalized event shape.
#[derive(Debug, Clone)]
pub struct NormalizedEvent {
    pub event: Event,
    pub item_id: Option<String>,
    pub item: Option<StreamItem>,
}

/// Responses APIs report inclusive totals; derive the non-cached breakdown
/// (port of `mapUsage`, subtract direction with `max(0, …)` clamps).
#[must_use]
pub fn map_usage(usage: Option<&Value>, provider_metadata_key: &str) -> Option<Usage> {
    let usage = usage?;
    let number = |value: Option<&Value>, field: &str| -> Option<f64> {
        value?.get(field).and_then(Value::as_f64)
    };
    let details = usage.get("input_tokens_details").filter(|value| !value.is_null());
    let cached = number(details, "cached_tokens");
    let cache_write = number(details, "cache_write_tokens");
    let output_details = usage.get("output_tokens_details").filter(|value| !value.is_null());
    let reasoning = number(output_details, "reasoning_tokens");
    let input_tokens = usage.get("input_tokens").and_then(Value::as_f64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_f64);
    let total = usage.get("total_tokens").and_then(Value::as_f64);
    let non_cached = subtract_tokens(input_tokens, sum_tokens(&[cached, cache_write]));
    Some(Usage {
        context_tokens: None,
        input_tokens,
        output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cached,
        cache_write_input_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens: total_tokens(input_tokens, output_tokens, total),
        provider_metadata: Some(
            std::iter::once((
                provider_metadata_key.to_string(),
                serde_json::from_value::<BTreeMap<String, Value>>(usage.clone())
                    .unwrap_or_default(),
            ))
            .collect(),
        ),
    })
}

fn map_finish_reason(event: &Event, has_function_call: bool) -> FinishReason {
    match event.incomplete_reason() {
        None => {
            if has_function_call {
                FinishReason::ToolCalls
            } else if event.kind == "response.incomplete" {
                FinishReason::Unknown
            } else {
                FinishReason::Stop
            }
        }
        Some(reason) if reason == "max_output_tokens" => FinishReason::Length,
        Some(reason) if reason == "content_filter" => FinishReason::ContentFilter,
        Some(_) if has_function_call => FinishReason::ToolCalls,
        Some(_) => FinishReason::Unknown,
    }
}

pub(crate) fn provider_metadata(state: &ParserState, metadata: Value) -> ProviderMetadata {
    std::iter::once((
        state.provider_metadata_key.clone(),
        serde_json::from_value::<BTreeMap<String, Value>>(metadata).unwrap_or_default(),
    ))
    .collect()
}

/// Types that end the stream; keep aligned with [`step`] (port of
/// `TERMINAL_TYPES`).
pub const TERMINAL_TYPES: [&str; 4] =
    ["error", "response.completed", "response.incomplete", "response.failed"];

/// Terminal frame predicate (port of `terminal`).
#[must_use]
pub fn terminal(event: &Event) -> bool {
    TERMINAL_TYPES.contains(&event.kind.as_str())
}

type StepResult = (ParserState, Vec<LLMEvent>);

fn no_events(state: &ParserState) -> StepResult {
    (state.clone(), Vec::new())
}

fn on_output_text_delta(state: &ParserState, event: &Event, id: &str) -> StepResult {
    // JS truthiness: an empty-string delta is skipped like a missing one.
    let Some(delta) = event.delta().filter(|delta| !delta.is_empty()) else {
        return no_events(state);
    };
    if state.message.as_ref().map(|message| message.id.as_str()) != Some(id) {
        return no_events(state);
    }
    let mut events = Vec::new();
    let phase = state.message.as_ref().and_then(|message| message.phase);
    let mut metadata = Map::new();
    metadata.insert("itemId".into(), json!(id));
    if let Some(phase) = phase_json(phase) {
        metadata.insert("phase".into(), phase);
    }
    let metadata = provider_metadata(state, Value::Object(metadata));
    let lifecycle = lifecycle::text_start(&state.lifecycle, &mut events, id, Some(metadata));
    let mut next = ParserState {
        lifecycle: lifecycle::text_delta(&lifecycle, &mut events, id, delta, None),
        ..state.clone()
    };
    next.open_text_insert(id);
    (next, events)
}

fn on_output_text_done(state: &ParserState, event: &Event, id: &str) -> StepResult {
    if state.message.as_ref().map(|message| message.id.as_str()) == Some(id) {
        if state.open_text_has(id) || event.text().is_none() {
            return no_events(state);
        }
        // The done final reconciles as a single delta (TS spreads
        // `{ ...event, delta: event.text }`).
        let mut rewritten = event.clone();
        if let Some(object) = rewritten.raw.as_object_mut() {
            object.insert("delta".into(), json!(event.text().unwrap_or_default()));
        }
        return on_output_text_delta(state, &rewritten, id);
    }
    let mut events = Vec::new();
    let lifecycle = lifecycle::text_end(&state.lifecycle, &mut events, id, None, None);
    let mut next = ParserState { lifecycle, ..state.clone() };
    next.open_text_remove(id);
    (next, events)
}

fn join_reasoning_text<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    let collected: Vec<&str> =
        parts.into_iter().filter_map(|part| part.filter(|part| !part.is_empty())).collect();
    (!collected.is_empty()).then(|| collected.join("\n\n"))
}

fn encrypted_metadata(item_id: &str, encrypted: Option<&str>) -> Value {
    json!({
        "itemId": item_id,
        "reasoningEncryptedContent": encrypted.map_or(Value::Null, Value::from),
    })
}

fn start_reasoning_summary_part(state: &ParserState, item_id: &str, index: u64) -> StepResult {
    let Some(item) = state.reasoning_items.get(item_id) else {
        return no_events(state);
    };
    if index == 0 || item.summary_parts.contains_key(&index) {
        return no_events(state);
    }

    let mut events = Vec::new();
    let mut lifecycle = state.lifecycle.clone();
    // Each later part start is an implicit boundary for the previous part.
    for (part_index, status) in &item.summary_parts {
        if *status == SummaryStatus::Concluded {
            continue;
        }
        lifecycle = lifecycle::reasoning_end(
            &lifecycle,
            &mut events,
            &format!("{item_id}:{part_index}"),
            Some(provider_metadata(state, json!({ "itemId": item_id }))),
            None,
        );
    }
    let metadata =
        provider_metadata(state, encrypted_metadata(item_id, item.encrypted_content.as_deref()));
    let lifecycle = lifecycle::reasoning_start(
        &lifecycle,
        &mut events,
        &format!("{item_id}:{index}"),
        Some(metadata),
    );
    let mut summary_parts = BTreeMap::new();
    for (part_index, status) in &item.summary_parts {
        summary_parts.insert(*part_index, SummaryStatus::Concluded);
        let _ = status;
    }
    summary_parts.insert(index, SummaryStatus::Active);
    let mut reasoning_items = state.reasoning_items.clone();
    reasoning_items.insert(
        item_id.to_string(),
        ReasoningStreamItem {
            encrypted_content: item.encrypted_content.clone(),
            summary_parts,
            delta_indexes: item.delta_indexes.clone(),
        },
    );
    (ParserState { lifecycle, reasoning_items, ..state.clone() }, events)
}

#[must_use]
pub fn on_reasoning_delta(state: &ParserState, event: &Event, item_id: &str) -> StepResult {
    // JS truthiness: an empty-string delta is skipped like a missing one.
    let Some(delta) = event.delta().filter(|delta| !delta.is_empty()) else {
        return no_events(state);
    };
    if !state.reasoning_items.contains_key(item_id) {
        return no_events(state);
    }
    let index = event.summary_index().unwrap_or(0);
    if state
        .reasoning_items
        .get(item_id)
        .is_some_and(|item| item.summary_parts.get(&index) == Some(&SummaryStatus::Concluded))
    {
        return no_events(state);
    }
    let (mut started, emitted) = start_reasoning_summary_part(state, item_id, index);
    let Some(current) = started.reasoning_items.get(item_id).cloned() else {
        return (started, emitted);
    };
    let mut events = emitted;
    let lifecycle = lifecycle::reasoning_delta(
        &started.lifecycle,
        &mut events,
        &format!("{item_id}:{index}"),
        delta,
        None,
    );
    let mut delta_indexes = current.delta_indexes.clone();
    delta_indexes.insert(index);
    started.reasoning_items.insert(
        item_id.to_string(),
        ReasoningStreamItem {
            encrypted_content: current.encrypted_content,
            summary_parts: current.summary_parts,
            delta_indexes,
        },
    );
    started.lifecycle = lifecycle;
    (started, events)
}

/// Some compatible gateways emit a reasoning final without streaming deltas;
/// reconcile the complete text as a single delta unless that summary index
/// already streamed one (port of `onReasoningDone`).
#[must_use]
pub fn on_reasoning_done(state: &ParserState, event: &Event, item_id: &str) -> StepResult {
    if event.text().is_none() {
        return no_events(state);
    }
    let index = event.summary_index().unwrap_or(0);
    if state.reasoning_items.get(item_id).is_some_and(|item| item.delta_indexes.contains(&index)) {
        return no_events(state);
    }
    let mut rewritten = event.clone();
    if let Some(object) = rewritten.raw.as_object_mut() {
        object.insert("delta".into(), json!(event.text().unwrap_or_default()));
    }
    on_reasoning_delta(state, &rewritten, item_id)
}

fn on_output_item_added(state: &ParserState, event: &NormalizedEvent) -> StepResult {
    let Some(item) = event.item.as_ref() else {
        return no_events(state);
    };
    let item_id = item.id.clone().unwrap_or_default();
    if item.kind == "message" {
        let phase = item.phase();
        // A new message closes earlier messages, including ones that never streamed.
        let mut events = Vec::new();
        let mut lifecycle = state.lifecycle.clone();
        let mut open_text = state.open_text.clone();
        for id in state.open_text.iter().filter(|id| id.as_str() != item_id).collect::<Vec<_>>() {
            let open_phase = if state.message.as_ref().map(|m| m.id.as_str()) == Some(id.as_str()) {
                state.message.as_ref().and_then(|message| message.phase)
            } else {
                None
            };
            let mut metadata = Map::new();
            metadata.insert("itemId".into(), json!(id));
            if let Some(phase) = phase_json(open_phase) {
                metadata.insert("phase".into(), phase);
            }
            lifecycle = lifecycle::text_end(
                &lifecycle,
                &mut events,
                id,
                Some(provider_metadata(state, Value::Object(metadata))),
                None,
            );
            open_text.retain(|open| open != id);
        }
        let message = ActiveMessage {
            id: item_id.clone(),
            phase: match phase {
                None if state.message.as_ref().map(|m| m.id.as_str()) == Some(item_id.as_str()) => {
                    state.message.as_ref().and_then(|message| message.phase)
                }
                other => other,
            },
        };
        return (
            ParserState { lifecycle, message: Some(message), open_text, ..state.clone() },
            events,
        );
    }
    if item.kind == "reasoning" {
        if state.reasoning_items.contains_key(&item_id) {
            return no_events(state);
        }
        let mut events = Vec::new();
        let metadata = provider_metadata(
            state,
            encrypted_metadata(&item_id, item.encrypted_content.as_deref()),
        );
        let lifecycle = lifecycle::reasoning_start(
            &state.lifecycle,
            &mut events,
            &format!("{item_id}:0"),
            Some(metadata),
        );
        let mut reasoning_items = state.reasoning_items.clone();
        reasoning_items.insert(
            item_id.clone(),
            ReasoningStreamItem {
                encrypted_content: item.encrypted_content.clone(),
                summary_parts: [(0, SummaryStatus::Active)].into_iter().collect(),
                delta_indexes: BTreeSet::new(),
            },
        );
        return (ParserState { lifecycle, reasoning_items, ..state.clone() }, events);
    }
    if item.kind != "function_call" || item.call_id.is_none() {
        return no_events(state);
    }
    if state.tools.get(&item_id).is_some() {
        return no_events(state);
    }
    let metadata = provider_metadata(state, json!({ "itemId": item_id }));
    let mut events = Vec::new();
    let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
    let tools = tool_stream::start(
        &state.tools,
        &item_id,
        PendingTool {
            id: item.call_id.clone().unwrap_or_default(),
            name: item.name.clone().unwrap_or_default(),
            namespace: item.namespace.clone(),
            input: item.arguments.clone().unwrap_or_default(),
            provider_executed: None,
            provider_metadata: Some(metadata.clone()),
        },
    );
    events.push(LLMEvent::ToolInputStart {
        id: item.call_id.clone().unwrap_or_default(),
        name: item.name.clone().unwrap_or_default(),
        namespace: item.namespace.clone(),
        provider_executed: None,
        provider_metadata: Some(metadata),
    });
    (ParserState { tools, lifecycle, ..state.clone() }, events)
}

fn on_reasoning_summary_part_added(state: &ParserState, event: &Event) -> StepResult {
    match (event.item_id(), event.summary_index()) {
        (Some(item_id), Some(index)) => start_reasoning_summary_part(state, &item_id, index),
        _ => no_events(state),
    }
}

fn on_reasoning_summary_part_done(state: &ParserState, event: &Event) -> StepResult {
    let (Some(item_id), Some(index)) = (event.item_id(), event.summary_index()) else {
        return no_events(state);
    };
    let Some(item) = state.reasoning_items.get(&item_id) else {
        return no_events(state);
    };
    if item.summary_parts.get(&index) != Some(&SummaryStatus::Active) {
        return no_events(state);
    }
    let mut reasoning_items = state.reasoning_items.clone();
    let mut updated = item.clone();
    updated.summary_parts.insert(index, SummaryStatus::CanConclude);
    reasoning_items.insert(item_id, updated);
    (ParserState { reasoning_items, ..state.clone() }, Vec::new())
}

fn on_function_call_arguments_delta(
    state: &ParserState,
    event: &Event,
) -> Result<StepResult, AiError> {
    let Some(item_id) = event.item_id() else {
        return Ok(no_events(state));
    };
    let Some(tool) = state.tools.get(&item_id).cloned() else {
        return Ok(no_events(state));
    };
    let final_arguments = (event.kind == "response.function_call_arguments.done")
        .then(|| event.arguments())
        .flatten();
    if event.kind == "response.function_call_arguments.done" && final_arguments.is_none() {
        return Ok(no_events(state));
    }
    if let Some(final_arguments) = final_arguments {
        if !final_arguments.starts_with(&tool.input) {
            let tools = tool_stream::start(
                &state.tools,
                &item_id,
                PendingTool { input: final_arguments.to_string(), ..tool },
            );
            return Ok((ParserState { tools, ..state.clone() }, Vec::new()));
        }
    }
    let delta = match final_arguments {
        Some(final_arguments) => &final_arguments[tool.input.len()..],
        None => event.delta().unwrap_or_default(),
    };
    if delta.is_empty() {
        return Ok(no_events(state));
    }
    let result = tool_stream::append_existing(
        &state.id,
        &state.tools,
        &item_id,
        delta,
        &format!("{} tool argument delta is missing its tool call", state.name),
    )?;
    let mut events = Vec::new();
    let lifecycle = if result.events.is_empty() {
        state.lifecycle.clone()
    } else {
        lifecycle::step_start(&state.lifecycle, &mut events)
    };
    events.extend(result.events);
    Ok((ParserState { lifecycle, tools: result.tools, ..state.clone() }, events))
}

fn on_output_item_done(state: &ParserState, item: &StreamItem) -> Result<StepResult, AiError> {
    let item_id = item.id.clone().unwrap_or_default();

    if item.kind == "compaction" {
        let Some(encrypted) = item.encrypted_content.clone() else {
            return Err(event_error(
                state.id.clone(),
                "Compaction output is missing its encrypted content",
            ));
        };
        if state.completed_compactions.contains(&item_id) {
            return Ok(no_events(state));
        }
        let mut events = Vec::new();
        let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        events.push(LLMEvent::Compaction {
            provider: state.provider.clone(),
            id: Some(item_id.clone()),
            encrypted: Some(encrypted),
            text: None,
        });
        let mut completed = state.completed_compactions.clone();
        completed.insert(item_id);
        return Ok((
            ParserState { lifecycle, completed_compactions: completed, ..state.clone() },
            events,
        ));
    }

    if item.kind == "message" {
        let active =
            state.message.as_ref().map(|message| message.id.as_str()) == Some(item_id.as_str());
        let item_phase = item.phase();
        let phase = if item_phase.is_none() && active {
            state.message.as_ref().and_then(|message| message.phase)
        } else {
            item_phase
        };
        let mut content = Vec::new();
        for part in item.content_array() {
            match part.get("type").and_then(Value::as_str) {
                Some("output_text") => {
                    content.push(part.get("text").and_then(Value::as_str).unwrap_or_default());
                }
                Some("refusal") => {
                    content.push(part.get("refusal").and_then(Value::as_str).unwrap_or_default());
                }
                _ => {}
            }
        }
        let text = (!content.is_empty()).then(|| content.concat());
        let mut metadata = Map::new();
        metadata.insert("itemId".into(), json!(item_id));
        if let Some(phase) = phase_json(phase) {
            metadata.insert("phase".into(), phase);
        }
        let metadata = Some(provider_metadata(state, Value::Object(metadata)));
        let mut events = Vec::new();
        let lifecycle = if text.as_deref().is_some_and(|text| !text.is_empty()) {
            lifecycle::text_start(&state.lifecycle, &mut events, &item_id, metadata.clone())
        } else {
            state.lifecycle.clone()
        };
        let lifecycle = lifecycle::text_end(&lifecycle, &mut events, &item_id, metadata, text);
        let mut next = ParserState {
            lifecycle,
            message: if active { None } else { state.message.clone() },
            ..state.clone()
        };
        next.open_text_remove(&item_id);
        return Ok((next, events));
    }

    if item.kind == "function_call" {
        let (Some(call_id), Some(name)) = (item.call_id.clone(), item.name.clone()) else {
            return Ok(no_events(state));
        };
        let metadata = provider_metadata(state, json!({ "itemId": item_id }));
        let registered = state.tools.get(&item_id).is_some();
        let tools = if registered {
            state.tools.clone()
        } else {
            tool_stream::start(
                &state.tools,
                &item_id,
                PendingTool {
                    id: call_id.clone(),
                    name: name.clone(),
                    namespace: item.namespace.clone(),
                    input: String::new(),
                    provider_executed: None,
                    provider_metadata: Some(metadata.clone()),
                },
            )
        };
        let result = match item.arguments.clone() {
            None => tool_stream::finish(&state.id, &tools, &item_id)?,
            Some(arguments) => {
                tool_stream::finish_with_input(&state.id, &tools, &item_id, &arguments)?
            }
        };
        let mut events = Vec::new();
        let finished = result.events.clone();
        // A done-only call never streamed a start event, so open its lifecycle here.
        let result_events: Vec<LLMEvent> = if registered || finished.is_empty() {
            finished
        } else {
            let mut prefixed = vec![LLMEvent::ToolInputStart {
                id: call_id.clone(),
                name: name.clone(),
                namespace: item.namespace.clone(),
                provider_executed: None,
                provider_metadata: Some(metadata.clone()),
            }];
            prefixed.extend(finished);
            prefixed
        };
        let lifecycle = if result_events.is_empty() {
            state.lifecycle.clone()
        } else {
            lifecycle::step_start(&state.lifecycle, &mut events)
        };
        events.extend(result_events);
        let has_function_call = state.has_function_call
            || events.iter().any(|event| {
                matches!(event, LLMEvent::ToolCall { .. } | LLMEvent::ToolInputError { .. })
            });
        return Ok((
            ParserState { lifecycle, has_function_call, tools: result.tools, ..state.clone() },
            events,
        ));
    }

    if item.kind == "reasoning" {
        let metadata = provider_metadata(
            state,
            encrypted_metadata(&item_id, item.encrypted_content.as_deref()),
        );
        let mut summary: Vec<Option<String>> = Vec::new();
        for part in item.summary_array() {
            // Keep missing entries so the array still matches the summary indexes.
            summary.push(
                (part.get("type").and_then(Value::as_str) == Some("summary_text"))
                    .then(|| part.get("text").and_then(Value::as_str))
                    .flatten()
                    .map(str::to_string),
            );
        }
        let mut content: Vec<String> = Vec::new();
        for part in item.content_array() {
            if part.get("type").and_then(Value::as_str) == Some("reasoning_text") {
                content
                    .push(part.get("text").and_then(Value::as_str).unwrap_or_default().to_string());
            }
        }
        let item_text = join_reasoning_text(summary.iter().map(|part| part.as_deref()))
            .or_else(|| join_reasoning_text(content.iter().map(|part| Some(part.as_str()))));
        let mut events = Vec::new();
        if let Some(reasoning_item) = state.reasoning_items.get(&item_id).cloned() {
            let fragments: Vec<(u64, SummaryStatus)> = reasoning_item
                .summary_parts
                .iter()
                .map(|(index, status)| (*index, *status))
                .collect();
            let mut lifecycle = state.lifecycle.clone();
            for (index, status) in fragments {
                if status == SummaryStatus::Concluded {
                    continue;
                }
                // Do not repeat earlier summaries that were already emitted
                // as separate fragments.
                let final_text = if reasoning_item.summary_parts.len() == 1 {
                    item_text.clone()
                } else {
                    summary.get(index as usize).cloned().flatten()
                };
                lifecycle = lifecycle::reasoning_end(
                    &lifecycle,
                    &mut events,
                    &format!("{item_id}:{index}"),
                    Some(metadata.clone()),
                    final_text.filter(|text| !text.is_empty()),
                );
            }
            let mut reasoning_items = state.reasoning_items.clone();
            reasoning_items.remove(&item_id);
            return Ok((ParserState { lifecycle, reasoning_items, ..state.clone() }, events));
        }
        let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        events.push(LLMEvent::ReasoningStart {
            id: item_id.clone(),
            provider_metadata: Some(metadata.clone()),
        });
        events.push(LLMEvent::ReasoningEnd {
            id: item_id,
            provider_metadata: Some(metadata),
            text: item_text,
        });
        return Ok((ParserState { lifecycle, ..state.clone() }, events));
    }

    Ok(no_events(state))
}

fn on_response_finish(state: &ParserState, event: &Event) -> Result<StepResult, AiError> {
    let mut current = state.clone();
    let mut events = Vec::new();
    if event.kind == "response.completed" {
        // An output item's array position is its output index.
        for (index, raw) in event.response_output().iter().enumerate() {
            let Some(decoded) = decode_stream_item(raw) else {
                continue;
            };
            let item = resolve_item(&mut current, &decoded, Some(index as u64));
            let item_id = item.id.clone().unwrap_or_default();
            if item.kind == "compaction"
                && current.step_started()
                && !current.completed_compactions.contains(&item_id)
            {
                return Err(event_error(
                    state.id.clone(),
                    "Cannot recover a compaction checkpoint after output has been emitted",
                ));
            }
            let recoverable = item.kind == "compaction"
                || (item.kind == "function_call" && current.tools.get(&item_id).is_some());
            if !recoverable {
                continue;
            }
            let (next, emitted) = on_output_item_done(&current, &item)?;
            current = next;
            events.extend(emitted);
        }
        // Some compatible providers omit output_item.done even after
        // completing the response.
        let pending = tool_stream::finish_all(&current.id, &current.tools)?;
        current.has_function_call = current.has_function_call
            || pending.events.iter().any(|event| {
                matches!(event, LLMEvent::ToolCall { .. } | LLMEvent::ToolInputError { .. })
            });
        current.tools = pending.tools;
        events.extend(pending.events);
    }
    let response_id = event.response_id();
    let service_tier = event.response_service_tier();
    let finish_metadata = if response_id.is_some_and(|id| !id.is_empty())
        || service_tier.is_some_and(|tier| !tier.is_null())
    {
        let mut metadata = Map::new();
        if let Some(response_id) = response_id {
            metadata.insert("responseId".into(), json!(response_id));
        }
        if let Some(tier) = service_tier {
            metadata.insert("serviceTier".into(), tier.clone());
        }
        Some(provider_metadata(&current, Value::Object(metadata)))
    } else {
        None
    };
    let lifecycle = lifecycle::finish(
        &current.lifecycle,
        &mut events,
        lifecycle::FinishInput {
            reason: FinishReasonDetails {
                normalized: map_finish_reason(event, current.has_function_call),
                raw: event.incomplete_reason(),
                category: None,
                explanation: None,
            },
            usage: map_usage(event.response_usage(), &current.provider_metadata_key),
            provider_metadata: finish_metadata,
        },
    );
    current.lifecycle = lifecycle;
    current.open_text.clear();
    Ok((current, events))
}

fn as_text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

/// Error code and message from wherever the frame put them (port of
/// `errorDetail`); top-level fields win over nested ones.
#[must_use]
pub fn error_detail(event: &Event) -> (Option<String>, Option<String>) {
    let raw = event
        .raw
        .get("error")
        .or_else(|| event.response().and_then(|response| response.get("error")));
    let nested = match raw {
        Some(Value::String(text)) => Some(json!({ "message": text })),
        Some(value) if value.as_object().is_some() => Some(value.clone()),
        _ => None,
    };
    let message = as_text(event.raw.get("message"))
        .or_else(|| nested.as_ref().and_then(|nested| as_text(nested.get("message"))));
    let code = as_text(event.raw.get("code"))
        .or_else(|| nested.as_ref().and_then(|nested| as_text(nested.get("code"))));
    (message, code)
}

/// Build the provider failure for `response.failed` / `error` frames (port of
/// `providerFailure`): prefix the code when both are present, fall back to
/// the raw frame, and keep bare errors internal.
pub fn provider_failure(event: &Event, fallback: &str, body: Option<&str>) -> AiError {
    let (message, code) = error_detail(event);
    let summary = match (&message, &code) {
        (Some(message), Some(code)) => Some(format!("{code}: {message}")),
        (Some(message), None) => Some(message.clone()),
        (None, Some(code)) => Some(code.clone()),
        (None, None) => None,
    };
    let body = body.map_or_else(|| encode_json(&event.raw), str::to_string);
    let message = summary.clone().unwrap_or_else(|| {
        if body == "{}" {
            fallback.to_string()
        } else {
            body.clone()
        }
    });
    let status = event
        .raw
        .get("status")
        .and_then(Value::as_f64)
        .and_then(|status| u16::try_from(status as i64).ok())
        .or_else(|| {
            event
                .raw
                .get("status_code")
                .and_then(Value::as_f64)
                .and_then(|status| u16::try_from(status as i64).ok())
        });
    if event.kind == "error"
        && !event.raw.as_object().is_some_and(|object| object.contains_key("error"))
        && event.response().is_none()
        && summary.is_none()
        && status.is_none()
    {
        return AiError::new(AiErrorReason::ProviderInternal {
            fields: ReasonFields { message, body: Some(body), http: None, cause: None },
            retry_after_ms: None,
        });
    }
    AiError::new(crate::provider_error::classify_provider_failure(
        crate::provider_error::ProviderFailure {
            message: &message,
            status,
            raw_body: Some(&body),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        },
    ))
}

/// The step machine (port of `step`). Events must pass through [`normalize`]
/// first (the protocol wrapper does it).
pub fn step(state: &ParserState, event: NormalizedEvent) -> Result<StepResult, AiError> {
    let kind = event.event.kind.as_str();
    if kind == "response.output_text.delta" || kind == "response.output_text.done" {
        let Some(item_id) = event.item_id.clone() else {
            return Err(event_error(
                state.id.clone(),
                format!("{} is missing item_id", event.event.kind),
            ));
        };
        return Ok(if kind == "response.output_text.delta" {
            on_output_text_delta(state, &event.event, &item_id)
        } else {
            on_output_text_done(state, &event.event, &item_id)
        });
    }
    if kind == "response.refusal.delta" || kind == "response.refusal.done" {
        let value = if kind == "response.refusal.delta" {
            event.event.delta()
        } else {
            event.event.refusal()
        };
        let (Some(item_id), Some(value)) = (event.item_id.clone(), value) else {
            return Err(event_error(
                state.id.clone(),
                format!("{} is malformed", event.event.kind),
            ));
        };
        return Ok(if kind == "response.refusal.delta" {
            on_output_text_delta(state, &event.event, &item_id)
        } else {
            let mut rewritten = event.event.clone();
            if let Some(object) = rewritten.raw.as_object_mut() {
                object.insert("text".into(), json!(value));
            }
            on_output_text_done(state, &rewritten, &item_id)
        });
    }
    if kind == "response.reasoning.delta" || kind == "response.reasoning_summary_text.delta" {
        let Some(item_id) = event.item_id.clone() else {
            return Err(event_error(
                state.id.clone(),
                format!("{} is missing item_id", event.event.kind),
            ));
        };
        return Ok(on_reasoning_delta(state, &event.event, &item_id));
    }
    if kind == "response.reasoning.done"
        || kind == "response.reasoning_summary_text.done"
        || kind == "response.reasoning_text.done"
    {
        let Some(item_id) = event.item_id.clone() else {
            return Err(event_error(
                state.id.clone(),
                format!("{} is missing item_id", event.event.kind),
            ));
        };
        return Ok(on_reasoning_done(state, &event.event, &item_id));
    }
    if kind == "response.reasoning_summary_part.added"
        || kind == "response.reasoning_summary_part.done"
    {
        let Some(item_id) = event.item_id.clone() else {
            return Err(event_error(
                state.id.clone(),
                format!("{} is missing item_id", event.event.kind),
            ));
        };
        let _ = item_id;
        return Ok(if kind == "response.reasoning_summary_part.added" {
            on_reasoning_summary_part_added(state, &event.event)
        } else {
            on_reasoning_summary_part_done(state, &event.event)
        });
    }
    if kind == "response.output_item.added" {
        if let Some(item) = event.item.as_ref() {
            let item_id = item.id.clone().unwrap_or_default();
            if item.kind == "reasoning"
                && !state.reasoning_items.contains_key(&item_id)
                && state.open_reasoning_blocks()
            {
                return Err(event_error(
                    state.id.clone(),
                    format!("{kind} started reasoning before the previous item ended"),
                ));
            }
        }
        let mut next = state.clone();
        if let (Some(index), Some(item)) = (event.event.output_index(), event.item.as_ref()) {
            if let Some(id) = item.id.clone() {
                next.output_items.insert(index, id);
            }
        }
        return Ok(on_output_item_added(&next, &event));
    }
    if kind == "response.function_call_arguments.delta"
        || kind == "response.function_call_arguments.done"
    {
        if event.item_id.is_none() {
            return Err(event_error(
                state.id.clone(),
                format!("{} is missing item_id", event.event.kind),
            ));
        }
        return on_function_call_arguments_delta(state, &event.event);
    }
    if kind == "response.output_item.done" {
        return match event.item.as_ref() {
            Some(item) => on_output_item_done(state, item),
            None => Ok(no_events(state)),
        };
    }
    if kind == "response.completed" || kind == "response.incomplete" {
        return on_response_finish(state, &event.event);
    }
    if kind == "response.failed" {
        return Err(provider_failure(
            &event.event,
            &format!("{} response failed", state.name),
            None,
        ));
    }
    if kind == "error" {
        return Err(provider_failure(&event.event, &format!("{} stream error", state.name), None));
    }
    Ok(no_events(state))
}

// =============================================================================
// Protocol + route factory
// =============================================================================

/// The Open Responses protocol bound to one provider adapter.
pub struct OpenResponsesProtocol {
    adapter: Arc<dyn ProviderAdapter>,
}

impl OpenResponsesProtocol {
    #[must_use]
    pub fn new(adapter: Arc<dyn ProviderAdapter>) -> Self {
        Self { adapter }
    }

    /// Drive the machine over one raw frame: decode → normalize → step, and
    /// report whether the frame terminates the stream. Used by the parity
    /// tests and the checkpoint/compaction operations.
    pub fn step_frame(
        &self,
        state: &mut ParserState,
        frame: &str,
    ) -> Result<(Vec<LLMEvent>, bool), AiError> {
        let event = decode_event(frame)?;
        let is_terminal = terminal(&event);
        let normalized = normalize(state, event);
        let (next, events) = step(state, normalized)?;
        *state = next;
        Ok((events, is_terminal))
    }
}

impl Protocol for OpenResponsesProtocol {
    fn id(&self) -> &str {
        self.adapter.id()
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request_with_adapter(request, self.adapter.as_ref())
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState::new(request, self.adapter.as_ref()))
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let mut state = *state.downcast::<ParserState>().expect("OpenResponses parser state");
        let (events, _) = self.step_frame(&mut state, frame)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        decode_event(frame).is_ok_and(|event| terminal(&event))
    }
}

/// Inputs for [`open_responses_route`].
pub struct OpenResponsesRouteSpec {
    pub id: String,
    pub provider: String,
    pub provider_metadata_key: Option<String>,
    pub base_url: String,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
    pub defaults: RouteDefaults,
}

/// Build a provider-neutral Open Responses route (the seam the moonshot /
/// minimax style providers build on).
#[must_use]
pub fn open_responses_route(spec: OpenResponsesRouteSpec) -> Route {
    Route::new(RouteInput {
        id: spec.id,
        provider: Some(spec.provider),
        provider_metadata_key: spec.provider_metadata_key,
        protocol: Arc::new(OpenResponsesProtocol::new(Arc::new(BaseAdapter))),
        endpoint: Endpoint::path(PATH).with_base_url(spec.base_url),
        auth: spec.auth,
        framing: Framing::sse(),
        headers: None,
        transport: spec.transport,
        defaults: spec.defaults,
    })
}

#[cfg(test)]
mod tests {
    //! Ports of `open-responses-lifecycle.test.ts`, `open-responses-finals.test.ts`
    //! and the decode half of `open-responses-errors.test.ts`, driven directly
    //! through the step machine (the TS tests stream through a fixed HTTP
    //! layer; the frames are identical).

    use super::*;
    use crate::schema::LLMRequest;

    fn request() -> LLMRequest {
        crate::test_support::request_for_tests(vec![Message::user("Respond.")])
    }

    fn adapter_state() -> ParserState {
        ParserState::new(&request(), &BaseAdapter)
    }

    /// Run frames through the baseline machine, collecting events until the
    /// terminal frame (mirrors the route's takeUntil + requireTerminalEvent).
    fn collect(frames: &[Value]) -> Vec<LLMEvent> {
        let protocol = OpenResponsesProtocol::new(Arc::new(BaseAdapter));
        let mut state = adapter_state();
        let mut events = Vec::new();
        for frame in frames {
            let (emitted, terminal) =
                protocol.step_frame(&mut state, &encode_json(frame)).expect("step");
            events.extend(emitted);
            if terminal {
                // Route termination must also prevent events after completion.
                break;
            }
        }
        events
    }

    fn completed() -> Value {
        json!({ "type": "response.completed", "response": { "id": "resp_1" } })
    }

    fn reasoning_events(events: &[LLMEvent]) -> Vec<&LLMEvent> {
        events.iter().filter(|event| event.kind().starts_with("reasoning-")).collect()
    }

    fn text_events(events: &[LLMEvent]) -> Vec<&LLMEvent> {
        events.iter().filter(|event| event.kind().starts_with("text-")).collect()
    }

    fn event_id(event: &LLMEvent) -> &str {
        match event {
            LLMEvent::ReasoningStart { id, .. }
            | LLMEvent::ReasoningDelta { id, .. }
            | LLMEvent::ReasoningEnd { id, .. }
            | LLMEvent::TextStart { id, .. }
            | LLMEvent::TextDelta { id, .. }
            | LLMEvent::TextEnd { id, .. } => id,
            other => panic!("unexpected event {}", other.kind()),
        }
    }

    #[test]
    fn open_responses_closes_implicit_summary_boundaries() {
        let item =
            json!({ "type": "reasoning", "id": "rs_1", "encrypted_content": "encrypted-state" });
        let events = collect(&[
            json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "reasoning", "id": "rs_1", "encrypted_content": null } }),
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_1", "encrypted_content": null } }),
            json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "summary_index": 0, "delta": "First" }),
            json!({ "type": "response.reasoning_summary_part.added", "item_id": "rs_1", "summary_index": 1 }),
            json!({ "type": "response.reasoning_summary_part.done", "item_id": "rs_1", "summary_index": 0 }),
            json!({ "type": "response.reasoning_summary_text.done", "item_id": "rs_1", "summary_index": 1, "text": "Second" }),
            // The third part omits both explicit summary boundaries.
            json!({ "type": "response.reasoning_summary_text.delta", "output_index": 0, "item_id": "wrong", "summary_index": 2, "delta": "Third" }),
            json!({ "type": "response.output_item.done", "item": item }),
            completed(),
        ]);

        let expected: Vec<Value> = vec![
            json!({
                "type": "reasoning-start", "id": "rs_1:0",
                "providerMetadata": { "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": null } },
            }),
            json!({ "type": "reasoning-delta", "id": "rs_1:0", "text": "First" }),
            json!({ "type": "reasoning-end", "id": "rs_1:0", "providerMetadata": { "openresponses": { "itemId": "rs_1" } } }),
            json!({
                "type": "reasoning-start", "id": "rs_1:1",
                "providerMetadata": { "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": null } },
            }),
            json!({ "type": "reasoning-delta", "id": "rs_1:1", "text": "Second" }),
            json!({ "type": "reasoning-end", "id": "rs_1:1", "providerMetadata": { "openresponses": { "itemId": "rs_1" } } }),
            json!({
                "type": "reasoning-start", "id": "rs_1:2",
                "providerMetadata": { "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": null } },
            }),
            json!({ "type": "reasoning-delta", "id": "rs_1:2", "text": "Third" }),
            json!({
                "type": "reasoning-end", "id": "rs_1:2",
                "providerMetadata": { "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": "encrypted-state" } },
            }),
        ];
        let actual: Vec<Value> = reasoning_events(&events)
            .into_iter()
            .map(|event| serde_json::to_value(wire(event)).unwrap())
            .collect();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }

    /// Wire shape with TS field names for assertions.
    fn wire(event: &LLMEvent) -> Value {
        let mut object = Map::new();
        object.insert("type".into(), json!(event.kind()));
        match event {
            LLMEvent::ReasoningStart { id, provider_metadata, .. }
            | LLMEvent::ReasoningEnd { id, provider_metadata, .. }
            | LLMEvent::TextStart { id, provider_metadata, .. }
            | LLMEvent::TextEnd { id, provider_metadata, .. } => {
                object.insert("id".into(), json!(id));
                if let Some(text) = match event {
                    LLMEvent::ReasoningEnd { text, .. } | LLMEvent::TextEnd { text, .. } => {
                        text.clone()
                    }
                    _ => None,
                } {
                    object.insert("text".into(), json!(text));
                }
                if let Some(metadata) = metadata_value(provider_metadata.as_ref()) {
                    object.insert("providerMetadata".into(), metadata);
                }
            }
            LLMEvent::ReasoningDelta { id, text, .. } | LLMEvent::TextDelta { id, text, .. } => {
                object.insert("id".into(), json!(id));
                object.insert("text".into(), json!(text));
            }
            other => {
                let _ = other;
            }
        }
        Value::Object(object)
    }

    fn metadata_value(metadata: Option<&ProviderMetadata>) -> Option<Value> {
        metadata.map(|metadata| {
            Value::Object(
                metadata
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            Value::Object(
                                value
                                    .iter()
                                    .map(|(field, item)| (field.clone(), item.clone()))
                                    .collect(),
                            ),
                        )
                    })
                    .collect(),
            )
        })
    }

    #[test]
    fn open_responses_preserves_done_only_reasoning_text_and_encryption() {
        let events = collect(&[
            json!({ "type": "response.output_item.done", "item": { "type": "reasoning", "id": "rs_1", "encrypted_content": "encrypted-state", "summary": [{ "type": "summary_text", "text": "Not streamed" }] } }),
            completed(),
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_after" } }),
        ]);
        let actual: Vec<Value> = reasoning_events(&events)
            .into_iter()
            .map(|event| serde_json::to_value(wire(event)).unwrap())
            .collect();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(vec![
                json!({
                    "type": "reasoning-start", "id": "rs_1",
                    "providerMetadata": { "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": "encrypted-state" } },
                }),
                json!({
                    "type": "reasoning-end", "id": "rs_1", "text": "Not streamed",
                    "providerMetadata": { "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": "encrypted-state" } },
                }),
            ])
            .unwrap()
        );
    }

    #[test]
    fn open_responses_forgets_never_streamed_messages_and_preserves_refusal_phases() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_empty" } }),
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_1", "phase": "commentary" } }),
            json!({ "type": "response.output_text.done", "item_id": "msg_1", "text": "Checking" }),
            json!({ "type": "response.output_text.done", "item_id": "msg_1", "text": "Duplicate" }),
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_2", "phase": null } }),
            json!({ "type": "response.output_text.delta", "item_id": "msg_empty", "delta": "stale" }),
            json!({ "type": "response.output_text.done", "item_id": "msg_empty", "text": "stale final" }),
            json!({ "type": "response.output_text.delta", "item_id": "msg_1", "delta": "late" }),
            json!({ "type": "response.refusal.delta", "item_id": "msg_2", "delta": "Cannot help." }),
            json!({ "type": "response.refusal.done", "item_id": "msg_2", "refusal": "Cannot help." }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_2", "phase": "final_answer" } }),
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_3", "phase": null } }),
            json!({ "type": "response.refusal.done", "item_id": "msg_3", "refusal": "Done-only refusal." }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_3" } }),
            completed(),
        ]);
        let actual: Vec<Value> = text_events(&events)
            .into_iter()
            .map(|event| serde_json::to_value(wire(event)).unwrap())
            .collect();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(vec![
                json!({
                    "type": "text-start", "id": "msg_1",
                    "providerMetadata": { "openresponses": { "itemId": "msg_1", "phase": "commentary" } },
                }),
                json!({ "type": "text-delta", "id": "msg_1", "text": "Checking" }),
                json!({
                    "type": "text-end", "id": "msg_1",
                    "providerMetadata": { "openresponses": { "itemId": "msg_1", "phase": "commentary" } },
                }),
                json!({
                    "type": "text-start", "id": "msg_2",
                    "providerMetadata": { "openresponses": { "itemId": "msg_2", "phase": null } },
                }),
                json!({ "type": "text-delta", "id": "msg_2", "text": "Cannot help." }),
                json!({
                    "type": "text-end", "id": "msg_2",
                    "providerMetadata": { "openresponses": { "itemId": "msg_2", "phase": "final_answer" } },
                }),
                json!({
                    "type": "text-start", "id": "msg_3",
                    "providerMetadata": { "openresponses": { "itemId": "msg_3", "phase": null } },
                }),
                json!({ "type": "text-delta", "id": "msg_3", "text": "Done-only refusal." }),
                json!({
                    "type": "text-end", "id": "msg_3",
                    "providerMetadata": { "openresponses": { "itemId": "msg_3", "phase": null } },
                }),
            ])
            .unwrap()
        );
    }

    #[test]
    fn open_responses_preserves_non_empty_done_only_message_content() {
        let events = collect(&[
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_text", "content": [{ "type": "output_text", "text": "Done-only text." }] } }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_empty", "content": [{ "type": "output_text", "text": "" }] } }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_refusal", "content": [{ "type": "refusal", "refusal": "Done-only refusal." }] } }),
            completed(),
        ]);
        let actual: Vec<Value> = text_events(&events)
            .into_iter()
            .map(|event| serde_json::to_value(wire(event)).unwrap())
            .collect();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(vec![
                json!({ "type": "text-start", "id": "msg_text", "providerMetadata": { "openresponses": { "itemId": "msg_text" } } }),
                json!({ "type": "text-end", "id": "msg_text", "text": "Done-only text.", "providerMetadata": { "openresponses": { "itemId": "msg_text" } } }),
                json!({ "type": "text-start", "id": "msg_refusal", "providerMetadata": { "openresponses": { "itemId": "msg_refusal" } } }),
                json!({ "type": "text-end", "id": "msg_refusal", "text": "Done-only refusal.", "providerMetadata": { "openresponses": { "itemId": "msg_refusal" } } }),
            ])
            .unwrap()
        );
    }

    // Captured from Bedrock Mantle (openai.gpt-oss-120b): the terminal
    // function_call items rename `id` to `item_id` and carry a stray
    // `output_index`.
    #[test]
    fn open_responses_recovers_a_terminal_function_call_id_from_its_output_slot() {
        let terminal = json!({
            "type": "function_call", "item_id": "fc_828bee50dee1d029", "call_id": "call_bc1eb4b42e70ee53",
            "name": "get_weather", "arguments": "{\n  \"city\": \"Paris\"\n}", "output_index": 1, "status": "completed",
        });
        let events = collect(&[
            json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "reasoning", "id": "msg_879a68b589198b4c" } }),
            json!({ "type": "response.output_item.done", "output_index": 0, "item": { "type": "reasoning", "id": "msg_879a68b589198b4c" } }),
            json!({ "type": "response.output_item.added", "output_index": 1, "item": { "type": "function_call", "id": "fc_828bee50dee1d029", "call_id": "call_bc1eb4b42e70ee53", "name": "get_weather", "arguments": "", "status": "in_progress" } }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 1, "item_id": "fc_828bee50dee1d029", "delta": "{\n  \"city\": \"Paris\"\n}" }),
            json!({ "type": "response.function_call_arguments.done", "output_index": 1, "item_id": "fc_828bee50dee1d029", "arguments": "{\n  \"city\": \"Paris\"\n}" }),
            json!({ "type": "response.output_item.done", "output_index": 1, "item": terminal }),
            json!({ "type": "response.completed", "response": { "id": "resp_1", "output": [{ "type": "reasoning", "id": "msg_879a68b589198b4c" }, terminal] } }),
        ]);
        let tool_events: Vec<&LLMEvent> =
            events.iter().filter(|event| event.kind().starts_with("tool-")).collect();
        assert_eq!(tool_events.len(), 4);
        assert!(
            matches!(tool_events[0], LLMEvent::ToolInputStart { id, name, .. } if id == "call_bc1eb4b42e70ee53" && name == "get_weather")
        );
        match tool_events[1] {
            LLMEvent::ToolInputDelta { input, text, .. } => {
                assert_eq!(text, "{\n  \"city\": \"Paris\"\n}");
                assert_eq!(input, &Some(json!({ "city": "Paris" })));
            }
            other => panic!("unexpected {}", other.kind()),
        }
        assert!(matches!(tool_events[2], LLMEvent::ToolInputEnd { .. }));
        assert!(
            matches!(tool_events[3], LLMEvent::ToolCall { input, .. } if *input == json!({ "city": "Paris" }))
        );
    }

    #[test]
    fn open_responses_mints_an_id_for_a_done_only_tool_that_never_had_one() {
        let events = collect(&[
            json!({ "type": "response.output_item.done", "output_index": 0, "item": { "type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{\"query\":\"weather\"}" } }),
            completed(),
        ]);
        let call = events
            .iter()
            .find(|event| matches!(event, LLMEvent::ToolCall { .. }))
            .expect("tool call");
        match call {
            LLMEvent::ToolCall { id, name, input, provider_metadata, .. } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "lookup");
                assert_eq!(*input, json!({ "query": "weather" }));
                let metadata = provider_metadata.as_ref().unwrap();
                let item_id = metadata["openresponses"]["itemId"].as_str().unwrap();
                assert!(
                    regex::Regex::new(r"^fc_[0-9a-f]{32}$").unwrap().is_match(item_id),
                    "minted id shape: {item_id}"
                );
            }
            other => panic!("unexpected {}", other.kind()),
        }
    }

    #[test]
    fn open_responses_opens_and_closes_a_done_only_tool() {
        let events = collect(&[
            json!({ "type": "response.output_item.done", "item": { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "lookup", "arguments": "{\"query\":\"weather\"}" } }),
            completed(),
        ]);
        let tool_events: Vec<&LLMEvent> =
            events.iter().filter(|event| event.kind().starts_with("tool-")).collect();
        assert_eq!(tool_events.len(), 3);
        assert!(matches!(tool_events[0], LLMEvent::ToolInputStart { .. }));
        assert!(matches!(tool_events[1], LLMEvent::ToolInputEnd { .. }));
        assert!(
            matches!(tool_events[2], LLMEvent::ToolCall { input, .. } if *input == json!({ "query": "weather" }))
        );
        let finish = events.iter().find(|event| event.is_finish()).unwrap();
        match finish {
            LLMEvent::Finish { reason, provider_metadata, .. } => {
                assert_eq!(reason.normalized, FinishReason::ToolCalls);
                assert_eq!(
                    provider_metadata.as_ref().unwrap()["openresponses"]["responseId"],
                    json!("resp_1")
                );
                assert!(!provider_metadata.as_ref().unwrap()["openresponses"]
                    .contains_key("serviceTier"));
            }
            other => panic!("unexpected {}", other.kind()),
        }
    }

    #[test]
    fn open_responses_recovers_pending_calls_without_reconciling_terminal_reasoning() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "lookup" } }),
            json!({ "type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{\"query\":\"draft\"}" }),
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_1", "encrypted_content": null } }),
            json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "delta": "Thinking" }),
            json!({ "type": "response.reasoning_summary_part.done", "item_id": "rs_1", "summary_index": 0 }),
            json!({ "type": "response.completed", "response": { "id": "resp_1", "output": [
                { "type": "reasoning", "id": "rs_1", "encrypted_content": "terminal-state" },
                { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "lookup", "arguments": "{\"query\":\"final\"}" },
                { "type": "function_call", "id": "fc_unseen", "call_id": "call_unseen", "name": "lookup", "arguments": "{}" },
            ] } }),
        ]);
        // step-start, tool-input-start, tool-input-delta, reasoning-start,
        // reasoning-delta, then recovery, then step-finish + finish
        assert_eq!(events.len(), 10);
        let recovered = &events[5..events.len() - 2];
        assert_eq!(recovered.len(), 3);
        assert!(matches!(&recovered[0], LLMEvent::ToolInputEnd { id, .. } if id == "call_1"));
        assert!(
            matches!(&recovered[1], LLMEvent::ToolCall { input, .. } if *input == json!({ "query": "final" }))
        );
        assert!(matches!(&recovered[2], LLMEvent::ReasoningEnd { id, .. } if id == "rs_1:0"));
    }

    #[test]
    fn open_responses_keeps_text_and_reasoning_identities_separate_with_empty_item_ids() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "" } }),
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "" } }),
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "" } }),
            json!({ "type": "response.reasoning_summary_text.delta", "item_id": "", "delta": "Thinking" }),
            json!({ "type": "response.output_text.delta", "item_id": "", "delta": "Answer" }),
            json!({ "type": "response.output_item.done", "item": { "type": "reasoning", "id": "", "encrypted_content": "state" } }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "" } }),
            completed(),
        ]);
        let reasoning_deltas: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                LLMEvent::ReasoningDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(reasoning_deltas, ["Thinking"]);
        let text_deltas: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                LLMEvent::TextDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text_deltas, ["Answer"]);
        let reasoning_ends: Vec<&LLMEvent> =
            events.iter().filter(|event| matches!(event, LLMEvent::ReasoningEnd { .. })).collect();
        assert_eq!(reasoning_ends.len(), 1);
        assert_eq!(event_id(reasoning_ends[0]), ":0");
        assert_eq!(
            serde_json::to_value(
                match reasoning_ends[0] {
                    LLMEvent::ReasoningEnd { provider_metadata, .. } =>
                        metadata_value(provider_metadata.as_ref()),
                    _ => None,
                }
                .unwrap()
            )
            .unwrap(),
            json!({ "openresponses": { "itemId": "", "reasoningEncryptedContent": "state" } })
        );
    }

    #[test]
    fn open_responses_does_not_recover_a_tool_from_a_tracked_message_with_the_same_id() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "item_1" } }),
            json!({ "type": "response.output_text.delta", "item_id": "item_1", "delta": "Answer" }),
            json!({ "type": "response.completed", "response": { "id": "resp_1", "output": [
                { "type": "function_call", "id": "item_1", "call_id": "call_1", "name": "lookup", "arguments": "{}" },
            ] } }),
        ]);
        assert!(!events.iter().any(|event| matches!(event, LLMEvent::ToolCall { .. })));
        let finish = events.iter().find(|event| event.is_finish()).unwrap();
        match finish {
            LLMEvent::Finish { reason, .. } => assert_eq!(reason.normalized, FinishReason::Stop),
            other => panic!("unexpected {}", other.kind()),
        }
    }

    #[test]
    fn open_responses_flushes_pending_calls_and_open_text_without_completed_output() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_1", "phase": "final_answer" } }),
            json!({ "type": "response.output_text.delta", "item_id": "msg_1", "delta": "Answer" }),
            json!({ "type": "response.output_item.added", "item": { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "lookup", "arguments": "{}" } }),
            completed(),
        ]);
        let recovered = &events[4..events.len() - 2];
        assert_eq!(recovered.len(), 3);
        assert!(matches!(&recovered[0], LLMEvent::ToolInputEnd { id, .. } if id == "call_1"));
        assert!(
            matches!(&recovered[1], LLMEvent::ToolCall { id, input, .. } if id == "call_1" && *input == json!({}))
        );
        assert!(matches!(&recovered[2], LLMEvent::TextEnd { id, .. } if id == "msg_1"));
    }

    #[test]
    fn open_responses_does_not_reconcile_on_incomplete_responses() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_1", "encrypted_content": null } }),
            json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "delta": "Partial" }),
            json!({ "type": "response.output_item.added", "item": { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "lookup" } }),
            json!({ "type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{\"query\":" }),
            json!({ "type": "response.incomplete", "response": { "id": "resp_1", "incomplete_details": { "reason": "max_output_tokens" }, "output": [
                { "type": "reasoning", "id": "rs_1", "encrypted_content": "not-reconciled" },
                { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "lookup", "arguments": "{\"query\":\"not-reconciled\"}" },
            ] } }),
        ]);
        assert!(!events.iter().any(|event| matches!(event, LLMEvent::ToolInputEnd { .. })));
        assert!(!events.iter().any(|event| matches!(event, LLMEvent::ToolCall { .. })));
        let reasoning_ends: Vec<&LLMEvent> =
            events.iter().filter(|event| matches!(event, LLMEvent::ReasoningEnd { .. })).collect();
        assert_eq!(reasoning_ends.len(), 1);
        assert_eq!(event_id(reasoning_ends[0]), "rs_1:0");
        let finish = events.iter().find(|event| event.is_finish()).unwrap();
        match finish {
            LLMEvent::Finish { reason, provider_metadata, .. } => {
                assert_eq!(reason.normalized, FinishReason::Length);
                assert_eq!(reason.raw.as_deref(), Some("max_output_tokens"));
                assert_eq!(
                    provider_metadata.as_ref().unwrap()["openresponses"]["responseId"],
                    json!("resp_1")
                );
            }
            other => panic!("unexpected {}", other.kind()),
        }
    }

    // ---- finals (completed item text / reasoning) ----

    #[test]
    fn open_responses_replaces_streamed_text_with_completed_item_text() {
        for text in ["Draft expanded", "D", "Replacement", ""] {
            let events = collect(&[
                json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_1", "phase": "commentary" } }),
                json!({ "type": "response.output_text.delta", "item_id": "msg_1", "delta": "Draft" }),
                json!({ "type": "response.output_text.done", "item_id": "msg_1", "text": "Part final" }),
                json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_1", "phase": "final_answer", "content": [{ "type": "output_text", "text": text }] } }),
                completed(),
            ]);
            let state = events
                .clone()
                .into_iter()
                .fold(crate::schema::ResponseState::empty(), crate::schema::ResponseState::reduce);
            let response = state.complete().expect("completed");
            assert_eq!(response_text_of(&response), text, "text {text:?}");
            let deltas: Vec<&str> = events
                .iter()
                .filter_map(|event| match event {
                    LLMEvent::TextDelta { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(deltas, ["Draft"]);
            let ends: Vec<&LLMEvent> =
                events.iter().filter(|event| matches!(event, LLMEvent::TextEnd { .. })).collect();
            assert_eq!(ends.len(), 1);
            match ends[0] {
                LLMEvent::TextEnd { text: end_text, provider_metadata, .. } => {
                    assert_eq!(end_text.as_deref(), Some(text));
                    assert_eq!(
                        serde_json::to_value(metadata_value(provider_metadata.as_ref()).unwrap())
                            .unwrap(),
                        json!({ "openresponses": { "itemId": "msg_1", "phase": "final_answer" } })
                    );
                }
                other => panic!("unexpected {}", other.kind()),
            }
        }
    }

    fn response_text_of(response: &crate::schema::LLMResponse) -> String {
        response
            .message
            .content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn open_responses_joins_completed_text_and_refusal_parts() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_1" } }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_1", "content": [
                { "type": "output_text", "text": "Answer. " },
                { "type": "refusal", "refusal": "Cannot help." },
            ] } }),
            completed(),
        ]);
        let state = events
            .clone()
            .into_iter()
            .fold(crate::schema::ResponseState::empty(), crate::schema::ResponseState::reduce);
        let response = state.complete().unwrap();
        assert_eq!(response_text_of(&response), "Answer. Cannot help.");
        assert_eq!(events.iter().filter(|e| matches!(e, LLMEvent::TextStart { .. })).count(), 1);
        assert_eq!(events.iter().filter(|e| matches!(e, LLMEvent::TextEnd { .. })).count(), 1);
    }

    #[test]
    fn open_responses_does_not_create_an_empty_fragment_for_an_empty_completed_message() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_1" } }),
            json!({ "type": "response.output_text.done", "item_id": "msg_1", "text": "" }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "id": "msg_1", "content": [{ "type": "output_text", "text": "" }] } }),
            completed(),
        ]);
        let state = events
            .clone()
            .into_iter()
            .fold(crate::schema::ResponseState::empty(), crate::schema::ResponseState::reduce);
        let response = state.complete().unwrap();
        assert!(response.message.content.is_empty());
        assert!(!events.iter().any(|e| matches!(e, LLMEvent::TextStart { .. })));
    }

    #[test]
    fn open_responses_uses_completed_item_reasoning_text_variants() {
        let fixtures = [
            (vec![("Final", false), ("summary", false)], vec!["Raw"], "Final\n\nsummary"),
            (vec![("", false), ("", false)], vec!["Raw"], "Raw"),
            (vec![("", false), ("", false)], vec!["", ""], "Draft"),
        ];
        for (summary, content, expected) in fixtures {
            let summary_json: Vec<Value> = summary
                .iter()
                .map(|(text, _)| json!({ "type": "summary_text", "text": text }))
                .collect();
            let content_json: Vec<Value> = content
                .iter()
                .map(|text| json!({ "type": "reasoning_text", "text": text }))
                .collect();
            let events = collect(&[
                json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_1" } }),
                json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "delta": "Draft" }),
                json!({ "type": "response.reasoning_summary_text.done", "item_id": "rs_1", "text": "Part final" }),
                json!({ "type": "response.output_item.done", "item": { "type": "reasoning", "id": "rs_1", "summary": summary_json, "content": content_json, "encrypted_content": "encrypted" } }),
                completed(),
            ]);
            let state = events
                .clone()
                .into_iter()
                .fold(crate::schema::ResponseState::empty(), crate::schema::ResponseState::reduce);
            let response = state.complete().unwrap();
            let reasoning: String = response
                .message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Reasoning { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(reasoning, expected);
            assert_eq!(
                events.iter().filter(|e| matches!(e, LLMEvent::ReasoningEnd { .. })).count(),
                1
            );
            let metadata = response
                .message
                .content
                .iter()
                .find_map(|part| match part {
                    ContentPart::Reasoning { provider_metadata, .. } => provider_metadata.clone(),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                serde_json::to_value(metadata_value(Some(&metadata)).unwrap()).unwrap(),
                json!({ "openresponses": { "itemId": "rs_1", "reasoningEncryptedContent": "encrypted" } })
            );
        }
    }

    #[test]
    fn open_responses_replaces_only_the_still_open_summary() {
        let events = collect(&[
            json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_1" } }),
            json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "summary_index": 0, "delta": "First " }),
            json!({ "type": "response.reasoning_summary_part.added", "item_id": "rs_1", "summary_index": 1 }),
            json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "summary_index": 1, "delta": "draft" }),
            json!({ "type": "response.output_item.done", "item": { "type": "reasoning", "id": "rs_1", "summary": [
                { "type": "summary_text", "text": "First " },
                { "type": "summary_text", "text": "final" },
            ] } }),
            completed(),
        ]);
        let state = events
            .clone()
            .into_iter()
            .fold(crate::schema::ResponseState::empty(), crate::schema::ResponseState::reduce);
        let response = state.complete().unwrap();
        let reasoning: String = response
            .message
            .content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Reasoning { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(reasoning, "First final");
        let ends: Vec<Option<String>> = events
            .iter()
            .filter_map(|event| match event {
                LLMEvent::ReasoningEnd { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(ends, [None, Some("final".to_string())]);
    }

    #[test]
    fn open_responses_keeps_streamed_text_when_part_finals_precede_completion() {
        for kind in ["response.completed", "response.incomplete"] {
            let events = collect(&[
                json!({ "type": "response.output_item.added", "item": { "type": "message", "id": "msg_1" } }),
                json!({ "type": "response.output_text.delta", "item_id": "msg_1", "content_index": 0, "delta": "Hel" }),
                json!({ "type": "response.output_text.delta", "item_id": "msg_1", "content_index": 1, "delta": "world" }),
                json!({ "type": "response.output_text.done", "item_id": "msg_1", "content_index": 0, "text": "Hello " }),
                json!({ "type": "response.content_part.done", "item_id": "msg_1", "content_index": 0, "part": { "type": "output_text", "text": "Hello " } }),
                json!({ "type": "response.output_item.added", "item": { "type": "reasoning", "id": "rs_1" } }),
                json!({ "type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "delta": "Draft" }),
                json!({ "type": "response.reasoning_summary_text.done", "item_id": "rs_1", "text": "Part final" }),
                json!({ "type": "response.reasoning_summary_part.done", "item_id": "rs_1", "summary_index": 0, "part": { "type": "summary_text", "text": "Part final" } }),
                json!({ "type": kind, "response": {
                    "id": "resp_1",
                    "incomplete_details": if kind == "response.incomplete" { json!({ "reason": "max_output_tokens" }) } else { Value::Null },
                } }),
            ]);
            let state = events
                .clone()
                .into_iter()
                .fold(crate::schema::ResponseState::empty(), crate::schema::ResponseState::reduce);
            let response = state.complete().unwrap();
            assert_eq!(response_text_of(&response), "Helworld");
            let reasoning: String = response
                .message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Reasoning { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(reasoning, "Draft");
            let text_ends: Vec<Option<String>> = events
                .iter()
                .filter_map(|event| match event {
                    LLMEvent::TextEnd { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(text_ends, [None]);
            let reasoning_ends: Vec<Option<String>> = events
                .iter()
                .filter_map(|event| match event {
                    LLMEvent::ReasoningEnd { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(reasoning_ends, [None]);
        }
    }

    // ---- errors (decode half; classification parity lives in
    //      tests/responses_parity.rs) ----

    #[test]
    fn open_responses_decodes_error_frames_verbatim() {
        let frames = [
            json!({ "type": "error", "sequence_number": 4, "code": "server_shutting_down", "message": "Shutting down", "param": null }),
            json!({ "type": "error" }),
            json!({ "type": "error", "error": "Gateway failed" }),
            json!({ "type": "error", "error": { "code": 429, "message": "slow down" } }),
            json!({ "type": "error", "error": 42 }),
            json!({ "type": "error", "code": 500, "message": ["not", "a", "string"] }),
            json!({ "type": "response.failed", "response": { "id": "resp_failed", "error": "Gateway failed" } }),
            json!({ "type": "response.failed", "response": { "id": "resp_failed", "error": ["weird"] } }),
            json!({ "type": "response.failed", "response": { "id": "resp_failed", "error": { "code": "server_error", "message": "Internal server error" } } }),
            json!({ "type": "response.output_text.delta", "item_id": "msg_text", "delta": "Hello" }),
        ];
        for frame in frames {
            let event = decode_event(&encode_json(&frame)).expect("decodes");
            assert_eq!(event.raw, frame, "frame kept verbatim");
        }
        assert!(decode_event("not json").is_err());
        assert!(decode_string(json!(42).to_string().as_str()).is_err());
        assert!(decode_string(json!({ "delta": 1 }).to_string().as_str()).is_err());
    }

    fn decode_string(frame: &str) -> Result<Event, AiError> {
        decode_event(frame)
    }

    #[test]
    fn open_responses_mints_deterministic_ids_in_order() {
        // Two id-less done-only items mint sequentially.
        let events = collect(&[
            json!({ "type": "response.output_item.done", "item": { "type": "function_call", "call_id": "call_1", "name": "a", "arguments": "{}" } }),
            json!({ "type": "response.output_item.done", "item": { "type": "message", "content": [{ "type": "output_text", "text": "hi" }] } }),
            completed(),
        ]);
        let ids: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                LLMEvent::ToolInputStart { provider_metadata, .. } => provider_metadata
                    .as_ref()
                    .and_then(|m| m.get("openresponses"))
                    .and_then(|m| m.get("itemId"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                LLMEvent::TextStart { provider_metadata, .. } => provider_metadata
                    .as_ref()
                    .and_then(|m| m.get("openresponses"))
                    .and_then(|m| m.get("itemId"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            [
                format!("fc_{}000000000001", "0".repeat(20)),
                format!("msg_{}000000000002", "0".repeat(20)),
            ]
        );
    }
}
