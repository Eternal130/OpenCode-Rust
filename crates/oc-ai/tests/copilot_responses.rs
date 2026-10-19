//! C7b AI-PROVIDERS slice 2 — GitHub Copilot responses lane + reasoning
//! continuation (ledger `AI-COPILOT-C1` + `AI-COPILOT-C2`, cluster C7).
//!
//! **AI-COPILOT-C1** — v2 selects the Copilot endpoint per model in the
//! `aisdk.language` hook (`packages/core/src/plugin/provider/github-copilot.ts:317-344`):
//! the advertised `endpoint` option (the `/models` sync's
//! `settings.endpoint`) wins; otherwise GPT ≥ 5 (except `gpt-5-mini*`),
//! `grok-*`, `gemini-*`, and `mai-code-*` models are served on
//! `/responses` while everything else stays on `/chat/completions`.
//!
//! **AI-COPILOT-C2** — multi-turn reasoning continuation: the chat lane
//! captures `delta.reasoning_opaque` into provider metadata
//! (`{copilot: {reasoningOpaque}}`) and replays it on the next request's
//! assistant message as `reasoning_opaque` (+ `reasoning_text` only when
//! the handle exists) —
//! `packages/core/src/github-copilot/chat/openai-compatible-chat-language-model.ts:223-253`
//! + `convert-to-openai-compatible-chat-messages.ts:69-124`.
//!
//! All runtime rows run the production route pipeline against a local SSE
//! mock bound to `127.0.0.1:0` (ADR-0006: cassette-only, no real network).

use std::sync::Arc;

use oc_ai::protocols::copilot::{CopilotConfig, COPILOT_BASE_URL};
use oc_ai::protocols::copilot_responses::{
    copilot_lane, copilot_lane_for_request, model_id_uses_responses, CopilotLane,
};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::schema::{
    ContentPart, GenerationOptions, LLMRequest, Message, MessageRole, ProviderMetadata, SystemPart,
};
use serde_json::{json, Value};

fn transport() -> Arc<ReqwestTransport> {
    Arc::new(ReqwestTransport::new())
}

fn copilot_route_at(mock_port: u16) -> oc_ai::route::Route {
    oc_ai::protocols::copilot::copilot_chat_route(
        transport(),
        CopilotConfig {
            base_url: Some(format!("http://127.0.0.1:{mock_port}")),
            token: Some(Secret::Value("gh-copilot-token".into())),
        },
    )
}

fn request_on(route: &oc_ai::route::Route, model_id: &str) -> LLMRequest {
    let model = route.model(model_id, None).expect("model");
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

/// The lane table for the 21 responses-only / base-chat model families
/// (ledger symptom: "gpt-5+/grok-/gemini-/mai-code- 21/34 models hit
/// /chat/completions, 404").
#[test]
fn model_family_endpoint() {
    // responses-only families (v2 regexes, github-copilot.ts:336-341)
    for id in [
        "gpt-5",
        "gpt-5-codex",
        "gpt-5.1",
        "gpt-5.1-codex",
        "gpt-6-mini",
        "grok-4",
        "grok-4-fast",
        "gemini-2.5-pro",
        "gemini-3-flash",
        "mai-code-1",
    ] {
        assert_eq!(copilot_lane(id, None), CopilotLane::Responses, "{id}");
        assert!(model_id_uses_responses(id), "{id}");
    }
    // base chat endpoint: GPT < 5, the gpt-5-mini chat-only exception,
    // o-series, Claude-on-Copilot
    for id in [
        "gpt-4o",
        "gpt-4.1-mini",
        "gpt-5-mini",
        "gpt-5-mini-codex",
        "o3",
        "o4-mini",
        "claude-sonnet-4.5",
        "text-embedding-3",
    ] {
        assert_eq!(copilot_lane(id, None), CopilotLane::Chat, "{id}");
        assert!(!model_id_uses_responses(id), "{id}");
    }
    // advertised endpoint metadata (the /models sync) wins over the regex
    assert_eq!(copilot_lane("gpt-4o", Some("responses")), CopilotLane::Responses);
    assert_eq!(copilot_lane("gpt-5-codex", Some("chat")), CopilotLane::Chat);
    // `messages` (Claude lane) and unknown values fall through to the regex
    assert_eq!(copilot_lane("gpt-5-codex", Some("messages")), CopilotLane::Responses);
    assert_eq!(copilot_lane("claude-sonnet-4.5", Some("messages")), CopilotLane::Chat);
    assert_eq!(copilot_lane("gpt-5-codex", Some("")), CopilotLane::Responses);
}

/// The prepared wire shape per lane: URL path + body form + the shared
/// Copilot header policy (nothing dialed — `prepare_transport` only).
#[test]
fn lane_wire_shapes_prepared() {
    let route = oc_ai::protocols::copilot::copilot_chat_route(
        transport(),
        CopilotConfig { base_url: None, token: Some(Secret::Value("t-1".into())) },
    );
    // responses lane: /responses with the OpenAI Responses body (input array)
    let request = request_on(&route, "gpt-5-codex");
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(wire.url, format!("{COPILOT_BASE_URL}/responses"));
    let body: Value = serde_json::from_slice(&wire.body).expect("json");
    assert!(body.get("input").is_some(), "responses body carries input: {body}");
    assert!(body.get("messages").is_none(), "responses body has no messages");
    assert_eq!(body["model"], "gpt-5-codex");

    // chat lane: /chat/completions with the chat body (messages + stream)
    let request = request_on(&route, "gpt-4o");
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(wire.url, format!("{COPILOT_BASE_URL}/chat/completions"));
    let body: Value = serde_json::from_slice(&wire.body).expect("json");
    assert!(body.get("messages").is_some(), "chat body carries messages");
    assert_eq!(body["stream"], json!(true));

    // shared header policy on both lanes
    for model in ["gpt-5-codex", "gpt-4o"] {
        let request = request_on(&route, model);
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(wire.header("authorization"), Some("Bearer t-1"), "{model}");
        assert_eq!(wire.header("x-github-api-version"), Some("2026-08-01"), "{model}");
        assert_eq!(wire.header("openai-intent"), Some("conversation-edits"), "{model}");
    }
}

/// End-to-end through the production pipeline against the local mock: a
/// responses-only family model must land on `/responses` and decode a
/// Responses-shaped stream; a base model must land on `/chat/completions`
/// and decode a chat stream (the C1 symptom row: both used to dial
/// `/chat/completions`).
#[tokio::test]
async fn responses_family_models_dial_the_responses_endpoint() {
    let mock = sse_mock(responses_stream()).await;
    let route = copilot_route_at(mock.port);
    for model in ["gpt-5-codex", "grok-4-fast", "gemini-2.5-pro", "mai-code-1"] {
        let stream = route.stream(request_on(&route, model)).await.expect("stream");
        let events = collect(stream).await;
        assert!(
            events.iter().all(std::result::Result::is_ok),
            "{model}: {:?}",
            events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
        );
        assert!(
            events.iter().any(|item| item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal)),
            "{model}: terminal event"
        );
        assert!(mock.last_request().starts_with("/responses"), "{model}: {}", mock.last_request());
    }
}

