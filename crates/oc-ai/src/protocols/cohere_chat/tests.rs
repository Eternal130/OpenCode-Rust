//! Port of the cohere test essentials (`test/provider/cohere.test.ts` +
//! `cohere.recorded.test.ts`) and cassette parity against the TS dumps in
//! `test-fixtures/expected/cohere/`.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::route::framing::FramingDecoder;
use crate::route::protocol::Protocol as _;
use crate::route::{Framing, Route, SharedTransport};
use crate::schema::media;
use crate::schema::{
    ContentPart, GenerationOptions, LLMEvent, LLMRequest, LanguageModel, Message, MessageRole,
    SystemPart, ToolDefinition, ToolResultValue,
};
use crate::test_support::{Cassette, MockTransport, RecordedInteraction};

use super::{cohere_route, CohereChatProtocol, DEFAULT_BASE_URL};

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

fn interaction(body: &str) -> RecordedInteraction {
    RecordedInteraction {
        method: "POST".into(),
        url: "https://api.cohere.com/v2/chat".into(),
        status: 200,
        body: body.into(),
        request_body: None,
    }
}

fn fixture(body: &str) -> Route {
    let transport: SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![interaction(body)]));
    cohere_route(transport)
}

fn model(route: &Route) -> LanguageModel {
    route.model("command-a-03-2025", None).unwrap()
}

fn compile_body(route: &Route, request: &LLMRequest) -> Value {
    let prepared = route.prepare_request(request);
    route.build_body(&prepared).unwrap()
}

fn ts_wire(value: &Value) -> Value {
    match value {
        Value::Number(number) => json!(number.as_f64().unwrap_or_default()),
        Value::Array(items) => Value::Array(items.iter().map(ts_wire).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let key = if key == "provider_metadata" { "providerMetadata" } else { key };
                    (key.to_string(), ts_wire(value))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn events_json(events: &[LLMEvent]) -> Vec<Value> {
    events.iter().map(|event| ts_wire(&serde_json::to_value(event).unwrap())).collect()
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

// =============================================================================
// Request lowering
// =============================================================================

#[test]
fn cohere_lowers_native_history_thinking_tools_and_sampling() {
    let route = fixture("");
    let mut request = LLMRequest::new(model(&route))
        .with_message(Message::user("Lookup Paris"))
        .with_message(Message::assistant(vec![tool_call_part(
            "lookup-1",
            "lookup",
            json!({ "city": "Paris" }),
        )]))
        .with_message(Message::tool_result(
            "lookup-1",
            "lookup",
            ToolResultValue::Json { value: json!({ "sunny": true }) },
        ));
    request.system = vec![
        SystemPart::new("Be concise.\nKeep this newline."),
        SystemPart::new("Second instructions."),
    ];
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "Look up a city",
        json!({ "type": "object", "properties": {} }),
    ))];
    request.tool_choice = Some(crate::schema::ToolChoice::required());
    request.provider_options =
        Some(json!({ "thinking": { "tokenBudget": 128 } }).as_object().unwrap().clone());
    request.generation = Some(GenerationOptions {
        max_tokens: Some(2048.0),
        top_p: Some(0.9),
        top_k: Some(10.0),
        ..Default::default()
    });
    let body = compile_body(&route, &request);
    assert_eq!(
        body,
        json!({
            "model": "command-a-03-2025",
            "messages": [
                {
                    "role": "system",
                    "content": [
                        { "type": "text", "text": "Be concise.\nKeep this newline." },
                        { "type": "text", "text": "Second instructions." }
                    ]
                },
                { "role": "user", "content": [{ "type": "text", "text": "Lookup Paris" }] },
                {
                    "role": "assistant",
                    "tool_calls": [
                        { "id": "lookup-1", "type": "function", "function": { "name": "lookup", "arguments": "{\"city\":\"Paris\"}" } }
                    ]
                },
                { "role": "tool", "tool_call_id": "lookup-1", "content": "{\"sunny\":true}" }
            ],
            "stream": true,
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "description": "Look up a city",
                        "parameters": { "type": "object", "properties": {} }
                    }
                }
            ],
            "tool_choice": "REQUIRED",
            "thinking": { "type": "enabled", "token_budget": 128.0 },
            "max_tokens": 2048.0,
            "p": 0.9,
            "k": 10.0
        })
    );
}

#[test]
fn cohere_rejects_unsupported_media() {
    let route = fixture("");
    let request = LLMRequest::new(model(&route)).with_message(Message::new(
        MessageRole::User,
        vec![ContentPart::Media {
            media: media::base64("Zm9v", "audio/wav"),
            filename: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }],
    ));
    let error = route.build_body(&route.prepare_request(&request)).unwrap_err();
    assert_eq!(error.tag(), "InvalidRequest");
    assert!(error.message().contains("only support text and media content"), "{}", error);
}

