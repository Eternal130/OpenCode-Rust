//! `alibaba-responses` (port of `packages/ai/src/protocols/
//! alibaba-responses.ts`).
//!
//! Alibaba Model Studio speaks the Open Responses baseline with a Model
//! Studio dialect: `enable_thinking` / `previous_response_id` /
//! `conversation` request knobs, named tool selection expressed through
//! `allowed_tools`, and the hosted tool vocabulary `web_search` /
//! `web_extractor` / `code_interpreter` (`native.alibaba`).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::open_responses::{
    self, decode_event, normalize, ProviderAdapter, ParserState,
};
use crate::protocols::utils::responses_hosted_tools::{self, Definition, Item};
use crate::route::auth::Definition as AuthDefinition;
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{Route, RouteInput};
use crate::schema::{AiError, LLMEvent, LLMRequest};

/// Stable protocol id (TS `adapter.id`).
pub const ADAPTER: &str = "alibaba-responses";
/// Display name (TS `adapter.name`).
pub const NAME: &str = "Alibaba Responses";
/// Endpoint path (`responsesRoute`, providers/alibaba.ts:66-74).
pub const PATH: &str = open_responses::PATH;

// =============================================================================
// Provider adapter (alibaba-responses.ts:16-38)
// =============================================================================

/// The `native.alibaba` tool vocabulary: `web_search`, `web_extractor`,
/// `code_interpreter`.
const NATIVE_TOOL_TYPES: [&str; 3] = ["web_search", "web_extractor", "code_interpreter"];

/// The `WebExtractorItem` shape: `type === "web_extractor_call"` with a
/// string `id` (urls/goal optional, rest fields open).
fn is_web_extractor_item(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("web_extractor_call")
        && item.get("id").and_then(Value::as_str).is_some()
}

/// The Alibaba adapter: validates `native.alibaba` hosted tools and
/// restores `web_extractor_call` items for replay.
pub struct AlibabaResponsesAdapter;

impl ProviderAdapter for AlibabaResponsesAdapter {
    fn id(&self) -> &str {
        ADAPTER
    }
    fn name(&self) -> &str {
        NAME
    }
    fn native_tool(&self, native: &BTreeMap<String, Value>) -> Option<Result<Value, AiError>> {
        let alibaba = native.get("alibaba")?;
        let invalid = || {
            crate::protocols::shared::invalid_request(format!(
                "{NAME} native tool must be one of {NATIVE_TOOL_TYPES:?}"
            ))
        };
        Some(match alibaba {
            Value::Object(tool)
                if tool.get("type").and_then(Value::as_str).is_some_and(
                    |kind| NATIVE_TOOL_TYPES.contains(&kind),
                ) =>
            {
                Ok(alibaba.clone())
            }
            _ => Err(invalid()),
        })
    }
    fn restore_hosted_tool_item(&self, item: &Value) -> Option<Value> {
        is_web_extractor_item(item).then(|| item.clone())
    }
}

// =============================================================================
// Hosted tool definitions (alibaba-responses.ts:40-43, 84-86)
// =============================================================================

fn action_input(item: &Item) -> Value {
    item.field("action").cloned().unwrap_or(json!({}))
}

fn code_interpreter_input(item: &Item) -> Value {
    match item.field("code") {
        Some(code) => json!({ "code": code }),
        None => json!({}),
    }
}

/// The hosted tool table (`tools`).
pub static HOSTED_TOOLS: &[(&str, Definition)] = &[
    ("web_search_call", Definition { name: "web_search", input: action_input, result: None }),
    (
        "code_interpreter_call",
        Definition { name: "code_interpreter", input: code_interpreter_input, result: None },
    ),
];

/// The web-extractor `onDone` definition (`web_extractor_call`):
/// `{ urls, goal }` input from the completed item.
fn web_extractor_input(item: &Item) -> Value {
    let mut object = Map::new();
    for field in ["urls", "goal"] {
        if let Some(value) = item.field(field) {
            object.insert(field.to_string(), value.clone());
        }
    }
    Value::Object(object)
}

// =============================================================================
// Request lowering (alibaba-responses.ts:45-64)
// =============================================================================

/// The Alibaba provider options (`Options`): `enableThinking`,
/// `previousResponseId`, `conversation`.
#[derive(Debug, Default)]
struct AlibabaOptions {
    enable_thinking: Option<bool>,
    previous_response_id: Option<String>,
    conversation: Option<String>,
}

