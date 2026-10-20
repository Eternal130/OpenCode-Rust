//! C7c AI-MEDIA — `google-interactions` fixture replay (ledger `AI-M3`).
//!
//! The four v2 recordings under
//! `test-fixtures/recordings/google-interactions/` replay through the
//! production route stack (protocol + endpoint + auth + SSE framing)
//! against the cassette mock transport: text+usage, reasoning with
//! retained thought signatures, stateless native tool-result replay, and
//! `previous_interaction_id` continuation. Wire bodies match the
//! recordings (ADR-0006: no live network).

use std::sync::Arc;

use oc_ai::protocols::google_interactions::{google_interactions_route, GoogleInteractionsConfig};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::SharedTransport;
use oc_ai::schema::{
    ContentPart, FinishReason, GenerationOptions, LLMRequest, Message, MessageRole, ToolChoice,
    ToolDefinition, ToolEntry, ToolResultValue,
};
use oc_ai::test_support::{Cassette, MockTransport};
use serde_json::{json, Value};

fn weather_tool() -> ToolEntry {
    ToolEntry::Definition(ToolDefinition {
        name: "get_weather".into(),
        description: "Get current weather for a city.".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"],
            "additionalProperties": false
        }),
        output_schema: None,
        cache: None,
        metadata: None,
        native: None,
    })
}

fn route(cassette: &Cassette) -> (oc_ai::route::Route, Arc<MockTransport>) {
    let transport = Arc::new(MockTransport::from_cassette(cassette));
    let route = google_interactions_route(
        Arc::clone(&transport) as SharedTransport,
        GoogleInteractionsConfig { api_key: Some(Secret::Value("fixture".into())), base_url: None },
    );
    (route, transport)
}

fn request(route: &oc_ai::route::Route, model: &str) -> LLMRequest {
    LLMRequest::new(route.model(model, None).expect("model binds"))
}

fn options(pairs: &[(&str, Value)]) -> oc_ai::schema::ProviderOptions {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn recorded_request_body(cassette: &Cassette, index: usize) -> Value {
    let raw = cassette.interactions[index]
        .request
        .as_ref()
        .and_then(|request| request.body.as_ref())
        .and_then(|body| body.as_str())
        .expect("recorded body");
    serde_json::from_str(raw).expect("recorded body parses")
}

fn sent_body(transport: &Arc<MockTransport>, index: usize) -> Value {
    serde_json::from_slice(&transport.requests()[index].body).expect("sent body parses")
}

/// The ledger repro row (`crates/oc-ai/tests/google_interactions.rs::
/// fixture_replay`): all four cassettes replay through the route stack.
#[tokio::test]
async fn fixture_replay() {
    streams_text_and_reports_usage().await;
    streams_reasoning_and_retains_thought_signatures().await;
    replays_native_tool_results_and_signatures_statelessly().await;
    continues_tool_results_with_previous_interaction_id().await;
}

async fn streams_text_and_reports_usage() {
    let path = oc_ai::test_support::recordings_dir()
        .join("google-interactions")
        .join("streams-text-and-reports-usage.json");
    let cassette = Cassette::load(&path).expect("cassette parses");
    let (route, transport) = route(&cassette);
    let mut request = request(&route, "gemini-3.8-flash");
    request.messages = vec![Message::user("Reply with exactly one word: hello")];
    request.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
    request.provider_options = Some(options(&[("thinkingLevel", json!("low"))]));

    let response = route.generate(request).await.expect("stream completes");
    assert_eq!(response.text().trim().to_lowercase(), "hello");
    assert_eq!(response.finish_reason.normalized, FinishReason::Stop);
    assert!(response
        .events
        .iter()
        .any(|event| matches!(event, oc_ai::schema::LLMEvent::TextDelta { .. })));
    let usage = response.usage.as_ref().expect("usage reported");
    assert!(usage.input_tokens.unwrap_or(0.0) > 0.0);
    assert!(usage.context_tokens.unwrap_or(0.0) > 0.0);
    let metadata = usage.provider_metadata.as_ref().expect("usage metadata");
    assert!(metadata["google"].contains_key("total_input_tokens"));

    // wire: the recorded URL, auth header, and body
    let sent = &transport.requests()[0];
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.url, "https://generativelanguage.googleapis.com/v1beta/interactions");
    assert_eq!(sent.header("x-goog-api-key"), Some("fixture"));
    assert_eq!(sent_body(&transport, 0), recorded_request_body(&cassette, 0));
}

