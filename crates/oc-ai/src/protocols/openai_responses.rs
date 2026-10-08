//! `OpenAI` Responses (port of `src/protocols/openai-responses.ts`, HTTP/SSE
//! path): the Open Responses baseline plus `OpenAI` extensions —
//! `context_management` compaction, the `compaction_trigger` checkpoint,
//! route defaults `{store: false, include: ["reasoning.encrypted_content"]}`,
//! and hosted tools via `ToolDefinition.native.openai`.
//!
//! Deferred (per ai-map §10): the WebSocket channel transport with the
//! `openai-beta` protocol header and rotation — the SSE route is the parity
//! path here. Per ai-map, hosted-tools support is a partial port: the
//! baseline hosted items (`web_search_call`, `file_search_call`,
//! `code_interpreter_call`, `mcp_call`, `computer_call`,
//! `web_search_preview_call`, `image_generation_call`) replay as
//! provider-executed tool calls/results, but provider-specific item schemas
//! are validated lightly (type + id) rather than through the full TS schema
//! guards.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::protocols::open_responses::{
    self, decode_event, normalize, on_reasoning_delta, ProviderAdapter,
};
use crate::protocols::utils::open_responses_options as options;
use crate::protocols::utils::responses_compaction::{
    self, CompactionResponse, ContextManagementEntry,
};
use crate::protocols::utils::responses_hosted_tools::{self, Definition, Item};
use crate::route::auth::Definition as AuthDefinition;
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{merge_route_defaults, Route, RouteDefaults, RouteInput};
use crate::schema::{AiError, LLMEvent, LLMRequest, ToolDefinition, ToolEntry};

pub const ADAPTER: &str = "openai-responses";
pub const NAME: &str = "OpenAI Responses";
pub const PATH: &str = open_responses::PATH;
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// The `OpenAI` adapter: restores `OpenAI`-only hosted items for replay.
pub struct OpenAIAdapter;

/// Hosted item types only `OpenAI` emits (port of
/// `OpenAIResponsesHostedToolItem`); light validation of the TS schema
/// guard (type + non-empty string id).
const OPENAI_HOSTED_ITEM_TYPES: [&str; 3] =
    ["computer_call", "web_search_preview_call", "image_generation_call"];

fn is_openai_hosted_item(value: &Value) -> bool {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    OPENAI_HOSTED_ITEM_TYPES.contains(&kind)
        && value.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
}

impl ProviderAdapter for OpenAIAdapter {
    fn id(&self) -> &str {
        ADAPTER
    }
    fn name(&self) -> &str {
        NAME
    }
    fn restore_hosted_tool_item(&self, item: &Value) -> Option<Value> {
        is_openai_hosted_item(item).then(|| item.clone())
    }
}

// =============================================================================
// Hosted tool definitions (port of HOSTED_TOOLS)
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
        object.insert("code".into(), code.clone());
    }
    if let Some(container) = item.field("container_id") {
        object.insert("container_id".into(), container.clone());
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
    (
        "web_search_preview_call",
        Definition { name: "web_search_preview", input: action_input, result: None },
    ),
    ("file_search_call", Definition { name: "file_search", input: queries_input, result: None }),
    (
        "code_interpreter_call",
        Definition { name: "code_interpreter", input: code_interpreter_input, result: None },
    ),
    ("computer_call", Definition { name: "computer_use", input: action_input, result: None }),
    (
        "image_generation_call",
        Definition {
            name: "image_generation",
            input: |_| json!({}),
            result: Some(responses_hosted_tools::image_generation_result),
        },
    ),
    ("mcp_call", Definition { name: "mcp", input: mcp_input, result: None }),
];

// =============================================================================
// Request lowering
// =============================================================================

fn is_one_of(value: Option<&Value>, known: &[&str]) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| known.contains(&text))
}

fn is_int_in(value: Option<&Value>, range: std::ops::RangeInclusive<i64>) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|number| number.fract() == 0.0 && range.contains(&(number as i64)))
}

fn is_int_min(value: Option<&Value>, minimum: i64) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|number| number.fract() == 0.0 && number >= minimum as f64)
}

fn is_image_size(value: &Value) -> bool {
    let Some(text) = value.as_str() else { return false };
    if text == "auto" {
        return true;
    }
    let Some((width, height)) = text.split_once('x') else { return false };
    width.parse::<u32>().is_ok_and(|w| w > 0) && height.parse::<u32>().is_ok_and(|h| h > 0)
}