fn alibaba_options(request: &LLMRequest) -> AlibabaOptions {
    let empty = Map::new();
    let options = request.provider_options.as_ref().unwrap_or(&empty);
    AlibabaOptions {
        enable_thinking: options.get("enableThinking").and_then(Value::as_bool),
        previous_response_id: options
            .get("previousResponseId")
            .and_then(Value::as_str)
            .map(str::to_string),
        conversation: options
            .get("conversation")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Model Studio expresses named selection through `allowed_tools`:
/// `{type: "allowed_tools", mode: "required", tools: [choice]}` wrapping the
/// named `{type: "function", name}` choice (alibaba-responses.ts:58-62).
fn wrap_named_tool_choice(choice: Value) -> Value {
    let is_named = choice
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "function");
    if is_named {
        json!({ "type": "allowed_tools", "mode": "required", "tools": [choice] })
    } else {
        choice
    }
}

/// `fromRequest`: the Open Responses baseline with the Alibaba adapter,
/// plus the `enable_thinking` / `previous_response_id` / `conversation`
/// knobs and the named-tool `allowed_tools` transform.
pub fn from_request(request: &LLMRequest) -> Result<Value, AiError> {
    let options = alibaba_options(request);
    let mut body = open_responses::from_request_with_adapter(request, &AlibabaResponsesAdapter)?;
    if let Some(enable) = options.enable_thinking {
        body["enable_thinking"] = json!(enable);
    }
    if let Some(previous) = &options.previous_response_id {
        body["previous_response_id"] = json!(previous);
    }
    if let Some(conversation) = &options.conversation {
        body["conversation"] = json!(conversation);
    }
    if let Some(choice) = body.get("tool_choice").cloned() {
        body["tool_choice"] = wrap_named_tool_choice(choice);
    }
    Ok(body)
}

// =============================================================================
// Protocol (alibaba-responses.ts:66-93)
// =============================================================================

/// The `alibaba-responses` protocol: the Open Responses baseline plus the
/// hosted tool item completion (web_search / web_extractor /
/// code_interpreter).
pub struct AlibabaResponsesProtocol;

impl AlibabaResponsesProtocol {
    /// Drive the machine over one raw frame (mirrors the baseline's
    /// `step_frame`): the hosted-tool `output_item.done` branches run
    /// before the baseline step.
    pub fn step_frame(
        &self,
        state: &mut ParserState,
        frame: &str,
    ) -> Result<(Vec<LLMEvent>, bool), AiError> {
        let event = decode_event(frame)?;
        let is_terminal = open_responses::terminal(&event);
        let normalized = normalize(state, event);
        if normalized.event.kind == "response.output_item.done" {
            if let Some(item) = normalized.item.as_ref() {
                if item.kind == "web_extractor_call" {
                    if !is_web_extractor_item(&item.raw) {
                        return Err(crate::protocols::shared::event_error(
                            ADAPTER,
                            "Alibaba returned an invalid web extraction item",
                        ));
                    }
                    let hosted = Item {
                        id: item.id.clone().unwrap_or_default(),
                        kind: item.kind.clone(),
                        raw: item.raw.clone(),
                    };
                    let definitions: Vec<(&'static str, Definition)> = vec![(
                        "web_extractor_call",
                        Definition {
                            name: "web_extractor",
                            input: web_extractor_input,
                            result: None,
                        },
                    )];
                    let (next, events) =
                        responses_hosted_tools::on_done(state, &hosted, &definitions)?;
                    *state = next;
                    return Ok((events, is_terminal));
                }
                if responses_hosted_tools::is_item(&item.kind, HOSTED_TOOLS) {
                    let hosted = Item {
                        id: item.id.clone().unwrap_or_default(),
                        kind: item.kind.clone(),
                        raw: item.raw.clone(),
                    };
                    let (next, events) =
                        responses_hosted_tools::on_done(state, &hosted, HOSTED_TOOLS)?;
                    *state = next;
                    return Ok((events, is_terminal));
                }
            }
        }
        let (next, events) = open_responses::step(state, normalized)?;
        *state = next;
        Ok((events, is_terminal))
    }
}

impl Protocol for AlibabaResponsesProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState::new(request, &AlibabaResponsesAdapter))
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let mut state =
            *state.downcast::<ParserState>().expect("Alibaba Responses parser state");
        let (events, _) = self.step_frame(&mut state, frame)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        decode_event(frame).is_ok_and(|event| open_responses::terminal(&event))
    }
}

// =============================================================================
// Route factory (providers/alibaba.ts:66-74, 105)
// =============================================================================

/// Inputs for [`alibaba_responses_route`].
pub struct AlibabaResponsesRouteSpec {
    pub base_url: String,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
}

