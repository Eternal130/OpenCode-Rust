//! Port of `test/provider/gemini.test.ts` (request lowering + stream
//! machine) plus cassette-parity replay against the TS dumps in
//! `test-fixtures/expected/gemini/`.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::route::auth::{self, Secret};
use crate::route::framing::FramingDecoder;
use crate::route::{EndpointPatch, Framing, Route, RoutePatch, SharedTransport};
use crate::schema::media;
use crate::schema::{
    ContentPart, GenerationOptions, LLMEvent, LLMRequest, LanguageModel, Message, MessageRole,
    SystemPart, ToolDefinition, ToolResultValue,
};
use crate::test_support::{Cassette, MockTransport, RecordedInteraction};

use super::{
    gemini_route, google_auth, google_route, GeminiProtocol, GoogleConfig, DEFAULT_BASE_URL,
};
use crate::route::protocol::Protocol as _;

// =============================================================================
// Helpers (ports of the TS test fixtures)
// =============================================================================

fn sse_events(chunks: &[Value]) -> String {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str("data: ");
        body.push_str(&serde_json::to_string(chunk).unwrap());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

fn sse_raw(lines: &[&str]) -> String {
    let mut body = String::new();
    for line in lines {
        body.push_str(line);
        body.push_str("\n\n");
    }
    body
}

fn interaction(body: &str) -> RecordedInteraction {
    RecordedInteraction {
        method: "POST".into(),
        url: "https://generativelanguage.test/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse".into(),
        status: 200,
        body: body.into(),
        request_body: None,
    }
}

fn test_route(transport: SharedTransport) -> Route {
    gemini_route(transport).with(RoutePatch {
        endpoint: Some(EndpointPatch {
            base_url: Some("https://generativelanguage.test/v1beta/".into()),
            path: None,
            query: None,
        }),
        auth: Some(auth::Definition::header("x-goog-api-key", Secret::Value("test".into()))),
        ..Default::default()
    })
}

struct Fixture {
    route: Route,
}

fn fixture(body: &str) -> Fixture {
    let transport: SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![interaction(body)]));
    Fixture { route: test_route(transport) }
}

impl Fixture {
    fn model(&self, id: &str) -> LanguageModel {
        self.route.model(id, None).unwrap()
    }

    async fn run(
        &self,
        request: LLMRequest,
    ) -> Result<crate::schema::LLMResponse, crate::schema::AiError> {
        self.route.generate(request).await
    }
}

fn model_2_5(route: &Route) -> LanguageModel {
    route.model("gemini-2.5-flash", None).unwrap()
}

fn gemini_3(route: &Route) -> LanguageModel {
    route.model("gemini-3-flash-preview", None).unwrap()
}

fn base_request(model: &LanguageModel) -> LLMRequest {
    let mut request = LLMRequest::new(model.clone()).with_prompt("Say hello.");
    request.system = vec![SystemPart::new("You are concise.")];
    request.generation = Some(GenerationOptions {
        max_tokens: Some(20.0),
        temperature: Some(0.0),
        ..Default::default()
    });
    request
}

fn empty_body_route() -> Fixture {
    fixture(&sse_events(&[json!({})]))
}

fn compile_body(route: &Route, request: &LLMRequest) -> Value {
    let prepared = route.prepare_request(request);
    route.build_body(&prepared).unwrap()
}

/// Normalize a serialized event for comparison against the TS wire form.
/// The `LLMEvent` tree serializes in the contract's `camelCase` shape
/// natively (oracle gate 3 finding 1); only numbers still need `f64`
/// coercion (serde `u64`-vs-`f64`).
fn ts_wire(value: &Value) -> Value {
    match value {
        Value::Number(number) => json!(number.as_f64().unwrap_or_default()),
        Value::Array(items) => Value::Array(items.iter().map(ts_wire).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(key, value)| (key.clone(), ts_wire(value))).collect())
        }
        other => other.clone(),
    }
}

/// `ContentPart` (the assembled message tree, NOT the `LLMEvent` tree)
/// still serializes `provider_metadata` as `snake_case`; its wire shape is owned
/// by the session-persistence lane, so rename per-site here.
fn content_wire(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(content_wire).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let key = if key == "provider_metadata" { "providerMetadata" } else { key };
                    (key.to_string(), content_wire(value))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn events_json(events: &[LLMEvent]) -> Vec<Value> {
    events.iter().map(|event| ts_wire(&event_value_of(event).unwrap())).collect()
}

fn event_value_of<T: serde::Serialize>(value: &T) -> Result<Value, serde_json::Error> {
    serde_json::to_value(value)
}

fn event_of_kind<'a>(events: &'a [LLMEvent], kind: &str) -> Option<&'a LLMEvent> {
    events.iter().find(|event| event.kind() == kind)
}

fn tool_call_part(id: &str, name: &str, input: Value) -> ContentPart {
    ContentPart::ToolCall {
        id: id.into(),
        name: name.into(),
        namespace: None,
        input,
        provider_executed: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    }
}

fn tool_call_part_signed(id: &str, name: &str, input: Value, signature: &str) -> ContentPart {
    let metadata: crate::schema::ProviderMetadata = std::iter::once((
        "google".to_string(),
        std::iter::once(("thoughtSignature".to_string(), Value::String(signature.to_string())))
            .collect(),
    ))
    .collect();
    ContentPart::ToolCall {
        id: id.into(),
        name: name.into(),
        namespace: None,
        input,
        provider_executed: None,
        cache: None,
        metadata: None,
        provider_metadata: Some(metadata),
    }
}

fn text_part_with_metadata(text: &str, signature: &str) -> ContentPart {
    ContentPart::Text {
        text: text.into(),
        cache: None,
        metadata: None,
        provider_metadata: Some(
            std::iter::once((
                "google".to_string(),
                std::iter::once((
                    "thoughtSignature".to_string(),
                    Value::String(signature.to_string()),
                ))
                .collect(),
            ))
            .collect(),
        ),
    }
}

fn tool_result_text(id: &str, name: &str, text: &str) -> Message {
    Message::tool_result(id, name, ToolResultValue::Text { value: json!(text) })
}

// =============================================================================
// Request lowering
// =============================================================================

#[test]
fn gemini_prepares_target() {
    let fixture = empty_body_route();
    let body = compile_body(&fixture.route, &base_request(&fixture.model("gemini-2.5-flash")));
    assert_eq!(
        body,
        json!({
            "contents": [{ "role": "user", "parts": [{ "text": "Say hello." }] }],
            "systemInstruction": { "parts": [{ "text": "You are concise." }] },
            "generationConfig": { "maxOutputTokens": 20.0, "temperature": 0.0 }
        })
    );
}

#[test]
fn gemini_normalizes_thinking_options() {
    let fixture = empty_body_route();
    let model = fixture.model("gemini-2.5-flash");
    let with_options = |options: Value| {
        let mut request = base_request(&model);
        request.provider_options = Some(options.as_object().unwrap().clone());
        compile_body(&fixture.route, &request)
    };

    let prepared = with_options(json!({
        "cachedContent": "cachedContents/example",
        "safetySettings": [{ "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_ONLY_HIGH" }],
        "serviceTier": "priority",
        "thinkingConfig": { "thinkingBudget": 0, "includeThoughts": false, "thinkingLevel": "high" }
    }));
    let filtered = with_options(json!({
        "thinkingConfig": { "thinkingBudget": "invalid", "includeThoughts": false }
    }));
    let defaulted = with_options(json!({ "thinkingConfig": { "thinkingLevel": "high" } }));
    let empty_safety = with_options(json!({ "safetySettings": [] }));

    assert_eq!(
        prepared["generationConfig"]["thinkingConfig"],
        json!({ "thinkingBudget": 0.0, "includeThoughts": false, "thinkingLevel": "high" })
    );
    assert_eq!(prepared["cachedContent"], json!("cachedContents/example"));
    assert_eq!(
        prepared["safetySettings"],
        json!([{ "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_ONLY_HIGH" }])
    );
    assert_eq!(prepared["serviceTier"], json!("priority"));
    assert_eq!(filtered["generationConfig"]["thinkingConfig"], json!({ "includeThoughts": false }));
    assert_eq!(
        defaulted["generationConfig"]["thinkingConfig"],
        json!({ "includeThoughts": true, "thinkingLevel": "high" })
    );
    assert_eq!(empty_safety["safetySettings"], json!([]));
}

