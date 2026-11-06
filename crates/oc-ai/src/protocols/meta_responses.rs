//! `meta-responses` (port of `packages/ai/src/protocols/
//! meta-responses.ts`).
//!
//! Meta speaks the Open Responses baseline with a Meta dialect: the
//! `native.meta` hosted tool vocabulary (`web_search` /
//! `image_generation`), a signed-ID replay transform for
//! provider-executed image results (the handle is replayed, not the
//! image bytes), base64 image results with format sniffing, message
//! annotations surfaced on `text-end` events, and terminal-only item
//! recovery: Muse Image delivers its image only in
//! `response.completed`, so completed items are recovered in order
//! without duplicating streamed ones (`completedItems` dedup).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::protocols::open_responses::{
    self, decode_event, normalize, ParserState, ProviderAdapter,
};
use crate::protocols::shared;
use crate::protocols::utils::responses_hosted_tools::{self, Definition, Item};
use crate::route::auth::{self, Definition as AuthDefinition, Secret};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{merge_route_defaults, Route, RouteDefaults, RouteInput};
use crate::schema::{AiError, LLMEvent, LLMRequest, ToolResultValue};

/// Stable protocol id (TS `adapter.id`).
pub const ADAPTER: &str = "meta-responses";
/// Display name (TS `adapter.name`).
pub const NAME: &str = "Meta Responses";
/// Endpoint path (`responsesRoute`, providers/meta.ts:89-98).
pub const PATH: &str = open_responses::PATH;
/// Baked Meta API root (providers/meta.ts:15).
pub const DEFAULT_BASE_URL: &str = "https://api.meta.ai/v1";

// =============================================================================
// Provider adapter (meta-responses.ts:12-59)
// =============================================================================

/// The `native.meta` tool vocabulary: `web_search`, `image_generation`.
const NATIVE_TOOL_TYPES: [&str; 2] = ["web_search", "image_generation"];

/// The `ImageItem` shape: `type === "image_generation_call"` with a string
/// `id` (status/result/output_format/error optional — light validation of
/// the TS `Schema.is(ImageItem)` guard).
fn is_image_item(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("image_generation_call")
        && value.get("id").and_then(Value::as_str).is_some()
}

/// The Meta adapter: validates `native.meta` hosted tools and restores
/// `image_generation_call` items for replay.
pub struct MetaResponsesAdapter;

impl ProviderAdapter for MetaResponsesAdapter {
    fn id(&self) -> &str {
        ADAPTER
    }
    fn name(&self) -> &str {
        NAME
    }
    fn native_tool(&self, native: &BTreeMap<String, Value>) -> Option<Result<Value, AiError>> {
        let meta = native.get("meta")?;
        let invalid = || {
            shared::invalid_request(format!(
                "{NAME} native tool must be one of {NATIVE_TOOL_TYPES:?}"
            ))
        };
        Some(match meta {
            Value::Object(tool)
                if tool
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| NATIVE_TOOL_TYPES.contains(&kind)) =>
            {
                Ok(meta.clone())
            }
            _ => Err(invalid()),
        })
    }
    fn restore_hosted_tool_item(&self, item: &Value) -> Option<Value> {
        is_image_item(item).then(|| item.clone())
    }
}

// =============================================================================
// Hosted tool definitions (meta-responses.ts:93-137)
// =============================================================================

fn action_input(item: &Item) -> Value {
    item.field("action").cloned().unwrap_or(json!({}))
}

