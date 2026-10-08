//! The standalone `/compact` Responses endpoint (port of
//! `src/protocols/utils/responses-compaction.ts`).
//!
//! `/compact` has a smaller wire contract than `/responses`; the request
//! keeps conversation + the request controls it accepts, and the response is
//! a `response.compaction` object whose output replays as replacement
//! messages (compaction checkpoint, encrypted reasoning, summary messages).

use serde_json::{json, Map, Value};

use crate::protocols::open_responses::{
    lower_conversation, lower_generation_for, map_usage, metadata_key, ProviderAdapter,
};
use crate::protocols::utils::responses_checkpoint::{compact_endpoint, invalid_output};
use crate::route::executor;
use crate::route::transport::read_body_limited;
use crate::route::Route;
use crate::schema::media;
use crate::schema::{
    AiError, ContentPart, LLMRequest, Message, MessageRole, ProviderMetadata, Usage,
};

/// The replacement-context result of a compaction call.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionResponse {
    pub replacement: Vec<Message>,
    pub usage: Option<Usage>,
}

/// One `context_management` entry (port of `ContextManagement`, camelCase
/// provider options).
#[derive(Debug, Clone, PartialEq)]
pub struct ContextManagementEntry {
    pub compact_threshold: Option<i64>,
}

/// Validate `providerOptions.contextManagement` (port of the
/// `UndefinedOr(ContextManagement)` decode).
pub fn context_management(
    request: &LLMRequest,
) -> Result<Option<Vec<ContextManagementEntry>>, AiError> {
    let Some(raw) =
        request.provider_options.as_ref().and_then(|options| options.get("contextManagement"))
    else {
        return Ok(None);
    };
    let Some(entries) = raw.as_array() else {
        return Err(crate::protocols::shared::invalid_request(
            "OpenAI Responses context management options are invalid",
        ));
    };
    let mut parsed = Vec::new();
    for entry in entries {
        if entry.get("type").and_then(Value::as_str) != Some("compaction") {
            return Err(crate::protocols::shared::invalid_request(
                "OpenAI Responses context management options are invalid",
            ));
        }
        let compact_threshold = match entry.get("compactThreshold") {
            None => None,
            Some(value) => {
                let number = value.as_f64().filter(|number| {
                    number.fract() == 0.0 && *number > 0.0 && *number <= i64::MAX as f64
                });
                match number {
                    Some(number) => Some(number as i64),
                    None => {
                        return Err(crate::protocols::shared::invalid_request(
                            "OpenAI Responses context management options are invalid",
                        ))
                    }
                }
            }
        };
        parsed.push(ContextManagementEntry { compact_threshold });
    }
    Ok(Some(parsed))
}

/// Build the `/compact` request body: native conversation + the generation
/// controls the endpoint accepts + caller `http.body` overlay (port of the
/// body assembly in `ResponsesCompaction.make`).
pub fn compact_body(
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
    tools: Option<Value>,
) -> Result<(Value, Value), AiError> {
    let stripped = crate::effort_updates::strip_effort_updates(request);
    let native = lower_conversation(&stripped, adapter)?;
    let generation = lower_generation_for(request);
    let mut body: Map<String, Value> = native.clone();
    for field in
        ["service_tier", "reasoning", "text", "include", "parallel_tool_calls", "prompt_cache_key"]
    {
        if let Some(value) = generation.get(field) {
            body.insert(field.to_string(), value.clone());
        }
    }
    if let Some(tools) = tools {
        body.insert("tools".into(), tools);
    }
    if let Some(overlay) = request.http.as_ref().and_then(|http| http.body.as_ref()) {
        if let Some(merged) =
            crate::schema::options::merge_json_records(&[Some(&body), Some(overlay)])
        {
            body = merged;
        }
    }
    validate_compaction_body(&Value::Object(body.clone())).map_err(|message| {
        crate::protocols::shared::invalid_request(format!(
            "OpenAI Responses compaction request body is invalid: {message}"
        ))
    })?;
    Ok((Value::Object(body), Value::Object(native)))
}

