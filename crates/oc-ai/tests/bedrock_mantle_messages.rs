//! Bedrock Mantle Messages recorded-fixture tests (G-19): the
//! anthropic-messages protocol on the mantle host
//! (`bedrock-mantle.{region}.api.aws/anthropic/v1/messages`) — wire-body
//! parity against the four copied recordings under
//! `test-fixtures/recordings/bedrock-mantle-messages/` plus the behavioral
//! assertions of the ref recorded suite
//! (`bedrock-mantle.test.ts` "Amazon Bedrock Mantle Messages recorded").

use std::path::PathBuf;
use std::sync::Arc;

use oc_ai::protocols::bedrock::mantle_messages::{
    bedrock_mantle_messages_route, MantleMessagesConfig,
};
use oc_ai::protocols::bedrock::BedrockAuthMode;
use oc_ai::route::client::Route;
use oc_ai::route::SharedTransport;
use oc_ai::schema::{
    CachePolicy, ContentPart, FinishReason, GenerationOptions, LLMRequest, Message, ToolDefinition,
    ToolEntry, ToolResultValue,
};
use oc_ai::test_support::{Cassette, MockTransport};
use serde_json::{json, Value};

fn recording(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test-fixtures/recordings/bedrock-mantle-messages")
        .join(format!("{name}.json"))
}

fn recorded_request_body(cassette: &Cassette, index: usize) -> Value {
    let body = cassette
        .http_interactions()
        .nth(index)
        .and_then(|interaction| interaction.request.as_ref())
        .and_then(|request| request.body.as_ref())
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("interaction {index} has a recorded request body"));
    serde_json::from_str(body).expect("recorded body parses")
}

/// Deep equality over parsed JSON with numbers compared as `f64`.
#[allow(clippy::float_cmp)]
fn json_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            a.as_f64().is_some_and(|a| b.as_f64().is_some_and(|b| a == b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| json_equal(value, other)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| json_equal(x, y))
        }
        _ => actual == expected,
    }
}

/// The mantle messages route wired like the recording run (bearer fixture
/// key, us-east-1) with a cassette-backed transport.
fn fixture_route(name: &str) -> (Route, Arc<MockTransport>) {
    let cassette = Cassette::load(&recording(name)).unwrap();
    let mock = Arc::new(MockTransport::from_cassette(&cassette));
    let route = bedrock_mantle_messages_route(
        Arc::clone(&mock) as SharedTransport,
        MantleMessagesConfig {
            api_key: Some("fixture".into()),
            region: Some("us-east-1".into()),
            ..Default::default()
        },
    )
    .expect("route builds");
    (route, mock)
}

fn options(pairs: &[(&str, Value)]) -> serde_json::Map<String, Value> {
    pairs.iter().map(|(key, value)| (key.to_string(), value.clone())).collect()
}

fn weather_tool() -> ToolEntry {
    ToolEntry::Definition(ToolDefinition {
        name: "get_weather".into(),
        description: "Get the current weather in a city".into(),
        input_schema: json!({
            "type": "object",
            "properties": {"city": {"type": "string", "enum": ["Paris"]}},
            "required": ["city"],
            "additionalProperties": false
        }),
        output_schema: None,
        cache: None,
        metadata: None,
        native: None,
    })
}

/// The compiled wire body (post http.body overlay) of a request.
fn compiled_body(route: &Route, request: &LLMRequest) -> Value {
    let prepared = route.prepare_request(request);
    let body = route.build_body(&prepared).expect("body builds");
    let (_, body_text) = route.prepare_transport(&prepared, &body).expect("transport prepares");
    serde_json::from_str(&body_text).expect("body parses")
}

