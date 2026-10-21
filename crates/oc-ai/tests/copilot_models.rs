//! AI-P06 — GitHub Copilot dynamic `/models` sync (port of
//! `packages/core/src/github-copilot/models.ts` + the `load()` half of
//! `plugin/provider/github-copilot.ts:172-251`): fetch
//! `{baseURL}/models` with the credential headers, keep only usable
//! entries, derive the model list (aliases kept while their advertised
//! API model exists, stale entries dropped, unseen remote models added)
//! and select each model's endpoint from `supported_endpoints`
//! (`/v1/messages` → messages (+`{baseURL}/v1`), `/responses` →
//! responses, `/chat/completions` → chat).
//!
//! The endpoint values are the `settings.endpoint` overlay the Copilot
//! dispatch lane reads (`protocols/copilot_responses.rs`); the fetch runs
//! against a local stub bound to `127.0.0.1:0` (ADR-0006 — no real
//! network).

use std::collections::BTreeMap;

use oc_ai::protocols::copilot::{derive_copilot_models, load_copilot_models, CopilotEndpoint};
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::route::transport::TransportRequest;
use serde_json::json;

// ---------------------------------------------------------------------------
// Local /models stub (127.0.0.1:0), header-capturing
// ---------------------------------------------------------------------------

struct ModelsStub {
    port: u16,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl ModelsStub {
    fn last_request(&self) -> String {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

async fn models_stub(body: String) -> ModelsStub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    tokio::spawn({
        let requests = std::sync::Arc::clone(&requests);
        async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let requests = std::sync::Arc::clone(&requests);
                let body = body.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buffer = [0u8; 16384];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(text);
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        }
    });
    ModelsStub { port, requests }
}

fn remote_model(id: &str, endpoints: &[&str]) -> serde_json::Value {
    json!({
        "model_picker_enabled": true,
        "id": id,
        "name": format!("Model {id}"),
        "version": "2026-01-01",
        "supported_endpoints": endpoints,
        "capabilities": {
            "family": "test",
            "limits": { "max_output_tokens": 4096, "max_prompt_tokens": 100_000 },
            "supports": { "tool_calls": true, "streaming": true }
        }
    })
}

/// The named regression: the sync fetches `/models` with the credential
/// headers, keeps only usable entries, and `derive()` merges remote facts
/// into the model list with endpoint selection.
#[tokio::test]
async fn models_sync() {
    let payload = {
        let mut data = vec![
            remote_model("gpt-5-codex", &["/responses"]),
            remote_model("claude-sonnet-4-5", &["/v1/messages", "/chat/completions"]),
            remote_model("gpt-4o", &["/chat/completions"]),
            remote_model("gpt-4o-preview", &[]),
        ];
        // unusable: disabled policy
        let mut disabled = remote_model("disabled-model", &["/chat/completions"]);
        disabled["policy"] = json!({ "state": "disabled" });
        data.push(disabled);
        // unusable: no advertised limits
        let mut limitless = remote_model("limitless-model", &["/chat/completions"]);
        limitless["capabilities"]["limits"] = json!({});
        data.push(limitless);
        // undecodable: capabilities missing entirely → dropped
        data.push(json!({ "id": "broken-model", "name": "Broken" }));
        json!({ "data": data }).to_string()
    };
    let stub = models_stub(payload).await;
    let base_url = format!("http://127.0.0.1:{}", stub.port);
    let transport = std::sync::Arc::new(ReqwestTransport::new());

    let remote = load_copilot_models(
        std::sync::Arc::clone(&transport) as oc_ai::route::transport::SharedTransport,
        &base_url,
        vec![
            ("Authorization".to_string(), "Bearer ghu_sync_token".to_string()),
            ("X-GitHub-Api-Version".to_string(), "2026-08-01".to_string()),
        ],
    )
    .await
    .expect("models load");
    let ids: Vec<&str> = remote.keys().map(String::as_str).collect();
    assert_eq!(
        ids,
        vec!["claude-sonnet-4-5", "gpt-4o", "gpt-4o-preview", "gpt-5-codex"],
        "usable models only, id-keyed"
    );

    // the fetch rode the credential headers (header names arrive lowercased)
    let request = stub.last_request();
    let lowered = request.to_ascii_lowercase();
    assert!(request.starts_with("GET /models "), "{request}");
    assert!(lowered.contains("authorization: bearer ghu_sync_token"), "{request}");
    assert!(lowered.contains("x-github-api-version: 2026-08-01"), "{request}");

    // derive: alias kept (rebuilt from remote), stale alias dropped, new remote added
    let existing = vec![
        ("gpt-5-codex-chat".to_string(), "gpt-5-codex".to_string()),
        ("stale-alias".to_string(), "removed-model".to_string()),
    ];
    let derived = derive_copilot_models(&base_url, &remote, &existing);
    let by_id: BTreeMap<&str, _> = derived.iter().map(|entry| (entry.id.as_str(), entry)).collect();
    assert_eq!(by_id.len(), 5, "4 remote + the kept alias");
    assert!(!by_id.contains_key("stale-alias"), "an alias whose API model is gone drops");

    let alias = by_id["gpt-5-codex-chat"];
    assert_eq!(alias.model_id, "gpt-5-codex", "the alias keeps pointing at its API model");
    assert_eq!(alias.endpoint, Some(CopilotEndpoint::Responses));
    assert_eq!(alias.base_url, base_url);

    let claude = by_id["claude-sonnet-4-5"];
    assert_eq!(claude.endpoint, Some(CopilotEndpoint::Messages), "/v1/messages wins");
    assert_eq!(claude.base_url, format!("{base_url}/v1"), "the messages lane pins the /v1 root");

    let gpt4o = by_id["gpt-4o"];
    assert_eq!(gpt4o.endpoint, Some(CopilotEndpoint::Chat));
    assert_eq!(gpt4o.base_url, base_url);

    let preview = by_id["gpt-4o-preview"];
    assert_eq!(preview.endpoint, None, "no advertised endpoints → no endpoint overlay");
    assert_eq!(preview.base_url, base_url);
}

/// A non-OK `/models` response fails the sync with the v2 message shape
/// (models.ts:86) — the caller treats that as "no refresh", not a crash.
#[tokio::test]
async fn non_ok_response_fails_the_sync() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n")
                    .await;
            });
        }
    });
    let transport: oc_ai::route::transport::SharedTransport =
        std::sync::Arc::new(ReqwestTransport::new());
    let error = load_copilot_models(
        transport,
        &format!("http://127.0.0.1:{port}"),
        Vec::<(String, String)>::new(),
    )
    .await
    .expect_err("401 fails the sync");
    assert!(error.contains("Failed to fetch Copilot models: 401"), "{error}");
}