/// Field-type validation of the `/compact` wire contract (a light port of
/// the `Body` schema decode; error text is local).
fn validate_compaction_body(body: &Value) -> Result<(), String> {
    let object = body.as_object().ok_or("expected an object")?;
    if !object.get("model").is_some_and(Value::is_string) {
        return Err("model must be a string".into());
    }
    if !object.get("input").is_some_and(Value::is_array) {
        return Err("input must be an array".into());
    }
    for field in [
        "instructions",
        "previous_response_id",
        "service_tier",
        "prompt_cache_key",
        "prompt_cache_retention",
    ] {
        if let Some(value) = object.get(field) {
            if !value.is_string() && !value.is_null() {
                return Err(format!("{field} must be a string or null"));
            }
        }
    }
    for field in ["reasoning", "text", "prompt_cache_options"] {
        if let Some(value) = object.get(field) {
            if !value.is_object() {
                return Err(format!("{field} must be an object"));
            }
        }
    }
    if let Some(value) = object.get("include") {
        if !value.as_array().is_some_and(|items| items.iter().all(Value::is_string)) {
            return Err("include must be an array of strings".into());
        }
    }
    if let Some(tools) = object.get("tools") {
        if !tools.as_array().is_some_and(|items| items.iter().all(Value::is_object)) {
            return Err("tools must be an array of objects".into());
        }
    }
    if let Some(parallel) = object.get("parallel_tool_calls") {
        if !parallel.is_boolean() {
            return Err("parallel_tool_calls must be a boolean".into());
        }
    }
    Ok(())
}

/// Execute the compaction request against `/{path}/compact` on the route's
/// transport (port of `ResponsesCompaction.make`'s execution half).
pub async fn execute_compaction(
    route: &Route,
    request: &LLMRequest,
    body: &Value,
    native: &Value,
) -> Result<CompactionResponse, AiError> {
    let mut compact_route = route.clone();
    compact_route.endpoint = compact_endpoint(route, request, native);
    // The overlay is already merged into `body`; don't apply it twice.
    let stripped = strip_http_body(request);
    let (transport_request, _body_text) = compact_route.prepare_transport(&stripped, body)?;
    let response = route
        .transport
        .execute(transport_request.clone())
        .await
        .map_err(crate::route::transport::TransportError::into_ai_error)?;
    if response.status >= 400 {
        let body_text =
            read_body_limited(response.body, executor::MAX_BODY_CHARS).await.unwrap_or_default();
        let message = executor::provider_message(response.status, Some(&body_text));
        let response_headers: Vec<(String, String)> =
            response.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        return Err(executor::http_failure(executor::HttpFailureInput {
            message: &message,
            url: Some(&transport_request.url),
            status: Some(response.status),
            data: None,
            response_headers: &response_headers,
            response_body: Some(&body_text),
        }));
    }
    let text = read_body_limited(response.body, executor::MAX_BODY_CHARS).await.unwrap_or_default();
    parse_compaction_response(&text, route, request)
}

fn strip_http_body(request: &LLMRequest) -> LLMRequest {
    let Some(http) = request.http.as_ref() else {
        return request.clone();
    };
    request.clone().update(crate::schema::LLMRequestPatch {
        http: Some(crate::schema::HttpOptions { body: None, ..http.clone() }),
        ..Default::default()
    })
}

/// Parse a `response.compaction` document into replacement messages + usage
/// (port of the response decode + `toMessage`).
pub fn parse_compaction_response(
    text: &str,
    route: &Route,
    request: &LLMRequest,
) -> Result<CompactionResponse, AiError> {
    let invalid = |message: &str| invalid_output(&route.id, message, text);
    let document: Value = serde_json::from_str(text)
        .map_err(|cause| invalid(&format!("Invalid compaction response: {cause}")))?;
    if document.get("object").and_then(Value::as_str) != Some("response.compaction") {
        return Err(invalid("Invalid compaction response"));
    }
    let Some(output) = document.get("output").and_then(Value::as_array) else {
        return Err(invalid("Invalid compaction response"));
    };
    if !output.iter().any(|item| item.get("type").and_then(Value::as_str) == Some("compaction")) {
        return Err(invalid("Compaction response did not contain a checkpoint"));
    }
    let key = metadata_key(request);
    let replacement = output
        .iter()
        .map(|item| to_message(item, request, &key))
        .collect::<Result<Vec<_>, AiError>>()?;
    Ok(CompactionResponse {
        replacement,
        usage: map_usage(document.get("usage").filter(|value| !value.is_null()), &key),
    })
}

/// `{key: {fields}}` provider metadata from a JSON object value.
fn provider_meta(key: &str, value: Value) -> ProviderMetadata {
    let fields: std::collections::BTreeMap<String, Value> =
        serde_json::from_value(value).unwrap_or_default();
    std::iter::once((key.to_string(), fields)).collect()
}

