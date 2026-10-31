//! `generateObject` (port of `packages/ai/src/llm.ts:86-196`).
//!
//! Runs a model and decodes its output against a JSON Schema. Works on every
//! protocol because it forces a synthetic tool call internally —
//! provider-native JSON modes are intentionally avoided so behaviour is
//! uniform. The forced tool flows through the existing protocol/route tool
//! plumbing (no new protocol code).
//!
//! Rust ports the TS *dynamic* mode (`jsonSchema`): the schema is only
//! available at runtime (`serde_json::Value`), so the decoded `.object` is a
//! `serde_json::Value`. The typed mode's `tool._decode(call.input)` step is
//! mirrored by a self-contained JSON Schema subset validator (see
//! [`validate_json_schema`]) so malformed tool input still surfaces as the
//! ref's `InvalidProviderOutput` "schema decode" error.

use serde_json::Value;

use crate::schema::{
    AiError, AiErrorReason, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest,
    LLMRequestPatch, LLMResponse, ReasonFields, ToolChoice, ToolDefinition, ToolEntry, Usage,
};

/// The synthetic forced tool name (ref `GENERATE_OBJECT_TOOL_NAME`).
pub const GENERATE_OBJECT_TOOL_NAME: &str = "generate_object";

/// The synthetic tool description (ref `GENERATE_OBJECT_TOOL_DESCRIPTION`).
pub const GENERATE_OBJECT_TOOL_DESCRIPTION: &str =
    "Return the structured result by calling this tool.";

/// Result of [`generate_object`] (port of `GenerateObjectResponse<T>`; the
/// Rust object is the raw decoded `serde_json::Value`).
#[derive(Debug, Clone)]
pub struct GenerateObjectResponse {
    /// The decoded tool-call input.
    pub object: Value,
    /// The full assembled response the object was extracted from.
    pub response: LLMResponse,
}

impl GenerateObjectResponse {
    /// All provider-neutral events of the underlying response.
    #[must_use]
    pub fn events(&self) -> &[LLMEvent] {
        &self.response.events
    }

    /// Token usage of the underlying response.
    #[must_use]
    pub fn usage(&self) -> Option<&Usage> {
        self.response.usage.as_ref()
    }
}

/// Run a model and decode its output against `json_schema` (port of
/// `LLM.generateObject`, dynamic `jsonSchema` mode).
///
/// The request is rebuilt with exactly one tool — `generate_object` carrying
/// the schema — and a named tool choice forcing it, then run through the
/// model's route via the standard `generate` pipeline.
pub async fn generate_object(
    request: LLMRequest,
    json_schema: Value,
) -> Result<GenerateObjectResponse, AiError> {
    let tool = ToolDefinition::new(
        GENERATE_OBJECT_TOOL_NAME,
        GENERATE_OBJECT_TOOL_DESCRIPTION,
        json_schema.clone(),
    );
    let generate_request = request.update(LLMRequestPatch {
        tools: Some(vec![ToolEntry::Definition(tool)]),
        tool_choice: Some(ToolChoice::named(GENERATE_OBJECT_TOOL_NAME)),
        ..Default::default()
    });
    let route = std::sync::Arc::clone(&generate_request.model.route);
    let route_label = format!("{}/{}", generate_request.model.provider.0, route.id);
    let response = route.generate(generate_request).await?;

    // ref: `response.toolCalls.find(event => is.toolCall && name matches)`
    // — FIRST matching call, matching JS `find` semantics.
    let call = response.events.iter().find_map(|event| match event {
        LLMEvent::ToolCall { name, input, .. } if name == GENERATE_OBJECT_TOOL_NAME => {
            Some(input.clone())
        }
        _ => None,
    });
    let Some(object) = call else {
        return Err(AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new(format!(
                "generateObject: model did not call the forced `{GENERATE_OBJECT_TOOL_NAME}` tool"
            )),
            classification: None,
            route: Some(route_label),
        }));
    };

    // ref: `tool._decode(call.input)` mapped onto InvalidProviderOutput.
    if let Err(error) = validate_json_schema(&object, &json_schema, "") {
        return Err(AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new(format!(
                "generateObject: tool input failed schema decode: {error}"
            )),
            classification: None,
            route: Some(route_label),
        }));
    }

    Ok(GenerateObjectResponse { object, response })
}