#[test]
fn gemini_fits_the_thinking_budget_to_half_the_output_limit() {
    let fixture = empty_body_route();
    let model = fixture.model("gemini-2.5-flash");
    let budget = |budget: f64, max_tokens: f64| {
        let mut request = base_request(&model);
        request.generation = Some(GenerationOptions {
            max_tokens: Some(max_tokens),
            temperature: Some(0.0),
            ..Default::default()
        });
        request.provider_options = Some(
            json!({ "thinkingConfig": { "thinkingBudget": budget } }).as_object().unwrap().clone(),
        );
        compile_body(&fixture.route, &request)["generationConfig"]["thinkingConfig"]
            ["thinkingBudget"]
            .as_f64()
            .unwrap()
    };

    assert_eq!(budget(32_768.0, 32_000.0), 16_000.0);
    assert_eq!(budget(8_000.0, 32_000.0), 8_000.0);
    assert_eq!(budget(-1.0, 32_000.0), -1.0);
    assert_eq!(budget(8_192.0, 1_000.0), 512.0);
}

#[test]
fn gemini_forwards_standard_generation_options() {
    let fixture = empty_body_route();
    let model = fixture.model("gemini-2.5-flash");
    let mut request = LLMRequest::new(model).with_prompt("Say hello.");
    request.generation = Some(GenerationOptions {
        max_tokens: Some(40.0),
        temperature: Some(0.2),
        top_p: Some(0.8),
        top_k: Some(12.0),
        frequency_penalty: Some(0.3),
        presence_penalty: Some(0.4),
        seed: Some(42.0),
        stop: Some(vec!["done".into()]),
    });
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["generationConfig"],
        json!({
            "maxOutputTokens": 40.0,
            "temperature": 0.2,
            "topP": 0.8,
            "topK": 12.0,
            "frequencyPenalty": 0.3,
            "presencePenalty": 0.4,
            "seed": 42.0,
            "stopSequences": ["done"]
        })
    );
}

#[test]
fn gemini_lowers_chronological_system_updates_to_wrapped_user_text_in_order() {
    let fixture = empty_body_route();
    let model = fixture.model("gemini-2.5-flash");
    let request = LLMRequest::new(model)
        .with_message(Message::user("Before."))
        .with_message(Message::system("Update."))
        .with_message(Message::assistant(vec![ContentPart::text("After.")]));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            { "role": "user", "parts": [{ "text": "Before." }, { "text": "<system-update>\nUpdate.\n</system-update>" }] },
            { "role": "model", "parts": [{ "text": "After." }] }
        ])
    );
}

#[test]
fn gemini_keeps_system_updates_separate_from_function_responses() {
    let fixture = empty_body_route();
    let model = fixture.model("gemini-2.5-flash");
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![tool_call_part(
            "call_1",
            "lookup",
            json!({ "query": "weather" }),
        )]))
        .with_message(tool_result_text("call_1", "lookup", "done"))
        .with_message(Message::system("Update."))
        .with_message(Message::system("Later update."));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": { "query": "weather" } } }] },
            {
                "role": "user",
                "parts": [{ "functionResponse": { "name": "lookup", "response": { "name": "lookup", "content": "done" } } }]
            },
            {
                "role": "user",
                "parts": [
                    { "text": "<system-update>\nUpdate.\n</system-update>" },
                    { "text": "<system-update>\nLater update.\n</system-update>" }
                ]
            }
        ])
    );
}

#[test]
fn gemini_merges_parallel_tool_results_into_one_function_response_turn() {
    let fixture = empty_body_route();
    let model = fixture.model("gemini-2.5-flash");
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![
            tool_call_part("call_1", "lookup", json!({ "query": "weather" })),
            tool_call_part("call_2", "lookup", json!({ "query": "time" })),
        ]))
        .with_message(tool_result_text("call_1", "lookup", "sunny"))
        .with_message(tool_result_text("call_2", "lookup", "noon"));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            {
                "role": "model",
                "parts": [
                    { "functionCall": { "name": "lookup", "args": { "query": "weather" } } },
                    { "functionCall": { "name": "lookup", "args": { "query": "time" } } }
                ]
            },
            {
                "role": "user",
                "parts": [
                    { "functionResponse": { "name": "lookup", "response": { "name": "lookup", "content": "sunny" } } },
                    { "functionResponse": { "name": "lookup", "response": { "name": "lookup", "content": "noon" } } }
                ]
            }
        ])
    );
}

#[test]
fn gemini_lowers_function_call_ids_for_gemini_3_models() {
    let fixture = empty_body_route();
    let model = gemini_3(&fixture.route);
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![tool_call_part(
            "call_1",
            "lookup",
            json!({ "query": "weather" }),
        )]))
        .with_message(tool_result_text("call_1", "lookup", "done"));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            {
                "role": "model",
                "parts": [
                    {
                        "functionCall": { "id": "call_1", "name": "lookup", "args": { "query": "weather" } },
                        "thoughtSignature": "skip_thought_signature_validator"
                    }
                ]
            },
            {
                "role": "user",
                "parts": [
                    {
                        "functionResponse": {
                            "id": "call_1",
                            "name": "lookup",
                            "response": { "name": "lookup", "content": "done" }
                        }
                    }
                ]
            }
        ])
    );
}

#[test]
fn gemini_omits_function_call_ids_entirely_for_pre_gemini_3_models() {
    let fixture = empty_body_route();
    let messages = vec![
        Message::assistant(vec![tool_call_part("call_1", "lookup", json!({ "query": "weather" }))]),
        tool_result_text("call_1", "lookup", "done"),
    ];
    let with_messages = |model: LanguageModel| {
        let mut request = LLMRequest::new(model);
        for message in messages.clone() {
            request = request.with_message(message);
        }
        request
    };
    let legacy = compile_body(&fixture.route, &with_messages(model_2_5(&fixture.route)));
    let older = compile_body(
        &fixture.route,
        &with_messages(fixture.route.model("gemini-1.5-flash", None).unwrap()),
    );
    assert_eq!(
        legacy["contents"],
        json!([
            { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": { "query": "weather" } } }] },
            { "role": "user", "parts": [{ "functionResponse": { "name": "lookup", "response": { "name": "lookup", "content": "done" } } }] }
        ])
    );
    assert!(!serde_json::to_string(&legacy["contents"]).unwrap().contains("\"id\""));
    assert!(!serde_json::to_string(&older["contents"]).unwrap().contains("\"id\""));
}

#[test]
fn gemini_includes_function_call_ids_for_non_gemini_model_ids() {
    let fixture = empty_body_route();
    let model = fixture.route.model("gemma-3-27b-it", None).unwrap();
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![tool_call_part(
            "call_1",
            "lookup",
            json!({ "query": "weather" }),
        )]))
        .with_message(tool_result_text("call_1", "lookup", "done"));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            { "role": "model", "parts": [{ "functionCall": { "id": "call_1", "name": "lookup", "args": { "query": "weather" } } }] },
            {
                "role": "user",
                "parts": [{ "functionResponse": { "id": "call_1", "name": "lookup", "response": { "name": "lookup", "content": "done" } } }]
            }
        ])
    );
}