/// Build the `alibaba-responses` route (port of `responsesRoute`: the
/// Alibaba provider id/metadata key at `<base>/responses`, SSE framing).
#[must_use]
pub fn alibaba_responses_route(spec: AlibabaResponsesRouteSpec) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("alibaba".to_string()),
        provider_metadata_key: Some("alibaba".to_string()),
        protocol: Arc::new(AlibabaResponsesProtocol),
        endpoint: Endpoint::path(PATH).with_base_url(spec.base_url),
        auth: spec.auth,
        framing: Framing::sse(),
        headers: None,
        transport: spec.transport,
        defaults: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::transport::FailingTransport;
    use crate::schema::ToolChoice;

    fn request() -> LLMRequest {
        let mut request =
            crate::test_support::request_for_tests(vec![crate::schema::Message::user("hi")]);
        request.provider_options = None;
        request
    }

    fn route() -> Route {
        alibaba_responses_route(AlibabaResponsesRouteSpec {
            base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into(),
            auth: None,
            transport: Arc::new(FailingTransport { message: "unit" }),
        })
    }

    #[test]
    fn knobs_and_named_tool_choice_lower_into_the_body() {
        let mut request = request();
        request.provider_options = Some(
            [
                ("enableThinking".to_string(), json!(true)),
                ("previousResponseId".to_string(), json!("resp_1")),
                ("conversation".to_string(), json!("conv_9")),
            ]
            .into_iter()
            .collect(),
        );
        request.tool_choice = Some(ToolChoice::Named {
            name: "lookup".to_string(),
            disable_parallel_tool_use: Some(false),
        });
        let body = from_request(&request).unwrap();
        assert_eq!(body["enable_thinking"], json!(true));
        assert_eq!(body["previous_response_id"], json!("resp_1"));
        assert_eq!(body["conversation"], json!("conv_9"));
        assert_eq!(
            body["tool_choice"],
            json!({
                "type": "allowed_tools",
                "mode": "required",
                "tools": [{ "type": "function", "name": "lookup" }]
            })
        );
    }

    #[test]
    fn non_named_tool_choice_passes_through() {
        let mut request = request();
        request.tool_choice = Some(ToolChoice::Required { disable_parallel_tool_use: Some(false) });
        let body = from_request(&request).unwrap();
        assert_eq!(body["tool_choice"], json!("required"));
        // absent knobs stay absent
        assert!(body.get("enable_thinking").is_none());
        assert!(body.get("previous_response_id").is_none());
        assert!(body.get("conversation").is_none());
    }

    #[test]
    fn native_alibaba_tools_lower_and_validate() {
        let adapter = AlibabaResponsesAdapter;
        let native = BTreeMap::from([(
            "alibaba".to_string(),
            json!({ "type": "web_search" }),
        )]);
        assert_eq!(
            adapter.native_tool(&native),
            Some(Ok(json!({ "type": "web_search" })))
        );
        let bad = BTreeMap::from([(
            "alibaba".to_string(),
            json!({ "type": "computer_use" }),
        )]);
        assert!(adapter.native_tool(&bad).unwrap().is_err());
    }

    #[test]
    fn web_extractor_item_restores_and_completes() {
        let adapter = AlibabaResponsesAdapter;
        let raw = json!({
            "type": "web_extractor_call",
            "id": "we_1",
            "urls": ["https://example.com"],
            "goal": "extract pricing"
        });
        assert!(adapter.restore_hosted_tool_item(&raw).is_some());
        assert!(adapter
            .restore_hosted_tool_item(&json!({ "type": "web_search_call", "id": "ws_1" }))
            .is_none());
        assert!(adapter.restore_hosted_tool_item(&json!({ "type": "web_extractor_call" })).is_none());

        let protocol = AlibabaResponsesProtocol;
        let mut state = ParserState::new(&request(), &AlibabaResponsesAdapter);
        let done = json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": raw
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &done).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolCall { name, provider_executed: Some(true), input, .. }
                if name == "web_extractor"
                    && input["urls"] == json!(["https://example.com"])
                    && input["goal"] == json!("extract pricing")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult { name, provider_executed: Some(true), .. }
                if name == "web_extractor"
        )));

        // an invalid item fails typed
        let bad_done = json!({
            "type": "response.output_item.done",
            "output_index": 2,
            "item": { "type": "web_extractor_call", "urls": ["https://example.com"] }
        })
        .to_string();
        let error = protocol.step_frame(&mut state, &bad_done).unwrap_err();
        assert!(error.to_string().contains("invalid web extraction item"));
    }

    #[test]
    fn hosted_search_and_code_interpreter_complete() {
        let protocol = AlibabaResponsesProtocol;
        let mut state = ParserState::new(&request(), &AlibabaResponsesAdapter);
        let search = json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": {
                "type": "web_search_call",
                "id": "ws_1",
                "status": "completed",
                "action": { "query": "qwen pricing" }
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &search).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolCall { name, provider_executed: Some(true), input, .. }
                if name == "web_search" && input["query"] == json!("qwen pricing")
        )));

        let code = json!({
            "type": "response.output_item.done",
            "output_index": 2,
            "item": {
                "type": "code_interpreter_call",
                "id": "ci_1",
                "status": "completed",
                "code": "print(1)"
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &code).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolCall { name, provider_executed: Some(true), input, .. }
                if name == "code_interpreter" && input["code"] == json!("print(1)")
        )));
    }

    #[test]
    fn route_identity_and_wire_shape() {
        let route = route();
        assert_eq!(route.id, "alibaba-responses");
        assert_eq!(route.provider_metadata_key.as_deref(), Some("alibaba"));
        let prepared = route.prepare_request(&request());
        let body = route.build_body(&prepared).unwrap();
        let (wire, _) = route.prepare_transport(&prepared, &body).unwrap();
        assert_eq!(
            wire.url,
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/responses"
        );
    }
}
