//! C7b AI-PROVIDERS slice 2 — native Cohere + Mistral protocol wiring
//! (ledger `AI-COHERE-MISTRAL`, cluster C7).
//!
//! v2's providers map their native packages onto the dedicated protocols:
//!
//! - Cohere (`packages/ai/src/providers/cohere.ts:21-44`): the native
//!   `cohere-chat` route at `https://api.cohere.com/v2/chat` plus the
//!   OpenAI-compatibility `cohere-chat-completions` route at
//!   `https://api.cohere.ai/compatibility/v1`; both behind optional bearer
//!   auth (`AuthOptions.bearer(input, "COHERE_API_KEY")`).
//! - Mistral (`packages/ai/src/providers/mistral.ts:23-30`): the native
//!   `mistral-chat` route at `https://api.mistral.ai/v1/chat/completions`
//!   with optional bearer auth (`MISTRAL_API_KEY`).
//!
//! The Rust seam is `cohere_route_for_package` / `mistral_route_for_package`
//! (the oc-ai half of the facade's `family_of` selection); the runtime rows
//! run the production route pipeline against a local SSE mock bound to
//! `127.0.0.1:0` (ADR-0006: no real network) and assert the wire shapes the
//! mocks actually received.

use std::sync::Arc;

use oc_ai::protocols::cohere_chat::{
    cohere_configured, cohere_route_for_package, CohereConfig, CHAT_COMPAT_PACKAGE,
    COMPATIBILITY_BASE_URL, NATIVE_PACKAGE as COHERE_NATIVE_PACKAGE,
};
use oc_ai::protocols::mistral_chat::{
    mistral_configured, mistral_route_for_package, MistralConfig,
    NATIVE_PACKAGE as MISTRAL_NATIVE_PACKAGE,
};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::schema::{
    ContentPart, GenerationOptions, LLMRequest, Message, MessageRole, SystemPart, ToolChoice,
    ToolDefinition, ToolEntry,
};
use serde_json::{json, Value};

fn transport() -> Arc<ReqwestTransport> {
    Arc::new(ReqwestTransport::new())
}

fn key() -> Secret {
    Secret::Value("native-key-1".into())
}

/// Ledger repro: `cargo nextest run -p oc-ai -E 'test(family_mapping)'` —
/// the native Cohere/Mistral packages map onto their native protocols
/// (the symptom row: `@ai-sdk/cohere` → `UnsupportedProtocol` and the
/// facade mapping both providers onto `OpenAIChat`).
#[test]
fn family_mapping() {
    // Cohere native: the Cohere v2 chat protocol at api.cohere.com/v2/chat
    let route = cohere_route_for_package(
        COHERE_NATIVE_PACKAGE,
        transport(),
        CohereConfig { api_key: Some(key()), base_url: None },
    )
    .expect("native cohere package maps");
    assert_eq!(route.protocol.id(), "cohere-chat");
    assert_eq!(route.endpoint.base_url.as_deref(), Some("https://api.cohere.com/v2"));
    assert_eq!(endpoint_path(&route), Some("/chat".to_string()));

    // Cohere compatibility lane: the OpenAI chat protocol at
    // api.cohere.ai/compatibility/v1 (providers/cohere.ts chatRoute)
    let route = cohere_route_for_package(
        CHAT_COMPAT_PACKAGE,
        transport(),
        CohereConfig { api_key: Some(key()), base_url: None },
    )
    .expect("compat cohere package maps");
    assert_eq!(route.protocol.id(), "openai-chat");
    assert_eq!(route.id, "cohere-chat-completions");
    assert_eq!(route.endpoint.base_url.as_deref(), Some(COMPATIBILITY_BASE_URL));
    assert_eq!(endpoint_path(&route), Some("/chat/completions".to_string()));

    // Mistral native: the Mistral chat protocol at api.mistral.ai/v1
    let route = mistral_route_for_package(
        MISTRAL_NATIVE_PACKAGE,
        transport(),
        MistralConfig { api_key: Some(key()), base_url: None },
    )
    .expect("native mistral package maps");
    assert_eq!(route.protocol.id(), "mistral-chat");
    assert_eq!(route.endpoint.base_url.as_deref(), Some("https://api.mistral.ai/v1"));
    assert_eq!(endpoint_path(&route), Some("/chat/completions".to_string()));

    // foreign packages do not map
    assert!(cohere_route_for_package(
        "@opencode/ai/providers/groq",
        transport(),
        CohereConfig::default()
    )
    .is_none());
    assert!(mistral_route_for_package(
        "@opencode/ai/providers/groq",
        transport(),
        MistralConfig::default()
    )
    .is_none());
}

/// The prepared wire shapes with explicit credentials: bearer auth headers
/// on both native protocols (nothing dialed — `prepare_transport` only).
#[test]
fn native_bearer_auth_shapes() {
    let route =
        cohere_configured(transport(), CohereConfig { api_key: Some(key()), base_url: None });
    let (wire, _) = wire_request(&route, "command-a-03-2025");
    assert_eq!(wire.url, "https://api.cohere.com/v2/chat", "cohere native URL shape");
    assert_eq!(wire.header("authorization"), Some("Bearer native-key-1"));

    let route =
        mistral_configured(transport(), MistralConfig { api_key: Some(key()), base_url: None });
    let (wire, _) = wire_request(&route, "mistral-large-latest");
    assert_eq!(wire.url, "https://api.mistral.ai/v1/chat/completions", "mistral native URL shape");
    assert_eq!(wire.header("authorization"), Some("Bearer native-key-1"));
}

