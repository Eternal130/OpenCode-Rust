//! Port of the mistral-chat test essentials
//! (`test/provider/mistral-chat.test.ts` + `mistral.recorded.test.ts`) and
//! cassette parity against the TS dumps in
//! `test-fixtures/expected/mistral-chat{,-glm}/`.

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

use super::{mistral_route, MistralChatProtocol, DEFAULT_BASE_URL, PATH};

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
        url: "https://api.mistral.ai/v1/chat/completions".into(),
        status: 200,
        body: body.into(),
        request_body: None,
    }
}

fn fixture(body: &str) -> Route {
    let transport: SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![interaction(body)]));
    mistral_route(transport)
}

fn model(route: &Route, id: &str) -> LanguageModel {
    route.model(id, None).unwrap()
}

fn compile_body(route: &Route, request: &LLMRequest) -> Value {
    let prepared = route.prepare_request(request);
    route.build_body(&prepared).unwrap()
}

/// The `LLMEvent` tree serializes in the contract's `camelCase` shape natively
/// (oracle gate 3 finding 1); only numbers still need `f64` coercion (serde
/// `u64`-vs-`f64`).
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

fn reasoning_part(text: &str) -> ContentPart {
    ContentPart::Reasoning {
        text: text.into(),
        encrypted: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    }
}

fn content_result(items: Value) -> ToolResultValue {
    ToolResultValue::Content {
        value: crate::schema::ToolContentItems(items.as_array().unwrap().clone()),
    }
}

// =============================================================================
// Request lowering
// =============================================================================

#[test]
fn mistral_lowers_native_messages_media_tool_choice_options_and_replay_ids() {
    let route = fixture("");
    let mut request = LLMRequest::new(model(&route, "mistral-large-latest"))
        .with_message(Message::system("Updated"))
        .with_message(Message::new(
            MessageRole::User,
            vec![
                ContentPart::text("Inspect"),
                ContentPart::Media {
                    media: media::base64("aW1hZ2U=", "image/png"),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::Media {
                    media: media::base64("cGRm", "application/pdf"),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ],
        ))
        .with_message(Message::assistant(vec![
            reasoning_part("Think"),
            ContentPart::text("Calling"),
            tool_call_part("call.same-prefix-1", "lookup", json!({ "city": "Paris" })),
            tool_call_part("call.same-prefix-2", "other", json!({})),
        ]))
        .with_message(Message::tool_result(
            "call.same-prefix-1",
            "lookup",
            ToolResultValue::Json { value: json!({ "ok": true }) },
        ));
    request.system = vec![
        SystemPart::new("Initial\nKeep this newline."),
        SystemPart::new("Second instructions."),
    ];
    request.tools = vec![
        crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "lookup",
            "Look up a city",
            json!({ "type": "object" }),
        )),
        crate::schema::ToolEntry::Definition(ToolDefinition::new(
            "other",
            "Other operation",
            json!({}),
        )),
    ];
    request.tool_choice = Some(crate::schema::ToolChoice::named("lookup"));
    request.prompt_cache_key = Some("session-1".into());
    request.generation = Some(GenerationOptions {
        max_tokens: Some(64.0),
        seed: Some(7.0),
        temperature: Some(0.2),
        top_p: Some(0.8),
        frequency_penalty: Some(0.1),
        presence_penalty: Some(0.3),
        stop: Some(vec!["done".into()]),
        ..Default::default()
    });
    request.provider_options = Some(
        json!({
            "safePrompt": true,
            "documentImageLimit": 3,
            "documentPageLimit": 8,
            "parallelToolCalls": false,
            "reasoningEffort": "high"
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    let body = compile_body(&route, &request);

    assert_eq!(body["model"], json!("mistral-large-latest"));
    assert_eq!(
        body["tool_choice"],
        json!({ "type": "function", "function": { "name": "lookup" } })
    );
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["max_tokens"], json!(64.0));
    assert_eq!(body["random_seed"], json!(7.0));
    assert_eq!(body["temperature"], json!(0.2));
    assert_eq!(body["top_p"], json!(0.8));
    assert_eq!(body["frequency_penalty"], json!(0.1));
    assert_eq!(body["presence_penalty"], json!(0.3));
    assert_eq!(body["stop"], json!(["done"]));
    assert_eq!(body["prompt_cache_key"], json!("session-1"));
    assert_eq!(body["safe_prompt"], json!(true));
    assert_eq!(body["document_image_limit"], json!(3.0));
    assert_eq!(body["document_page_limit"], json!(8.0));
    assert_eq!(body["parallel_tool_calls"], json!(false));
    assert_eq!(body["reasoning_effort"], json!("high"));
    assert_eq!(body["tools"][0]["function"]["name"], json!("lookup"));
    assert_eq!(body["tools"][0]["function"]["strict"], json!(false));
    assert_eq!(body["tools"][1]["function"]["parameters"], json!({ "type": "object" }));
    assert_eq!(
        body["messages"][0],
        json!({
            "role": "system",
            "content": [
                { "type": "text", "text": "Initial\nKeep this newline." },
                { "type": "text", "text": "Second instructions." }
            ]
        })
    );
    assert_eq!(
        body["messages"][1],
        json!({ "role": "user", "content": "<system-update>\nUpdated\n</system-update>" })
    );
    assert_eq!(
        body["messages"][2],
        json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "Inspect" },
                { "type": "image_url", "image_url": "data:image/png;base64,aW1hZ2U=" },
                { "type": "document_url", "document_url": "data:application/pdf;base64,cGRm" }
            ]
        })
    );
    assert_eq!(body["messages"][3]["role"], json!("assistant"));
    assert_eq!(body["messages"][3]["content"], json!("ThinkCalling"));
    let assistant = &body["messages"][3];
    let ids: Vec<&str> = assistant["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| call["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| id.len() == 9 && id.bytes().all(|b| b.is_ascii_alphanumeric())));
    assert_ne!(ids[0], ids[1]);
    assert_eq!(body["messages"][4]["tool_call_id"], json!(ids[0]));
    assert_eq!(body["messages"][4]["name"], json!("lookup"));
}