/// Validate a value against the supported JSON Schema subset
/// (`type`, `enum`, `required`, `properties`, `items`), returning the first
/// violation as a human-readable path message. Schemas outside the subset
/// pass through unchecked (caller-supplied keyword coverage is opt-in, like
/// the ref dynamic mode where the handler validates what it needs).
fn validate_json_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    match schema {
        Value::Bool(true) | Value::Null => Ok(()),
        Value::Bool(false) => Err(format!("{}: schema forbids any value", at(path))),
        Value::Object(schema) => {
            if let Some(types) = schema.get("type") {
                check_type(value, types, path)?;
            }
            if let Some(enumeration) = schema.get("enum").and_then(Value::as_array) {
                if !enumeration.contains(value) {
                    return Err(format!(
                        "{}: expected one of {} but found {}",
                        at(path),
                        Value::Array(enumeration.clone()),
                        value
                    ));
                }
            }
            if let (Some(required), Value::Object(object)) =
                (schema.get("required").and_then(Value::as_array), value)
            {
                for name in required {
                    if let Some(name) = name.as_str() {
                        if !object.contains_key(name) {
                            return Err(format!(
                                "{}: missing required property `{name}`",
                                at(path)
                            ));
                        }
                    }
                }
            }
            if let (Some(properties), Value::Object(object)) = (schema.get("properties"), value) {
                for (name, property_schema) in properties.as_object().into_iter().flatten() {
                    if let Some(property) = object.get(name) {
                        validate_json_schema(property, property_schema, &format!("{path}/{name}"))?;
                    }
                }
            }
            if let Some(items) = schema.get("items") {
                if let Value::Array(elements) = value {
                    for (index, element) in elements.iter().enumerate() {
                        validate_json_schema(element, items, &format!("{path}/{index}"))?;
                    }
                }
            }
            Ok(())
        }
        other => Err(format!("{}: unsupported schema {other}", at(path))),
    }
}

