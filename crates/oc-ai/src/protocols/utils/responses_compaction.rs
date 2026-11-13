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
        // Full (safety-capped) body for classification; display copy is
        // truncated by `provider_message` (G-346).
        let body_text = read_body_limited(response.body, executor::MAX_ERROR_BODY_CHARS)
            .await
            .unwrap_or_default();
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
    // The ref folds the whole decoded text stream with no cap; keep a
    // generous safety limit (compaction bodies carry large encrypted blobs).
    let text =
        read_body_limited(response.body, executor::MAX_ERROR_BODY_CHARS).await.unwrap_or_default();
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
    let route_id = route.id.clone();
    let replacement = output
        .iter()
        .map(|item| to_message(item, request, &key, &route_id, text))
        .collect::<Result<Vec<_>, AiError>>()?;
    // G-400: `usage` is `Schema.optional(StructWithRest)` in the ref —
    // absent is fine, but explicit null (or a non-object) fails the
    // decode rather than being treated as "no usage".
    let usage = match document.get("usage") {
        None => None,
        Some(value) if value.is_object() => map_usage(Some(value), &key),
        Some(_) => {
            return Err(invalid("Invalid compaction response: usage must be an object"));
        }
    };
    Ok(CompactionResponse { replacement, usage })
}

/// `{key: {fields}}` provider metadata from a JSON object value.
fn provider_meta(key: &str, value: Value) -> ProviderMetadata {
    let fields: std::collections::BTreeMap<String, Value> =
        serde_json::from_value(value).unwrap_or_default();
    std::iter::once((key.to_string(), fields)).collect()
}