/// "replays signed thinking through a tool loop and native system update"
/// (opus-4-8): tool loop with adaptive summarized thinking, signed thinking
/// replay, and a native (role:system) system update.
#[tokio::test]
async fn replays_signed_thinking_through_a_tool_loop_and_native_system_update() {
    let (route, mock) =
        fixture_route("replays-signed-thinking-through-a-tool-loop-and-native-system-update");
    let cassette = Cassette::load(&recording(
        "replays-signed-thinking-through-a-tool-loop-and-native-system-update",
    ))
    .unwrap();

    let mut initial =
        LLMRequest::new(route.model("anthropic.claude-opus-4-8", None).expect("model binds"));
    initial.system = vec![oc_ai::schema::SystemPart {
        text: "You are a concise assistant.".into(),
        cache: None,
        metadata: None,
    }];
    initial.messages = vec![Message::user(
        "First calculate 37 * 43. Then call get_weather for Paris. After receiving the tool result, state both the product and the weather.",
    )];
    initial.tools = vec![weather_tool()];
    initial.provider_options = Some(options(&[
        ("thinking", json!({"type": "adaptive", "display": "summarized"})),
        ("effort", json!("medium")),
    ]));
    initial.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });

    let first = route.generate(initial.clone()).await.expect("first turn");
    assert_eq!(first.finish_reason.normalized, FinishReason::ToolCalls);
    let tool_call = first
        .message
        .content
        .iter()
        .find_map(|part| match part {
            ContentPart::ToolCall { name, input, .. } => Some((name.clone(), input.clone())),
            _ => None,
        })
        .expect("tool call in the response");
    assert_eq!(tool_call.0, "get_weather");
    assert_eq!(tool_call.1, json!({"city": "Paris"}));
    assert_ne!(first.reasoning(), "", "reasoning recorded");
    let signature = first
        .message
        .content
        .iter()
        .find_map(|part| match part {
            ContentPart::Reasoning { provider_metadata, .. } => provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mantle"))
                .and_then(|mantle| mantle.get("signature"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .expect("signed thinking carries the mantle signature");

    // The sent wire body matches the recording.
    let sent = mock.requests();
    assert!(json_equal(
        &serde_json::from_slice::<Value>(&sent[0].body).unwrap(),
        &recorded_request_body(&cassette, 0)
    ));

    // Follow-up: replay the signed thinking + tool result + system update.
    let call_id = first
        .message
        .content
        .iter()
        .find_map(|part| match part {
            ContentPart::ToolCall { id, .. } => Some(id.clone()),
            _ => None,
        })
        .expect("tool call id");
    let mut follow_up = initial.clone();
    let additional_messages = vec![
        first.message.clone(),
        Message::tool_result(
            call_id,
            "get_weather",
            ToolResultValue::Json { value: json!({"condition": "sunny", "temperatureC": 18}) },
        ),
        Message::system("Reply in French in one short sentence."),
    ];
    follow_up.messages = [initial.messages.clone(), additional_messages].concat();
    let body = compiled_body(&route, &follow_up);
    // Signed thinking replays under the assistant turn...
    let assistant = &body["messages"][1];
    assert_eq!(assistant["role"], "assistant");
    let thinking = &assistant["content"][0];
    assert_eq!(thinking["type"], "thinking");
    assert_eq!(thinking["signature"], json!(signature));
    // ...and the system update lowers to a native role:system message.
    let last = &body["messages"][body["messages"].as_array().unwrap().len() - 1];
    assert_eq!(last["role"], "system");
    assert_eq!(last["content"][0]["text"], json!("Reply in French in one short sentence."));
    assert!(json_equal(&body, &recorded_request_body(&cassette, 1)));

    let second = route.generate(follow_up).await.expect("second turn");
    assert_eq!(second.finish_reason.normalized, FinishReason::Stop);
    assert!(second.text().contains("1591"), "{}", second.text());
    assert!(second.text().contains("18"), "{}", second.text());
}

/// "lowers system updates to wrapped user text on Haiku 4.5 with budget
/// thinking": no native system role; the update wraps into user text.
#[tokio::test]
async fn lowers_system_updates_to_wrapped_user_text_on_haiku_4_5() {
    let (route, mock) = fixture_route(
        "lowers-system-updates-to-wrapped-user-text-on-haiku-4-5-with-budget-thinking",
    );
    let cassette = Cassette::load(&recording(
        "lowers-system-updates-to-wrapped-user-text-on-haiku-4-5-with-budget-thinking",
    ))
    .unwrap();

    let mut request =
        LLMRequest::new(route.model("anthropic.claude-haiku-4-5", None).expect("model binds"));
    request.messages = vec![
        Message::user("What is 19 multiplied by 23?"),
        Message::system("Reply with only the integer."),
    ];
    request.provider_options =
        Some(options(&[("thinking", json!({"type": "enabled", "budgetTokens": 1024}))]));
    request.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });

    let body = compiled_body(&route, &request);
    assert_eq!(body["thinking"], json!({"type": "enabled", "budget_tokens": 1024}));
    assert!(
        !body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "system"),
        "haiku has no native system messages: {body}"
    );
    assert!(body.to_string().contains("<system-update>"), "{body}");
    assert!(json_equal(&body, &recorded_request_body(&cassette, 0)));

    let response = route.generate(request).await.expect("generates");
    assert_eq!(response.finish_reason.normalized, FinishReason::Stop);
    assert_ne!(response.reasoning(), "");
    assert!(response.text().contains("437"), "{}", response.text());
    assert_eq!(mock.requests().len(), 1);
}