#[test]
fn gemini_prepares_multimodal_user_input_and_tool_history() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let mut request = LLMRequest::new(model)
        .with_message(Message::new(
            MessageRole::User,
            vec![
                ContentPart::text("What is in this image?"),
                ContentPart::Media {
                    media: media::base64("AAECAw==", "image/png"),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::Media {
                    media: media::base64("JVBERi0xLjQ=", "application/pdf"),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ],
        ))
        .with_message(Message::assistant(vec![tool_call_part(
            "call_1",
            "lookup",
            json!({ "query": "weather" }),
        )]))
        .with_message(Message::tool_result(
            "call_1",
            "lookup",
            ToolResultValue::Json { value: json!({ "forecast": "sunny" }) },
        ));
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
    ))];
    request.tool_choice = Some(crate::schema::ToolChoice::named("lookup"));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body,
        json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        { "text": "What is in this image?" },
                        { "inlineData": { "mimeType": "image/png", "data": "AAECAw==" } },
                        { "inlineData": { "mimeType": "application/pdf", "data": "JVBERi0xLjQ=" } }
                    ]
                },
                { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": { "query": "weather" } } }] },
                {
                    "role": "user",
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "lookup",
                                "response": { "name": "lookup", "content": "{\"forecast\":\"sunny\"}" }
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "lookup",
                            "description": "Lookup data",
                            "parametersJsonSchema": { "type": "object", "properties": { "query": { "type": "string" } } }
                        }
                    ]
                }
            ],
            "toolConfig": { "functionCallingConfig": { "mode": "ANY", "allowedFunctionNames": ["lookup"] } }
        })
    );
}

fn content_result(items: Value) -> ToolResultValue {
    ToolResultValue::Content {
        value: crate::schema::ToolContentItems(items.as_array().unwrap().clone()),
    }
}

#[test]
fn gemini_continues_media_tool_results_as_inline_model_input_without_base64_text() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![tool_call_part(
            "call_image",
            "read",
            json!({ "path": "pixel.png" }),
        )]))
        .with_message(Message::tool_result(
            "call_image",
            "read",
            content_result(json!([
                { "type": "text", "text": "Image read successfully" },
                { "type": "file", "uri": "data:image/png;base64,AAECAw==", "mime": "image/png", "name": "pixel.png" },
                { "type": "file", "uri": "data:application/pdf;base64,JVBERi0xLjQ=", "mime": "application/pdf" }
            ])),
        ));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            { "role": "model", "parts": [{ "functionCall": { "name": "read", "args": { "path": "pixel.png" } } }] },
            {
                "role": "user",
                "parts": [
                    { "functionResponse": { "name": "read", "response": { "name": "read", "content": "Image read successfully" } } }
                ]
            },
            {
                "role": "user",
                "parts": [
                    { "text": "Attached media from tool result:" },
                    { "inlineData": { "mimeType": "image/png", "data": "AAECAw==" } },
                    { "inlineData": { "mimeType": "application/pdf", "data": "JVBERi0xLjQ=" } }
                ]
            }
        ])
    );
    assert!(!serde_json::to_string(&body["contents"])
        .unwrap()
        .contains("\"content\":\"AAECAw==\""));
}

#[test]
fn gemini_strips_matching_data_urls_to_raw_base64() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let request = LLMRequest::new(model)
        .with_message(Message::new(
            MessageRole::User,
            vec![ContentPart::Media {
                media: media::parse_data_url("data:image/png;base64,AAEC").unwrap(),
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        ))
        .with_message(Message::tool_result(
            "call_image",
            "read",
            content_result(json!([
                { "type": "file", "uri": "data:image/jpeg;base64,/9j/", "mime": "image/jpeg" }
            ])),
        ));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            { "role": "user", "parts": [{ "inlineData": { "mimeType": "image/png", "data": "AAEC" } }] },
            {
                "role": "user",
                "parts": [{ "functionResponse": { "name": "read", "response": { "name": "read", "content": "" } } }]
            },
            {
                "role": "user",
                "parts": [
                    { "text": "Attached media from tool result:" },
                    { "inlineData": { "mimeType": "image/jpeg", "data": "/9j/" } }
                ]
            }
        ])
    );
}

#[test]
fn gemini_nests_media_inside_function_responses_for_gemini_3() {
    let fixture = empty_body_route();
    let model = gemini_3(&fixture.route);
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![tool_call_part_signed(
            "call_image",
            "read",
            json!({ "path": "pixel.png" }),
            "sig_1",
        )]))
        .with_message(Message::tool_result(
            "call_image",
            "read",
            content_result(json!([
                { "type": "text", "text": "Image read successfully" },
                { "type": "file", "uri": "data:image/png;base64,AAECAw==", "mime": "image/png", "name": "pixel.png" }
            ])),
        ));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            {
                "role": "model",
                "parts": [
                    {
                        "functionCall": { "id": "call_image", "name": "read", "args": { "path": "pixel.png" } },
                        "thoughtSignature": "sig_1"
                    }
                ]
            },
            {
                "role": "user",
                "parts": [
                    {
                        "functionResponse": {
                            "id": "call_image",
                            "name": "read",
                            "response": { "name": "read", "content": "Image read successfully" },
                            "parts": [{ "inlineData": { "mimeType": "image/png", "data": "AAECAw==" } }]
                        }
                    }
                ]
            }
        ])
    );
}

#[test]
fn gemini_flushes_pending_media_before_system_update_text() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![tool_call_part("call_1", "shot", json!({}))]))
        .with_message(Message::tool_result(
            "call_1",
            "shot",
            content_result(json!([
                { "type": "file", "uri": "data:image/png;base64,AAEC", "mime": "image/png" }
            ])),
        ))
        .with_message(Message::system("Update."));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            { "role": "model", "parts": [{ "functionCall": { "name": "shot", "args": {} } }] },
            { "role": "user", "parts": [{ "functionResponse": { "name": "shot", "response": { "name": "shot", "content": "" } } }] },
            {
                "role": "user",
                "parts": [
                    { "text": "Attached media from tool result:" },
                    { "inlineData": { "mimeType": "image/png", "data": "AAEC" } },
                    { "text": "<system-update>\nUpdate.\n</system-update>" }
                ]
            }
        ])
    );
}

#[test]
fn gemini_collects_legacy_tool_media_into_one_turn_after_merged_responses() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let request = LLMRequest::new(model)
        .with_message(Message::assistant(vec![
            tool_call_part("call_1", "shot", json!({})),
            tool_call_part("call_2", "shot", json!({})),
        ]))
        .with_message(Message::tool_result(
            "call_1",
            "shot",
            content_result(json!([
                { "type": "file", "uri": "data:image/png;base64,AAEC", "mime": "image/png" }
            ])),
        ))
        .with_message(Message::tool_result(
            "call_2",
            "shot",
            content_result(json!([{ "type": "text", "text": "no image here" }])),
        ));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            {
                "role": "model",
                "parts": [
                    { "functionCall": { "name": "shot", "args": {} } },
                    { "functionCall": { "name": "shot", "args": {} } }
                ]
            },
            {
                "role": "user",
                "parts": [
                    { "functionResponse": { "name": "shot", "response": { "name": "shot", "content": "" } } },
                    { "functionResponse": { "name": "shot", "response": { "name": "shot", "content": "no image here" } } }
                ]
            },
            {
                "role": "user",
                "parts": [
                    { "text": "Attached media from tool result:" },
                    { "inlineData": { "mimeType": "image/png", "data": "AAEC" } }
                ]
            }
        ])
    );
}

#[test]
fn gemini_passes_encoded_media_through_without_local_validation() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let media_part = |asset| ContentPart::Media {
        media: asset,
        filename: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    };
    let request = LLMRequest::new(model).with_message(Message::new(
        MessageRole::User,
        vec![
            media_part(media::base64("%%%=", "image/png")),
            media_part(media::parse_data_url("data:image/jpeg;base64,/9j/").unwrap()),
            media_part(media::base64("PHN2Zz4=", "image/svg+xml")),
        ],
    ));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            {
                "role": "user",
                "parts": [
                    { "inlineData": { "mimeType": "image/png", "data": "%%%=" } },
                    { "inlineData": { "mimeType": "image/jpeg", "data": "/9j/" } },
                    { "inlineData": { "mimeType": "image/svg+xml", "data": "PHN2Zz4=" } }
                ]
            }
        ])
    );
}

#[test]
fn gemini_keeps_tools_and_sends_function_calling_mode_none() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let mut request = LLMRequest::new(model).with_prompt("Say hello.");
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    request.tool_choice = Some(crate::schema::ToolChoice::none());
    let body = compile_body(&fixture.route, &request);
    assert_eq!(body["contents"], json!([{ "role": "user", "parts": [{ "text": "Say hello." }] }]));
    assert_eq!(
        body["tools"],
        json!([{ "functionDeclarations": [{ "name": "lookup", "description": "Lookup data", "parametersJsonSchema": { "type": "object" } }] }])
    );
    assert_eq!(body["toolConfig"], json!({ "functionCallingConfig": { "mode": "NONE" } }));
}