/// Validate the hosted image generation tool options (port of
/// `OpenAIResponsesImageGenerationTool`).
#[must_use]
pub fn is_image_generation_tool(native: &Value) -> bool {
    if native.get("type").and_then(Value::as_str) != Some("image_generation") {
        return false;
    }
    if let Some(action) = native.get("action") {
        if !is_one_of(Some(action), &["auto", "generate", "edit"]) {
            return false;
        }
    }
    if let Some(background) = native.get("background") {
        if !is_one_of(Some(background), &["auto", "opaque", "transparent"]) {
            return false;
        }
    }
    if let Some(fidelity) = native.get("input_fidelity") {
        if !is_one_of(Some(fidelity), &["low", "high"]) {
            return false;
        }
    }
    if let Some(compression) = native.get("output_compression") {
        if !is_int_in(Some(compression), 0..=100) {
            return false;
        }
    }
    if let Some(format) = native.get("output_format") {
        if !is_one_of(Some(format), &["png", "jpeg", "webp"]) {
            return false;
        }
    }
    if let Some(partial) = native.get("partial_images") {
        if !is_int_min(Some(partial), 0) {
            return false;
        }
    }
    if let Some(quality) = native.get("quality") {
        if !is_one_of(Some(quality), &["auto", "low", "medium", "high"]) {
            return false;
        }
    }
    if let Some(size) = native.get("size") {
        if !is_image_size(size) {
            return false;
        }
    }
    true
}

/// The `native.openai` image generation config, when this tool is one.
fn native_image_tool_input(tool: &ToolDefinition) -> Option<&Value> {
    let native = tool.native.as_ref()?.get("openai")?;
    (native.get("type").and_then(Value::as_str) == Some("image_generation")).then_some(native)
}

/// Lower one tool: hosted image generation configs validate locally; other
/// tools lower through the baseline (port of `lowerTool`).
fn lower_tool(tool: &ToolDefinition) -> Result<Value, AiError> {
    if let Some(native) = native_image_tool_input(tool) {
        if is_image_generation_tool(native) {
            return Ok(native.clone());
        }
        return Err(crate::protocols::shared::invalid_request(
            "OpenAI Responses image generation tool options are invalid",
        ));
    }
    open_responses::lower_tool(NAME, tool)
}

/// Lower one tool entry: native namespaces hold only function tools, so
/// deeper levels flatten into leaf names (port of `lowerToolEntry`).
fn lower_tool_entry(entry: &ToolEntry) -> Result<Value, AiError> {
    match entry {
        ToolEntry::Definition(tool) => lower_tool(tool),
        ToolEntry::Namespace(namespace) => {
            // OpenAI requires a namespace description; fall back to a
            // generic one so a missing description never blocks the request.
            let tools = crate::protocols::shared::flatten_tools(&namespace.tools);
            Ok(json!({
                "type": "namespace",
                "name": namespace.name,
                "description": namespace
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Tools in the {} namespace.", namespace.name)),
                "tools": open_responses::lower_tools(&tools, &OpenAIAdapter)?,
            }))
        }
    }
}

/// Lower the request's tool tree (port of `lowerTools`): namespaces stay
/// native, leaves stay whole.
fn lower_tools(request: &LLMRequest) -> Result<Value, AiError> {
    let tools: Vec<Value> = request.tools.iter().map(lower_tool_entry).collect::<Result<_, _>>()?;
    Ok(Value::Array(tools))
}

fn is_native_image_tool(tool: &ToolEntry) -> bool {
    tool.as_definition().and_then(native_image_tool_input).is_some_and(is_image_generation_tool)
}

/// Lower the tool choice; named selection of a hosted image tool lowers to
/// `{type: "image_generation"}` (port of `lowerToolChoice`).
fn lower_tool_choice(
    tool_choice: &crate::schema::ToolChoice,
    tools: &[ToolEntry],
) -> Result<Value, AiError> {
    match tool_choice {
        crate::schema::ToolChoice::Auto { .. } => Ok(json!("auto")),
        crate::schema::ToolChoice::None { .. } => Ok(json!("none")),
        crate::schema::ToolChoice::Required { .. } => Ok(json!("required")),
        crate::schema::ToolChoice::Named { name, .. } if name.is_empty() => {
            Err(crate::protocols::shared::invalid_request(format!(
                "{NAME} tool choice requires a tool name"
            )))
        }
        crate::schema::ToolChoice::Named { name, .. } => {
            let hosted = tools.iter().any(|tool| {
                tool.as_definition().map(|definition| definition.name.as_str())
                    == Some(name.as_str())
                    && is_native_image_tool(tool)
            });
            Ok(if hosted {
                json!({ "type": "image_generation" })
            } else {
                json!({ "type": "function", "name": name })
            })
        }
    }
}