#[tokio::test]
async fn base_models_stay_on_chat_completions() {
    let mock = sse_mock(chat_stream()).await;
    let route = copilot_route_at(mock.port);
    for model in ["gpt-4o", "gpt-5-mini", "claude-sonnet-4.5"] {
        let stream = route.stream(request_on(&route, model)).await.expect("stream");
        let events = collect(stream).await;
        assert!(
            events.iter().any(|item| item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal)),
            "{model}: terminal event"
        );
        assert!(
            mock.last_request().starts_with("/chat/completions"),
            "{model}: {}",
            mock.last_request()
        );
    }
}

/// The advertised endpoint option (model settings overlay landing in the
/// request's provider options) overrides the family regex per request.
#[test]
fn endpoint_option_overrides_the_family_regex() {
    let route = oc_ai::protocols::copilot::copilot_chat_route(
        transport(),
        CopilotConfig { base_url: None, token: Some(Secret::Value("t-1".into())) },
    );
    let mut request = request_on(&route, "gpt-4o");
    request.provider_options =
        Some([("endpoint".to_string(), json!("responses"))].into_iter().collect());
    assert_eq!(copilot_lane_for_request(&request), CopilotLane::Responses);
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert!(wire.url.ends_with("/responses"), "option wins: {}", wire.url);
}

/// The lowered wire body for a conversation ending in the given messages
/// (prepare + build only; nothing is dialed).
fn wire_body(route: &oc_ai::route::Route, messages: Vec<Message>) -> Value {
    let model = route.model("gpt-4o", None).expect("model");
    let mut request = LLMRequest::new(model);
    request.messages = messages;
    let prepared = route.prepare_request(&request);
    route.build_body(&prepared).expect("body")
}

/// The lowered assistant message of a built body.
fn wire_assistant(body: &Value) -> Value {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("assistant message")
        .clone()
}