#[test]
fn gemini_normalizes_the_json_schema_shapes_gemini_rejects() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let mut request = LLMRequest::new(model).with_prompt("Use the tool.");
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({
            "type": "object",
            "required": ["ttl", "items", "parent", "missing"],
            "properties": {
                "ttl": { "type": "number", "minimum": 0, "exclusiveMinimum": true, "maximum": 60, "exclusiveMaximum": false },
                "pair": { "type": "array", "items": [{ "type": "string" }, { "type": "integer" }], "additionalItems": false },
                "items": { "type": "array", "items": { "type": "string" } },
                "required": { "type": "boolean" },
                "params": { "$ref": "#/$defs/Json" },
                "tree": { "$ref": "#/$defs/Node" },
                "chain": { "$ref": "#/$defs/Link" },
                "batch": { "$ref": "#/$defs/Batch" },
                "self": { "$ref": "#" },
                "parent": { "$ref": "#" },
                "cart": { "type": "object", "default": { "items": ["a"] } }
            },
            "dependentRequired": { "items": ["ttl"] },
            "$defs": {
                "Json": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "$ref": "#/$defs/Json" } },
                        { "type": "object", "additionalProperties": { "$ref": "#/$defs/Json" } }
                    ]
                },
                "Node": { "type": "object", "properties": { "child": { "$ref": "#/$defs/Node" } } },
                "Link": { "type": "object", "required": ["next"], "properties": { "next": { "$ref": "#/$defs/Link" } } },
                "Batch": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/Batch" } }
            }
        }),
    ))];
    let body = compile_body(&fixture.route, &request);
    let expected = json!({
        "type": "object",
        "required": ["ttl", "items", "parent"],
        "properties": {
            "ttl": { "type": "number", "exclusiveMinimum": 0, "maximum": 60 },
            "pair": { "type": "array", "prefixItems": [{ "type": "string" }, { "type": "integer" }], "items": false },
            "items": { "type": "array", "items": { "type": "string" } },
            "required": { "type": "boolean" },
            "params": { "$ref": "#/$defs/Json" },
            "tree": { "$ref": "#/$defs/Node" },
            "chain": { "$ref": "#/$defs/Link" },
            "batch": { "$ref": "#/$defs/Batch" },
            "self": { "$ref": "#" },
            "parent": {},
            "cart": { "type": "object", "default": { "items": ["a"] } }
        },
        "dependentRequired": { "items": ["ttl"] },
        "$defs": {
            "Json": {
                "anyOf": [
                    { "type": "string" },
                    { "type": "array", "items": { "$ref": "#/$defs/Json" } },
                    { "type": "object", "additionalProperties": {} }
                ]
            },
            "Node": { "type": "object", "properties": { "child": { "$ref": "#/$defs/Node" } } },
            "Link": { "type": "object", "required": ["next"], "properties": { "next": {} } },
            "Batch": { "type": "array", "minItems": 1, "items": {} }
        }
    });
    assert_eq!(
        ts_wire(&body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]),
        ts_wire(&expected)
    );
}

#[test]
fn gemini_preserves_nested_empty_object_tool_schemas() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let mut request = LLMRequest::new(model).with_prompt("Use the tool.");
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "configure",
        "Configure the operation",
        json!({
            "type": "object",
            "required": ["options"],
            "properties": {
                "options": { "type": "object", "description": "Optional provider settings", "properties": {} }
            }
        }),
    ))];
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["tools"],
        json!([
            {
                "functionDeclarations": [
                    {
                        "name": "configure",
                        "description": "Configure the operation",
                        "parametersJsonSchema": {
                            "type": "object",
                            "required": ["options"],
                            "properties": {
                                "options": { "type": "object", "description": "Optional provider settings", "properties": {} }
                            }
                        }
                    }
                ]
            }
        ])
    );
}

#[test]
fn gemini_sends_standard_json_schema_tool_parameters_unchanged() {
    let input_schema = json!({
        "type": "object",
        "required": ["level"],
        "properties": {
            "level": { "type": "integer", "enum": [1, 2, 3] },
            "status": { "type": ["number", "string"], "description": "Status filter" },
            "maybe": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
            "kind": { "const": "fixed" },
            "mode": { "enum": ["fast", "safe"] },
            "tags": { "type": "array" },
            "env": { "type": "object", "additionalProperties": { "type": "string" } },
            "action": {
                "oneOf": [
                    { "type": "object", "properties": { "type": { "const": "move" }, "to": { "type": "string" } } },
                    { "type": "object", "properties": { "type": { "const": "delete" } } }
                ]
            },
            "item": { "$ref": "#/$defs/Item" }
        },
        "$defs": { "Item": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] } }
    });
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let mut request = LLMRequest::new(model).with_prompt("Use the tool.");
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "filter",
        "Filter values",
        input_schema.clone(),
    ))];
    let body = compile_body(&fixture.route, &request);
    assert_eq!(body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"], input_schema);
}

#[test]
fn gemini_replays_generated_assistant_media_as_model_inline_data() {
    let fixture = empty_body_route();
    let model = model_2_5(&fixture.route);
    let media_part = |signature: &str| ContentPart::Media {
        media: media::base64("AAECAw==", "image/png"),
        filename: None,
        cache: None,
        metadata: None,
        provider_metadata: Some(
            std::iter::once((
                "google".to_string(),
                std::iter::once((
                    "thoughtSignature".to_string(),
                    Value::String(signature.to_string()),
                ))
                .collect(),
            ))
            .collect(),
        ),
    };
    let request = LLMRequest::new(model)
        .with_message(Message::user("Draw a cat"))
        .with_message(Message::assistant(vec![
            ContentPart::text("Here you go."),
            media_part("sig_1"),
        ]))
        .with_message(Message::user("Now make it orange"));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"][1],
        json!({
            "role": "model",
            "parts": [
                { "text": "Here you go." },
                { "inlineData": { "mimeType": "image/png", "data": "AAECAw==" }, "thoughtSignature": "sig_1" }
            ]
        })
    );
}

// =============================================================================
// Stream parsing
// =============================================================================

#[tokio::test]
async fn gemini_parses_text_reasoning_and_usage_stream_fixtures() {
    let body = sse_events(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "thinking", "thought": true }] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hello" }] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "!" }] }, "finishReason": "STOP" }] }),
        json!({ "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 2, "totalTokenCount": 7, "thoughtsTokenCount": 1, "cachedContentTokenCount": 1 } }),
    ]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();

    assert_eq!(response.text(), "Hello!");
    assert_eq!(response.reasoning(), "thinking");
    let usage = json!({
        "inputTokens": 5.0,
        "outputTokens": 3.0,
        "nonCachedInputTokens": 4.0,
        "cacheReadInputTokens": 1.0,
        "reasoningTokens": 1.0,
        "totalTokens": 7.0,
        "providerMetadata": {
            "google": {
                "promptTokenCount": 5.0,
                "candidatesTokenCount": 2.0,
                "totalTokenCount": 7.0,
                "thoughtsTokenCount": 1.0,
                "cachedContentTokenCount": 1.0
            }
        }
    });
    assert_eq!(
        events_json(&response.events),
        vec![
            json!({ "type": "step-start", "index": 0.0 }),
            json!({ "type": "reasoning-start", "id": "reasoning-0" }),
            json!({ "type": "reasoning-delta", "id": "reasoning-0", "text": "thinking" }),
            json!({ "type": "reasoning-end", "id": "reasoning-0" }),
            json!({ "type": "text-start", "id": "text-0" }),
            json!({ "type": "text-delta", "id": "text-0", "text": "Hello" }),
            json!({ "type": "text-delta", "id": "text-0", "text": "!" }),
            json!({ "type": "text-end", "id": "text-0" }),
            json!({
                "type": "step-finish",
                "index": 0.0,
                "reason": { "normalized": "stop", "raw": "STOP" },
                "usage": usage
            }),
            json!({
                "type": "finish",
                "reason": { "normalized": "stop", "raw": "STOP" },
                "usage": usage
            }),
        ]
    );
}