/// "applies mid-conversation effort updates and thinking block binding on
/// Opus 5.5": the thinking binding default plus a native effort-update
/// system message with the previous effort hoisted to `output_config`.
#[tokio::test]
async fn applies_mid_conversation_effort_updates_and_thinking_block_binding_on_opus_5_5() {
    let (route, _mock) = fixture_route(
        "applies-mid-conversation-effort-updates-and-thinking-block-binding-on-opus-5-5",
    );
    let cassette = Cassette::load(&recording(
        "applies-mid-conversation-effort-updates-and-thinking-block-binding-on-opus-5-5",
    ))
    .unwrap();

    let mut first_request =
        LLMRequest::new(route.model("anthropic.claude-opus-5-5", None).expect("model binds"));
    first_request.messages =
        vec![Message::user("Compute 37 * 43 step by step, then reply with only the integer.")];
    first_request.provider_options = Some(options(&[
        ("thinking", json!({"type": "adaptive", "display": "summarized"})),
        ("effort", json!("high")),
    ]));
    first_request.generation =
        Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });

    let first_body = compiled_body(&route, &first_request);
    assert_eq!(
        first_body["thinking"],
        json!({
            "type": "adaptive",
            "display": "summarized",
            "block_binding": {"prefix_mismatch_behavior": "drop_block"}
        })
    );
    assert!(json_equal(&first_body, &recorded_request_body(&cassette, 0)));

    let first = route.generate(first_request.clone()).await.expect("first turn");
    assert_eq!(first.finish_reason.normalized, FinishReason::Stop);
    assert_ne!(first.reasoning(), "");
    assert!(first.text().replace(',', "").contains("1591"), "{}", first.text());

    let mut second_request =
        LLMRequest::new(route.model("anthropic.claude-opus-5-5", None).expect("model binds"));
    second_request.messages = [
        first_request.messages.clone(),
        vec![
            first.message.clone(),
            Message::effort(Some("low".into()), Some("high".into())),
            Message::user("Add 9 to that result. Reply with only the integer."),
        ],
    ]
    .concat();
    second_request.provider_options = Some(options(&[
        ("thinking", json!({"type": "adaptive", "display": "summarized"})),
        ("effort", json!("low")),
    ]));
    second_request.generation =
        Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });

    let second_body = compiled_body(&route, &second_request);
    assert_eq!(second_body["output_config"], json!({"effort": "high"}));
    let system_messages: Vec<&Value> = second_body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "system")
        .collect();
    assert_eq!(
        system_messages,
        vec![&json!({"role": "system", "content": [], "output_config": {"effort": "low"}})],
        "{second_body}"
    );
    assert!(json_equal(&second_body, &recorded_request_body(&cassette, 1)));

    let second = route.generate(second_request).await.expect("second turn");
    assert_eq!(second.finish_reason.normalized, FinishReason::Stop);
    assert!(second.text().replace(',', "").contains("1600"), "{}", second.text());
}

/// "strips unsupported mid-conversation effort updates on Opus 5.0":
/// Mantle rejects mid-conversation `output_config` below Claude 5.1, so the
/// marker is dropped and the effort rides the top-level `output_config`.
#[tokio::test]
async fn strips_unsupported_mid_conversation_effort_updates_on_opus_5_0() {
    let (route, _mock) =
        fixture_route("strips-unsupported-mid-conversation-effort-updates-on-opus-5-0");
    let cassette = Cassette::load(&recording(
        "strips-unsupported-mid-conversation-effort-updates-on-opus-5-0",
    ))
    .unwrap();

    let mut request =
        LLMRequest::new(route.model("anthropic.claude-opus-5", None).expect("model binds"));
    request.messages = vec![
        Message::user("What is 12 + 30?"),
        Message::assistant(vec![ContentPart::text("42")]),
        Message::effort(Some("low".into()), Some("high".into())),
        Message::user("Add 8 to that result. Reply with only the integer."),
    ];
    request.provider_options = Some(options(&[
        ("thinking", json!({"type": "adaptive", "display": "summarized"})),
        ("effort", json!("low")),
    ]));
    request.generation = Some(GenerationOptions { max_tokens: Some(1024.0), ..Default::default() });

    let body = compiled_body(&route, &request);
    assert_eq!(body["output_config"], json!({"effort": "low"}));
    assert!(
        !body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "system"),
        "opus 5.0 strips the effort-update system message: {body}"
    );
    assert!(json_equal(&body, &recorded_request_body(&cassette, 0)));

    let response = route.generate(request).await.expect("generates");
    assert_eq!(response.finish_reason.normalized, FinishReason::Stop);
    assert!(response.text().contains("50"), "{}", response.text());
}

// ===========================================================================
// Route shape (unit-level)
// ===========================================================================

