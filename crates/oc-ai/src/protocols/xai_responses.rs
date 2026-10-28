//! `xai-responses` (port of `packages/ai/src/protocols/xai-responses.ts`).
//! Grok speaks the standard Responses reasoning dialect
//! (`reasoning_summary_text.*`, handled by the Open Responses baseline);
//! only its hosted tool vocabulary differs — `x_search_call` and
//! `image_generation_call` replay as provider-executed tools — and
//! in-band `contextManagement` is explicitly unsupported (xAI requires
//! explicit compaction through `LLMClient.compact`).

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::open_responses::{self, decode_event, normalize, ProviderAdapter};
use crate::protocols::shared::flatten_tools;
use crate::protocols::utils::responses_compaction::{self, CompactionResponse};
use crate::protocols::utils::responses_hosted_tools::{self, Definition, Item};
use crate::route::auth::{self, Definition as AuthDefinition, Secret};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{merge_route_defaults, Route, RouteDefaults, RouteInput};
use crate::schema::{AiError, AiErrorReason, LLMEvent, LLMRequest, ReasonFields};
pub const ADAPTER: &str = "xai-responses";
pub const NAME: &str = "xAI Responses";
pub const PATH: &str = open_responses::PATH;
pub const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";

/// The WS socket rotation age (REF `RESPONSES_WEBSOCKET_ROTATE_AFTER_MS`,
/// 24 hours — xAI keeps sockets far longer than OpenAI's 55 minutes).
pub const WEBSOCKET_ROTATE_AFTER_MS: u64 = 24 * 60 * 1000;

/// The `xai-responses` channel transport options (port of
/// `OpenResponsesChannel.transport` in providers/xai.ts:43): the socket
/// rotates after 24 hours, and xAI continues a chain only from stored
/// responses — with `store: false` (the route default)
/// `previous_response_id` fails with "Response with id=… not found", so
/// those steps are sent in full over the reused connection. It also
/// rejects `instructions` next to `previous_response_id` and keeps the
/// instructions of the response it continues, so the continuation drops
/// that field.
#[must_use]
pub fn responses_channel_options() -> crate::protocols::open_responses_channel::ChannelOptions {
    crate::protocols::open_responses_channel::ChannelOptions::new(ADAPTER, NAME)
        .with_rotate_after_ms(WEBSOCKET_ROTATE_AFTER_MS)
        .with_continuation(Arc::new(|request: &Map<String, Value>| {
            // `continuation: ({ instructions: _instructions, ...request }) =>
            // (request.store === false ? undefined : request)`
            if request.get("store") == Some(&Value::Bool(false)) {
                return None;
            }
            let mut fields = request.clone();
            fields.remove("instructions");
            Some(fields)
        }))
}

// =============================================================================
// Provider adapter (xai-responses.ts:12-38)
// =============================================================================

/// The `xAI` adapter: restores `x_search_call` / `image_generation_call`
/// hosted items for replay (light validation of the TS schema guard:
/// type + non-empty string id).
pub struct XaiResponsesAdapter;

const XAI_HOSTED_ITEM_TYPES: [&str; 2] = ["x_search_call", "image_generation_call"];

fn is_xai_hosted_item(value: &Value) -> bool {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    XAI_HOSTED_ITEM_TYPES.contains(&kind)
        && value.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
}

impl ProviderAdapter for XaiResponsesAdapter {
    fn id(&self) -> &str {
        ADAPTER
    }
    fn name(&self) -> &str {
        NAME
    }
    fn restore_hosted_tool_item(&self, item: &Value) -> Option<Value> {
        is_xai_hosted_item(item).then(|| item.clone())
    }
}

// =============================================================================
// Hosted tool definitions (xai-responses.ts:52-65)
// =============================================================================

fn action_input(item: &Item) -> Value {
    item.field("action").cloned().unwrap_or(json!({}))
}

fn queries_input(item: &Item) -> Value {
    json!({ "queries": item.field("queries").and_then(Value::as_array).cloned().unwrap_or_default() })
}

fn code_interpreter_input(item: &Item) -> Value {
    let mut object = Map::new();
    if let Some(code) = item.field("code") {
        object.insert("code".to_string(), code.clone());
    }
    if let Some(container) = item.field("container_id") {
        object.insert("container_id".to_string(), container.clone());
    }
    Value::Object(object)
}

fn mcp_input(item: &Item) -> Value {
    let mut object = Map::new();
    for field in ["server_label", "name", "arguments"] {
        if let Some(value) = item.field(field) {
            object.insert(field.to_string(), value.clone());
        }
    }
    Value::Object(object)
}