#[test]
fn cohere_thinking_budgets_must_be_positive_integers() {
    let route = fixture("");
    let mut request = LLMRequest::new(model(&route));
    request.provider_options =
        Some(json!({ "thinking": { "tokenBudget": 0 } }).as_object().unwrap().clone());
    let error = route.build_body(&route.prepare_request(&request)).unwrap_err();
    assert_eq!(error.tag(), "InvalidRequest");
}

#[test]
fn cohere_fits_thinking_budgets_under_the_output_limit() {
    let route = fixture("");
    let mut request = LLMRequest::new(model(&route)).with_prompt("Hi");
    request.provider_options =
        Some(json!({ "thinking": { "tokenBudget": 31_999 } }).as_object().unwrap().clone());
    request.generation = Some(GenerationOptions { max_tokens: Some(4096.0), ..Default::default() });
    let body = compile_body(&route, &request);
    assert_eq!(body["thinking"], json!({ "type": "enabled", "token_budget": 2048.0 }));
}

#[test]
fn cohere_route_targets_the_native_v2_endpoint() {
    let transport: SharedTransport =
        Arc::new(crate::route::transport::FailingTransport { message: "no requests" });
    let route = cohere_route(transport);
    assert_eq!(route.endpoint.base_url.as_deref(), Some(DEFAULT_BASE_URL));
    let request = LLMRequest::new(model(&route)).with_prompt("Hi");
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(transport_request.url, "https://api.cohere.com/v2/chat");
}

// =============================================================================
// Stream machine
// =============================================================================

#[tokio::test]
async fn cohere_maps_inclusive_usage_while_retaining_billed_units() {
    let body = sse_events(&[
        json!({ "type": "message-start" }),
        json!({ "type": "content-start", "index": 0, "delta": { "message": { "content": { "type": "thinking", "thinking": "" } } } }),
        json!({ "type": "content-delta", "index": 0, "delta": { "message": { "content": { "thinking": "Think" } } } }),
        json!({ "type": "content-end", "index": 0 }),
        json!({ "type": "content-start", "index": 1, "delta": { "message": { "content": { "type": "text", "text": "" } } } }),
        json!({ "type": "content-delta", "index": 1, "delta": { "message": { "content": { "text": "OK" } } } }),
        json!({ "type": "content-end", "index": 1 }),
        json!({
            "type": "message-end",
            "delta": {
                "finish_reason": "COMPLETE",
                "usage": {
                    "tokens": { "input_tokens": 100, "output_tokens": 20, "reasoning_tokens": 10 },
                    "billed_units": { "input_tokens": 30, "output_tokens": 15 },
                    "cached_tokens": 60
                }
            }
        }),
    ]);
    let route = fixture(&body);
    let response = route.generate(LLMRequest::new(model(&route)).with_prompt("Hi")).await.unwrap();
    assert_eq!(response.text(), "OK");
    assert_eq!(response.reasoning(), "Think");
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(100.0));
    assert_eq!(usage.non_cached_input_tokens, Some(40.0));
    assert_eq!(usage.cache_read_input_tokens, Some(60.0));
    assert_eq!(usage.output_tokens, Some(20.0));
    assert_eq!(usage.reasoning_tokens, Some(10.0));
    assert_eq!(usage.total_tokens, Some(120.0));
    let metadata = usage.provider_metadata.as_ref().unwrap();
    let inner = metadata.get("cohere").unwrap();
    assert_eq!(
        inner.get("tokens").map(ts_wire).as_ref(),
        Some(&json!({ "input_tokens": 100.0, "output_tokens": 20.0, "reasoning_tokens": 10.0 }))
    );
    assert_eq!(
        inner.get("billed_units").map(ts_wire).as_ref(),
        Some(&json!({ "input_tokens": 30.0, "output_tokens": 15.0 }))
    );
    assert_eq!(inner.get("cached_tokens").map(ts_wire).as_ref(), Some(&json!(60.0)));
    assert_eq!(response.events.iter().filter(|event| event.is_finish()).count(), 1);
}

#[tokio::test]
async fn cohere_rejects_incomplete_streams() {
    let route = fixture(&sse_events(&[json!({ "type": "message-start" })]));
    let error = route.generate(LLMRequest::new(model(&route)).with_prompt("Hi")).await.unwrap_err();
    assert!(error.message().contains("without message-end"), "{}", error);
}