/// The `image_generation_call` result (meta-responses.ts:98-135): strict
/// item decode, error passthrough, mandatory base64 data, and mime
/// sniffing when `output_format` is omitted.
fn image_generation_result(item: &Item) -> Result<ToolResultValue, AiError> {
    if !is_image_item(&item.raw) {
        return Err(shared::event_error(ADAPTER, "Meta returned an invalid image item"));
    }
    if let Some(error) = item.field("error").filter(|error| !error.is_null()) {
        return Ok(ToolResultValue::Error { value: error.clone() });
    }
    let Some(result) = item.string_field("result") else {
        return Err(shared::event_error(ADAPTER, "Meta returned an image without data"));
    };
    // Strict base64 decode (the decoded bytes drive the mime sniff below).
    let Some(data) = responses_hosted_tools::decode_base64(result) else {
        return Err(shared::event_error(ADAPTER, "Meta returned invalid image base64"));
    };
    // Responses image items can omit output_format, including when
    // PNG/JPEG was requested.
    let mime = match item.string_field("output_format") {
        Some(format) => format!("image/{format}"),
        None => crate::media::utils::detect_media_type(&data)
            .unwrap_or("application/octet-stream")
            .to_string(),
    };
    Ok(ToolResultValue::Content {
        value: crate::schema::ToolContentItems(vec![json!({
            "type": "file",
            "uri": format!("data:{mime};base64,{result}"),
            "mime": mime,
        })]),
    })
}

/// The hosted tool table (`HOSTED_TOOLS`).
pub static HOSTED_TOOLS: &[(&str, Definition)] = &[
    ("web_search_call", Definition { name: "web_search", input: action_input, result: None }),
    (
        "image_generation_call",
        Definition {
            name: "image_generation",
            input: |_| json!({}),
            result: Some(image_generation_result),
        },
    ),
];

// =============================================================================
// Request lowering (meta-responses.ts:61-91)
// =============================================================================

/// The signed-ID replay transform (meta-responses.ts:66-86): a
/// provider-executed `image_generation` tool result whose provider
/// metadata `itemId` matches the part id is replaced by the raw item
/// handle — Meta's signed image ID carries edit state, so the handle is
/// replayed instead of the image bytes as a user message.
fn replay_image_handles(request: &LLMRequest) -> LLMRequest {
    use crate::schema::{ContentPart, ToolResultValue as ResultValue};
    let key = open_responses::metadata_key(request);
    let mut next = request.clone();
    next.messages = request
        .messages
        .iter()
        .map(|message| {
            let mut message = message.clone();
            message.content = message
                .content
                .iter()
                .map(|part| {
                    let ContentPart::ToolResult {
                        id,
                        name,
                        provider_executed: Some(true),
                        result: ResultValue::Content { .. },
                        provider_metadata,
                        ..
                    } = part
                    else {
                        return part.clone();
                    };
                    if name != "image_generation" {
                        return part.clone();
                    }
                    let item_id = provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get(&key))
                        .and_then(|fields| fields.get("itemId"))
                        .and_then(Value::as_str);
                    if item_id != Some(id.as_str()) {
                        return part.clone();
                    }
                    let mut replaced = part.clone();
                    if let ContentPart::ToolResult { result, .. } = &mut replaced {
                        *result = ResultValue::Json {
                            value: json!({
                                "type": "image_generation_call",
                                "id": id,
                                "status": "completed",
                                "result": null,
                            }),
                        };
                    }
                    replaced
                })
                .collect();
            message
        })
        .collect();
    next
}

/// `fromRequest`: the signed-ID replay transform, then the Open Responses
/// baseline with the Meta adapter.
pub fn from_request(request: &LLMRequest) -> Result<Value, AiError> {
    open_responses::from_request_with_adapter(&replay_image_handles(request), &MetaResponsesAdapter)
}

// =============================================================================
// Protocol (meta-responses.ts:139-210)
// =============================================================================

/// Parser state (port of `ParserState extends OpenResponses.ParserState`):
/// the baseline state plus the `completedItems` dedup set.
#[derive(Debug, Clone)]
pub struct MetaParserState {
    pub base: ParserState,
    pub completed_items: BTreeSet<String>,
}

/// The `meta-responses` protocol: the Open Responses baseline plus the
/// hosted tool item completion (web_search / image_generation), message
/// annotations on text-end events, and terminal-only item recovery.
pub struct MetaResponsesProtocol;