async fn streams_reasoning_and_retains_thought_signatures() {
    let path = oc_ai::test_support::recordings_dir()
        .join("google-interactions")
        .join("streams-reasoning-and-retains-thought-signatures.json");
    let cassette = Cassette::load(&path).expect("cassette parses");
    let (route, _transport) = route(&cassette);
    let mut request = request(&route, "gemini-3.8-flash");
    request.messages = vec![Message::user(
        "Find the smallest positive integer that leaves remainder 1 modulo 7, 2 modulo 9, and 3 modulo 11. Explain briefly.",
    )];
    request.generation = Some(GenerationOptions { max_tokens: Some(4096.0), ..Default::default() });
    request.provider_options =
        Some(options(&[("thinkingLevel", json!("high")), ("thinkingSummaries", json!("auto"))]));

    let response = route.generate(request).await.expect("stream completes");
    assert_ne!(response.reasoning(), "", "reasoning text recorded");
    assert!(response
        .events
        .iter()
        .any(|event| matches!(event, oc_ai::schema::LLMEvent::ReasoningDelta { .. })));
    let signatures: Vec<&oc_ai::schema::ProviderMetadata> = response
        .events
        .iter()
        .filter_map(|event| match event {
            oc_ai::schema::LLMEvent::ReasoningEnd { provider_metadata, .. } => {
                provider_metadata.as_ref()
            }
            _ => None,
        })
        .collect();
    assert!(
        signatures.iter().any(|metadata| metadata["google"].contains_key("interactionSignature")),
        "reasoning end retains the thought signature"
    );
    assert_ne!(response.text(), "");
    assert_eq!(response.finish_reason.normalized, FinishReason::Stop);
}