#[tokio::test]
async fn cohere_preserves_native_tool_plans_in_continued_history() {
    let body = sse_events(&[
        json!({ "type": "message-start" }),
        json!({ "type": "tool-plan-delta", "delta": { "message": { "tool_plan": "Look up the weather." } } }),
        json!({ "type": "tool-call-start", "index": 0, "delta": { "message": { "tool_calls": { "id": "lookup-1", "function": { "name": "lookup", "arguments": "" } } } } }),
        json!({ "type": "tool-call-delta", "index": 0, "delta": { "message": { "tool_calls": { "function": { "arguments": "{\"city\":\"Paris\"}" } } } } }),
        json!({ "type": "tool-call-end", "index": 0 }),
        json!({ "type": "message-end", "delta": { "finish_reason": "TOOL_CALL" } }),
    ]);
    let route = fixture(&body);
    let response = route.generate(LLMRequest::new(model(&route)).with_prompt("Hi")).await.unwrap();
    let tool_calls = response.tool_calls();
    match tool_calls[0] {
        LLMEvent::ToolCall { input, .. } => assert_eq!(*input, json!({ "city": "Paris" })),
        other => panic!("expected a tool call, got {}", other.kind()),
    }

    // the continued history lowers the tool plan and the tool call
    let continued = LLMRequest::new(model(&route))
        .with_message(response.message.clone())
        .with_message(Message::tool_result(
            "lookup-1",
            "lookup",
            ToolResultValue::Json { value: json!({ "sunny": true }) },
        ));
    let body = compile_body(&route, &continued);
    assert_eq!(
        body["messages"],
        json!([
            {
                "role": "assistant",
                "tool_plan": "Look up the weather.",
                "tool_calls": [
                    { "id": "lookup-1", "type": "function", "function": { "name": "lookup", "arguments": "{\"city\":\"Paris\"}" } }
                ]
            },
            { "role": "tool", "tool_call_id": "lookup-1", "content": "{\"sunny\":true}" }
        ])
    );
}

// =============================================================================
// Cassette parity
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

fn run_step_machine(request: &LLMRequest, body: &str) -> (Vec<Value>, Option<(String, String)>) {
    let protocol = CohereChatProtocol;
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
                events.extend(
                    emitted.iter().map(|event| ts_wire(&serde_json::to_value(event).unwrap())),
                );
            }
            Err(error) => {
                failure = Some((error.tag().to_string(), error.message().to_string()));
                break;
            }
        }
    }
    if failure.is_none() {
        if let Ok((_, emitted)) = protocol.on_halt(state.expect("state present without failure")) {
            events
                .extend(emitted.iter().map(|event| ts_wire(&serde_json::to_value(event).unwrap())));
        }
    }
    (events, failure)
}

#[test]
fn cohere_replay_step_machine_matches_ts_dumps() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/expected/cohere");
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("expected/cohere dumps exist")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    paths.sort();
    let mut interactions_checked = 0;
    for path in &paths {
        let dump: Dump = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cassette_path =
            crate::test_support::recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path).unwrap();
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport needed" });
        let route = cohere_route(transport);
        let model = LanguageModel::new(dump.model.clone(), dump.provider.clone(), Arc::new(route));
        // The cohere parser state does not read the request, so one bound
        // request covers every interaction of a scenario.
        let request = LLMRequest::new(model).with_prompt("hi");

        let interactions: Vec<_> = cassette.http_interactions().collect();
        assert_eq!(interactions.len(), dump.interactions.len(), "{}", dump.scenario);
        for (index, (interaction, expected)) in
            interactions.iter().zip(&dump.interactions).enumerate()
        {
            let body =
                interaction.response.as_ref().and_then(|r| r.body.clone()).unwrap_or_default();
            let (events, failure) = run_step_machine(&request, &body);
            match (&expected.error, &failure) {
                (None, None) => {}
                (Some(expected), Some((tag, _))) => assert_eq!(tag, &expected.tag),
                (expected, actual) => {
                    panic!("{}#{index}: error mismatch — {expected:?} vs {actual:?}", dump.scenario)
                }
            }
            assert_eq!(
                events,
                expected.events.iter().map(ts_wire).collect::<Vec<_>>(),
                "{}#{index}: events diverge from the TS dump",
                dump.scenario
            );
            interactions_checked += 1;
        }
    }
    assert_eq!(interactions_checked, 4, "expected the 4 recorded cohere-chat interactions");
}

#[tokio::test]
async fn cohere_replay_route_stack_matches_ts_dumps() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/expected/cohere");
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("dumps")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    paths.sort();
    for path in &paths {
        let dump: Dump = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cassette_path =
            crate::test_support::recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path).unwrap();
        let transport = MockTransport::from_cassette(&cassette).with_strict_url();
        let route = cohere_route(Arc::new(transport) as SharedTransport);
        let model =
            LanguageModel::new(dump.model.clone(), dump.provider.clone(), Arc::new(route.clone()));
        let request = LLMRequest::new(model).with_prompt("hi");
        for (index, expected) in dump.interactions.iter().enumerate() {
            let response = route
                .generate(request.clone())
                .await
                .unwrap_or_else(|error| panic!("{}#{index}: {error}", dump.scenario));
            assert_eq!(
                events_json(&response.events),
                expected.events.iter().map(ts_wire).collect::<Vec<_>>(),
                "{}#{}: route-stack events diverge",
                dump.scenario,
                index
            );
        }
    }
}