#[test]
fn mistral_preserves_valid_replay_ids() {
    let route = fixture("");
    let request = LLMRequest::new(model(&route, "mistral-large-latest"))
        .with_message(Message::assistant(vec![tool_call_part("Ab12Cd34E", "lookup", json!({}))]))
        .with_message(Message::tool_result(
            "Ab12Cd34E",
            "lookup",
            ToolResultValue::Text { value: json!("ok") },
        ));
    let body = compile_body(&route, &request);
    assert_eq!(body["messages"][0]["tool_calls"][0]["id"], json!("Ab12Cd34E"));
    assert_eq!(body["messages"][1]["tool_call_id"], json!("Ab12Cd34E"));
}

#[test]
fn mistral_applies_trailing_prefix_cache_and_reasoning_options() {
    let route = fixture("");
    let mut request = LLMRequest::new(model(&route, "mistral-large-latest"))
        .with_message(Message::assistant(vec![ContentPart::text("Earlier")]))
        .with_message(Message::user("Continue"))
        .with_message(Message::assistant(vec![ContentPart::text("Prefix")]));
    request.prompt_cache_key = Some("common-key".into());
    request.provider_options = Some(
        json!({ "promptCacheKey": "native-key", "promptMode": "reasoning" })
            .as_object()
            .unwrap()
            .clone(),
    );
    let body = compile_body(&route, &request);
    assert_eq!(body["prompt_cache_key"], json!("native-key"));
    assert_eq!(body["prompt_mode"], json!("reasoning"));
    assert_eq!(
        body["messages"],
        json!([
            { "role": "assistant", "content": "Earlier" },
            { "role": "user", "content": "Continue" },
            { "role": "assistant", "content": "Prefix", "prefix": true }
        ])
    );

    // cache: "none" drops the cache key entirely
    let mut uncached = LLMRequest::new(model(&route, "mistral-large-latest")).with_prompt("Hello");
    uncached.prompt_cache_key = Some("common-key".into());
    uncached.cache = Some(crate::schema::CachePolicy::None);
    uncached.provider_options =
        Some(json!({ "promptCacheKey": "native-key" }).as_object().unwrap().clone());
    let body = compile_body(&route, &uncached);
    assert!(body.get("prompt_cache_key").is_none());

    // reasoningEffort + promptMode are mutually exclusive
    let mut conflict = LLMRequest::new(model(&route, "mistral-large-latest")).with_prompt("Hello");
    conflict.provider_options = Some(
        json!({ "reasoningEffort": "high", "promptMode": "reasoning" })
            .as_object()
            .unwrap()
            .clone(),
    );
    let error = route.build_body(&route.prepare_request(&conflict)).unwrap_err();
    assert!(error.message().contains("mutually exclusive"), "{}", error);
}