impl MetaResponsesProtocol {
    /// `onEvent` (meta-responses.ts:139-174): the hosted-tool
    /// `output_item.done` branch runs before the baseline step; message
    /// items get their annotations attached to `text-end` events.
    fn on_event(
        &self,
        state: &ParserState,
        normalized: open_responses::NormalizedEvent,
    ) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
        if normalized.event.kind == "response.output_item.done" {
            if let Some(item) = normalized.item.as_ref() {
                if responses_hosted_tools::is_item(&item.kind, HOSTED_TOOLS) {
                    let hosted = Item {
                        id: item.id.clone().unwrap_or_default(),
                        kind: item.kind.clone(),
                        raw: item.raw.clone(),
                    };
                    return responses_hosted_tools::on_done(state, &hosted, HOSTED_TOOLS);
                }
            }
        }
        let is_message_done = normalized.event.kind == "response.output_item.done"
            && normalized.item.as_ref().is_some_and(|item| item.kind == "message");
        let message_item = is_message_done.then(|| normalized.item.clone().expect("checked above"));
        let (next, mut events) = open_responses::step(state, normalized)?;
        let Some(item) = message_item else {
            return Ok((next, events));
        };
        let annotations = message_annotations(&item).ok_or_else(|| {
            shared::event_error(ADAPTER, "Meta returned invalid message annotations")
        })?;
        if annotations.is_empty() {
            return Ok((next, events));
        }
        let key = state.provider_metadata_key.clone();
        for event in &mut events {
            if let LLMEvent::TextEnd { provider_metadata, .. } = event {
                let mut metadata = provider_metadata.take().unwrap_or_default();
                let fields = metadata.entry(key.clone()).or_default();
                fields.insert("annotations".to_string(), Value::Array(annotations.clone()));
                *provider_metadata = Some(metadata);
            }
        }
        Ok((next, events))
    }

    /// `step` (meta-responses.ts:176-199): duplicate `output_item.done`
    /// events are skipped, and `response.completed` recovers terminal-only
    /// items in order — Muse Image delivers its image and optional summary
    /// only in the terminal snapshot.
    pub fn step_frame(
        &self,
        state: &mut MetaParserState,
        frame: &str,
    ) -> Result<(Vec<LLMEvent>, bool), AiError> {
        let event = decode_event(frame)?;
        let is_terminal = open_responses::terminal(&event);
        let mut completed = state.completed_items.clone();
        let normalized = normalize(&mut state.base, event.clone());
        if normalized.event.kind == "response.output_item.done"
            && normalized
                .item
                .as_ref()
                .and_then(|item| item.id.as_ref())
                .is_some_and(|id| completed.contains(id))
        {
            return Ok((Vec::new(), is_terminal));
        }
        let mut events: Vec<LLMEvent> = Vec::new();
        if normalized.event.kind == "response.completed" {
            for (index, raw) in event.response_output().iter().enumerate() {
                let done = normalize(
                    &mut state.base,
                    open_responses::Event {
                        kind: "response.output_item.done".to_string(),
                        raw: json!({
                            "type": "response.output_item.done",
                            "item": raw,
                            "output_index": index,
                        }),
                    },
                );
                // Spark changes reasoning IDs in the terminal snapshot;
                // output indices still identify the streamed items.
                let Some(id) = done.item.as_ref().and_then(|item| item.id.clone()) else {
                    continue;
                };
                if completed.contains(&id)
                    || state
                        .base
                        .output_items
                        .get(&(index as u64))
                        .is_some_and(|previous| completed.contains(previous))
                {
                    continue;
                }
                let (next, emitted) = self.on_event(&state.base, done)?;
                state.base = next;
                events.extend(emitted);
                completed.insert(id);
            }
        }
        let (next, emitted) = self.on_event(&state.base, normalized)?;
        state.base = next;
        events.extend(emitted);
        if event.kind == "response.output_item.done" {
            if let Some(id) = event.item().and_then(|item| item.id) {
                completed.insert(id);
            }
        }
        state.completed_items = completed;
        Ok((events, is_terminal))
    }
}