fn check_type(value: &Value, types: &Value, path: &str) -> Result<(), String> {
    let matches = |name: &str| match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    };
    let ok = match types {
        Value::String(name) => matches(name),
        Value::Array(names) => names.iter().any(|name| name.as_str().is_none_or(matches)),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{}: expected type {} but found {}", at(path), types, json_type(value)))
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn at(path: &str) -> &str {
    if path.is_empty() {
        "value"
    } else {
        path
    }
}

/// Convenience re-export used by tests assembling finish events.
#[allow(dead_code)]
fn stop_finish(usage: Option<Usage>) -> LLMEvent {
    LLMEvent::Finish {
        reason: FinishReasonDetails::new(FinishReason::Stop),
        usage,
        provider_metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::route::protocol::{Protocol, ProtocolState};
    use crate::route::{Endpoint, Framing, Route, RouteInput};
    use crate::schema::{FinishReason, FinishReasonDetails, LLMRequest, Message};
    use crate::test_support::{MockTransport, RecordedInteraction};

    /// A protocol that answers every request with one synthetic tool call
    /// (configurable name/input) followed by a terminal finish, and whose
    /// request body embeds the prepared tools/toolChoice for assertions.
    struct ToolCallProtocol {
        tool_name: String,
        tool_input: Value,
    }

    impl Protocol for ToolCallProtocol {
        fn id(&self) -> &'static str {
            "tool-call-fixture"
        }

        fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|tool| match tool.as_definition() {
                    Some(definition) => json!({
                        "name": definition.name,
                        "description": definition.description,
                        "inputSchema": definition.input_schema,
                    }),
                    None => json!({"namespace": tool.name()}),
                })
                .collect();
            Ok(json!({
                "tools": tools,
                "toolChoice": match &request.tool_choice {
                    Some(ToolChoice::Named { name, .. }) => json!({ "type": "tool", "name": name }),
                    other => json!(other.is_none()),
                },
            }))
        }

        fn initial(&self, _request: &LLMRequest) -> ProtocolState {
            Box::new(())
        }

        fn step(
            &self,
            state: ProtocolState,
            _frame: &str,
        ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
            let events = vec![
                LLMEvent::ToolCall {
                    id: "call_1".into(),
                    name: self.tool_name.clone(),
                    namespace: None,
                    input: self.tool_input.clone(),
                    provider_executed: None,
                    provider_metadata: None,
                },
                LLMEvent::Finish {
                    reason: FinishReasonDetails::new(FinishReason::ToolCalls),
                    usage: Some(Usage::default().with_input_tokens(3.0).with_output_tokens(5.0)),
                    provider_metadata: None,
                },
            ];
            Ok((state, events))
        }

        fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
            Ok((state, Vec::new()))
        }
    }

    fn tool_call_route(tool_name: &str, tool_input: Value) -> (Arc<MockTransport>, Route) {
        let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://tool.test/call".into(),
            status: 200,
            body: "data: go\n\n".into(),
            request_body: None,
        }]));
        let route = Route::new(RouteInput {
            id: "tool-call".into(),
            provider: Some("test".into()),
            provider_metadata_key: None,
            protocol: Arc::new(ToolCallProtocol { tool_name: tool_name.into(), tool_input }),
            endpoint: Endpoint::path("/call").with_base_url("https://tool.test"),
            auth: None,
            framing: Framing::sse(),
            headers: None,
            transport: Arc::clone(&transport) as _,
            defaults: Default::default(),
        });
        (transport, route)
    }

    fn object_request(route: &Route) -> LLMRequest {
        LLMRequest::new(route.model("object-model", None).unwrap()).with_prompt("extract")
    }

    fn person_schema() -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" },
            }
        })
    }

    #[tokio::test]
    async fn extracts_the_object_and_carries_response_metadata() {
        let (_transport, route) =
            tool_call_route("generate_object", json!({ "name": "Ada", "age": 37 }));
        let result =
            generate_object(object_request(&route), person_schema()).await.expect("object");
        assert_eq!(result.object, json!({ "name": "Ada", "age": 37 }));
        // usage/events metadata rides along (ref GenerateObjectResponse getters)
        let usage = result.usage().expect("usage");
        assert_eq!(usage.input_tokens, Some(3.0));
        assert_eq!(usage.output_tokens, Some(5.0));
        assert!(result.events().iter().any(|event| event.kind() == "tool-call"));
        assert_eq!(result.response.usage.as_ref(), result.usage());
    }

    #[tokio::test]
    async fn forces_the_generate_object_tool_on_the_wire() {
        let (transport, route) = tool_call_route("generate_object", json!({ "name": "Ada" }));
        generate_object(object_request(&route), person_schema()).await.unwrap();
        let sent: Value = serde_json::from_slice(&transport.requests()[0].body).unwrap();
        // tools are replaced with exactly the forced synthetic tool…
        assert_eq!(sent["tools"].as_array().unwrap().len(), 1);
        assert_eq!(sent["tools"][0]["name"], "generate_object");
        assert_eq!(sent["tools"][0]["description"], GENERATE_OBJECT_TOOL_DESCRIPTION);
        assert_eq!(sent["tools"][0]["inputSchema"], person_schema());
        // …and the tool choice forces it by name
        assert_eq!(sent["toolChoice"], json!({ "type": "tool", "name": "generate_object" }));
    }

    #[tokio::test]
    async fn missing_tool_call_fails_with_invalid_provider_output() {
        // a route whose model answers with text only (echo protocol)
        let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
            method: "POST".into(),
            url: "https://echo.test/echo".into(),
            status: 200,
            body: "data: no tool here\n\n".into(),
            request_body: None,
        }]));
        let route = crate::test_support::echo_route(Arc::clone(&transport) as _);
        let error = generate_object(object_request(&route), person_schema())
            .await
            .expect_err("model did not call the tool");
        match error.reason {
            AiErrorReason::InvalidProviderOutput { .. } => {}
            other => panic!("expected InvalidProviderOutput, got {}", other.tag()),
        }
        assert!(
            error.message().contains("did not call the forced `generate_object` tool"),
            "got: {}",
            error.message()
        );
    }

    #[tokio::test]
    async fn schema_decode_failure_fails_with_invalid_provider_output() {
        // tool called, but input misses the required `name`
        let (_transport, route) =
            tool_call_route("generate_object", json!({ "age": "not a number" }));
        let error = generate_object(object_request(&route), person_schema())
            .await
            .expect_err("input violates the schema");
        match error.reason {
            AiErrorReason::InvalidProviderOutput { .. } => {}
            other => panic!("expected InvalidProviderOutput, got {}", other.tag()),
        }
        assert!(
            error.message().contains("tool input failed schema decode"),
            "got: {}",
            error.message()
        );
        // both the missing required property and the bad type are caught
        assert!(error.message().contains("missing required property `name`"));
    }

    #[test]
    fn validator_supports_the_schema_subset() {
        let schema = json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": { "type": "array", "items": { "type": "string", "enum": ["a", "b"] } },
            }
        });
        assert!(validate_json_schema(&json!({ "items": ["a", "b"] }), &schema, "").is_ok());
        assert!(validate_json_schema(&json!({}), &schema, "").is_err());
        assert!(validate_json_schema(&json!({ "items": ["a", "c"] }), &schema, "").is_err());
        assert!(validate_json_schema(&json!({ "items": [1] }), &schema, "").is_err());
        // bare `true` accepts anything; a typed schema rejects a null value
        assert!(validate_json_schema(&Value::Null, &json!(true), "").is_ok());
        assert!(validate_json_none(&schema).is_err());
    }

    fn validate_json_none(schema: &Value) -> Result<(), String> {
        validate_json_schema(&Value::Null, schema, "")
    }

    #[test]
    fn finish_helper_builds_a_terminal_event() {
        let event = stop_finish(Some(Usage::default().with_output_tokens(1.0)));
        assert_eq!(event.kind(), "finish");
        assert!(event.is_terminal());
    }

    #[test]
    fn base_request_messages_flow_through() {
        // the forced-tool patch must not disturb the caller's messages
        let (_transport, route) = tool_call_route("generate_object", json!({ "name": "x" }));
        let request = LLMRequest::new(route.model("m", None).unwrap())
            .with_system("be structured")
            .with_message(Message::user("hello"));
        let patched = request.update(LLMRequestPatch {
            tools: Some(vec![ToolEntry::Definition(ToolDefinition::new(
                "generate_object",
                "d",
                json!({}),
            ))]),
            tool_choice: Some(ToolChoice::named("generate_object")),
            ..Default::default()
        });
        assert_eq!(patched.messages.len(), 1);
        assert_eq!(patched.system.len(), 1);
    }
}