/// GPT-6 and later default to `configuration_update` support, except in
/// `reasoning.mode: "pro"` or alongside automatic `context_management`
/// compaction (port of `supportsEffortUpdates`).
pub fn supports_effort_updates(request: &LLMRequest) -> bool {
    if request
        .provider_options
        .as_ref()
        .is_some_and(|options| options.contains_key("contextManagement"))
    {
        return false;
    }
    if request
        .http
        .as_ref()
        .and_then(|http| http.body.as_ref())
        .and_then(|body| body.get("reasoning"))
        .and_then(|reasoning| reasoning.get("mode"))
        .and_then(Value::as_str)
        == Some("pro")
    {
        return false;
    }
    if let Some(override_) = request
        .model
        .compatibility
        .as_ref()
        .and_then(|compatibility| compatibility.supports_effort_updates)
    {
        return override_;
    }
    let id = request.model.id.as_str();
    // /(?:^|\/)gpt-(\d+)(?:\.\d+)?(?:-|$)/i
    let lower = id.to_ascii_lowercase();
    for segment in lower.split('/') {
        let Some(rest) = segment.strip_prefix("gpt-") else { continue };
        let mut digits = String::new();
        for ch in rest.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                break;
            }
        }
        if digits.is_empty() {
            continue;
        }
        let remainder = &rest[digits.len()..];
        let major: u32 = digits.parse().unwrap_or(0);
        let _ = remainder;
        return major >= 6;
    }
    false
}

/// Build the `OpenAI` Responses body (port of `fromRequest`).
pub fn from_request(request: &LLMRequest) -> Result<Value, AiError> {
    let management = responses_compaction::context_management(request)?;
    let resolved = options::resolve(request);
    let (updated, effort) = crate::effort_updates::resolve_effort_updates(
        request,
        resolved.reasoning_effort.as_deref(),
    );
    let mut body = open_responses::lower_conversation(&updated, &OpenAIAdapter)?;
    for (key, value) in open_responses::lower_generation(
        request,
        &options::Resolved { reasoning_effort: effort, ..resolved },
    ) {
        body.insert(key, value);
    }
    if let Some(management) = management {
        body.insert(
            "context_management".into(),
            Value::Array(
                management
                    .into_iter()
                    .map(|entry: ContextManagementEntry| {
                        let mut object = Map::new();
                        object.insert("type".into(), json!("compaction"));
                        if let Some(threshold) = entry.compact_threshold {
                            object.insert("compact_threshold".into(), json!(threshold));
                        }
                        Value::Object(object)
                    })
                    .collect(),
            ),
        );
    }
    if !request.tools.is_empty() {
        body.insert("tools".into(), lower_tools(request)?);
        let tool_choice =
            open_responses::allowed_tool_choice(request).or(match request.tool_choice.as_ref() {
                Some(choice) => Some(lower_tool_choice(choice, &request.tools)?),
                None => None,
            });
        if let Some(tool_choice) = tool_choice {
            body.insert("tool_choice".into(), tool_choice);
        }
    }
    Ok(Value::Object(body))
}

/// Build the `compaction_trigger` body (port of `checkpointBody.from`):
/// complete history is required for stateless replay and SSE recovery.
pub fn checkpoint_body(request: &LLMRequest) -> Result<Value, AiError> {
    let overlay = request.http.as_ref().and_then(|http| http.body.as_ref());
    if overlay.is_some_and(|overlay| {
        overlay.contains_key("input")
            || overlay.contains_key("previous_response_id")
            || overlay.contains_key("conversation")
    }) {
        return Err(crate::protocols::shared::invalid_request(
            "Trigger compaction requires complete canonical history, not an input or continuation override",
        ));
    }
    if let Some(stream) = overlay.and_then(|overlay| overlay.get("stream")) {
        if stream != &json!(true) {
            return Err(crate::protocols::shared::invalid_request(
                "Trigger compaction requires a streamed response",
            ));
        }
    }
    let mut native = from_request(request)?;
    if let Some(object) = native.as_object_mut() {
        if let Some(input) = object.get_mut("input") {
            if let Some(items) = input.as_array_mut() {
                items.push(json!({ "type": "compaction_trigger" }));
            }
        }
    }
    Ok(native)
}

// =============================================================================
// Protocol
// =============================================================================

/// The `OpenAI` Responses protocol: the baseline machine plus
/// `response.reasoning_text.delta` and hosted tool item completion (port of
/// the `OpenAI` `step` override).
pub struct OpenAIResponsesProtocol;