/// The `MessageAnnotations` decode (meta-responses.ts:46-48, 148-158):
/// `content` is an array of objects whose optional `annotations` are
/// arrays of JSON objects; anything else is invalid.
fn message_annotations(item: &open_responses::StreamItem) -> Option<Vec<Value>> {
    let content = item.raw.get("content")?.as_array()?;
    let mut annotations = Vec::new();
    for part in content {
        if !part.is_object() {
            return None;
        }
        let Some(value) = part.get("annotations").filter(|value| !value.is_null()) else {
            continue;
        };
        let list = value.as_array()?;
        if list.iter().all(Value::is_object) {
            annotations.extend(list.iter().cloned());
        } else {
            return None;
        }
    }
    Some(annotations)
}

impl Protocol for MetaResponsesProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(MetaParserState {
            base: ParserState::new(request, &MetaResponsesAdapter),
            completed_items: BTreeSet::new(),
        })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let mut state = *state.downcast::<MetaParserState>().expect("Meta Responses parser state");
        let (events, _) = self.step_frame(&mut state, frame)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        decode_event(frame).is_ok_and(|event| open_responses::terminal(&event))
    }
}

// =============================================================================
// Route factory (providers/meta.ts:89-98)
// =============================================================================

/// Route defaults for the `meta-responses` route (port of
/// `defaults: { providerOptions: { store: false, include:
/// ["reasoning.encrypted_content"] } }`).
#[must_use]
pub fn default_route_defaults() -> RouteDefaults {
    RouteDefaults {
        headers: None,
        generation: None,
        provider_options: Some(
            [
                ("store".to_string(), json!(false)),
                ("include".to_string(), json!(["reasoning.encrypted_content"])),
            ]
            .into_iter()
            .collect(),
        ),
        http: None,
    }
}

/// Meta-facade auth (port of `AuthOptions.bearer(options, "META_API_KEY")`):
/// optional explicit key with the `META_API_KEY` fallback.
#[must_use]
pub fn meta_auth(api_key: Option<Secret>) -> AuthDefinition {
    auth::or_else(&auth::optional(api_key), &auth::config("META_API_KEY")).bearer()
}

/// Inputs for [`meta_responses_route`].
pub struct MetaResponsesRouteSpec {
    pub base_url: Option<String>,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
    /// Caller route defaults merged over the Meta defaults.
    pub defaults: RouteDefaults,
}

