//! HOTFIX-2 regressions: a failed turn (custom provider rejecting the
//! request, the live newapi failure mode) must leave
//! `GET /api/session/:id/message` answering normally, and the request the
//! engine sent must carry integer LLM params (no `32000.0` on the wire).
//!
//! Repro shape from live verification: prompt → provider 400 → step.failed
//! with the synthetic assistant error part, title/renamed + usage events
//! around it, then GET /message must NOT reset the TCP connection.

mod support;

use std::sync::Arc;
use std::time::Duration;

use oc_client::types::{SessionListQuery, SessionMessageListQuery};
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The provider config: an openai-compatible custom provider pointed at the
/// in-test mock (the user's newapi shape).
fn user_config(provider_port: u16) -> String {
    format!(
        r#"{{
            "model": "mock/mock-1",
            "providers": {{
                "mock": {{
                    "name": "Mock",
                    "npm": "@ai-sdk/openai-compatible",
                    "settings": {{
                        "baseURL": "http://127.0.0.1:{provider_port}/v1",
                        "apiKey": "sk-dummy"
                    }},
                    "models": {{
                        "mock-1": {{ "name": "Mock 1" }}
                    }}
                }}
            }}
        }}"#
    )
}

/// The newapi rejection byte-for-byte as the live daemon received it.
const NEWAPI_REJECT: &str = "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n\
    {\"error\":{\"message\":\"json: cannot unmarshal number 32000.0 into Go struct field GeneralOpenAIRequest.max_completion_tokens of type uint\",\"type\":\"new_api_error\"}}";

/// A minimal successful chat-completion SSE (the title call's answer —
/// title requests carry no output limit, so newapi accepted them live).
const OK_SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
    data: {\"id\":\"cmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"A greeting\"}}]}\n\n\
    data: {\"id\":\"cmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n\
    data: [DONE]\n\n";

/// Serves the live failure mix: requests carrying `max_completion_tokens`
/// (the primary turn, BUG-C limits) get the newapi 400; limit-free requests
/// (title generation) stream a completion. Runs until `shutdown` fires,
/// capturing the first request body.
async fn mock_provider(
    listener: TcpListener,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    captured: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    loop {
        let (mut stream, _) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(_) => return,
            },
            _ = shutdown.changed() => return,
        };
        if let Some(body) = read_one_request(&mut stream).await {
            {
                let mut first = captured.lock().unwrap();
                if first.is_none() {
                    *first = Some(body.clone());
                }
            }
            let limited = body.contains("max_completion_tokens") || body.contains("\"max_tokens\"");
            let response = if limited { NEWAPI_REJECT } else { OK_SSE };
            let _ = stream.write_all(response.as_bytes()).await;
        }
        let _ = stream.shutdown().await;
    }
}