/// The hosted tool table (port of `HOSTED_TOOLS`).
pub static HOSTED_TOOLS: &[(&str, Definition)] = &[
    ("web_search_call", Definition { name: "web_search", input: action_input, result: None }),
    ("x_search_call", Definition { name: "x_search", input: action_input, result: None }),
    ("file_search_call", Definition { name: "file_search", input: queries_input, result: None }),
    (
        "code_interpreter_call",
        Definition { name: "code_interpreter", input: code_interpreter_input, result: None },
    ),
    // xAI's image results stay as the whole item JSON — no base64 file
    // content result like the `OpenAI` lane (xai-responses.ts:60).
    (
        "image_generation_call",
        Definition { name: "image_generation", input: |_| json!({}), result: None },
    ),
    ("mcp_call", Definition { name: "mcp", input: mcp_input, result: None }),
];

// =============================================================================
// Request lowering (xai-responses.ts:40-50)
// =============================================================================

/// `fromRequest`: a present `contextManagement` fails typed — xAI requires
/// explicit compaction; then the Open Responses baseline with the `xAI`
/// adapter.
pub fn from_request(request: &LLMRequest) -> Result<Value, AiError> {
    if request
        .provider_options
        .as_ref()
        .is_some_and(|options| options.contains_key("contextManagement"))
    {
        return Err(AiError::new(AiErrorReason::UnsupportedOperation {
            fields: ReasonFields::new(
                "xAI requires explicit compaction through LLMClient.compact; automatic context management is not supported",
            ),
            operation: "in-band-compaction".to_string(),
            provider: Some(request.model.provider.clone()),
            route: Some(request.model.route.id.clone()),
        }));
    }
    open_responses::from_request_with_adapter(request, &XaiResponsesAdapter)
}

/// The tool lowering for the standalone compaction operations
/// (`OpenResponses.lowerTools(flattenTools(request.tools), adapter)`).
fn lower_compaction_tools(request: &LLMRequest) -> Result<Option<Value>, AiError> {
    let tools = flatten_tools(&request.tools);
    if tools.is_empty() {
        return Ok(None);
    }
    let lowered = open_responses::lower_tools(&tools, &XaiResponsesAdapter)?;
    Ok(Some(Value::Array(lowered)))
}

// =============================================================================
// Protocol (xai-responses.ts:69-88)
// =============================================================================

/// The `xai-responses` protocol: the Open Responses baseline plus hosted
/// tool item completion.
pub struct XaiResponsesProtocol;