impl OpenAIResponsesProtocol {
    /// Drive the machine over one raw frame (mirrors
    /// [`open_responses::OpenResponsesProtocol::step_frame`]).
    pub fn step_frame(
        &self,
        state: &mut open_responses::ParserState,
        frame: &str,
    ) -> Result<(Vec<LLMEvent>, bool), AiError> {
        let event = decode_event(frame)?;
        let is_terminal = open_responses::terminal(&event);
        let normalized = normalize(state, event);
        if normalized.event.kind == "response.reasoning_text.delta" {
            let Some(item_id) = normalized.item_id.clone() else {
                return Err(crate::protocols::shared::event_error(
                    ADAPTER,
                    "response.reasoning_text.delta is missing item_id",
                ));
            };
            let (next, events) = on_reasoning_delta(state, &normalized.event, &item_id);
            *state = next;
            return Ok((events, is_terminal));
        }
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

impl Protocol for OpenAIResponsesProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        from_request(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(open_responses::ParserState::new(request, &OpenAIAdapter))
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let mut state = *state
            .downcast::<open_responses::ParserState>()
            .expect("OpenAI Responses parser state");
        let (events, _) = self.step_frame(&mut state, frame)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        decode_event(frame).is_ok_and(|event| open_responses::terminal(&event))
    }

    fn supports_effort_updates(&self, request: &LLMRequest) -> bool {
        supports_effort_updates(request)
    }
}

// =============================================================================
// Route factory + compaction operations
// =============================================================================

/// Route defaults for the `OpenAI` Responses route (port of
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

/// Inputs for [`openai_responses_route`].
pub struct OpenAIResponsesRouteSpec {
    pub base_url: Option<String>,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
    /// Caller route defaults merged over the `OpenAI` defaults.
    pub defaults: RouteDefaults,
}

/// Build the `OpenAI` Responses route (port of `OpenAIResponses.route`, with
/// the SSE transport as the parity path).
#[must_use]
pub fn openai_responses_route(spec: OpenAIResponsesRouteSpec) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some("openai".into()),
        provider_metadata_key: Some("openai".into()),
        protocol: Arc::new(OpenAIResponsesProtocol),
        endpoint: Endpoint::path(PATH)
            .with_base_url(spec.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string())),
        auth: spec.auth,
        framing: Framing::sse(),
        headers: None,
        transport: spec.transport,
        defaults: merge_route_defaults(Some(&default_route_defaults()), &spec.defaults),
    })
}

/// Run the standalone `/compact` endpoint for a request on an `OpenAI`
/// Responses route (port of `Route.compact.endpoint`).
pub async fn compact(route: &Route, request: &LLMRequest) -> Result<CompactionResponse, AiError> {
    let tools = if request.tools.is_empty() { None } else { Some(lower_tools(request)?) };
    responses_compaction::compact(route, request, &OpenAIAdapter, tools).await
}

/// Trigger server-side compaction by appending a `compaction_trigger` item
/// and streaming the checkpoint response (port of `Route.compact.trigger`).
pub async fn trigger_compaction(
    route: &Route,
    request: &LLMRequest,
) -> Result<crate::protocols::utils::responses_checkpoint::CheckpointOutcome, AiError> {
    let body = checkpoint_body(request)?;
    crate::protocols::utils::responses_checkpoint::trigger_compaction(route, request, &body).await
}

#[cfg(test)]
mod tests {
    //! Ports of the HTTP-side tests in `openai-responses.test.ts` (the
    //! WebSocket channel/continuation cases are deferred with the channel),
    //! plus effort-update detection.

    use super::*;
    use crate::route::auth::{self, Secret};
    use crate::route::transport::FailingTransport;
    use crate::schema::{
        ContentPart, GenerationOptions, LLMRequest, Message, SystemPart, ToolChoice,
        ToolDefinition, ToolNamespace,
    };
    use serde_json::json;

    fn route() -> Route {
        openai_responses_route(OpenAIResponsesRouteSpec {
            base_url: Some("https://api.openai.test/v1/".into()),
            auth: Some(auth::Definition::bearer(Secret::Value("test".into()))),
            transport: std::sync::Arc::new(FailingTransport { message: "body tests" }),
            defaults: Default::default(),
        })
    }

    fn compile(request: &LLMRequest) -> Result<Value, AiError> {
        let route = route();
        let prepared = route.prepare_request(request);
        route.build_body(&prepared)
    }

    fn base_request() -> LLMRequest {
        let model = route().model("gpt-4.1-mini", None).unwrap();
        let mut request = LLMRequest::new(model);
        request.system = vec![SystemPart::new("You are concise.")];
        request.messages = vec![Message::user("Say hello.")];
        request.generation = Some(GenerationOptions {
            max_tokens: Some(20.0),
            temperature: Some(0.0),
            ..Default::default()
        });
        request
    }

    fn image_generation_tool(options: Value) -> ToolDefinition {
        let mut native = std::collections::BTreeMap::new();
        native.insert("openai".to_string(), options);
        ToolDefinition {
            name: "image_generation".into(),
            description: "Generate an image".into(),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            cache: None,
            metadata: None,
            native: Some(native),
        }
    }