#[tokio::test]
async fn gemini_assigns_unique_ids_to_separated_reasoning_blocks() {
    let body = sse_events(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "A", "thought": true, "thoughtSignature": "reasoning_sig_a" }] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "X", "thoughtSignature": "text_sig_x" }] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "B", "thought": true, "thoughtSignature": "reasoning_sig_b" }] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Y", "thoughtSignature": "text_sig_y" }] }, "finishReason": "STOP" }] }),
    ]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();

    let ids = |kind: &str| -> Vec<Value> {
        response
            .events
            .iter()
            .filter(|event| event.kind() == kind)
            .map(|event| event_value_of(event).unwrap().get("id").cloned().unwrap())
            .collect()
    };
    assert_eq!(Value::Array(ids("reasoning-start")), json!(["reasoning-0", "reasoning-1"]));
    assert_eq!(Value::Array(ids("reasoning-end")), json!(["reasoning-0", "reasoning-1"]));
    assert_eq!(Value::Array(ids("text-start")), json!(["text-0", "text-1"]));
    assert_eq!(Value::Array(ids("text-delta")), json!(["text-0", "text-1"]));
    assert_eq!(Value::Array(ids("text-end")), json!(["text-0", "text-1"]));
    assert_eq!(
        content_wire(&ts_wire(&event_value_of(&response.message.content).unwrap())),
        json!([
            { "type": "reasoning", "text": "A", "providerMetadata": { "google": { "thoughtSignature": "reasoning_sig_a" } } },
            { "type": "text", "text": "X", "providerMetadata": { "google": { "thoughtSignature": "text_sig_x" } } },
            { "type": "reasoning", "text": "B", "providerMetadata": { "google": { "thoughtSignature": "reasoning_sig_b" } } },
            { "type": "text", "text": "Y", "providerMetadata": { "google": { "thoughtSignature": "text_sig_y" } } }
        ])
    );
}

#[tokio::test]
async fn gemini_ignores_unknown_response_parts() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "text": "Hello " },
                    { "executableCode": { "language": "PYTHON", "code": "print('ignored')" } },
                    { "text": "world" }
                ]
            },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    assert_eq!(response.text(), "Hello world");
    assert_eq!(
        event_value_of(&response.finish_reason).unwrap(),
        json!({ "normalized": "stop", "raw": "STOP" })
    );
}

#[tokio::test]
async fn gemini_rejects_malformed_recognized_response_parts() {
    let body = sse_events(&[json!({
        "candidates": [{ "content": { "role": "model", "parts": [{ "text": 42 }] } }]
    })]);
    let fixture = fixture(&body);
    let error = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap_err();
    assert_eq!(error.tag(), "InvalidProviderOutput");
    assert!(error.message().contains("Invalid google/gemini stream event"), "{}", error);
}

#[tokio::test]
async fn gemini_preserves_thought_signature_for_reasoning_and_tool_call_continuation() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "text": "thinking", "thought": true },
                    { "text": "", "thought": true, "thoughtSignature": "thought_sig" },
                    { "functionCall": { "id": "provider_call", "name": "lookup", "args": { "query": "weather" } }, "thoughtSignature": "tool_sig" }
                ]
            },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();

    let reasoning_start = event_of_kind(&response.events, "reasoning-start").unwrap();
    let reasoning_end = event_of_kind(&response.events, "reasoning-end").unwrap();
    let tool_call = event_of_kind(&response.events, "tool-call").unwrap();
    assert_eq!(
        ts_wire(&event_value_of(reasoning_start).unwrap()),
        json!({ "type": "reasoning-start", "id": "reasoning-0" })
    );
    assert_eq!(
        ts_wire(&event_value_of(reasoning_end).unwrap()),
        json!({
            "type": "reasoning-end",
            "id": "reasoning-0",
            "providerMetadata": { "google": { "thoughtSignature": "thought_sig" } }
        })
    );
    assert_eq!(
        ts_wire(&event_value_of(tool_call).unwrap()),
        json!({
            "type": "tool-call",
            "id": "provider_call",
            "name": "lookup",
            "input": { "query": "weather" },
            "providerMetadata": { "google": { "thoughtSignature": "tool_sig" } }
        })
    );
    let reasoning_end_index =
        response.events.iter().position(|event| event.kind() == "reasoning-end").unwrap();
    let tool_call_index =
        response.events.iter().position(|event| event.kind() == "tool-call").unwrap();
    assert!(reasoning_end_index < tool_call_index);

    // the continuation re-lowers with the preserved signatures
    let gemini3_model = gemini_3(&fixture.route);
    let reasoning_metadata =
        ts_wire(&event_value_of(reasoning_end).unwrap()).get("providerMetadata").cloned();
    let tool_metadata =
        ts_wire(&event_value_of(tool_call).unwrap()).get("providerMetadata").cloned();
    let reasoning_part = ContentPart::Reasoning {
        text: "thinking".into(),
        encrypted: None,
        cache: None,
        metadata: None,
        provider_metadata: reasoning_metadata.and_then(|value| serde_json::from_value(value).ok()),
    };
    let continued_call = match tool_call {
        LLMEvent::ToolCall { id, name, input, .. } => ContentPart::ToolCall {
            id: id.clone(),
            name: name.clone(),
            namespace: None,
            input: input.clone(),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: tool_metadata.and_then(|value| serde_json::from_value(value).ok()),
        },
        _ => unreachable!(),
    };
    let continuation = LLMRequest::new(gemini3_model)
        .with_message(Message::assistant(vec![reasoning_part, continued_call]))
        .with_message(tool_result_text("provider_call", "lookup", "done"));
    let prepared = compile_body(&fixture.route, &continuation);
    assert_eq!(
        prepared["contents"],
        json!([
            {
                "role": "model",
                "parts": [
                    { "text": "thinking", "thought": true, "thoughtSignature": "thought_sig" },
                    { "functionCall": { "id": "provider_call", "name": "lookup", "args": { "query": "weather" } }, "thoughtSignature": "tool_sig" }
                ]
            },
            {
                "role": "user",
                "parts": [
                    { "functionResponse": { "id": "provider_call", "name": "lookup", "response": { "name": "lookup", "content": "done" } } }
                ]
            }
        ])
    );
}

#[tokio::test]
async fn gemini_preserves_thought_signature_on_visible_text_parts() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "text": "All done.", "thoughtSignature": "text_sig" }] },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let response = fixture.run(base_request(&model)).await.unwrap();
    let delta = event_of_kind(&response.events, "text-delta").unwrap();
    assert_eq!(
        ts_wire(&event_value_of(delta).unwrap()),
        json!({
            "type": "text-delta",
            "id": "text-0",
            "text": "All done.",
            "providerMetadata": { "google": { "thoughtSignature": "text_sig" } }
        })
    );

    let continuation = LLMRequest::new(model)
        .with_message(Message::assistant(vec![text_part_with_metadata("All done.", "text_sig")]));
    let prepared = compile_body(&fixture.route, &continuation);
    assert_eq!(
        prepared["contents"],
        json!([{ "role": "model", "parts": [{ "text": "All done.", "thoughtSignature": "text_sig" }] }])
    );
}

#[tokio::test]
async fn gemini_flushes_a_trailing_empty_signed_text_part_at_block_close() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "text": "Working." }, { "text": "", "thoughtSignature": "tail_sig" }] },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    let delta = event_of_kind(&response.events, "text-delta").unwrap();
    let end = event_of_kind(&response.events, "text-end").unwrap();
    assert_eq!(
        ts_wire(&event_value_of(delta).unwrap()),
        json!({ "type": "text-delta", "id": "text-0", "text": "Working." })
    );
    assert_eq!(
        ts_wire(&event_value_of(end).unwrap()),
        json!({
            "type": "text-end",
            "id": "text-0",
            "providerMetadata": { "google": { "thoughtSignature": "tail_sig" } }
        })
    );
}