/// Cohere native wire shape end-to-end: `POST {base}/chat` with the Cohere
/// v2 body — OpenAI-shaped messages with `tool_call_id` tool results,
/// `tool_choice: "NONE"|"REQUIRED"`, native `thinking` budget — and the
/// typed SSE stream decodes to a terminal `message-end`.
#[tokio::test]
async fn cohere_native_wire_shape() {
    let mock = SseMock::spawn(cohere_stream()).await;
    let route = cohere_configured(
        transport(),
        CohereConfig {
            api_key: Some(key()),
            base_url: Some(format!("http://127.0.0.1:{}", mock.port)),
        },
    );

    let assistant = Message {
        role: MessageRole::Assistant,
        content: vec![ContentPart::ToolCall {
            id: "call_9".to_string(),
            name: "lookup".to_string(),
            namespace: None,
            input: json!({"q": "x"}),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }],
        ..Message::user("")
    };
    let tool_result = Message {
        role: MessageRole::Tool,
        content: vec![ContentPart::ToolResult {
            id: "call_9".to_string(),
            name: "lookup".to_string(),
            namespace: None,
            result: oc_ai::schema::ToolResultValue::make(json!("found"), "text"),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }],
        ..Message::user("")
    };
    let model = route.model("command-a-03-2025", None).expect("model");
    let mut request = LLMRequest::new(model);
    request.system = vec![SystemPart::new("Be brief.")];
    request.messages = vec![Message::user("hi"), assistant, tool_result];
    request.tools = vec![ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "look things up",
        json!({"type":"object","properties":{"q":{"type":"string"}}}),
    ))];
    request.tool_choice = Some(ToolChoice::Required { disable_parallel_tool_use: None });
    request.generation = Some(GenerationOptions {
        max_tokens: Some(1000.0),
        temperature: Some(0.1),
        ..Default::default()
    });
    // native thinking option: token budget fitted against max_tokens
    request.provider_options = Some(
        [("thinking".to_string(), json!({"type": "enabled", "tokenBudget": 2000}))]
            .into_iter()
            .collect(),
    );

    let stream = route.stream(request).await.expect("stream");
    let events = collect(stream).await;
    assert!(
        events.iter().all(std::result::Result::is_ok),
        "{:?}",
        events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|item| item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal)),
        "message-end terminal decodes"
    );

    let (path, headers, body) = mock.last();
    assert_eq!(path, "/chat", "cohere native path");
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer native-key-1"),
        "bearer header"
    );
    let body: Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(body["model"], "command-a-03-2025");
    assert_eq!(body["stream"], json!(true));
    // system message lowered as a single string
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "Be brief.");
    // assistant tool call lowered with id + stringified arguments
    let wire_assistant = &body["messages"][2];
    assert_eq!(wire_assistant["role"], "assistant");
    assert_eq!(wire_assistant["tool_calls"][0]["id"], "call_9");
    assert_eq!(wire_assistant["tool_calls"][0]["function"]["name"], "lookup");
    assert_eq!(wire_assistant["tool_calls"][0]["function"]["arguments"], "{\"q\":\"x\"}");
    // tool result rides the Cohere tool role with tool_call_id
    let wire_tool = &body["messages"][3];
    assert_eq!(wire_tool["role"], "tool");
    assert_eq!(wire_tool["tool_call_id"], "call_9");
    assert_eq!(wire_tool["content"], "found");
    // required tool choice maps onto the native enum
    assert_eq!(body["tool_choice"], "REQUIRED");
    // thinking budget fitted under max_tokens (fitThinkingBudget: a budget
    // above max_tokens/2 is capped down to floor(max_tokens/2))
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["token_budget"], json!(500));
    assert_eq!(body["max_tokens"], json!(1000)); // integral floats encode as JSON ints
}