    #[test]
    fn openai_responses_prepares_the_target_body() {
        let body = compile(&base_request()).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "gpt-4.1-mini",
                "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "Say hello." }] }],
                "instructions": "You are concise.",
                "store": false,
                "include": ["reasoning.encrypted_content"],
                "stream": true,
                "max_output_tokens": 20,
                "temperature": 0,
            })
        );
    }

    #[test]
    fn openai_responses_lowers_the_hosted_image_generation_tool() {
        let mut request = base_request();
        request.messages = vec![Message::user("Show me a rooftop garden.")];
        request.tools = vec![ToolEntry::Definition(image_generation_tool(json!({
            "type": "image_generation", "action": "generate", "quality": "high", "size": "1024x1024",
        })))];
        request.tool_choice = Some(ToolChoice::named("image_generation"));
        let body = compile(&request).unwrap();
        assert_eq!(
            body["tools"],
            json!([{ "type": "image_generation", "action": "generate", "quality": "high", "size": "1024x1024" }])
        );
        assert_eq!(body["tool_choice"], json!({ "type": "image_generation" }));
    }

    fn namespace(name: &str, description: Option<&str>, tools: Vec<ToolEntry>) -> ToolEntry {
        ToolEntry::Namespace(ToolNamespace {
            name: name.into(),
            description: description.map(str::to_string),
            tools,
        })
    }

    fn function(name: &str, description: &str) -> ToolEntry {
        ToolEntry::Definition(ToolDefinition::new(name, description, json!({ "type": "object" })))
    }

    #[test]
    fn openai_responses_lowers_namespaces_without_flattening_leaf_names() {
        let mut request = base_request();
        request.messages = vec![Message::user("Find a customer and their orders.")];
        request.tools = vec![namespace(
            "crm",
            Some("Customer management"),
            vec![
                function("lookup", "Look up a customer"),
                function("orders", "List customer orders"),
            ],
        )];
        let body = compile(&request).unwrap();
        assert_eq!(
            body["tools"],
            json!([{
                "type": "namespace",
                "name": "crm",
                "description": "Customer management",
                "tools": [
                    { "type": "function", "name": "lookup", "description": "Look up a customer", "parameters": { "type": "object" }, "strict": false },
                    { "type": "function", "name": "orders", "description": "List customer orders", "parameters": { "type": "object" }, "strict": false },
                ],
            }])
        );
    }

    #[test]
    fn openai_responses_flattens_nested_levels_within_a_native_namespace() {
        let mut request = base_request();
        request.tools = vec![namespace(
            "crm",
            Some("Customer management"),
            vec![namespace(
                "orders",
                Some("Order management"),
                vec![function("list", "List orders")],
            )],
        )];
        let body = compile(&request).unwrap();
        assert_eq!(
            body["tools"],
            json!([{
                "type": "namespace",
                "name": "crm",
                "description": "Customer management",
                "tools": [
                    { "type": "function", "name": "orders_list", "description": "List orders", "parameters": { "type": "object" }, "strict": false },
                ],
            }])
        );
    }

    #[test]
    fn openai_responses_defaults_tool_namespace_descriptions() {
        let mut request = base_request();
        request.tools =
            vec![namespace("crm", None, vec![function("lookup", "Look up a customer")])];
        let body = compile(&request).unwrap();
        assert_eq!(body["tools"][0]["description"], json!("Tools in the crm namespace."));
    }

    #[test]
    fn openai_responses_rejects_invalid_hosted_image_options_locally() {
        let mut request = base_request();
        request.messages = vec![Message::user("Show me a rooftop garden.")];
        request.tools = vec![ToolEntry::Definition(image_generation_tool(json!({
            "type": "image_generation", "output_compression": -1, "partial_images": 4, "size": "bogus",
        })))];
        let error = compile(&request).unwrap_err();
        assert!(
            error.message().contains("image generation tool options are invalid"),
            "got {}",
            error.message()
        );
    }

    #[test]
    fn openai_responses_lowers_semantic_service_tier_options() {
        let request = base_request().update(crate::schema::LLMRequestPatch {
            provider_options: Some(
                [("serviceTier", json!("priority"))]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            ),
            ..Default::default()
        });
        let body = compile(&request).unwrap();
        assert_eq!(body["service_tier"], json!("priority"));
        assert!(body.get("serviceTier").is_none());
    }

    #[test]
    fn openai_responses_passes_custom_reasoning_effort_strings() {
        let request = base_request().update(crate::schema::LLMRequestPatch {
            provider_options: Some(
                [("reasoningEffort", json!("experimental"))]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            ),
            ..Default::default()
        });
        let body = compile(&request).unwrap();
        assert_eq!(body["reasoning"], json!({ "effort": "experimental" }));
    }

    #[test]
    fn openai_responses_passes_custom_text_verbosity_strings() {
        let request = base_request().update(crate::schema::LLMRequestPatch {
            provider_options: Some(
                [("textVerbosity", json!("verbose"))]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            ),
            ..Default::default()
        });
        let body = compile(&request).unwrap();
        assert_eq!(body["text"], json!({ "verbosity": "verbose" }));
    }

    #[test]
    fn openai_responses_preserves_function_schemas() {
        let mut request = base_request();
        request.tools = vec![ToolEntry::Definition(ToolDefinition::new(
            "read",
            "Read a path or resource.",
            json!({
                "type": "object",
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "reference": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                            "limit": { "type": "integer", "maximum": 2000 },
                        },
                        "required": ["path"],
                    },
                    {
                        "type": "object",
                        "properties": { "resource": { "type": "string" }, "limit": { "type": "integer", "maximum": 51200 } },
                        "required": ["resource"],
                    },
                ],
            }),
        ))];
        let body = compile(&request).unwrap();
        assert_eq!(
            body["tools"],
            json!([{
                "type": "function",
                "name": "read",
                "description": "Read a path or resource.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "reference": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                                "limit": { "type": "integer", "maximum": 2000 },
                            },
                            "required": ["path"],
                        },
                        {
                            "type": "object",
                            "properties": { "resource": { "type": "string" }, "limit": { "type": "integer", "maximum": 51200 } },
                            "required": ["resource"],
                        },
                    ],
                },
            }])
        );
    }

    #[test]
    fn openai_responses_maps_the_canonical_parallel_tool_setting() {
        let disabled = base_request().update(crate::schema::LLMRequestPatch {
            tool_choice: Some(ToolChoice::Auto { disable_parallel_tool_use: Some(true) }),
            ..Default::default()
        });
        let enabled = base_request().update(crate::schema::LLMRequestPatch {
            tool_choice: Some(ToolChoice::Auto { disable_parallel_tool_use: Some(false) }),
            ..Default::default()
        });
        let mut overridden_options = serde_json::Map::new();
        overridden_options.insert("parallelToolCalls".to_string(), json!(true));
        let overridden = base_request().update(crate::schema::LLMRequestPatch {
            tool_choice: Some(ToolChoice::Auto { disable_parallel_tool_use: Some(true) }),
            provider_options: Some(overridden_options),
            ..Default::default()
        });
        assert_eq!(compile(&disabled).unwrap()["parallel_tool_calls"], json!(false));
        assert_eq!(compile(&enabled).unwrap()["parallel_tool_calls"], json!(true));
        assert_eq!(compile(&overridden).unwrap()["parallel_tool_calls"], json!(true));
    }

    #[test]
    fn openai_responses_lowers_system_updates_to_developer_messages_in_order() {
        let mut request = base_request();
        request.system = Vec::new();
        request.generation = None;
        request.messages = vec![
            Message::user("Before."),
            Message::system("Operator update."),
            Message::assistant(vec![ContentPart::text("After.")]),
        ];
        let body = compile(&request).unwrap();
        assert_eq!(
            body["input"],
            json!([
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "Before." }] },
                { "type": "message", "role": "developer", "content": "Operator update." },
                { "type": "message", "role": "assistant", "status": "completed", "content": [{ "type": "output_text", "text": "After." }] },
            ])
        );
    }

    #[test]
    fn openai_responses_detects_effort_update_support() {
        let supports = |id: &str| {
            let request = base_request_with_model(id);
            supports_effort_updates(&request)
        };
        assert!(supports("gpt-6.1"));
        assert!(supports("gpt-10"));
        assert!(supports("openai/gpt-6"));
        assert!(!supports("gpt-4.1-mini"));
        assert!(!supports("gpt-5.5"));
        assert!(!supports("o4-mini"));
        // compatibility override wins
        let base = route();
        let model = base.model("gpt-4.1-mini", None).unwrap().with_compatibility(
            crate::schema::LanguageModelCompatibility {
                supports_effort_updates: Some(true),
                ..Default::default()
            },
        );
        let request = LLMRequest::new(model).with_prompt("hi");
        assert!(supports_effort_updates(&request));
        // automatic compaction disables updates
        let mut with_management = base_request_with_model("gpt-6.1");
        with_management.provider_options = Some(
            [("contextManagement", json!([{ "type": "compaction" }]))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        );
        assert!(!supports_effort_updates(&with_management));
        // pro reasoning mode disables updates
        let mut pro = base_request_with_model("gpt-6.1");
        pro.http = Some(crate::schema::HttpOptions {
            body: Some(
                [("reasoning", json!({ "mode": "pro" }))]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            ),
            ..Default::default()
        });
        assert!(!supports_effort_updates(&pro));
    }

    fn base_request_with_model(id: &str) -> LLMRequest {
        let model = route().model(id, None).unwrap();
        LLMRequest::new(model).with_prompt("hi")
    }

    #[test]
    fn openai_responses_lowest_effort_updates_into_configuration_updates() {
        let mut request = base_request_with_model("gpt-6.1");
        request.messages = vec![
            Message::user("Before."),
            Message::effort(Some("low".into()), Some("high".into())),
            Message::user("After."),
        ];
        request.provider_options = Some(
            [("reasoningEffort", json!("low"))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        );
        let route = route();
        let prepared = route.prepare_request(&request);
        assert!(route.supports_effort_updates(&prepared));
        let body = route.build_body(&prepared).unwrap();
        assert_eq!(
            body["input"],
            json!([
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "Before." }] },
                { "type": "configuration_update", "reasoning": { "effort": "low" } },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "After." }] },
            ])
        );
        assert_eq!(body["reasoning"], json!({ "effort": "high" }));
    }

    #[test]
    fn openai_responses_checkpoints_append_the_trigger_item() {
        let request = base_request_with_model("gpt-6.1");
        let body = checkpoint_body(&request).unwrap();
        assert_eq!(
            body["input"].as_array().unwrap().last().unwrap(),
            &json!({ "type": "compaction_trigger" })
        );
        // raw input overrides are rejected
        let mut raw = request.clone();
        raw.http = Some(crate::schema::HttpOptions {
            body: Some(
                [("input", json!("raw-http-input"))]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            ),
            ..Default::default()
        });
        let error = checkpoint_body(&raw).unwrap_err();
        assert!(error.message().contains("complete canonical history"), "got {}", error.message());
        let mut not_streamed = request.clone();
        not_streamed.http = Some(crate::schema::HttpOptions {
            body: Some(
                [("stream", json!(false))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            ),
            ..Default::default()
        });
        let error = checkpoint_body(&not_streamed).unwrap_err();
        assert!(error.message().contains("streamed response"), "got {}", error.message());
    }

    #[test]
    fn openai_responses_context_management_validation() {
        let mut request = base_request_with_model("gpt-6.1");
        request.provider_options = Some(
            [("contextManagement", json!([{ "type": "compaction", "compactThreshold": 5000 }]))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        );
        let body = compile(&request).unwrap();
        assert_eq!(
            body["context_management"],
            json!([{ "type": "compaction", "compact_threshold": 5000 }])
        );
        let mut invalid = base_request_with_model("gpt-6.1");
        invalid.provider_options = Some(
            [("contextManagement", json!([{ "type": "other" }]))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        );
        let error = compile(&invalid).unwrap_err();
        assert!(error.message().contains("context management options are invalid"));
    }

    #[test]
    fn openai_responses_finishes_with_the_machine_on_terminal_frames() {
        // sanity: the protocol's terminal predicate matches the base machine
        assert!(OpenAIResponsesProtocol.terminal(r#"{"type":"response.completed","response":{}}"#));
        assert!(OpenAIResponsesProtocol.terminal(r#"{"type":"error"}"#));
        assert!(!OpenAIResponsesProtocol.terminal(r#"{"type":"response.created","response":{}}"#));
    }

    #[test]
    fn openai_responses_routes_image_results_into_file_content() {
        let item = crate::protocols::utils::responses_hosted_tools::Item {
            id: "ig_1".into(),
            kind: "image_generation_call".into(),
            raw: json!({
                "type": "image_generation_call", "id": "ig_1", "status": "completed",
                "result": "QUJD", "output_format": "png",
            }),
        };
        let result =
            crate::protocols::utils::responses_hosted_tools::image_generation_result(&item)
                .unwrap();
        match result {
            crate::schema::ToolResultValue::Content { value } => {
                assert_eq!(
                    value.0,
                    vec![
                        json!({ "type": "file", "uri": "data:image/png;base64,QUJD", "mime": "image/png" })
                    ]
                );
            }
            other => panic!("unexpected result {other:?}"),
        }
        // invalid base64 fails typed
        let mut bad = item.clone();
        bad.raw = json!({ "type": "image_generation_call", "id": "ig_1", "result": "%%%" });
        assert!(
            crate::protocols::utils::responses_hosted_tools::image_generation_result(&bad).is_err()
        );
    }

    #[test]
    fn openai_responses_hosted_tools_emit_provider_executed_pairs() {
        let request = base_request_with_model("gpt-6.1");
        let mut state = open_responses::ParserState::new(&request, &OpenAIAdapter);
        let protocol = OpenAIResponsesProtocol;
        let (events, terminal) = protocol
            .step_frame(
                &mut state,
                r#"{"type":"response.output_item.done","item":{"type":"web_search_call","id":"ws_1","status":"completed","action":{"query":"hi"}}}"#,
            )
            .unwrap();
        assert!(!terminal);
        assert_eq!(events.len(), 3); // step-start + tool-call + tool-result
        match &events[1] {
            LLMEvent::ToolCall {
                id, name, input, provider_executed, provider_metadata, ..
            } => {
                assert_eq!(id, "ws_1");
                assert_eq!(name, "web_search");
                assert_eq!(*input, json!({ "query": "hi" }));
                assert_eq!(*provider_executed, Some(true));
                assert_eq!(provider_metadata.as_ref().unwrap()["openai"]["itemId"], json!("ws_1"));
            }
            other => panic!("unexpected {}", other.kind()),
        }
        assert!(matches!(&events[2], LLMEvent::ToolResult { name, .. } if name == "web_search"));
    }

    #[test]
    fn openai_responses_reasoning_text_delta_streams_through_the_override() {
        let request = base_request_with_model("gpt-6.1");
        let mut state = open_responses::ParserState::new(&request, &OpenAIAdapter);
        let protocol = OpenAIResponsesProtocol;
        let _ = protocol
            .step_frame(&mut state, r#"{"type":"response.output_item.added","item":{"type":"reasoning","id":"rs_1","encrypted_content":null}}"#)
            .unwrap();
        let (events, _) = protocol
            .step_frame(
                &mut state,
                r#"{"type":"response.reasoning_text.delta","item_id":"rs_1","delta":"Raw"}"#,
            )
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], LLMEvent::ReasoningDelta { id, text, .. } if id == "rs_1:0" && text == "Raw")
        );
        // missing item_id is a typed error
        let error = protocol
            .step_frame(&mut state, r#"{"type":"response.reasoning_text.delta","delta":"Raw"}"#)
            .unwrap_err();
        assert!(error.message().contains("missing item_id"), "got {}", error.message());
    }

    #[test]
    fn openai_responses_compaction_response_parses_replacement_messages() {
        let route = route();
        let request = base_request_with_model("gpt-6.1");
        let document = json!({
            "object": "response.compaction",
            "output": [
                { "type": "compaction", "id": "cmp_1", "encrypted_content": "enc" },
                { "type": "reasoning", "id": "rs_1", "summary": [{ "type": "summary_text", "text": "Thought" }], "encrypted_content": "re-encrypted" },
                { "type": "message", "id": "msg_1", "role": "user", "status": "completed", "content": [
                    { "type": "input_text", "text": "Summarized." },
                ] },
            ],
            "usage": { "input_tokens": 100, "output_tokens": 5, "total_tokens": 105 },
        });
        let parsed = crate::protocols::utils::responses_compaction::parse_compaction_response(
            &serde_json::to_string(&document).unwrap(),
            &route,
            &request,
        )
        .unwrap();
        assert_eq!(parsed.replacement.len(), 3);
        match &parsed.replacement[0].content[0] {
            crate::schema::ContentPart::Compaction { provider, id, encrypted, .. } => {
                assert_eq!(provider.0, "openai");
                assert_eq!(id.as_deref(), Some("cmp_1"));
                assert_eq!(encrypted.as_deref(), Some("enc"));
            }
            other => panic!("unexpected {other:?}"),
        }
        match &parsed.replacement[1].content[0] {
            crate::schema::ContentPart::Reasoning { text, provider_metadata, .. } => {
                assert_eq!(text, "Thought");
                assert_eq!(
                    provider_metadata.as_ref().unwrap()["openai"]["reasoningEncryptedContent"],
                    json!("re-encrypted")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        match &parsed.replacement[2].content[0] {
            crate::schema::ContentPart::Text { text, .. } => assert_eq!(text, "Summarized."),
            other => panic!("unexpected {other:?}"),
        }
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(100.0));
        assert_eq!(usage.total_tokens, Some(105.0));

        // no checkpoint → typed error
        let missing = json!({ "object": "response.compaction", "output": [] });
        let error = crate::protocols::utils::responses_compaction::parse_compaction_response(
            &serde_json::to_string(&missing).unwrap(),
            &route,
            &request,
        )
        .unwrap_err();
        assert!(error.message().contains("did not contain a checkpoint"));
    }

    #[test]
    fn openai_responses_checkpoint_step_collects_exactly_one_checkpoint() {
        use crate::protocols::utils::responses_checkpoint::{checkpoint_step, CheckpointState};
        let request = base_request_with_model("gpt-6.1");
        let mut state = CheckpointState::new(&request, "openai-responses");
        let frames = [
            r#"{"type":"response.created","response":{"id":"resp_1"}}"#,
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"compaction","id":"cmp_1"}}"#,
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"compaction","id":"cmp_1","encrypted_content":"enc"}}"#,
            r#"{"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[{"type":"compaction","id":"cmp_1","encrypted_content":"enc"}],"usage":{"input_tokens":9}}}"#,
        ];
        let mut events = Vec::new();
        for frame in frames {
            let event = open_responses::decode_event(frame).unwrap();
            let (next, emitted) = checkpoint_step(state, &event, "openai").unwrap();
            state = next;
            events.extend(emitted);
        }
        assert_eq!(events.len(), 1);
        assert!(events[0].is_finish());
        let outcome = state.outcome.expect("checkpoint");
        assert_eq!(outcome.checkpoint.id, "cmp_1");
        assert_eq!(outcome.checkpoint.encrypted, "enc");
        assert_eq!(outcome.response_id, "resp_1");
        assert_eq!(outcome.usage.unwrap().input_tokens, Some(9.0));

        // a second checkpoint in the same response is rejected
        let two = CheckpointState::new(&request, "openai-responses");
        let error = checkpoint_step(
            two.clone(),
            &open_responses::decode_event(
                r#"{"type":"response.completed","response":{"id":"resp_2","status":"completed","output":[
                    {"type":"compaction","id":"c1","encrypted_content":"a"},
                    {"type":"compaction","id":"c2","encrypted_content":"b"}
                ]}}"#,
            )
            .unwrap(),
            "openai",
        )
        .unwrap_err();
        assert!(error.message().contains("exactly one checkpoint"), "got {}", error.message());
    }
}