#[test]
fn gemini_replays_unsigned_gemini_3_tool_calls_with_the_validator_bypass_sentinel() {
    let fixture = empty_body_route();
    let request = LLMRequest::new(gemini_3(&fixture.route))
        .with_message(Message::assistant(vec![tool_call_part(
            "tool_0",
            "lookup",
            json!({ "query": "weather" }),
        )]))
        .with_message(tool_result_text("tool_0", "lookup", "done"));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"],
        json!([
            {
                "role": "model",
                "parts": [
                    {
                        "functionCall": { "id": "tool_0", "name": "lookup", "args": { "query": "weather" } },
                        "thoughtSignature": "skip_thought_signature_validator"
                    }
                ]
            },
            {
                "role": "user",
                "parts": [
                    { "functionResponse": { "id": "tool_0", "name": "lookup", "response": { "name": "lookup", "content": "done" } } }
                ]
            }
        ])
    );
}

#[test]
fn gemini_leaves_unsigned_parallel_calls_unchanged_after_a_signed_gemini_3_call() {
    let fixture = empty_body_route();
    let request = LLMRequest::new(gemini_3(&fixture.route)).with_message(Message::assistant(vec![
        tool_call_part_signed(
            "tool_0",
            "lookup",
            json!({ "query": "weather" }),
            "parallel_signature",
        ),
        tool_call_part("tool_1", "lookup", json!({ "query": "news" })),
        tool_call_part("tool_2", "lookup", json!({ "query": "sports" })),
    ]));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"][0],
        json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": { "id": "tool_0", "name": "lookup", "args": { "query": "weather" } },
                    "thoughtSignature": "parallel_signature"
                },
                { "functionCall": { "id": "tool_1", "name": "lookup", "args": { "query": "news" } } },
                { "functionCall": { "id": "tool_2", "name": "lookup", "args": { "query": "sports" } } }
            ]
        })
    );
}

#[test]
fn gemini_adds_the_validator_bypass_sentinel_to_every_call_in_an_unsigned_gemini_3_batch() {
    let fixture = empty_body_route();
    let request = LLMRequest::new(gemini_3(&fixture.route)).with_message(Message::assistant(vec![
        tool_call_part("tool_0", "lookup", json!({ "query": "weather" })),
        tool_call_part("tool_1", "lookup", json!({ "query": "news" })),
    ]));
    let body = compile_body(&fixture.route, &request);
    assert_eq!(
        body["contents"][0],
        json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": { "id": "tool_0", "name": "lookup", "args": { "query": "weather" } },
                    "thoughtSignature": "skip_thought_signature_validator"
                },
                {
                    "functionCall": { "id": "tool_1", "name": "lookup", "args": { "query": "news" } },
                    "thoughtSignature": "skip_thought_signature_validator"
                }
            ]
        })
    );
}

#[tokio::test]
async fn gemini_emits_streamed_tool_calls_and_maps_finish_reason() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": { "query": "weather" } } }] },
            "finishReason": "STOP"
        }],
        "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 1 }
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();
    let usage = json!({
        "inputTokens": 5.0,
        "outputTokens": 1.0,
        "nonCachedInputTokens": 5.0,
        "totalTokens": 6.0,
        "providerMetadata": { "google": { "promptTokenCount": 5.0, "candidatesTokenCount": 1.0 } }
    });
    let tool_call = event_of_kind(&response.events, "tool-call").unwrap();
    let id = match tool_call {
        LLMEvent::ToolCall { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    assert!(id.starts_with("tool_") && id.len() == 37, "id: {id}");
    assert_eq!(
        events_json(&response.events),
        vec![
            json!({ "type": "step-start", "index": 0.0 }),
            json!({
                "type": "tool-call",
                "id": id,
                "name": "lookup",
                "input": { "query": "weather" }
            }),
            json!({
                "type": "step-finish",
                "index": 0.0,
                "reason": { "normalized": "tool-calls", "raw": "STOP" },
                "usage": usage
            }),
            json!({
                "type": "finish",
                "reason": { "normalized": "tool-calls", "raw": "STOP" },
                "usage": usage
            }),
        ]
    );
}

#[tokio::test]
async fn gemini_separates_text_blocks_around_streamed_tool_calls() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "text": "before" },
                    { "functionCall": { "id": "call_1", "name": "lookup", "args": { "query": "weather" } } },
                    { "text": "after" }
                ]
            },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();
    assert_eq!(
        events_json(&response.events[1..8]),
        vec![
            json!({ "type": "text-start", "id": "text-0" }),
            json!({ "type": "text-delta", "id": "text-0", "text": "before" }),
            json!({ "type": "text-end", "id": "text-0" }),
            json!({ "type": "tool-call", "id": "call_1", "name": "lookup", "input": { "query": "weather" } }),
            json!({ "type": "text-start", "id": "text-1" }),
            json!({ "type": "text-delta", "id": "text-1", "text": "after" }),
            json!({ "type": "text-end", "id": "text-1" }),
        ]
    );
    assert_eq!(
        content_wire(&ts_wire(&event_value_of(&response.message.content).unwrap())),
        json!([
            { "type": "text", "text": "before" },
            { "type": "tool-call", "id": "call_1", "name": "lookup", "input": { "query": "weather" } },
            { "type": "text", "text": "after" }
        ])
    );
    assert_eq!(
        event_value_of(&response.finish_reason).unwrap(),
        json!({ "normalized": "tool-calls", "raw": "STOP" })
    );
}

#[tokio::test]
async fn gemini_defaults_omitted_function_call_args_to_an_empty_object() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "functionCall": { "name": "ping" } }] },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "ping",
        "Ping",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();
    let tool_calls = response.tool_calls();
    let id = match tool_calls[0] {
        LLMEvent::ToolCall { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    assert!(id.starts_with("tool_"));
    assert_eq!(
        ts_wire(&event_value_of(tool_calls[0]).unwrap()),
        json!({ "type": "tool-call", "id": id, "name": "ping", "input": {} })
    );
}

#[tokio::test]
async fn gemini_maps_tool_calls_without_a_finish_reason() {
    let body = sse_events(&[json!({
        "candidates": [{ "content": { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": { "query": "weather" } } }] } }],
        "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 1 }
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();
    assert_eq!(
        event_value_of(&response.finish_reason).unwrap(),
        json!({ "normalized": "tool-calls" })
    );
}

#[tokio::test]
async fn gemini_assigns_unique_ids_to_multiple_streamed_tool_calls() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "functionCall": { "id": "call_0", "name": "lookup", "args": { "query": "weather" } } },
                    { "functionCall": { "name": "lookup", "args": { "query": "news" } } }
                ]
            },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();
    let tool_calls = response.tool_calls();
    assert_eq!(
        ts_wire(&event_value_of(tool_calls[0]).unwrap()),
        json!({ "type": "tool-call", "id": "call_0", "name": "lookup", "input": { "query": "weather" } })
    );
    let second_id = match tool_calls[1] {
        LLMEvent::ToolCall { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    assert!(second_id.starts_with("tool_"));
    assert_ne!(tool_calls_id(tool_calls[0]), second_id);
    assert_eq!(
        ts_wire(&event_value_of(response.events.last().unwrap()).unwrap()),
        json!({ "type": "finish", "reason": { "normalized": "tool-calls", "raw": "STOP" } })
    );
}

fn tool_calls_id(event: &LLMEvent) -> String {
    match event {
        LLMEvent::ToolCall { id, .. } => id.clone(),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn gemini_replaces_repeated_supplier_ids_with_fresh_fallback_ids() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "functionCall": { "id": "dup_call", "name": "lookup", "args": { "query": "weather" } } },
                    { "functionCall": { "id": "dup_call", "name": "lookup", "args": { "query": "news" } } }
                ]
            },
            "finishReason": "STOP"
        }]
    })]);
    let fixture = fixture(&body);
    let model = fixture.model("gemini-2.5-flash");
    let mut request = base_request(&model);
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({ "type": "object" }),
    ))];
    let response = fixture.run(request).await.unwrap();
    let tool_calls = response.tool_calls();
    assert_eq!(tool_calls_id(tool_calls[0]), "dup_call");
    let second = tool_calls_id(tool_calls[1]);
    assert!(second.starts_with("tool_"));
    assert_ne!(second, "dup_call");
}