async fn replays_native_tool_results_and_signatures_statelessly() {
    let path = oc_ai::test_support::recordings_dir()
        .join("google-interactions")
        .join("replays-native-tool-results-and-signatures-statelessly.json");
    let cassette = Cassette::load(&path).expect("cassette parses");
    let (route, transport) = route(&cassette);

    let mut first = request(&route, "gemini-3.8-flash");
    first.system = vec![oc_ai::schema::SystemPart {
        text: "Use get_weather for weather questions. Answer concisely after receiving the result."
            .into(),
        cache: None,
        metadata: None,
    }];
    first.messages = vec![Message::user("What is the weather in Paris?")];
    first.tools = vec![weather_tool()];
    first.tool_choice =
        Some(ToolChoice::Named { name: "get_weather".into(), disable_parallel_tool_use: None });
    first.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
    first.provider_options =
        Some(options(&[("thinkingLevel", json!("low")), ("thinkingSummaries", json!("auto"))]));

    let response = route.generate(first).await.expect("first turn completes");
    assert_eq!(sent_body(&transport, 0), recorded_request_body(&cassette, 0), "first body matches");
    let tool_calls: Vec<&ContentPart> = response
        .message
        .content
        .iter()
        .filter(|part| matches!(part, ContentPart::ToolCall { .. }))
        .collect();
    assert_eq!(tool_calls.len(), 1);
    assert!(response
        .events
        .iter()
        .any(|event| matches!(event, oc_ai::schema::LLMEvent::ToolInputDelta { .. })));
    assert_eq!(response.finish_reason.normalized, FinishReason::ToolCalls);
    let ContentPart::ToolCall { id, name, input, provider_metadata, .. } = &tool_calls[0] else {
        panic!("tool call part")
    };
    assert_eq!(name, "get_weather");
    assert_eq!(input, &json!({ "city": "Paris" }));
    assert!(provider_metadata.as_ref().expect("signature metadata")["google"]
        .contains_key("interactionSignature"));

    // second turn: the assistant message replays statelessly beside the result
    let mut second = request(&route, "gemini-3.8-flash");
    second.system = vec![oc_ai::schema::SystemPart {
        text: "Use get_weather for weather questions. Answer concisely after receiving the result."
            .into(),
        cache: None,
        metadata: None,
    }];
    second.messages = vec![
        Message::user("What is the weather in Paris?"),
        response.message.clone(),
        Message {
            role: MessageRole::Tool,
            content: vec![ContentPart::ToolResult {
                id: id.clone(),
                name: name.clone(),
                namespace: None,
                result: ToolResultValue::Json {
                    value: json!({ "temperature": 22, "condition": "sunny" }),
                },
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
            ..Message::user("")
        },
    ];
    second.tools = vec![weather_tool()];
    second.tool_choice = Some(ToolChoice::None {});
    second.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
    second.provider_options =
        Some(options(&[("thinkingLevel", json!("low")), ("thinkingSummaries", json!("auto"))]));

    let second_response = route.generate(second).await.expect("second turn completes");
    assert_eq!(
        sent_body(&transport, 1),
        recorded_request_body(&cassette, 1),
        "the stateless replay body matches the recording"
    );
    assert!(second_response.text().to_lowercase().contains("sunny"));
    assert!(second_response.text().contains("22"));
    assert_eq!(second_response.finish_reason.normalized, FinishReason::Stop);
}

async fn continues_tool_results_with_previous_interaction_id() {
    let path = oc_ai::test_support::recordings_dir()
        .join("google-interactions")
        .join("continues-tool-results-with-previous-interaction-id.json");
    let cassette = Cassette::load(&path).expect("cassette parses");
    let (route, transport) = route(&cassette);

    let mut first = request(&route, "gemini-3.8-flash");
    first.system = vec![oc_ai::schema::SystemPart {
        text: "Use get_weather for weather questions. Answer concisely after receiving the result."
            .into(),
        cache: None,
        metadata: None,
    }];
    first.messages = vec![Message::user("What is the weather in Paris?")];
    first.tools = vec![weather_tool()];
    first.tool_choice = Some(ToolChoice::Required { disable_parallel_tool_use: None });
    first.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
    first.provider_options =
        Some(options(&[("thinkingLevel", json!("low")), ("store", json!(true))]));

    let response = route.generate(first).await.expect("first turn completes");
    assert_eq!(sent_body(&transport, 0), recorded_request_body(&cassette, 0));
    let interaction_id = response
        .events
        .iter()
        .find_map(|event| match event {
            oc_ai::schema::LLMEvent::Finish { provider_metadata, .. } => provider_metadata
                .as_ref()
                .and_then(|metadata| metadata["google"].get("interactionId"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .expect("finish carries the interaction id");
    assert_ne!(interaction_id, "");
    let ContentPart::ToolCall { id, name, .. } = response
        .message
        .content
        .iter()
        .find(|part| matches!(part, ContentPart::ToolCall { .. }))
        .expect("tool call")
    else {
        panic!("tool call part")
    };

    // second turn: continuation references the stored interaction
    let mut second = request(&route, "gemini-3.8-flash");
    second.system = vec![oc_ai::schema::SystemPart {
        text: "Use get_weather for weather questions. Answer concisely after receiving the result."
            .into(),
        cache: None,
        metadata: None,
    }];
    second.messages = vec![Message {
        role: MessageRole::Tool,
        content: vec![ContentPart::ToolResult {
            id: id.clone(),
            name: name.clone(),
            namespace: None,
            result: ToolResultValue::Json {
                value: json!({ "temperature": 22, "condition": "sunny" }),
            },
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }],
        ..Message::user("")
    }];
    second.tools = vec![weather_tool()];
    second.tool_choice = Some(ToolChoice::None {});
    second.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
    second.provider_options = Some(options(&[
        ("thinkingLevel", json!("low")),
        ("previousInteractionId", json!(interaction_id)),
        ("store", json!(false)),
    ]));

    let second_response = route.generate(second).await.expect("continuation completes");
    assert_eq!(
        sent_body(&transport, 1),
        recorded_request_body(&cassette, 1),
        "the continuation body matches the recording"
    );
    assert!(second_response.text().to_lowercase().contains("sunny"));
    assert_eq!(second_response.finish_reason.normalized, FinishReason::Stop);
}