/// Mistral native wire shape end-to-end: `POST {base}/chat/completions`
/// with the Mistral body — `tool_choice: "any"` for required, strict:false
/// tools, `random_seed`, `prompt_cache_key` passthrough — and the
/// `[DONE]`-terminated stream decodes.
#[tokio::test]
async fn mistral_native_wire_shape() {
    let mock = SseMock::spawn(mistral_stream()).await;
    let route = mistral_configured(
        transport(),
        MistralConfig {
            api_key: Some(key()),
            base_url: Some(format!("http://127.0.0.1:{}", mock.port)),
        },
    );

    let model = route.model("mistral-large-latest", None).expect("model");
    let mut request = LLMRequest::new(model);
    request.system = vec![SystemPart::new("Be brief.")];
    request.messages = vec![Message::user("hi")];
    request.tools = vec![ToolEntry::Definition(ToolDefinition::new(
        "lookup",
        "look things up",
        json!({"type":"object","properties":{"q":{"type":"string"}}}),
    ))];
    request.tool_choice = Some(ToolChoice::Required { disable_parallel_tool_use: None });
    request.generation =
        Some(GenerationOptions { max_tokens: Some(1000.0), seed: Some(7.0), ..Default::default() });
    request.prompt_cache_key = Some("session-42".to_string());

    let stream = route.stream(request).await.expect("stream");
    let events = collect(stream).await;
    assert!(
        events.iter().all(std::result::Result::is_ok),
        "{:?}",
        events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|item| item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal)),
        "terminal event decodes"
    );

    let (path, headers, body) = mock.last();
    assert_eq!(path, "/chat/completions", "mistral native path");
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer native-key-1"),
        "bearer header"
    );
    let body: Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(body["model"], "mistral-large-latest");
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["messages"][0]["role"], "system");
    // required tool choice maps onto Mistral's "any"; tools are strict:false
    assert_eq!(body["tool_choice"], "any");
    assert_eq!(body["tools"][0]["function"]["name"], "lookup");
    assert_eq!(body["tools"][0]["function"]["strict"], json!(false));
    assert_eq!(body["random_seed"], json!(7)); // integral floats encode as JSON ints
    assert_eq!(body["prompt_cache_key"], "session-42");
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn endpoint_path(route: &oc_ai::route::Route) -> Option<String> {
    // static endpoints only — these routes never use dynamic paths
    let oc_ai::route::EndpointPart::Static(path) = &route.endpoint.path else {
        return None;
    };
    Some(path.clone())
}

fn wire_request(
    route: &oc_ai::route::Route,
    model_id: &str,
) -> (oc_ai::route::transport::TransportRequest, String) {
    let model = route.model(model_id, None).expect("model");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    route.prepare_transport(&prepared, &body).expect("wire")
}

async fn collect(
    stream: impl futures::Stream<Item = Result<oc_ai::schema::LLMEvent, oc_ai::schema::AiError>>,
) -> Vec<Result<oc_ai::schema::LLMEvent, oc_ai::schema::AiError>> {
    use futures::StreamExt as _;
    let mut collected = Vec::new();
    let mut stream = Box::pin(stream);
    while let Some(item) = stream.next().await {
        let terminal = item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal);
        collected.push(item);
        if terminal {
            break;
        }
    }
    collected
}

// ---------------------------------------------------------------------------
// Local SSE mock (127.0.0.1:0), path/header/body capturing
// ---------------------------------------------------------------------------

struct SseMock {
    port: u16,
    inner: Arc<MockState>,
}

/// (request path, request headers, request body) as received.
type CapturedRequest = (String, Vec<(String, String)>, String);

struct MockState {
    requests: std::sync::Mutex<Vec<CapturedRequest>>,
}

impl SseMock {
    async fn spawn(body: String) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("port").port();
        let inner = Arc::new(MockState { requests: std::sync::Mutex::new(Vec::new()) });
        tokio::spawn({
            let state = Arc::clone(&inner);
            async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let state = Arc::clone(&state);
                    let body = body.clone();
                    tokio::spawn(async move {
                        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                        let mut buffer = [0u8; 16384];
                        let read = stream.read(&mut buffer).await.unwrap_or(0);
                        let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                        let mut lines = text.lines();
                        let request_line =
                            lines.next().unwrap_or_default().split(' ').nth(1).unwrap_or_default();
                        let headers: Vec<(String, String)> = lines
                            .take_while(|line| !line.is_empty())
                            .filter_map(|line| line.split_once(':'))
                            .map(|(name, value)| {
                                (name.trim().to_ascii_lowercase(), value.trim().to_string())
                            })
                            .collect();
                        let request_body =
                            text.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
                        state
                            .requests
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((request_line.to_string(), headers, request_body));
                        let reply = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(reply.as_bytes()).await;
                    });
                }
            }
        });
        SseMock { port, inner }
    }

    fn last(&self) -> (String, Vec<(String, String)>, String) {
        self.inner
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

/// A Cohere v2 typed stream: thinking + text content deltas, `message-end`
/// terminal with usage.
fn cohere_stream() -> String {
    concat!(
        "data: {\"type\":\"message-start\"}\n\n",
        "data: {\"type\":\"content-delta\",\"index\":0,\"delta\":{\"message\":{\"content\":{\"thinking\":\"hmm\"}}}}\n\n",
        "data: {\"type\":\"content-delta\",\"index\":0,\"delta\":{\"message\":{\"content\":{\"text\":\"hi\"}}}}\n\n",
        "data: {\"type\":\"content-end\",\"index\":0}\n\n",
        "data: {\"type\":\"message-end\",\"delta\":{\"finish_reason\":\"COMPLETE\",\"usage\":{\"tokens\":{\"input_tokens\":5,\"output_tokens\":2}}}}\n\n",
    )
    .to_string()
}

/// A Mistral chat stream: text delta + stop + `[DONE]`.
fn mistral_stream() -> String {
    concat!(
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
        "data: [DONE]\n\n",
    )
    .to_string()
}