/// AI-COPILOT-C2 — turn 1: a chat stream carrying `reasoning_opaque` (+
/// `reasoning_text`) produces events whose provider metadata carries the
/// captured handle; turn 2: the next request's body threads the handle
/// back onto the assistant message (`reasoning_opaque` + `reasoning_text`;
/// without a handle neither field is replayed).
#[tokio::test]
async fn reasoning_opaque_roundtrip() {
    let mock = sse_mock(opaque_chat_stream()).await;
    let route = copilot_route_at(mock.port);

    // ---- turn 1: capture ----
    let stream = route.stream(request_on(&route, "gpt-4o")).await.expect("stream");
    let events = collect(stream).await;
    assert!(
        events.iter().all(std::result::Result::is_ok),
        "{:?}",
        events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
    );
    let carried_handles: Vec<&Value> = events
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .filter_map(event_metadata)
        .filter_map(|metadata| metadata.get("copilot"))
        .filter_map(|copilot| copilot.get("reasoningOpaque"))
        .collect();
    assert!(
        carried_handles.iter().any(|value| value.as_str() == Some("opaque-handle-1")),
        "captured handle rides the event metadata: {carried_handles:?}"
    );

    // ---- turn 2: replay ----
    let captured: ProviderMetadata = [(
        "copilot".to_string(),
        [("reasoningOpaque".to_string(), json!("opaque-handle-1"))].into_iter().collect(),
    )]
    .into_iter()
    .collect();
    let assistant = Message {
        role: MessageRole::Assistant,
        content: vec![
            ContentPart::Reasoning {
                text: "thinking it through".to_string(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: Some(captured),
            },
            ContentPart::text("Hello!"),
            ContentPart::ToolCall {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                namespace: None,
                input: json!({"q": "hi"}),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            },
        ],
        ..Message::user("")
    };
    let tool_result = Message {
        role: MessageRole::Tool,
        content: vec![ContentPart::ToolResult {
            id: "call_1".to_string(),
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
    let body = wire_body(&route, vec![Message::user("Say hello."), assistant, tool_result]);
    let assistant_wire = wire_assistant(&body);
    assert_eq!(assistant_wire["reasoning_opaque"], "opaque-handle-1");
    assert_eq!(assistant_wire["reasoning_text"], "thinking it through");

    // negative: prior reasoning WITHOUT a captured handle replays neither
    // field (ref: `reasoning_text: reasoningOpaque ? reasoningText : undefined`)
    let plain_assistant = Message {
        role: MessageRole::Assistant,
        content: vec![ContentPart::Reasoning {
            text: "orphan thinking".to_string(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }],
        ..Message::user("")
    };
    let body =
        wire_body(&route, vec![Message::user("hi"), plain_assistant, Message::user("again")]);
    let wire_assistant = wire_assistant(&body);
    assert!(wire_assistant.get("reasoning_opaque").is_none(), "{wire_assistant}");
    assert!(wire_assistant.get("reasoning_text").is_none(), "{wire_assistant}");
    assert!(wire_assistant.get("reasoning_content").is_none(), "{wire_assistant}");
}

/// A second `reasoning_opaque` delta in one response is a provider defect
/// (ref throws `InvalidResponseDataError`).
#[tokio::test]
async fn duplicate_opaque_handle_fails_the_stream() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_text\":\"m\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_opaque\":\"h1\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_opaque\":\"h2\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    )
    .to_string();
    let mock = sse_mock(body).await;
    let route = copilot_route_at(mock.port);
    let stream = route.stream(request_on(&route, "gpt-4o")).await.expect("stream");
    let events = collect(stream).await;
    let error = events
        .into_iter()
        .find_map(std::result::Result::err)
        .expect("duplicate opaque handle fails");
    assert!(error.message().contains("Multiple reasoning_opaque values"), "got: {error}");
}

// ---------------------------------------------------------------------------
// Local SSE mock (127.0.0.1:0), body-capturing
// ---------------------------------------------------------------------------

/// The provider metadata carried by an event, when it has any.
fn event_metadata(event: &oc_ai::schema::LLMEvent) -> Option<&ProviderMetadata> {
    use oc_ai::schema::LLMEvent;
    match event {
        LLMEvent::TextStart { provider_metadata, .. }
        | LLMEvent::TextDelta { provider_metadata, .. }
        | LLMEvent::TextEnd { provider_metadata, .. }
        | LLMEvent::ReasoningStart { provider_metadata, .. }
        | LLMEvent::ReasoningDelta { provider_metadata, .. }
        | LLMEvent::ReasoningEnd { provider_metadata, .. }
        | LLMEvent::ToolInputStart { provider_metadata, .. }
        | LLMEvent::ToolInputEnd { provider_metadata, .. }
        | LLMEvent::ToolCall { provider_metadata, .. }
        | LLMEvent::StepFinish { provider_metadata, .. }
        | LLMEvent::Finish { provider_metadata, .. } => provider_metadata.as_ref(),
        _ => None,
    }
}

struct SseMock {
    port: u16,
    inner: Arc<MockState>,
}

struct MockState {
    requests: std::sync::Mutex<Vec<String>>,
}

impl SseMock {
    fn last_request(&self) -> String {
        self.inner
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

async fn sse_mock(body: String) -> SseMock {
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
                    let request_line = text
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .split(' ')
                        .nth(1)
                        .unwrap_or_default()
                        .to_string();
                    state
                        .requests
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(request_line);
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

fn responses_stream() -> String {
    concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"id\":\"msg_1\",\"content\":[{\"type\":\"output_text\",\"text\":\"hi\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n",
    )
    .to_string()
}

fn chat_stream() -> String {
    concat!(
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    )
    .to_string()
}

/// The Copilot chat reasoning shape: `reasoning_text` deltas with a single
/// `reasoning_opaque` handle, then content, then stop.
fn opaque_chat_stream() -> String {
    concat!(
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_text\":\"thinking it through\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_opaque\":\"opaque-handle-1\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello!\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    )
    .to_string()
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