#[tokio::test]
async fn gemini_assigns_distinct_unique_fallback_ids_across_separate_requests() {
    let body = sse_events(&[json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": { "query": "weather" } } }] },
            "finishReason": "STOP"
        }]
    })]);
    let mut ids_seen: Vec<String> = Vec::new();
    for _ in 0..2 {
        let fixture = fixture(&body);
        let model = fixture.model("gemini-2.5-flash");
        let mut request = base_request(&model);
        request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Lookup data",
            json!({ "type": "object" }),
        ))];
        let response = fixture.run(request).await.unwrap();
        ids_seen.push(tool_calls_id(response.tool_calls()[0]));
    }
    assert_ne!(ids_seen[0], ids_seen[1]);
    assert!(ids_seen.iter().all(|id| id.starts_with("tool_")));
}

#[tokio::test]
async fn gemini_maps_length_and_content_filter_finish_reasons() {
    let length_body = sse_events(&[json!({
        "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": "MAX_TOKENS" }]
    })]);
    let length_fixture = fixture(&length_body);
    let length =
        length_fixture.run(base_request(&length_fixture.model("gemini-2.5-flash"))).await.unwrap();
    assert_eq!(
        length.events.iter().map(LLMEvent::kind).collect::<Vec<_>>(),
        ["step-start", "step-finish", "finish"]
    );
    assert_eq!(
        event_value_of(length.events.last().unwrap()).unwrap(),
        json!({ "type": "finish", "reason": { "normalized": "length", "raw": "MAX_TOKENS" } })
    );

    let filtered_body = sse_events(&[json!({
        "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": "SAFETY" }]
    })]);
    let filtered_fixture = fixture(&filtered_body);
    let filtered = filtered_fixture
        .run(base_request(&filtered_fixture.model("gemini-2.5-flash")))
        .await
        .unwrap();
    assert_eq!(
        event_value_of(filtered.events.last().unwrap()).unwrap(),
        json!({ "type": "finish", "reason": { "normalized": "content-filter", "raw": "SAFETY" } })
    );
}

#[tokio::test]
async fn gemini_preserves_candidate_less_prompt_safety_blocks_as_content_filter_outcomes() {
    let blocked_body = sse_events(&[json!({
        "promptFeedback": {
            "blockReason": "FUTURE_SAFETY_REASON",
            "blockReasonMessage": "Prompt blocked",
            "safetyRatings": [{ "category": "HARM_CATEGORY_HARASSMENT", "blocked": true }]
        }
    })]);
    let blocked_fixture = fixture(&blocked_body);
    let blocked = blocked_fixture
        .run(base_request(&blocked_fixture.model("gemini-2.5-flash")))
        .await
        .unwrap();
    assert_eq!(
        blocked.events.iter().map(LLMEvent::kind).collect::<Vec<_>>(),
        ["step-start", "step-finish", "finish"]
    );
    assert_eq!(
        ts_wire(&event_value_of(blocked.events.last().unwrap()).unwrap()),
        json!({
            "type": "finish",
            "reason": { "normalized": "content-filter", "raw": "FUTURE_SAFETY_REASON" },
            "providerMetadata": {
                "google": {
                    "promptFeedback": {
                        "blockReason": "FUTURE_SAFETY_REASON",
                        "blockReasonMessage": "Prompt blocked",
                        "safetyRatings": [{ "category": "HARM_CATEGORY_HARASSMENT", "blocked": true }]
                    }
                }
            }
        })
    );

    let usage_body = sse_events(&[
        json!({ "promptFeedback": { "blockReason": "SAFETY" } }),
        json!({ "usageMetadata": { "promptTokenCount": 7, "totalTokenCount": 7 } }),
    ]);
    let fixture = fixture(&usage_body);
    let blocked_with_usage =
        fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    assert_eq!(
        event_value_of(&blocked_with_usage.finish_reason).unwrap(),
        json!({ "normalized": "content-filter", "raw": "SAFETY" })
    );
    let usage = blocked_with_usage.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(7.0));
    assert_eq!(usage.total_tokens, Some(7.0));
}

#[tokio::test]
async fn gemini_preserves_blocking_finishes_and_rejects_invalid_output_reasons() {
    for (raw, normalized) in [
        ("MODEL_ARMOR", "content-filter"),
        ("IMAGE_PROHIBITED_CONTENT", "content-filter"),
        ("IMAGE_RECITATION", "content-filter"),
        ("LANGUAGE", "content-filter"),
        ("IMAGE_OTHER", "unknown"),
    ] {
        let body = sse_events(&[json!({
            "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": raw }]
        })]);
        let fixture = fixture(&body);
        let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
        assert_eq!(
            event_value_of(&response.finish_reason).unwrap(),
            json!({ "normalized": normalized, "raw": raw })
        );
    }

    for raw in [
        "MALFORMED_FUNCTION_CALL",
        "UNEXPECTED_TOOL_CALL",
        "NO_IMAGE",
        "TOO_MANY_TOOL_CALLS",
        "MISSING_THOUGHT_SIGNATURE",
        "MALFORMED_RESPONSE",
    ] {
        let body = sse_events(&[json!({
            "candidates": [{ "finishReason": raw, "finishMessage": "Provider detail" }],
            "responseId": "failure"
        })]);
        let fixture = fixture(&body);
        let error =
            fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap_err();
        assert_eq!(error.tag(), "InvalidProviderOutput", "{raw}");
        assert_eq!(error.message(), format!("Gemini stopped with {raw}"));
    }
}

#[tokio::test]
async fn gemini_leaves_total_usage_undefined_when_component_counts_are_missing() {
    let body = sse_events(&[json!({ "usageMetadata": { "thoughtsTokenCount": 1 } })]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    let usage = response.usage.unwrap();
    assert_eq!(usage.reasoning_tokens, Some(1.0));
    assert_eq!(usage.total_tokens, None);
}

#[tokio::test]
async fn gemini_survives_explicit_null_usage_counts() {
    let body = sse_events(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hi" }] } }] }),
        json!({ "usageMetadata": { "promptTokenCount": null, "candidatesTokenCount": 5 } }),
    ]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    assert_eq!(response.text(), "Hi");
    let usage = response.usage.unwrap();
    assert_eq!(usage.output_tokens, Some(5.0));
    assert_eq!(usage.total_tokens, Some(5.0));
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.non_cached_input_tokens, None);
    assert_eq!(usage.cache_read_input_tokens, None);
    assert_eq!(usage.reasoning_tokens, None);
}