impl XaiResponsesProtocol {
    /// Drive the machine over one raw frame (mirrors the baseline's
    /// `step_frame`): the hosted-tool `output_item.done` branch runs
    /// before the baseline step.
    pub fn step_frame(
        &self,
        state: &mut open_responses::ParserState,
        frame: &str,
    ) -> Result<(Vec<LLMEvent>, bool), AiError> {
        let event = decode_event(frame)?;
        let is_terminal = open_responses::terminal(&event);
        let normalized = normalize(state, event);
        if normalized.event.kind == "response.output_item.done" {
            if let Some(item) = normalized.item.as_ref() {
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

impl Protocol for XaiResponsesProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(open_responses::ParserState::new(request, &XaiResponsesAdapter))
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let mut state =
            *state.downcast::<open_responses::ParserState>().expect("xAI Responses parser state");
        let (events, _) = self.step_frame(&mut state, frame)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        decode_event(frame).is_ok_and(|event| open_responses::terminal(&event))
    }
}

// =============================================================================
// Compaction (xai-responses.ts:90-93)
// =============================================================================

/// Run the standalone `/compact` endpoint for a request on an
/// `xai-responses` route (port of `XAIResponses.compact`).
pub async fn compact(route: &Route, request: &LLMRequest) -> Result<CompactionResponse, AiError> {
    responses_compaction::compact(
        route,
        request,
        &XaiResponsesAdapter,
        lower_compaction_tools(request)?,
    )
    .await
}

// =============================================================================
// Route factory (providers/xai.ts:36-49)
// =============================================================================

/// Route defaults for the `xai-responses` route (port of
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

/// Inputs for [`xai_responses_route`].
pub struct XaiResponsesRouteSpec {
    pub base_url: Option<String>,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
    /// Caller route defaults merged over the `xAI` defaults.
    pub defaults: RouteDefaults,
}

/// xAI-facade auth (port of `AuthOptions.bearer(options, "XAI_API_KEY")`):
/// optional explicit key with the `XAI_API_KEY` fallback.
#[must_use]
pub fn xai_auth(api_key: Option<Secret>) -> AuthDefinition {
    auth::or_else(&auth::optional(api_key), &auth::config("XAI_API_KEY")).bearer()
}

/// Build the `xai-responses` route at `https://api.x.ai/v1/responses`
/// (port of `responsesRoute`, providers/xai.ts:36-49).
#[must_use]
pub fn xai_responses_route(spec: XaiResponsesRouteSpec) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("xai".to_string()),
        provider_metadata_key: Some("xai".to_string()),
        protocol: Arc::new(XaiResponsesProtocol),
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
    use crate::schema::LLMRequest;

    fn route() -> Route {
        xai_responses_route(XaiResponsesRouteSpec {
            base_url: Some("https://api.xai.test/v1".into()),
            auth: Some(xai_auth(Some(Secret::Value("x-1".into())))),
            transport: Arc::new(FailingTransport { message: "unit" }),
            defaults: RouteDefaults::default(),
        })
    }

    fn request() -> LLMRequest {
        let mut request =
            crate::test_support::request_for_tests(vec![crate::schema::Message::user("hi")]);
        request.provider_options = None;
        request
    }

    #[test]
    fn context_management_fails_typed() {
        let mut request = request();
        request.provider_options = Some(
            [("contextManagement".to_string(), json!([{ "type": "compaction" }]))]
                .into_iter()
                .collect(),
        );
        let error = from_request(&request).unwrap_err();
        let AiErrorReason::UnsupportedOperation { operation, provider, route, fields } =
            &error.reason
        else {
            panic!("wrong reason: {:?}", error.tag());
        };
        assert_eq!(operation, "in-band-compaction");
        assert_eq!(provider.as_ref().map(|p| p.0.as_str()), Some("test-provider"));
        assert_eq!(route.as_deref(), Some("echo"));
        assert!(fields.message.contains("explicit compaction through LLMClient.compact"));
    }

    #[test]
    fn route_defaults_and_wire_shape() {
        let route = route();
        assert_eq!(route.id, "xai-responses");
        assert_eq!(route.provider_metadata_key.as_deref(), Some("xai"));
        let defaults = route.defaults.provider_options.as_ref().unwrap();
        assert_eq!(defaults.get("store"), Some(&json!(false)));
        assert_eq!(defaults.get("include"), Some(&json!(["reasoning.encrypted_content"])));
        let prepared = route.prepare_request(&request());
        let body = route.build_body(&prepared).unwrap();
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        let (wire, _) = route.prepare_transport(&prepared, &body).unwrap();
        assert_eq!(wire.url, "https://api.xai.test/v1/responses");
        assert_eq!(wire.header("authorization"), Some("Bearer x-1"));
    }

    #[test]
    fn hosted_tools_replay_provider_executed_pairs() {
        let protocol = XaiResponsesProtocol;
        let mut state = open_responses::ParserState::new(&request(), &XaiResponsesAdapter);
        // x_search_call completes with an action result
        let search_done = json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": {
                "type": "x_search_call",
                "id": "xs_1",
                "status": "completed",
                "action": { "query": "grok news" },
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &search_done).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolCall { name, provider_executed: Some(true), input, .. }
                if name == "x_search" && input["query"] == json!("grok news")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult { name, provider_executed: Some(true), .. } if name == "x_search"
        )));

        // image_generation_call completes; the result stays the whole
        // item JSON (no base64 file content on the xAI lane)
        let image_done = json!({
            "type": "response.output_item.done",
            "output_index": 2,
            "item": {
                "type": "image_generation_call",
                "id": "ig_1",
                "status": "completed",
            }
        })
        .to_string();
        let (events, _) = protocol.step_frame(&mut state, &image_done).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            LLMEvent::ToolResult {
                name,
                result: crate::schema::ToolResultValue::Json { value },
                ..
            } if name == "image_generation" && value["type"] == json!("image_generation_call")
        )));
    }

    #[test]
    fn hosted_items_restore_for_replay() {
        let adapter = XaiResponsesAdapter;
        assert!(adapter
            .restore_hosted_tool_item(&json!({ "type": "x_search_call", "id": "xs_1" }))
            .is_some());
        assert!(adapter
            .restore_hosted_tool_item(&json!({
                "type": "image_generation_call", "id": "ig_1", "result": "AQID"
            }))
            .is_some());
        // foreign item types and missing ids never restore
        assert!(adapter
            .restore_hosted_tool_item(&json!({ "type": "computer_call", "id": "c_1" }))
            .is_none());
        assert!(adapter.restore_hosted_tool_item(&json!({ "type": "x_search_call" })).is_none());
    }
}
