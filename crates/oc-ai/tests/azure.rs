//! C7b AI-PROVIDERS — Azure `OpenAI` wire shapes (ledger `AI-AZURE`, cluster
//! C7).
//!
//! v2's Azure provider (`packages/ai/src/providers/azure.ts:38-119`) builds
//! `https://{resourceName}.openai.azure.com/openai` (+`/v1` or
//! `/deployments/{model}`), sends `api-version=v1` (queryParams merge over
//! it; custom non-Azure gateways pass through without one), authenticates
//! with the `api-key` header (config key → `AZURE_OPENAI_API_KEY`, any
//! gateway `authorization` removed), and offers the responses WS channel
//! only on the Azure-native endpoint shape.
//!
//! The end-to-end row runs the production route pipeline against a local
//! SSE mock bound to `127.0.0.1:0` (ADR-0006: no real network); the URL and
//! header rows assert on the prepared transport request.

use std::sync::Arc;

use oc_ai::protocols::azure::{
    azure_chat_route, azure_config, azure_responses_route, responses_channel_options,
};
use oc_ai::protocols::open_responses_channel::prepare_channel;
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::schema::{AiErrorReason, GenerationOptions, LLMRequest, Message, SystemPart};

fn transport() -> Arc<ReqwestTransport> {
    Arc::new(ReqwestTransport::new())
}

fn azure_config_with_key(resource: &str) -> oc_ai::protocols::azure::AzureConfig {
    azure_config(Some(resource.to_string()), None, Some(Secret::Value("az-key-1".into())))
}

fn request_on(route: &oc_ai::route::Route) -> LLMRequest {
    let model = route.model("gpt-4.1", None).expect("model");
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

fn wire_request(
    route: &oc_ai::route::Route,
) -> (oc_ai::route::transport::TransportRequest, String) {
    let request = request_on(route);
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    route.prepare_transport(&prepared, &body).expect("wire request")
}

/// Ledger repro: `resource_url_headers` — the Azure resource URL shape,
/// `api-version` query, and `api-key` header on the wire. The URL row
/// prepares against the real Azure resource shape (nothing dialed); the
/// end-to-end row runs the production pipeline against a local SSE mock —
/// whose non-Azure host makes it a *gateway* deployment (v2: passthrough
/// base + caller query, no `api-version`) while keeping the header policy.
#[tokio::test]
async fn resource_url_headers() {
    // URL + header shape on the Azure-native endpoint (prepare only)
    let config = azure_config_with_key("res");
    let route = azure_responses_route(&config, transport(), "gpt-4.1").expect("route");
    let (transport_request, _body) = wire_request(&route);
    assert_eq!(
        transport_request.url, "https://res.openai.azure.com/openai/v1/responses?api-version=v1",
        "resource URL shape"
    );
    assert!(
        transport_request.header("api-key").is_some_and(|value| value == "az-key-1"),
        "api-key header"
    );
    assert!(transport_request.header("authorization").is_none(), "gateway authorization removed");
    assert!(transport_request
        .header("content-type")
        .is_some_and(|v| v.starts_with("application/json")));

    // end-to-end through the production pipeline against the local mock
    // (non-Azure base → gateway passthrough: no api-version, api-key kept)
    let mock = sse_mock().await;
    let config = oc_ai::protocols::azure::AzureConfig {
        base_url: Some(format!("http://127.0.0.1:{}/openai", mock.port)),
        ..azure_config_with_key("res")
    };
    let route = azure_responses_route(&config, transport(), "gpt-4.1").expect("route");
    let stream = route.stream(request_on(&route)).await.expect("stream");
    let events = collect(stream).await;
    assert!(
        events.iter().all(std::result::Result::is_ok),
        "stream errors: {:?}",
        events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|item| item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal)),
        "terminal event received"
    );
    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("/openai/responses"), "gateway wire: {}", requests[0]);
    assert!(!requests[0].contains("api-version"), "gateways send no api-version");
    let headers = mock.headers();
    assert!(headers[0].iter().any(|(name, value)| name == "api-key" && value == "az-key-1"));
    assert!(!headers[0].iter().any(|(name, _)| name == "authorization"));
}