#[tokio::test]
async fn gemini_survives_null_candidates_content_parts_and_finish_reason() {
    let body = sse_events(&[
        json!({ "candidates": null }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": null } }] }),
        json!({ "candidates": [{ "content": null, "finishReason": null }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hello" }] }, "finishReason": "STOP" }] }),
    ]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    assert_eq!(response.text(), "Hello");
    assert_eq!(
        event_value_of(&response.finish_reason).unwrap(),
        json!({ "normalized": "stop", "raw": "STOP" })
    );
}

#[tokio::test]
async fn gemini_treats_a_null_thought_flag_on_a_text_part_as_visible_output() {
    let body = sse_events(&[json!({
        "candidates": [
            { "content": { "role": "model", "parts": [{ "text": "Visible", "thought": null }] }, "finishReason": "STOP" }
        ]
    })]);
    let fixture = fixture(&body);
    let response = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap();
    assert!(event_of_kind(&response.events, "reasoning-start").is_none());
    assert_eq!(response.reasoning(), "");
    assert_eq!(response.text(), "Visible");
}

#[tokio::test]
async fn gemini_fails_invalid_stream_events() {
    let fixture = fixture(&sse_raw(&["data: {not json}"]));
    let error = fixture.run(base_request(&fixture.model("gemini-2.5-flash"))).await.unwrap_err();
    assert_eq!(error.tag(), "InvalidProviderOutput");
    assert!(error.message().contains("Invalid google/gemini stream event"), "{}", error);
}

// =============================================================================
// Route factory, endpoint, auth
// =============================================================================

#[test]
fn gemini_endpoint_embeds_the_model_in_the_path() {
    let transport: SharedTransport =
        Arc::new(crate::route::transport::FailingTransport { message: "no requests" });
    let route = test_route(transport);
    let request = base_request(&model_2_5(&route));
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _body_text) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(
        transport_request.url,
        "https://generativelanguage.test/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    );
}

#[test]
fn gemini_route_defaults_to_the_google_base_url() {
    let transport: SharedTransport =
        Arc::new(crate::route::transport::FailingTransport { message: "no requests" });
    let route = gemini_route(transport);
    assert_eq!(route.endpoint.base_url.as_deref(), Some(DEFAULT_BASE_URL));
    let request = base_request(&model_2_5(&route));
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(
        transport_request.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    );
}

#[tokio::test]
async fn gemini_google_auth_applies_the_x_goog_api_key_header() {
    let capture = Arc::new(CapturingTransport::default());
    let route = google_route(
        Arc::clone(&capture) as SharedTransport,
        GoogleConfig { api_key: Some(Secret::Value("g-key-1".into())), base_url: None },
    );
    let response = route.generate(base_request(&model_2_5(&route))).await.unwrap();
    assert_eq!(response.text(), "Hi");
    let sent = capture.requests();
    assert_eq!(sent[0].header("x-goog-api-key"), Some("g-key-1"));
}

#[derive(Default)]
struct CapturingTransport {
    requests: std::sync::Mutex<Vec<crate::route::transport::TransportRequest>>,
}

#[async_trait::async_trait]
impl crate::route::transport::Transport for CapturingTransport {
    async fn execute(
        &self,
        request: crate::route::transport::TransportRequest,
    ) -> Result<crate::route::transport::TransportResponse, crate::route::transport::TransportError>
    {
        self.requests.lock().unwrap().push(request.clone());
        Ok(crate::route::transport::TransportResponse {
            status: 200,
            headers: Default::default(),
            body: Box::pin(futures::stream::iter(vec![Ok::<bytes::Bytes, _>(bytes::Bytes::from(
                sse_events(&[serde_json::json!({
                    "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hi" }] }, "finishReason": "STOP" }]
                })]),
            ))])),
        })
    }
}

impl CapturingTransport {
    fn requests(&self) -> Vec<crate::route::transport::TransportRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[test]
fn gemini_google_auth_falls_back_to_the_config_environment() {
    // explicit auth wins over the env chain
    let definition = google_auth(Some(Secret::Value("literal".into())));
    let headers = definition
        .apply(&auth::AuthInput {
            request_http: None,
            method: "POST",
            url: "https://x.test",
            body: "{}",
            headers: auth::Headers::new(),
        })
        .unwrap();
    assert_eq!(headers.get("x-goog-api-key"), Some("literal"));
}

#[test]
fn gemini_protocol_declares_the_gemini_sanitizer() {
    use crate::schema::SanitizerCompatibility;
    let transport: SharedTransport =
        Arc::new(crate::route::transport::FailingTransport { message: "x" });
    assert_eq!(gemini_route(transport).sanitizer(), Some(SanitizerCompatibility::Gemini));
}

// =============================================================================
// Cassette parity: the REAL v2 TS step machine's dumps
// =============================================================================

#[derive(serde::Deserialize, Debug)]
struct DumpError {
    #[allow(dead_code)]
    message: String,
    #[allow(dead_code)]
    tag: String,
}

#[derive(serde::Deserialize)]
struct DumpInteraction {
    events: Vec<Value>,
    error: Option<DumpError>,
}

#[derive(serde::Deserialize)]
struct Dump {
    scenario: String,
    provider: String,
    model: String,
    interactions: Vec<DumpInteraction>,
}

fn expected_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/expected/gemini")
}

fn load_dumps() -> Vec<(std::path::PathBuf, Dump)> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(expected_dir())
        .expect("expected/gemini dumps exist")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let raw = std::fs::read_to_string(&path).unwrap();
            (path, serde_json::from_str(&raw).unwrap())
        })
        .collect()
}

fn run_step_machine(request: &LLMRequest, body: &str) -> (Vec<Value>, Option<(String, String)>) {
    let protocol = GeminiProtocol;
    let mut state = Some(protocol.initial(request));
    let mut events: Vec<Value> = Vec::new();
    let mut failure: Option<(String, String)> = None;
    let mut decoder = FramingDecoder::new(Framing::sse());
    let mut frames = Vec::new();
    for chunk in crate::test_support::chunked(body) {
        frames.extend(decoder.feed(&chunk).unwrap());
    }
    frames.extend(decoder.finish().unwrap());
    for frame in frames {
        let current = state.take().expect("state survives until a failure");
        match protocol.step(current, &frame) {
            Ok((next, emitted)) => {
                state = Some(next);
                events.extend(emitted.iter().map(|event| ts_wire(&event_value_of(event).unwrap())));
            }
            Err(error) => {
                failure = Some((error.tag().to_string(), error.message().to_string()));
                break;
            }
        }
    }
    if failure.is_none() {
        if let Ok((_, emitted)) = protocol.on_halt(state.expect("state present without failure")) {
            events.extend(emitted.iter().map(|event| ts_wire(&event_value_of(event).unwrap())));
        }
    }
    (events, failure)
}

/// Replay every gemini* cassette through the Rust step machine exactly the
/// way the TS dump driver drives the real v2 machine, and require exact
/// event parity.
#[test]
fn gemini_replay_step_machine_matches_ts_dumps() {
    let dumps = load_dumps();
    assert!(!dumps.is_empty(), "no dumps found in {}", expected_dir().display());
    let mut interactions_checked = 0;
    for (_path, dump) in &dumps {
        let cassette_path =
            crate::test_support::recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path)
            .unwrap_or_else(|error| panic!("cassette for {}: {error}", dump.scenario));
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport needed" });
        let route = gemini_route(transport);
        let model = LanguageModel::new(dump.model.clone(), dump.provider.clone(), Arc::new(route));
        let request = LLMRequest::new(model).with_prompt("hi");

        let interactions: Vec<_> = cassette.http_interactions().collect();
        assert_eq!(
            interactions.len(),
            dump.interactions.len(),
            "{}: interaction count",
            dump.scenario
        );
        for (index, (interaction, expected)) in
            interactions.iter().zip(&dump.interactions).enumerate()
        {
            let body =
                interaction.response.as_ref().and_then(|r| r.body.clone()).unwrap_or_default();
            let (events, failure) = run_step_machine(&request, &body);
            match (&expected.error, &failure) {
                (None, None) => {}
                (Some(expected), Some((tag, _message))) => {
                    assert_eq!(tag, &expected.tag, "{}#{index}", dump.scenario);
                }
                (expected, actual) => panic!(
                    "{}#{}: error mismatch — dump {expected:?}, rust {actual:?}",
                    dump.scenario, index
                ),
            }
            assert_eq!(
                events,
                expected.events.iter().map(ts_wire).collect::<Vec<_>>(),
                "{}#{}: events diverge from the TS dump",
                dump.scenario,
                index
            );
            interactions_checked += 1;
        }
    }
    assert_eq!(interactions_checked, 3, "expected the 3 recorded gemini interactions");
}

/// The same cassettes through the FULL Rust route stack (endpoint render +
/// framing + terminal-event enforcement) with strict URL matching.
#[tokio::test]
async fn gemini_replay_route_stack_matches_ts_dumps() {
    let dumps = load_dumps();
    for (_path, dump) in &dumps {
        let cassette_path =
            crate::test_support::recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path).unwrap();
        let transport = MockTransport::from_cassette(&cassette).with_strict_url();
        let route = gemini_route(Arc::new(transport) as SharedTransport);
        let model =
            LanguageModel::new(dump.model.clone(), dump.provider.clone(), Arc::new(route.clone()));
        let mut request = LLMRequest::new(model).with_prompt("hi");
        request.generation = Some(GenerationOptions::default());

        for (index, expected) in dump.interactions.iter().enumerate() {
            let response = route
                .generate(request.clone())
                .await
                .unwrap_or_else(|error| panic!("{}#{index}: {error}", dump.scenario));
            assert_eq!(
                events_json(&response.events),
                expected.events.iter().map(ts_wire).collect::<Vec<_>>(),
                "{}#{}: route-stack events diverge from the TS dump",
                dump.scenario,
                index
            );
        }
    }
}