#[test]
fn mistral_omits_empty_assistant_history_unless_it_carries_a_tool_call() {
    let route = fixture("");
    // The trailing call is answered so the base-lane tool-history
    // normalization does not synthesize a result message.
    let request = LLMRequest::new(model(&route, "mistral-large-latest"))
        .with_message(Message::assistant(vec![ContentPart::text(" \n ")]))
        .with_message(Message::assistant(vec![reasoning_part("\t")]))
        .with_message(Message::assistant(vec![tool_call_part("Ab12Cd34E", "lookup", json!({}))]))
        .with_message(Message::tool_result(
            "Ab12Cd34E",
            "lookup",
            ToolResultValue::Text { value: json!("ok") },
        ));
    let body = compile_body(&route, &request);
    assert_eq!(
        body["messages"][0],
        json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                { "id": "Ab12Cd34E", "type": "function", "function": { "name": "lookup", "arguments": "{}" } }
            ]
        })
    );
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn mistral_preserves_remote_media_urls_and_structured_tool_result_media() {
    let route = fixture("");
    let request = LLMRequest::new(model(&route, "mistral-large-latest"))
        .with_message(Message::new(
            MessageRole::User,
            vec![ContentPart::Media {
                media: media::url(
                    "https://assets.example.test/input.png",
                    Some("image/png".into()),
                    None,
                ),
                filename: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        ))
        .with_message(Message::tool_result(
            "Ab12Cd34E",
            "inspect",
            content_result(json!([
                { "type": "text", "text": "Result" },
                { "type": "file", "mime": "image/jpeg", "uri": "https://assets.example.test/output.jpg" },
                { "type": "file", "mime": "application/pdf", "uri": "cGRm" }
            ])),
        ));
    let body = compile_body(&route, &request);
    assert_eq!(
        body["messages"],
        json!([
            {
                "role": "user",
                "content": [{ "type": "image_url", "image_url": "https://assets.example.test/input.png" }]
            },
            {
                "role": "tool",
                "tool_call_id": "Ab12Cd34E",
                "name": "inspect",
                "content": [
                    { "type": "text", "text": "Result" },
                    { "type": "image_url", "image_url": "https://assets.example.test/output.jpg" },
                    { "type": "document_url", "document_url": "data:application/pdf;base64,cGRm" }
                ]
            }
        ])
    );
}

#[test]
fn mistral_concatenates_text_only_content_without_separators() {
    let route = fixture("");
    let request = LLMRequest::new(model(&route, "mistral-large-latest"))
        .with_message(Message::new(
            MessageRole::User,
            vec![ContentPart::text("first"), ContentPart::text("second")],
        ))
        .with_message(Message::tool_result(
            "Ab12Cd34E",
            "lookup",
            content_result(json!([
                { "type": "text", "text": "third" },
                { "type": "text", "text": "fourth" }
            ])),
        ));
    let body = compile_body(&route, &request);
    assert_eq!(
        body["messages"],
        json!([
            { "role": "user", "content": "firstsecond" },
            { "role": "tool", "tool_call_id": "Ab12Cd34E", "name": "lookup", "content": "thirdfourth" }
        ])
    );
}

#[test]
fn mistral_route_targets_the_native_endpoint() {
    let transport: SharedTransport =
        Arc::new(crate::route::transport::FailingTransport { message: "no requests" });
    let route = mistral_route(transport);
    assert_eq!(route.endpoint.base_url.as_deref(), Some(DEFAULT_BASE_URL));
    let request = LLMRequest::new(model(&route, "mistral-small-latest")).with_prompt("Hi");
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(transport_request.url, format!("https://api.mistral.ai/v1{PATH}"));
}

// =============================================================================
// Stream machine
// =============================================================================

#[tokio::test]
async fn mistral_maps_usage_variants_and_clamps_cache_reads() {
    let body = sse_events(&[
        json!({
            "choices": [{ "delta": { "content": "Hi" }, "finish_reason": null }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120,
                "num_cached_tokens": 150,
                "prompt_tokens_details": { "cached_tokens": 60 }
            }
        }),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }),
    ]);
    let route = fixture(&body);
    let response = route
        .generate(LLMRequest::new(model(&route, "mistral-small-latest")).with_prompt("Hi"))
        .await
        .unwrap();
    let usage = response.usage.clone().unwrap();
    // num_cached_tokens (150) exceeds prompt_tokens (100) — clamped
    assert_eq!(usage.input_tokens, Some(100.0));
    assert_eq!(usage.cache_read_input_tokens, Some(100.0));
    assert_eq!(usage.non_cached_input_tokens, Some(0.0));
    assert_eq!(usage.output_tokens, Some(20.0));
    assert_eq!(usage.total_tokens, Some(120.0));
    assert_eq!(response.text(), "Hi");
}