#[test]
fn deployment_urls_shape() {
    let config = oc_ai::protocols::azure::AzureConfig {
        use_deployment_based_urls: true,
        ..azure_config_with_key("res")
    };
    let route = azure_responses_route(&config, transport(), "gpt-4.1").expect("route");
    let (transport_request, _) = wire_request(&route);
    assert!(
        transport_request.url.ends_with("/deployments/gpt-4.1/responses?api-version=v1"),
        "deployment URL: {}",
        transport_request.url
    );

    let chat = azure_chat_route(&config, transport(), "gpt-4.1").expect("chat route");
    let (transport_request, _) = wire_request(&chat);
    assert!(
        transport_request.url.ends_with("/deployments/gpt-4.1/chat/completions?api-version=v1"),
        "chat deployment URL: {}",
        transport_request.url
    );
    assert!(transport_request.header("api-key").is_some_and(|value| value == "az-key-1"));
}

#[test]
fn custom_gateway_passthrough_drops_api_version() {
    let config = oc_ai::protocols::azure::AzureConfig {
        query_params: [("x".to_string(), "1".to_string())].into_iter().collect(),
        ..azure_config(
            None,
            Some("https://proxy.internal/api/".into()),
            Some(Secret::Value("k".into())),
        )
    };
    let route = azure_responses_route(&config, transport(), "gpt-4.1").expect("route");
    let (transport_request, _) = wire_request(&route);
    assert!(
        transport_request.url.starts_with("https://proxy.internal/api/responses?"),
        "gateway URL: {}",
        transport_request.url
    );
    assert!(!transport_request.url.contains("api-version"), "no api-version for gateways");
    assert!(transport_request.url.contains("x=1"), "caller query kept: {}", transport_request.url);
}

#[test]
fn resource_name_required_error() {
    let error = azure_responses_route(
        &oc_ai::protocols::azure::AzureConfig::default(),
        transport(),
        "gpt-4.1",
    )
    .unwrap_err();
    match error.reason {
        AiErrorReason::ProviderConfiguration { message, .. } => {
            assert_eq!(message, "Azure requires resourceName or baseURL");
        }
        reason => panic!("expected ProviderConfiguration, got {reason:?}"),
    }
}

/// The chat route: `/v1/chat/completions?api-version=v1` + `api-key` on the
/// Azure-native endpoint; the gateway deployment keeps the header policy.
#[tokio::test]
async fn chat_route_url_headers() {
    // URL shape on the Azure-native endpoint (prepare only)
    let config = azure_config_with_key("res");
    let chat = azure_chat_route(&config, transport(), "gpt-4.1").expect("route");
    let (transport_request, _) = wire_request(&chat);
    assert_eq!(
        transport_request.url,
        "https://res.openai.azure.com/openai/v1/chat/completions?api-version=v1",
        "chat URL shape"
    );

    // end-to-end against the local mock (gateway base keeps api-key auth)
    let mock = sse_chat_mock().await;
    let config = oc_ai::protocols::azure::AzureConfig {
        base_url: Some(format!("http://127.0.0.1:{}/openai", mock.port)),
        ..azure_config_with_key("res")
    };
    let route = azure_chat_route(&config, transport(), "gpt-4.1").expect("route");
    let stream = route.stream(request_on(&route)).await.expect("stream");
    let events = collect(stream).await;
    assert!(
        events.iter().any(|item| item.as_ref().is_ok_and(oc_ai::LLMEvent::is_terminal)),
        "chat terminal event"
    );
    let requests = mock.requests();
    assert!(requests[0].starts_with("/openai/chat/completions"), "chat wire: {}", requests[0]);
    let headers = mock.headers();
    assert!(headers[0].iter().any(|(name, value)| name == "api-key" && value == "az-key-1"));
}