/// Reads one HTTP/1.1 request (headers + content-length body).
async fn read_one_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_subsequence(&buf, b"\r\n\r\n") {
            break position;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
    _workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

async fn boot(workspace: std::path::PathBuf) -> TestServer {
    let data = tempfile::tempdir().expect("data tempdir");
    boot_at(workspace, data).await
}

async fn boot_at(workspace: std::path::PathBuf, data: tempfile::TempDir) -> TestServer {
    std::env::set_current_dir(&workspace).expect("chdir to workspace");
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, addr, state, _workspace: workspace, _data: data }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn failed_turn_then_message_list_answers_normally() {
    let workspace = tempfile::tempdir().expect("workspace");
    let provider_listener = TcpListener::bind("127.0.0.1:0").await.expect("mock provider bind");
    let provider_port = provider_listener.local_addr().unwrap().port();
    std::fs::write(workspace.path().join("opencode.json"), user_config(provider_port))
        .expect("write config");

    let captured = Arc::new(std::sync::Mutex::new(None::<String>));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let provider_captured = Arc::clone(&captured);
    tokio::spawn(async move {
        mock_provider(provider_listener, shutdown_rx, provider_captured).await;
    });

    let server = boot(workspace.path().to_path_buf()).await;
    let created = server
        .client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: workspace.path().to_string_lossy().into_owned(),
            }),
            ..Default::default()
        })
        .await
        .expect("create session");
    let session_id = created.id.clone();

    // The failed turn: prompt admits and returns immediately (CD-1: v2's
    // fire-and-forget wake — the HTTP response never waits on run
    // settlement); the provider request arrives when the woken run
    // reaches the model.
    tokio::time::timeout(
        Duration::from_secs(300),
        server.client.session().prompt(
            &session_id,
            &oc_client::types::SessionPromptBody { text: "hello".to_owned(), ..Default::default() },
        ),
    )
    .await
    .expect("prompt admits")
    .expect("prompt is a session outcome, not an HTTP error");

    // The captured provider request: poll — the run settles
    // asynchronously from the wake.
    let first_request = {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(request) = captured.lock().unwrap().clone() {
                break request;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the engine sent at least one provider request"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    assert!(
        !first_request.contains("32000.0"),
        "integer LLM params must not serialize as floats: {first_request}"
    );

    // THE REGRESSION: GET /message over raw TCP must produce a full HTTP
    // response (live bug: connection reset, curl exit 52). Poll until
    // the failed turn's assistant error part is projected (the run
    // settles asynchronously from the wake).
    let response = {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let response = tokio::time::timeout(
                Duration::from_secs(15),
                support::request(
                    server.addr,
                    "GET",
                    &format!("/api/session/{session_id}/message"),
                    &[],
                    b"",
                ),
            )
            .await
            .expect("GET /message must not hang")
            .expect("GET /message must not reset the connection");
            assert_eq!(response.status, 200, "message list answers 200: {}", response.text());
            let data = response.json()["data"].as_array().cloned().unwrap_or_default();
            let settled = data.iter().rev().any(|message| {
                (message["type"].as_str() == Some("assistant")
                    || message["role"].as_str() == Some("assistant"))
                    && message.get("error").is_some_and(|error| !error.is_null())
            });
            if settled || std::time::Instant::now() > deadline {
                break response;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    let _ = shutdown_tx.send(true);

    assert_eq!(response.status, 200, "message list answers 200: {}", response.text());
    let body = response.json();
    let data = body["data"].as_array().expect("data array");
    let assistant = data.iter().rev().find(|message| {
        message["type"].as_str().is_some_and(|type_| type_ == "assistant")
            || message["role"].as_str().is_some_and(|role| role == "assistant")
    });
    let assistant =
        assistant.unwrap_or_else(|| panic!("an assistant message exists — data: {data:?}"));
    assert!(
        assistant.get("error").is_some_and(|error| !error.is_null()),
        "the failed turn's error part is visible: {assistant}"
    );
}

/// BUG-E: a session whose directory vanished must not 500 (or reset) the
/// session endpoints — v2 answers `LocationNotFoundError` 404 through the
/// session-location middleware, and the session LIST still shows every
/// session. Other sessions are unaffected.
#[tokio::test]
async fn missing_directory_session_404s_like_v2_without_poisoning_the_list() {
    let workspace = tempfile::tempdir().expect("workspace");
    let vanished = tempfile::tempdir().expect("vanished directory");
    let server = boot(workspace.path().to_path_buf()).await;

    let healthy = server
        .client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: workspace.path().to_string_lossy().into_owned(),
            }),
            title: Some("healthy".to_owned()),
            ..Default::default()
        })
        .await
        .expect("create healthy session");

    let stale = server
        .client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: vanished.path().to_string_lossy().into_owned(),
            }),
            title: Some("stale".to_owned()),
            ..Default::default()
        })
        .await
        .expect("create the soon-stale session");

    // The directory vanishes (live: /tmp cleanup), and the daemon's cached
    // location goes cold (live: daemon restart) — the next request for the
    // stale session re-assembles and must find the directory missing.
    drop(vanished);
    let stale_location = oc_core::location::LocationRef::new(
        std::path::PathBuf::from(&stale.location.directory),
        None,
    );
    server.state.engine.invalidate(&stale_location).await;

    // v2 semantics: GET the stale session → 404 LocationNotFoundError with
    // the location envelope (never a 500 UnknownError).
    let error = server.client.session().get(&stale.id).await.expect_err("stale session 404s");
    match &error {
        oc_client::error::ClientError::Api { status, name, fields, .. } => {
            assert_eq!((*status, name.as_str()), (404, "LocationNotFoundError"), "{fields:?}");
            assert_eq!(
                fields["location"]["directory"],
                serde_json::json!(stale.location.directory)
            );
            assert!(
                fields["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("Location not found: ")),
                "v2 message shape: {fields:?}"
            );
        }
        other => panic!("expected LocationNotFoundError, got {other:?}"),
    }

    // GET /message on the stale session: same 404, over raw TCP — a full
    // HTTP response, not a reset (the live curl exit 52).
    let response = tokio::time::timeout(
        Duration::from_secs(15),
        support::request(
            server.addr,
            "GET",
            &format!("/api/session/{}/message", stale.id),
            &[],
            b"",
        ),
    )
    .await
    .expect("GET /message must not hang")
    .expect("GET /message must not reset the connection");
    assert_eq!(response.status, 404, "raw message list: {}", response.text());
    assert_eq!(response.json()["_tag"], "LocationNotFoundError");

    // The list still shows every session (v2 lists from the store, no
    // location assembly), and the healthy session keeps answering.
    let listed =
        server.client.session().list(&SessionListQuery::default()).await.expect("session list");
    let ids: Vec<&str> = listed.data.iter().map(|info| info.id.as_str()).collect();
    assert!(ids.contains(&healthy.id.as_str()), "healthy session listed: {ids:?}");
    assert!(ids.contains(&stale.id.as_str()), "stale session still listed: {ids:?}");

    let fetched = server.client.session().get(&healthy.id).await.expect("healthy get");
    assert_eq!(fetched.id, healthy.id);
    let messages = server
        .client
        .message()
        .list(&healthy.id, &SessionMessageListQuery::default())
        .await
        .expect("healthy session messages answer");
    assert!(messages.data.is_empty(), "no messages, but a well-formed answer");
}