#[tokio::test]
async fn mistral_rejects_invalid_stream_endings() {
    let body = sse_events(&[json!({ "choices": [{ "delta": { "content": "Hi" } }] })]);
    let route = fixture(&body);
    let error = route
        .generate(LLMRequest::new(model(&route, "mistral-small-latest")).with_prompt("Hi"))
        .await
        .unwrap_err();
    assert_eq!(error.tag(), "InvalidProviderOutput");
    assert!(error.message().contains("ended without finish_reason"), "{}", error);
    let classification = match &error.reason {
        crate::schema::AiErrorReason::InvalidProviderOutput { classification, .. } => {
            classification.clone()
        }
        _ => None,
    };
    assert_eq!(classification.as_deref(), Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM));
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
    let protocol = MistralChatProtocol;
    let mut state = Some(protocol.initial(request));
    let mut events: Vec<Value> = Vec::new();
    let mut failure: Option<(String, String)> = None;
    let mut decoder = FramingDecoder::new(Framing::sse_with_done());
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

fn mistral_dumps() -> Vec<(String, Dump)> {
    let mut dumps = Vec::new();
    for group in ["mistral-chat", "mistral-chat-glm"] {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-fixtures/expected")
            .join(group);
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{group} dumps: {error}"))
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        paths.sort();
        for path in paths {
            let dump: Dump =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            dumps.push((group.to_string(), dump));
        }
    }
    dumps
}

#[test]
fn mistral_replay_step_machine_matches_ts_dumps() {
    let mut interactions_checked = 0;
    for (group, dump) in mistral_dumps() {
        let cassette_path =
            crate::test_support::recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path).unwrap();
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "no transport needed" });
        let route = mistral_route(transport);
        let model = LanguageModel::new(dump.model.clone(), dump.provider.clone(), Arc::new(route));
        // The mistral parser state does not read the request, so one bound
        // request covers every interaction of a scenario.
        let request = LLMRequest::new(model).with_prompt("hi");
        let _ = group;

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
    assert_eq!(interactions_checked, 6, "expected the 6 recorded mistral-chat interactions");
}

#[tokio::test]
async fn mistral_replay_route_stack_matches_ts_dumps() {
    for (_group, dump) in mistral_dumps() {
        let cassette_path =
            crate::test_support::recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path).unwrap();
        let transport = MockTransport::from_cassette(&cassette).with_strict_url();
        let route = mistral_route(Arc::new(transport) as SharedTransport);
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