/// The WS channel on the Azure-native endpoint: offered with `api-version`
/// stripped; declined for gateways and deployments.
#[tokio::test]
async fn azure_channel_prepare_strips_api_version() {
    // the real Azure resource shape (prepare only — nothing is dialed)
    let config = azure_config_with_key("res");
    let route = azure_responses_route(&config, transport(), "gpt-4.1").expect("route");
    let options = responses_channel_options();
    let channel = prepare_channel(&route, &request_on(&route), &options)
        .expect("prepare")
        .expect("azure-native endpoint offers the channel");
    assert_eq!(
        channel.ws_url, "wss://res.openai.azure.com/openai/v1/responses",
        "https→wss with api-version stripped"
    );

    // deployment-based endpoints do not offer the channel
    let deployments = oc_ai::protocols::azure::AzureConfig {
        use_deployment_based_urls: true,
        ..azure_config_with_key("res")
    };
    let route = azure_responses_route(&deployments, transport(), "gpt-4.1").expect("route");
    let channel = prepare_channel(&route, &request_on(&route), &options).expect("prepare");
    assert!(channel.is_none(), "deployment endpoints stay on HTTP");
}

#[test]
fn azure_channel_declined_for_gateways() {
    // gate mirror of the module unit test, through the public gate fn
    assert!(!oc_ai::protocols::azure::responses_channel_enabled(
        "https://proxy.internal/openai/v1/responses?api-version=v1"
    ));
    assert!(!oc_ai::protocols::azure::responses_channel_enabled(
        "https://res.openai.azure.com/openai/deployments/m/responses?api-version=v1"
    ));
}

/// `AZURE_OPENAI_API_KEY` fallback (apply-time env read).
#[test]
fn api_key_env_fallback() {
    let guard = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    std::env::set_var("AZURE_OPENAI_API_KEY", "env-az-key");
    let config = azure_config(Some("res".into()), None, None);
    let route = azure_responses_route(&config, transport(), "gpt-4.1").expect("route");
    let (transport_request, _) = wire_request(&route);
    assert!(
        transport_request.header("api-key").is_some_and(|value| value == "env-az-key"),
        "api-key falls back to AZURE_OPENAI_API_KEY"
    );
    std::env::remove_var("AZURE_OPENAI_API_KEY");
    drop(guard);
}

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------------------------------------------------------------------------
// Local SSE mock (127.0.0.1:0)
// ---------------------------------------------------------------------------

struct SseMock {
    port: u16,
    inner: Arc<MockState>,
}

struct MockState {
    requests: std::sync::Mutex<Vec<String>>,
    headers: std::sync::Mutex<Vec<Vec<(String, String)>>>,
}

impl SseMock {
    fn requests(&self) -> Vec<String> {
        self.inner.requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn headers(&self) -> Vec<Vec<(String, String)>> {
        self.inner.headers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

async fn sse_mock() -> SseMock {
    mock(vec![concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"output\":[]}}\n\n",
    )
    .to_string()])
    .await
}

async fn sse_chat_mock() -> SseMock {
    mock(vec![
        concat!(
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        )
        .to_string(),
    ])
    .await
}

async fn mock(bodies: Vec<String>) -> SseMock {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    let inner = Arc::new(MockState {
        requests: std::sync::Mutex::new(Vec::new()),
        headers: std::sync::Mutex::new(Vec::new()),
    });
    tokio::spawn({
        let state = Arc::clone(&inner);
        async move {
            let mut bodies = bodies.into_iter();
            while let Ok((mut stream, _)) = listener.accept().await {
                let state = Arc::clone(&state);
                let body = bodies.next();
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
                    let headers: Vec<(String, String)> = text
                        .lines()
                        .skip(1)
                        .take_while(|line| !line.is_empty())
                        .filter_map(|line| line.split_once(':'))
                        .map(|(name, value)| {
                            (name.trim().to_ascii_lowercase(), value.trim().to_string())
                        })
                        .collect();
                    state
                        .requests
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(request_line);
                    state
                        .headers
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(headers);
                    let body = body.unwrap_or_default();
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