/// The transport seam stays process-agnostic: any `Transport` works, and
/// the request is a plain GET against `{base}/models`.
#[tokio::test]
async fn sync_uses_the_transport_seam() {
    use oc_ai::route::transport::Transport;
    use std::sync::{Arc, Mutex};
    struct RecordingTransport {
        request: Arc<Mutex<Option<TransportRequest>>>,
    }
    #[async_trait::async_trait]
    impl Transport for RecordingTransport {
        async fn execute(
            &self,
            request: TransportRequest,
        ) -> Result<
            oc_ai::route::transport::TransportResponse,
            oc_ai::route::transport::TransportError,
        > {
            *self.request.lock().unwrap() = Some(request);
            Err(oc_ai::route::transport::TransportError::connection(
                "recording stub",
                oc_ai::schema::TransportOperation::Request,
            ))
        }
    }
    let recorded = Arc::new(Mutex::new(None));
    let transport: oc_ai::route::transport::SharedTransport =
        Arc::new(RecordingTransport { request: Arc::clone(&recorded) });
    let error = load_copilot_models(
        transport,
        "http://127.0.0.1:9",
        vec![("Authorization".to_string(), "Bearer t".to_string())],
    )
    .await
    .expect_err("recording stub fails");
    assert!(error.contains("Failed to fetch Copilot models"), "{error}");
    let request = recorded.lock().unwrap().clone().expect("the GET was issued");
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, "http://127.0.0.1:9/models");
    assert_eq!(request.header("Authorization"), Some("Bearer t"));
}