/// Build the `meta-responses` route (port of `responsesRoute`: the Meta
/// provider id/metadata key at `<base>/responses`, SSE framing — Meta
/// Responses does not support WebSocket upgrades).
#[must_use]
pub fn meta_responses_route(spec: MetaResponsesRouteSpec) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("meta".to_string()),
        provider_metadata_key: Some("meta".to_string()),
        protocol: Arc::new(MetaResponsesProtocol),
        endpoint: Endpoint::path(PATH)
            .with_base_url(spec.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string())),
        auth: spec.auth,
        framing: Framing::sse(),
        headers: None,
        transport: spec.transport,
        defaults: merge_route_defaults(Some(&default_route_defaults()), &spec.defaults),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::transport::FailingTransport;
    use crate::schema::{ContentPart, Message, ProviderMetadata, ToolContentItems};

    fn request() -> LLMRequest {
        let mut request =
            crate::test_support::request_for_tests(vec![crate::schema::Message::user("hi")]);
        request.provider_options = None;
        request
    }

    fn route() -> Route {
        meta_responses_route(MetaResponsesRouteSpec {
            base_url: None,
            auth: Some(meta_auth(Some(Secret::Value("m-1".into())))),
            transport: Arc::new(FailingTransport { message: "unit" }),
            defaults: RouteDefaults::default(),
        })
    }

    #[test]
    fn native_meta_tools_lower_and_validate() {
        let adapter = MetaResponsesAdapter;
        let native = BTreeMap::from([(
            "meta".to_string(),
            json!({ "type": "web_search", "search_context_size": "high" }),
        )]);
        assert_eq!(
            adapter.native_tool(&native),
            Some(Ok(json!({ "type": "web_search", "search_context_size": "high" })))
        );
        let image = BTreeMap::from([(
            "meta".to_string(),
            json!({ "type": "image_generation", "size": "1024x1024" }),
        )]);
        assert_eq!(
            adapter.native_tool(&image),
            Some(Ok(json!({ "type": "image_generation", "size": "1024x1024" })))
        );
        let bad = BTreeMap::from([("meta".to_string(), json!({ "type": "computer_use" }))]);
        assert!(adapter.native_tool(&bad).unwrap().is_err());
        // other namespaces are not ours
        assert!(adapter
            .native_tool(&BTreeMap::from([(
                "alibaba".to_string(),
                json!({ "type": "web_search" })
            )]))
            .is_none());
    }

    #[test]
    fn image_items_restore_for_replay() {
        let adapter = MetaResponsesAdapter;
        assert!(adapter
            .restore_hosted_tool_item(&json!({
                "type": "image_generation_call", "id": "ig_1", "result": "AQID"
            }))
            .is_some());
        // foreign item types and missing ids never restore
        assert!(adapter
            .restore_hosted_tool_item(&json!({ "type": "web_search_call", "id": "ws_1" }))
            .is_none());
        assert!(adapter
            .restore_hosted_tool_item(&json!({ "type": "image_generation_call" }))
            .is_none());
    }

    #[test]
    fn signed_image_handles_replay_instead_of_bytes() {
        let mut request = request();
        // the fixture route carries no provider_metadata_key, so the
        // baseline `metadataKey` fallback ("openresponses") applies
        let key = open_responses::metadata_key(&request);
        let mut metadata = ProviderMetadata::new();
        metadata.insert(key, [("itemId".to_string(), json!("ig_1"))].into_iter().collect());
        request.messages.push(Message::assistant(vec![ContentPart::ToolResult {
            id: "ig_1".to_string(),
            name: "image_generation".to_string(),
            namespace: None,
            result: ToolResultValue::Content {
                value: ToolContentItems(vec![json!({
                    "type": "file",
                    "uri": "data:image/png;base64,AQID",
                    "mime": "image/png",
                })]),
            },
            provider_executed: Some(true),
            cache: None,
            metadata: None,
            provider_metadata: Some(metadata),
        }]));
        let body = from_request(&request).unwrap();
        let input = body["input"].as_array().unwrap();
        let replayed = input
            .iter()
            .find(|item| item["type"] == json!("image_generation_call"))
            .expect("image handle replayed");
        assert_eq!(replayed["id"], json!("ig_1"));
        assert_eq!(replayed["status"], json!("completed"));
        assert_eq!(replayed["result"], json!(null));
        // the image bytes never reach the wire as a user message
        assert!(!input.iter().any(|item| item.to_string().contains("data:image/png")));
    }

    #[test]
    fn hosted_search_and_image_complete() {
        let protocol = MetaResponsesProtocol;
        let mut state = MetaParserState {
            base: ParserState::new(&request(), &MetaResponsesAdapter),
            completed_items: BTreeSet::new(),
        };
        let search = json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": {
                "type": "web_search_call",
                "id": "ws_1",
                "status": "completed",
                "action": { "query": "meta ai news" }
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &search).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolCall { name, provider_executed: Some(true), input, .. }
                if name == "web_search" && input["query"] == json!("meta ai news")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult { name, provider_executed: Some(true), .. } if name == "web_search"
        )));

        // AQID decodes to bytes 01 02 03 — not a known magic, so the mime
        // falls back to application/octet-stream when output_format is
        // omitted; a PNG magic sniffs image/png.
        let png = "iVBORw0KGgo="; // PNG magic base64
        let image = json!({
            "type": "response.output_item.done",
            "output_index": 2,
            "item": {
                "type": "image_generation_call",
                "id": "ig_1",
                "status": "completed",
                "result": png,
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &image).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult {
                name,
                result: ToolResultValue::Content { value },
                ..
            } if name == "image_generation"
                && value.0[0]["mime"] == json!("image/png")
                && value.0[0]["uri"] == json!(format!("data:image/png;base64,{png}"))
        )));

        // explicit output_format wins over sniffing
        let explicit = json!({
            "type": "response.output_item.done",
            "output_index": 3,
            "item": {
                "type": "image_generation_call",
                "id": "ig_2",
                "status": "completed",
                "result": png,
                "output_format": "webp",
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &explicit).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult {
                result: ToolResultValue::Content { value },
                ..
            } if value.0[0]["mime"] == json!("image/webp")
        )));

        // errors pass through; missing data fails typed
        let errored = json!({
            "type": "response.output_item.done",
            "output_index": 4,
            "item": {
                "type": "image_generation_call",
                "id": "ig_3",
                "status": "failed",
                "error": { "code": "safety" },
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &errored).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult {
                result: ToolResultValue::Error { value },
                ..
            } if value["code"] == json!("safety")
        )));

        let empty = json!({
            "type": "response.output_item.done",
            "output_index": 5,
            "item": { "type": "image_generation_call", "id": "ig_4", "status": "completed" }
        })
        .to_string();
        let error = protocol.step_frame(&mut state, &empty).unwrap_err();
        assert!(error.to_string().contains("image without data"));

        let invalid = json!({
            "type": "response.output_item.done",
            "output_index": 6,
            "item": { "type": "image_generation_call", "id": "ig_5", "result": "!!!" }
        })
        .to_string();
        let error = protocol.step_frame(&mut state, &invalid).unwrap_err();
        assert!(error.to_string().contains("invalid image base64"));
    }

    #[test]
    fn terminal_snapshot_recovers_and_dedups_items() {
        let protocol = MetaResponsesProtocol;
        let mut state = MetaParserState {
            base: ParserState::new(&request(), &MetaResponsesAdapter),
            completed_items: BTreeSet::new(),
        };
        // Muse Image: the image item arrives only in response.completed.
        let completed = json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "output": [
                    { "type": "message", "id": "msg_1", "status": "completed", "content": [
                        { "type": "output_text", "text": "here", "annotations": [
                            { "type": "url_citation", "url": "https://example.com" }
                        ] }
                    ] },
                    { "type": "image_generation_call", "id": "ig_9", "status": "completed", "result": "AQID" }
                ]
            }
        })
        .to_string();
        let (events, terminal) = protocol.step_frame(&mut state, &completed).unwrap();
        assert!(terminal);
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult { name, provider_executed: Some(true), .. }
                if name == "image_generation"
        )));
        // annotations ride the text-end event under the provider metadata key
        let key = open_responses::metadata_key(&request());
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::TextEnd { provider_metadata: Some(metadata), .. }
                if metadata.get(&key).and_then(|fields| fields.get("annotations"))
                    == Some(&json!([{ "type": "url_citation", "url": "https://example.com" }]))
        )));

        // replaying the same done item is a no-op (completedItems dedup)
        let duplicate = json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": { "type": "image_generation_call", "id": "ig_9", "status": "completed", "result": "AQID" }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &duplicate).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn invalid_message_annotations_fail_typed() {
        let protocol = MetaResponsesProtocol;
        let mut state = MetaParserState {
            base: ParserState::new(&request(), &MetaResponsesAdapter),
            completed_items: BTreeSet::new(),
        };
        let done = json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": { "type": "message", "id": "msg_1", "content": "not-an-array" }
        })
        .to_string();
        let error = protocol.step_frame(&mut state, &done).unwrap_err();
        assert!(error.to_string().contains("invalid message annotations"));
    }

    #[test]
    fn route_defaults_and_wire_shape() {
        let route = route();
        assert_eq!(route.id, "meta-responses");
        assert_eq!(route.provider_metadata_key.as_deref(), Some("meta"));
        let defaults = route.defaults.provider_options.as_ref().unwrap();
        assert_eq!(defaults.get("store"), Some(&json!(false)));
        assert_eq!(defaults.get("include"), Some(&json!(["reasoning.encrypted_content"])));
        let prepared = route.prepare_request(&request());
        let body = route.build_body(&prepared).unwrap();
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        let (wire, _) = route.prepare_transport(&prepared, &body).unwrap();
        assert_eq!(wire.url, "https://api.meta.ai/v1/responses");
        assert_eq!(wire.header("authorization"), Some("Bearer m-1"));
    }
}