#[test]
fn route_points_at_the_mantle_anthropic_root_with_bearer_auth() {
    let transport: SharedTransport = Arc::new(MockTransport::from_interactions(vec![]));
    let route = bedrock_mantle_messages_route(
        Arc::clone(&transport) as SharedTransport,
        MantleMessagesConfig {
            api_key: Some("mantle-key".into()),
            region: Some("us-west-1".into()),
            ..Default::default()
        },
    )
    .expect("route builds");
    assert_eq!(route.id, "bedrock-mantle-messages");
    assert_eq!(route.protocol.id(), "anthropic-messages");
    assert_eq!(route.provider_metadata_key.as_deref(), Some("mantle"));
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://bedrock-mantle.us-west-1.api.aws/anthropic/v1")
    );

    let mut request =
        LLMRequest::new(route.model("anthropic.claude-opus-4-8", None).expect("model binds"));
    request.messages = vec![Message::user("Hi")];
    request.cache = Some(CachePolicy::None);
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(
        transport_request.url,
        "https://bedrock-mantle.us-west-1.api.aws/anthropic/v1/messages"
    );
    assert_eq!(transport_request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(transport_request.header("authorization"), Some("Bearer mantle-key"));
    let sent: Value = serde_json::from_slice(&transport_request.body).unwrap();
    assert_eq!(sent["model"], "anthropic.claude-opus-4-8");
    assert_eq!(sent["stream"], true);
}

#[test]
fn sigv4_signing_targets_the_bedrock_mantle_service() {
    let transport: SharedTransport = Arc::new(MockTransport::from_interactions(vec![]));
    let route = bedrock_mantle_messages_route(
        Arc::clone(&transport) as SharedTransport,
        MantleMessagesConfig {
            credentials: Some(oc_ai::protocols::bedrock::Credentials {
                region: "us-west-1".into(),
                access_key_id: "AKIAIOSFODNN7EXAMPLE".into(),
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
                session_token: None,
            }),
            ..Default::default()
        },
    )
    .expect("route builds");
    let mut request =
        LLMRequest::new(route.model("anthropic.claude-opus-4-8", None).expect("model binds"));
    request.messages = vec![Message::user("Hi")];
    request.cache = Some(CachePolicy::None);
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    let authorization = transport_request.header("authorization").expect("signed");
    assert!(authorization.contains("/us-west-1/bedrock-mantle/aws4_request"), "{authorization}");
}

#[test]
fn effort_updates_require_claude_5_1_on_mantle() {
    let transport: SharedTransport = Arc::new(MockTransport::from_interactions(vec![]));
    let route =
        bedrock_mantle_messages_route(transport, MantleMessagesConfig::default()).expect("route builds");
    let opus_5_0 = LLMRequest::new(route.model("anthropic.claude-opus-5", None).unwrap());
    assert!(!route.protocol.supports_effort_updates(&opus_5_0), "opus 5.0 unsupported");
    let opus_5_1 = LLMRequest::new(route.model("anthropic.claude-opus-5-1", None).unwrap());
    assert!(route.protocol.supports_effort_updates(&opus_5_1), "opus 5.1 supported");
    let opus_4_8 = LLMRequest::new(route.model("anthropic.claude-opus-4-8", None).unwrap());
    assert!(!route.protocol.supports_effort_updates(&opus_4_8), "opus 4.8 unsupported");
}

// ===========================================================================
// G-319 — the `configure` guards run at route construction (fail-fast
// ProviderConfigurationError instead of a silent SigV4 fallback).
// ===========================================================================

fn bare_transport() -> SharedTransport {
    Arc::new(MockTransport::from_interactions(vec![])) as SharedTransport
}

#[test]
fn bearer_mode_without_an_api_key_fails_route_construction() {
    std::env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
    let error = bedrock_mantle_messages_route(
        bare_transport(),
        MantleMessagesConfig { mode: Some(BedrockAuthMode::Bearer), ..Default::default() },
    )
    .expect_err("bearer without apiKey must fail fast");
    let message = format!("{error:?}");
    assert!(message.contains("bearer auth requires apiKey"), "got: {message}");
    // The typed reason is ProviderConfiguration (not a SigV4 fallback).
    assert_eq!(error.tag(), "ProviderConfiguration");
}

#[test]
fn bearer_mode_with_an_api_key_builds() {
    let route = bedrock_mantle_messages_route(
        bare_transport(),
        MantleMessagesConfig {
            mode: Some(BedrockAuthMode::Bearer),
            api_key: Some("mantle-key".into()),
            ..Default::default()
        },
    )
    .expect("bearer with apiKey builds");
    assert_eq!(route.id, "bedrock-mantle-messages");
}

#[test]
fn sigv4_mode_builds_without_an_api_key() {
    std::env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
    let route = bedrock_mantle_messages_route(
        bare_transport(),
        MantleMessagesConfig { mode: Some(BedrockAuthMode::SigV4), ..Default::default() },
    )
    .expect("sigv4 without apiKey builds");
    assert_eq!(route.id, "bedrock-mantle-messages");
}