/// Map one compaction output item to a replacement message (port of
/// `toMessage`).
///
/// Strict per the ref's `Response` schema decode (G-357): a malformed output
/// item fails the whole compaction with an `InvalidProviderOutput` error
/// instead of silently defaulting. Required surface (mirroring the ref
/// schemas — `CompactionItem`, `OpenResponsesReasoningItem`, and the
/// `message` variants of the output union):
/// - `compaction`: `encrypted_content` is a required string;
/// - `reasoning`: `summary` is a required array of `{type:"summary_text",
///   text:string}` parts (an empty array replays one empty summary);
/// - `message`: `role` must be `user` or `assistant`; `content` is a
///   required non-empty array; assistant content is text-only, user content
///   may add `input_image` (required `image_url`) and `input_file`
///   (required `filename` plus exactly one of `file_url`/`file_data`);
/// - unknown item types are rejected.
fn to_message(
    item: &Value,
    request: &LLMRequest,
    key: &str,
    route_id: &str,
    text: &str,
) -> Result<Message, AiError> {
    let invalid = |message: &str| invalid_output(route_id, message, text);
    let item = item
        .as_object()
        .ok_or_else(|| invalid("Invalid compaction response: output item is not an object"))?;
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    if kind == "compaction" {
        let Some(encrypted) = item.get("encrypted_content").and_then(Value::as_str) else {
            return Err(invalid(
                "Invalid compaction response: compaction item requires encrypted_content",
            ));
        };
        return Ok(Message::assistant(vec![ContentPart::Compaction {
            provider: request.model.provider.clone(),
            id: item.get("id").and_then(Value::as_str).map(str::to_string),
            encrypted: Some(encrypted.to_string()),
            text: None,
        }]));
    }

    if kind == "reasoning" {
        // `id` is `optionalKey(String)` in the ref (open-responses.ts:85):
        // absent is fine, but explicit null (or any non-string) fails the
        // decode (G-400) — unlike `encrypted_content`, which is
        // optionalNull and accepts null (corpus v08).
        let id = match item.get("id") {
            None => None,
            Some(value) => Some(value.as_str().map(str::to_string).ok_or_else(|| {
                invalid("Invalid compaction response: reasoning item id must be a string")
            })?),
        };
        let encrypted = item.get("encrypted_content").and_then(Value::as_str).map(str::to_string);
        let empty = SummaryPart { text: String::new() };
        let parts = item.get("summary").and_then(Value::as_array).ok_or_else(|| {
            invalid("Invalid compaction response: reasoning item requires a summary array")
        })?;
        let mut summary: Vec<SummaryPart> = Vec::with_capacity(parts.len());
        for part in parts {
            if part.get("type").and_then(Value::as_str) != Some("summary_text") {
                return Err(invalid(
                    "Invalid compaction response: reasoning summary requires summary_text parts",
                ));
            }
            let Some(part_text) = part.get("text").and_then(Value::as_str) else {
                return Err(invalid(
                    "Invalid compaction response: reasoning summary requires text",
                ));
            };
            summary.push(SummaryPart { text: part_text.to_string() });
        }
        if summary.is_empty() {
            summary.push(empty);
        }
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
        return Err(invalid("Invalid compaction response: unknown output item type"));
    }

    let role = match item.get("role").and_then(Value::as_str) {
        Some("assistant") => MessageRole::Assistant,
        Some("user") => MessageRole::User,
        _ => {
            return Err(invalid(
                "Invalid compaction response: message role must be user or assistant",
            ))
        }
    };
    if let Some(phase) = item.get("phase") {
        let legal =
            phase.is_null() || matches!(phase.as_str(), Some("commentary" | "final_answer"));
        if !legal {
            return Err(invalid("Invalid compaction response: invalid message phase"));
        }
    }
    // G-400: `id`/`status` are `Schema.optional(String)` in the ref
    // (responses-compaction.ts:54-59) — absent is fine, but a present
    // non-string (number, explicit null) fails the decode instead of
    // being cloned into provider metadata verbatim.
    for field in ["id", "status"] {
        if let Some(value) = item.get(field) {
            if !value.is_string() {
                return Err(invalid(&format!(
                    "Invalid compaction response: message item {field} must be a string"
                )));
            }
        }
    }
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
    let item_content = item
        .get("content")
        .and_then(Value::as_array)
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| {
            invalid("Invalid compaction response: message requires a non-empty content array")
        })?;
    for part in item_content {
        let part_kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
        if part_kind == "input_text" || part_kind == "output_text" {
            let Some(part_text) = part.get("text").and_then(Value::as_str) else {
                return Err(invalid("Invalid compaction response: text part requires text"));
            };
            content.push(ContentPart::text(part_text));
            continue;
        }
        // Assistant message content is text-only in the ref union.
        if role == MessageRole::Assistant {
            return Err(invalid(
                "Invalid compaction response: assistant message content must be text",
            ));
        }
        // G-417: `detail` is `Schema.optional(String)` in the ref
        // (open-responses.ts:42,47) — absent is fine, but a present
        // non-string fails the whole decode instead of being treated as
        // absent.
        if let Some(detail_value) = part.get("detail") {
            if !detail_value.is_string() {
                return Err(invalid(
                    "Invalid compaction response: media part detail must be a string",
                ));
            }
        }
        let detail = part.get("detail").and_then(Value::as_str).map(str::to_string);
        let part_metadata = detail.map(|detail| provider_meta(key, json!({ "detail": detail })));
        if part_kind == "input_image" {
            let Some(image_url) = part.get("image_url").and_then(Value::as_str) else {
                return Err(invalid("Invalid compaction response: input_image requires image_url"));
            };
            content.push(ContentPart::Media {
                media: Box::new(replay_media(image_url, "image/*")),
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: part_metadata,
            });
            continue;
        }
        let file_url = part.get("file_url").and_then(Value::as_str);
        let file_data = part.get("file_data").and_then(Value::as_str);
        let source = match (file_url, file_data) {
            (Some(url), None) => url,
            (None, Some(data)) => data,
            _ => {
                return Err(invalid(
                    "Invalid compaction response: input_file requires exactly one of file_url or file_data",
                ))
            }
        };
        let Some(filename) = part.get("filename").and_then(Value::as_str) else {
            return Err(invalid("Invalid compaction response: input_file requires filename"));
        };
        content.push(ContentPart::Media {
            media: Box::new(replay_media(source, "application/octet-stream")),
            filename: Some(filename.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(document: &Value) -> Result<CompactionResponse, AiError> {
        let route = crate::test_support::static_route_for_tests();
        let request = crate::test_support::request_for_tests(vec![]);
        parse_compaction_response(&serde_json::to_string(document).unwrap(), &route, &request)
    }

    fn checkpoint() -> Value {
        json!({ "type": "compaction", "id": "cmp_1", "encrypted_content": "enc" })
    }

    #[test]
    fn well_formed_compaction_still_succeeds() {
        let parsed = parse(&json!({
            "object": "response.compaction",
            "output": [
                checkpoint(),
                { "type": "reasoning", "id": "rs_1", "summary": [{ "type": "summary_text", "text": "Thought" }], "encrypted_content": "re" },
                { "type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
                  "content": [{ "type": "output_text", "text": "Summarized." }] },
            ],
            "usage": { "input_tokens": 10, "output_tokens": 2, "total_tokens": 12 },
        }))
        .unwrap();
        assert_eq!(parsed.replacement.len(), 3);
        assert_eq!(parsed.usage.unwrap().total_tokens, Some(12.0));
    }

    #[test]
    fn missing_encrypted_content_fails_the_compaction() {
        // G-357: no silent "" checkpoint — the ref schema requires the string.
        let error = parse(&json!({
            "object": "response.compaction",
            "output": [{ "type": "compaction", "id": "cmp_1" }],
        }))
        .unwrap_err();
        assert!(error.message().contains("requires encrypted_content"), "got {}", error.message());
    }

    #[test]
    fn unknown_item_type_fails_the_compaction() {
        let error = parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint(), { "type": "mystery" }],
        }))
        .unwrap_err();
        assert!(error.message().contains("unknown output item type"), "got {}", error.message());
    }

    #[test]
    fn unknown_or_missing_role_fails_the_compaction() {
        for role in [json!("system"), Value::Null] {
            let error = parse(&json!({
                "object": "response.compaction",
                "output": [checkpoint(), { "type": "message", "role": role, "content": [] }],
            }))
            .unwrap_err();
            assert!(
                error.message().contains("role must be user or assistant"),
                "got {}",
                error.message()
            );
        }
    }

    #[test]
    fn missing_or_empty_message_content_fails_the_compaction() {
        for content in [json!([]), Value::Null] {
            let error = parse(&json!({
                "object": "response.compaction",
                "output": [checkpoint(), { "type": "message", "role": "user", "content": content }],
            }))
            .unwrap_err();
            assert!(error.message().contains("non-empty content array"), "got {}", error.message());
        }
    }

    #[test]
    fn missing_reasoning_summary_fails_and_empty_summary_replays_one_part() {
        let error = parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint(), { "type": "reasoning" }],
        }))
        .unwrap_err();
        assert!(error.message().contains("summary array"), "got {}", error.message());

        let parsed = parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint(), { "type": "reasoning", "summary": [] }],
        }))
        .unwrap();
        assert_eq!(parsed.replacement[1].content.len(), 1);
    }

    // G-400 residuals: explicit null / wrong-typed fields the ref schema
    // rejects but the rust port used to launder into metadata or ignore.

    #[test]
    fn message_id_and_status_must_be_strings_when_present() {
        for (field, bad) in [("id", json!(5)), ("status", json!(7))] {
            let mut message = json!({ "type": "message", "role": "user",
                "content": [{ "type": "input_text", "text": "u" }] });
            message[field] = bad;
            let error = parse(&json!({
                "object": "response.compaction",
                "output": [checkpoint(), message],
            }))
            .unwrap_err();
            assert!(
                error.message().contains(&format!("{field} must be a string")),
                "got {}",
                error.message()
            );
        }
    }

    #[test]
    fn reasoning_id_null_is_rejected_but_encrypted_null_is_fine() {
        let error = parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint(), { "type": "reasoning", "id": null,
                "summary": [{ "type": "summary_text", "text": "T" }] }],
        }))
        .unwrap_err();
        assert!(error.message().contains("id must be a string"), "got {}", error.message());

        // v08 parity: encrypted_content null stays accepted (optionalNull).
        parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint(), { "type": "reasoning", "id": "rs",
                "summary": [{ "type": "summary_text", "text": "T" }],
                "encrypted_content": null }],
        }))
        .unwrap();
    }

    #[test]
    fn explicit_null_usage_is_rejected() {
        let error = parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint()],
            "usage": null,
        }))
        .unwrap_err();
        assert!(error.message().contains("usage must be an object"), "got {}", error.message());
    }

    #[test]
    fn media_part_detail_must_be_a_string_when_present() {
        // G-417: `detail` is Schema.optional(String) (open-responses.ts:42,47)
        // — a present non-string (e.g. 42) fails the whole decode instead of
        // being treated as absent.
        for part in [
            json!({ "type": "input_image", "image_url": "https://example.com/i.png", "detail": 42 }),
            json!({ "type": "input_file", "filename": "bad.pdf",
                "file_url": "https://example.com/report.pdf", "detail": 42 }),
        ] {
            let error = parse(&json!({
                "object": "response.compaction",
                "output": [checkpoint(), { "type": "message", "role": "user", "content": [part] }],
            }))
            .unwrap_err();
            assert!(error.message().contains("detail must be a string"), "got {}", error.message());
        }

        // Absent and string detail stay accepted.
        for detail in [None, Some(json!("low"))] {
            let mut part =
                json!({ "type": "input_image", "image_url": "https://example.com/i.png" });
            if let Some(detail) = detail {
                part["detail"] = detail;
            }
            parse(&json!({
                "object": "response.compaction",
                "output": [checkpoint(), { "type": "message", "role": "user", "content": [part] }],
            }))
            .unwrap();
        }
    }

    #[test]
    fn empty_payload_data_url_image_replays_with_empty_base64() {
        // G-405 (v14): data:image/png;base64, is a data URL with an empty
        // payload, not a whole-string base64 fallback.
        let parsed = parse(&json!({
            "object": "response.compaction",
            "output": [checkpoint(), { "type": "message", "id": "msg_4", "role": "user",
                "content": [{ "type": "input_image", "image_url": "data:image/png;base64," }] }],
        }))
        .unwrap();
        let ContentPart::Media { media, .. } = &parsed.replacement[1].content[0] else {
            panic!("expected media part");
        };
        assert_eq!(media.inline().unwrap().base64, "");
        assert_eq!(media.media_type, "image/png");
    }
}
