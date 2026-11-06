//! G-173: `ToolSchemaProjection` runs centrally in `Route::prepare_request`
//! (port of route/client.ts:565) — every protocol lane receives
//! pre-projected tool schemas (object root + the model's sanitizer).

use std::sync::Arc;

use oc_ai::protocols::anthropic_messages::anthropic_messages_route;
use oc_ai::protocols::bedrock::BedrockAuthOptions;
use oc_ai::protocols::openai_chat::{
    moonshot_chat_compatibility, moonshot_chat_route, openai_chat_route,
};
use oc_ai::protocols::openai_responses::{openai_responses_route, OpenAIResponsesRouteSpec};
use oc_ai::route::client::Route;
use oc_ai::route::transport::FailingTransport;
use oc_ai::schema::{LLMRequest, Message, ToolDefinition, ToolEntry};
use serde_json::{json, Value};

fn transport() -> Arc<FailingTransport> {
    Arc::new(FailingTransport { message: "projection unit test" })
}

/// A tool whose schema has no `type` root (the shape Effect emits for an
/// empty struct — `anyOf` with a bare object branch — plus a plain
/// properties map case).
fn unrooted_tool() -> ToolEntry {
    ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Look something up",
        json!({ "properties": { "q": { "type": "string" } } }),
    ))
}

fn request(route: &Route) -> LLMRequest {
    let model = route.model("some-model", None).expect("model binds");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    request.tools = vec![unrooted_tool()];
    request
}

fn body(route: &Route) -> Value {
    let request = request(route);
    let prepared = route.prepare_request(&request);
    route.build_body(&prepared).expect("body builds")
}

#[track_caller]
fn assert_object_root(schema: &Value, lane: &str) {
    assert_eq!(schema["type"], json!("object"), "{lane}: projected object root");
    assert!(schema.get("anyOf").is_none(), "{lane}: no leftover anyOf");
    assert_eq!(schema["properties"]["q"]["type"], json!("string"), "{lane}: properties preserved");
}

#[test]
fn anthropic_messages_lane_projects_tool_schemas() {
    let route = anthropic_messages_route(transport());
    let body = body(&route);
    assert_object_root(&body["tools"][0]["input_schema"], "anthropic");
}

#[test]
fn openai_chat_lane_projects_tool_schemas() {
    let route = openai_chat_route(transport());
    let body = body(&route);
    assert_object_root(&body["tools"][0]["function"]["parameters"], "openai-chat");
}

#[test]
fn openai_responses_lane_projects_tool_schemas() {
    let route = openai_responses_route(OpenAIResponsesRouteSpec {
        base_url: None,
        auth: None,
        transport: transport(),
        defaults: Default::default(),
    });
    let body = body(&route);
    assert_object_root(&body["tools"][0]["parameters"], "openai-responses");
}

#[test]
fn bedrock_converse_lane_projects_tool_schemas() {
    let route = oc_ai::protocols::bedrock::bedrock_converse_route(
        transport(),
        BedrockAuthOptions::default(),
    );
    let body = body(&route);
    assert_object_root(
        &body["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"],
        "bedrock-converse",
    );
}

#[test]
fn moonshot_sanitizer_applies_through_the_central_path() {
    // A model routed through the moonshot chat lane carries the moonshot
    // sanitizer via its compatibility preset (moonshot.ts:104-111): an
    // `enum` without a sibling `type` gains one, centrally.
    let route = moonshot_chat_route(transport(), None);
    let model =
        route.model("kimi-k2", None).unwrap().with_compatibility(moonshot_chat_compatibility());
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    request.tools = vec![ToolEntry::Definition(ToolDefinition::new(
        "pick",
        "Pick a color",
        json!({ "type": "object", "properties": { "color": { "enum": ["red", null] } } }),
    ))];
    let prepared = route.prepare_request(&request);
    let ToolEntry::Definition(tool) = &prepared.tools[0] else {
        panic!("tool");
    };
    let color = &tool.input_schema["properties"]["color"];
    assert_eq!(color["type"], json!(["string", "null"]), "moonshot enum typing");
    let body = route.build_body(&prepared).expect("body");
    assert_eq!(
        body["tools"][0]["function"]["parameters"]["properties"]["color"]["type"],
        json!(["string", "null"])
    );
}
