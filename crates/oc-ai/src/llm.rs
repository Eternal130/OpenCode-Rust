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
//! mirrored by the shared JSON Schema validator ([`crate::json_schema`],
//! the same engine the tool decode pipeline uses — G-100) so malformed
//! tool input still surfaces as the ref's `InvalidProviderOutput`
//! "schema decode" error.

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

/// Validate a value against the supported JSON Schema subset, returning
/// the first violation as a human-readable path message. G-100: this now
/// delegates to the shared validator ([`crate::json_schema::validate_input`],
/// the same engine the tool decode pipeline uses) so the full keyword set
/// — `oneOf`/`anyOf`/`allOf`, `$defs` + `$ref`, string/numeric/array
/// bounds, `pattern`, `additionalProperties`, `uniqueItems` — is enforced
/// like the ref's Effect JsonSchema decode. Boolean schemas (`true`
/// accepts anything / `false` forbids everything) are handled here since
/// the shared engine only walks object schemas.
fn validate_json_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    match schema {
        Value::Bool(true) | Value::Null => Ok(()),
        Value::Bool(false) => Err(format!("{}: schema forbids any value", at(path))),
        _ => match crate::json_schema::validate_input(schema, value).into_iter().next() {
            None => Ok(()),
            Some(issue) => {
                let location = match issue.path.as_str() {
                    "" | "root" => at(path).to_string(),
                    issue_path if path.is_empty() => issue_path.to_string(),
                    issue_path => format!("{path}/{issue_path}"),
                };
                Err(format!("{location}: {}", issue.message))
            }
        },
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
        // (shared-validator issue format: `name: is required`)
        assert!(error.message().contains("name: is required"));
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

    /// G-100 — the output validator enforces the full keyword set of the
    /// shared engine (mirroring the ref's Effect JsonSchema decode):
    /// one valid + one invalid case per newly covered keyword.
    #[test]
    fn validator_covers_one_of_any_of_all_of() {
        let one_of = json!({
            "oneOf": [
                { "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"], "additionalProperties": false },
                { "type": "object", "properties": { "b": { "type": "integer" } }, "required": ["b"], "additionalProperties": false },
            ]
        });
        assert!(validate_json_schema(&json!({ "a": "x" }), &one_of, "").is_ok());
        assert!(validate_json_schema(&json!({ "a": "x", "b": 1 }), &one_of, "").is_err());
        assert!(validate_json_schema(&json!({ "c": true }), &one_of, "").is_err());

        let any_of = json!({ "anyOf": [{ "type": "string" }, { "type": "integer" }] });
        assert!(validate_json_schema(&json!("x"), &any_of, "").is_ok());
        assert!(validate_json_schema(&json!(true), &any_of, "").is_err());

        let all_of = json!({ "allOf": [{ "type": "string" }, { "minLength": 2 }] });
        assert!(validate_json_schema(&json!("ab"), &all_of, "").is_ok());
        assert!(validate_json_schema(&json!("a"), &all_of, "").is_err());
    }

    #[test]
    fn validator_resolves_defs_refs() {
        let schema = json!({
            "$defs": { "hex": { "type": "string", "pattern": "^[0-9a-f]+$" } },
            "type": "object",
            "properties": { "hash": { "$ref": "#/$defs/hex" } }
        });
        assert!(validate_json_schema(&json!({ "hash": "deadbeef" }), &schema, "").is_ok());
        assert!(validate_json_schema(&json!({ "hash": "XYZ" }), &schema, "").is_err());
    }

    #[test]
    fn validator_enforces_string_and_numeric_bounds() {
        let text =
            json!({ "type": "string", "minLength": 2, "maxLength": 4, "pattern": "^[a-z]+$" });
        assert!(validate_json_schema(&json!("abc"), &text, "").is_ok());
        assert!(validate_json_schema(&json!("a"), &text, "").is_err());
        assert!(validate_json_schema(&json!("abcde"), &text, "").is_err());
        assert!(validate_json_schema(&json!("ABC"), &text, "").is_err());

        let number = json!({ "type": "number", "minimum": 1, "maximum": 10 });
        assert!(validate_json_schema(&json!(5), &number, "").is_ok());
        assert!(validate_json_schema(&json!(0), &number, "").is_err());
        assert!(validate_json_schema(&json!(11), &number, "").is_err());
    }

    #[test]
    fn validator_enforces_array_bounds_and_unique_items() {
        let schema = json!({ "type": "array", "minItems": 1, "maxItems": 3, "uniqueItems": true });
        assert!(validate_json_schema(&json!([1, 2]), &schema, "").is_ok());
        assert!(validate_json_schema(&json!([]), &schema, "").is_err());
        assert!(validate_json_schema(&json!([1, 2, 3, 4]), &schema, "").is_err());
        assert!(validate_json_schema(&json!([1, 1]), &schema, "").is_err());
    }

    #[test]
    fn validator_enforces_additional_properties_and_type_unions() {
        let closed = json!({
            "type": "object",
            "properties": { "known": { "type": "string" } },
            "additionalProperties": false
        });
        assert!(validate_json_schema(&json!({ "known": "x" }), &closed, "").is_ok());
        assert!(validate_json_schema(&json!({ "known": "x", "extra": 1 }), &closed, "").is_err());

        let typed = json!({ "additionalProperties": { "type": "string" } });
        assert!(validate_json_schema(&json!({ "any": "ok" }), &typed, "").is_ok());
        assert!(validate_json_schema(&json!({ "any": 5 }), &typed, "").is_err());

        // `type` unions (common in generateObject schemas) still work.
        let union = json!({ "type": ["string", "null"] });
        assert!(validate_json_schema(&json!("x"), &union, "").is_ok());
        assert!(validate_json_none(&union).is_ok());
        assert!(validate_json_schema(&json!(1), &union, "").is_err());
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