/// Map one compaction output item to a replacement message (port of
/// `toMessage`).
#[allow(clippy::unnecessary_wraps)] // keeps the output mapping uniform with the erroring branches
fn to_message(item: &Value, request: &LLMRequest, key: &str) -> Result<Message, AiError> {
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    if kind == "compaction" {
        return Ok(Message::assistant(vec![ContentPart::Compaction {
            provider: request.model.provider.clone(),
            id: item.get("id").and_then(Value::as_str).map(str::to_string),
            encrypted: Some(
                item.get("encrypted_content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ),
            text: None,
        }]));
    }

    if kind == "reasoning" {
        let id = item.get("id").and_then(Value::as_str).map(str::to_string);
        let encrypted = item.get("encrypted_content").and_then(Value::as_str).map(str::to_string);
        let empty = SummaryPart { text: String::new() };
        let summary: Vec<SummaryPart> = item
            .get("summary")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .map(|part| SummaryPart {
                        text: part
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                    .collect()
            })
            .filter(|parts: &Vec<SummaryPart>| !parts.is_empty())
            .unwrap_or_else(|| vec![empty]);
        return Ok(Message::assistant(
            summary
                .into_iter()
                .map(|part| ContentPart::Reasoning {
                    text: part.text,
                    encrypted: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: Some(provider_meta(
                        key,
                        json!({
                            "itemId": id,
                            "reasoningEncryptedContent": encrypted
                                .clone()
                                .map_or(Value::Null, Value::from),
                        }),
                    )),
                })
                .collect(),
        ));
    }

    if kind != "message" {
        // Unknown item types are not part of the /compact contract.
        return Ok(Message::new(MessageRole::User, Vec::new()));
    }

    let role = match item.get("role").and_then(Value::as_str) {
        Some("assistant") => MessageRole::Assistant,
        _ => MessageRole::User,
    };
    let item_metadata = provider_meta(
        key,
        json!({
            "itemId": item.get("id").cloned().unwrap_or(Value::Null),
            "type": kind,
            "status": item.get("status").cloned().unwrap_or(Value::Null),
            "phase": item.get("phase").cloned().unwrap_or(Value::Null),
        }),
    );
    let mut content = Vec::new();
    let empty_content = Vec::new();
    let item_content = item.get("content").and_then(Value::as_array).unwrap_or(&empty_content);
    for part in item_content {
        let part_kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
        if part_kind == "input_text" || part_kind == "output_text" {
            content.push(ContentPart::text(
                part.get("text").and_then(Value::as_str).unwrap_or_default(),
            ));
            continue;
        }
        let detail = part.get("detail").and_then(Value::as_str).map(str::to_string);
        let part_metadata = detail.map(|detail| provider_meta(key, json!({ "detail": detail })));
        if part_kind == "input_image" {
            content.push(ContentPart::Media {
                media: replay_media(
                    part.get("image_url").and_then(Value::as_str).unwrap_or_default(),
                    "image/*",
                ),
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: part_metadata,
            });
            continue;
        }
        let file_url = part.get("file_url").and_then(Value::as_str);
        let file_data = part.get("file_data").and_then(Value::as_str);
        content.push(ContentPart::Media {
            media: replay_media(
                file_url.or(file_data).unwrap_or_default(),
                "application/octet-stream",
            ),
            filename: part.get("filename").and_then(Value::as_str).map(str::to_string),
            cache: None,
            metadata: None,
            provider_metadata: part_metadata,
        });
    }
    Ok(Message {
        id: None,
        role,
        content,
        metadata: None,
        provider_metadata: Some(item_metadata),
        native: None,
    })
}

struct SummaryPart {
    text: String,
}

/// Replayed compaction items carry either a data URL or a remote URL; the
/// data URL's own type wins when present (port of `replayMedia`).
fn replay_media(value: &str, fallback_type: &str) -> media::Asset {
    if let Some(asset) = media::parse_data_url(value) {
        return asset;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return media::url(value, Some(fallback_type.to_string()), None);
    }
    media::base64(value, fallback_type)
}

/// Convenience: build + execute a compaction call in one step.
pub async fn compact(
    route: &Route,
    request: &LLMRequest,
    adapter: &dyn ProviderAdapter,
    tools: Option<Value>,
) -> Result<CompactionResponse, AiError> {
    let (body, native) = compact_body(request, adapter, tools)?;
    execute_compaction(route, request, &body, &native).await
}
