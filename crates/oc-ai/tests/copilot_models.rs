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
use std::sync::Arc;

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

/// G-167 — the derived fields mirror the ref math (models.ts:129-200):
/// package lane, family, modalities, variants, released, cost (AIC →
/// USD per million), and limits.
#[tokio::test]
async fn derived_fields_mirror_ref_math() {
    use oc_ai::protocols::copilot::{COPILOT_PACKAGE_AISDK, COPILOT_PACKAGE_ANTHROPIC};

    let claude = {
        let mut model = remote_model("claude-sonnet-4-5", &["/v1/messages", "/chat/completions"]);
        model["version"] = json!("claude-sonnet-4-5-2026.01.15");
        model["capabilities"]["family"] = json!("claude");
        model["capabilities"]["supports"]["vision"] = json!(true);
        model["capabilities"]["limits"]["vision"] = json!({
            "max_prompt_image_size": 2048,
            "max_prompt_images": 10,
            "supported_media_types": ["image/png", "application/pdf"],
        });
        model["capabilities"]["limits"]["max_context_window_tokens"] = json!(200_000);
        model["capabilities"]["supports"]["reasoning_effort"] = json!(["low", "high"]);
        model["capabilities"]["supports"]["adaptive_thinking"] = json!(true);
        // AIC per billing batch: 10_000 / 5_000 = 2 USD per AIC unit.
        model["billing"] = json!({
            "token_prices": {
                "batch_size": 5_000,
                "default": {
                    "input_price": 1.25,
                    "output_price": 10.0,
                    "cache_read_price": 0.125,
                },
            },
        });
        model
    };
    let legacy_cache = {
        let mut model = remote_model("legacy-cache", &["/chat/completions"]);
        model["capabilities"]["family"] = json!("legacy");
        model["billing"] = json!({
            "token_prices": {
                "batch_size": 10_000,
                "default": { "input_price": 3.0, "output_price": 6.0, "cache_price": 0.3 },
            },
        });
        model
    };
    let payload = json!({ "data": [claude, legacy_cache] }).to_string();
    let stub = models_stub(payload).await;
    let base_url = format!("http://127.0.0.1:{}", stub.port);
    let transport: oc_ai::route::transport::SharedTransport =
        Arc::new(ReqwestTransport::new());
    let remote = load_copilot_models(Arc::clone(&transport), &base_url, Vec::new())
        .await
        .expect("models load");

    let derived = derive_copilot_models(&base_url, &remote, &[]);
    let by_id: BTreeMap<&str, _> = derived.iter().map(|entry| (entry.id.as_str(), entry)).collect();

    let claude = by_id["claude-sonnet-4-5"];
    // The anthropic escape lane: `/v1/messages` models rewrite the
    // package (models.ts:164) and pin the `{base}/v1` root.
    assert_eq!(claude.package, COPILOT_PACKAGE_ANTHROPIC);
    assert!(claude.is_anthropic());
    assert_eq!(claude.base_url, format!("{base_url}/v1"));
    assert_eq!(claude.endpoint, Some(CopilotEndpoint::Messages));
    assert_eq!(claude.family.as_deref(), Some("claude"));
    // vision + image/* media → image; the vision media list carries pdf.
    assert_eq!(claude.input, vec!["text", "image", "pdf"]);
    // messages + adaptive thinking + efforts → the adaptive variants
    // (models.ts:219-232), NOT the effort variants.
    let variant_ids: Vec<&str> = claude.variants.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(variant_ids, vec!["low", "high"]);
    assert_eq!(claude.variants[0].settings["thinking"]["type"], "adaptive");
    // 10_000 / 5_000 = 2 → the AIC prices double into USD per million.
    assert!((claude.cost.input - 2.5).abs() < 1e-9, "{}", claude.cost.input);
    assert!((claude.cost.output - 20.0).abs() < 1e-9, "{}", claude.cost.output);
    assert!((claude.cost.cache_read - 0.25).abs() < 1e-9, "{}", claude.cost.cache_read);
    // `Date.parse("2026.01.15")` — not ISO → 0 (Number.isFinite → 0).
    assert_eq!(claude.released, 0.0);
    assert_eq!(claude.limit.context, 200_000);
    assert_eq!(claude.limit.input, 100_000);
    assert_eq!(claude.limit.output, 4_096);

    let legacy = by_id["legacy-cache"];
    assert_eq!(legacy.package, COPILOT_PACKAGE_AISDK);
    assert!(!legacy.is_anthropic());
    assert_eq!(legacy.base_url, base_url);
    assert_eq!(legacy.input, vec!["text"]);
    // batch_size 10_000 → ×1.0; the legacy `cache_price` spelling wins
    // when `cache_read_price` is absent (models.ts:188).
    assert!((legacy.cost.input - 3.0).abs() < 1e-9);
    assert!((legacy.cost.cache_read - 0.3).abs() < 1e-9);
    assert!(legacy.variants.is_empty(), "no reasoning surface → no variants");
}

/// The released derivation (models.ts:154-155, 175-176): the `{id}-`
/// version prefix strips, ISO dates parse to epoch ms, and a kept local
/// release time wins over the remote version.
#[test]
fn released_derivation() {
    use oc_ai::protocols::copilot::{copilot_model_entry, CopilotPrevious};

    let mut remote: serde_json::Value = remote_model("gpt-9", &["/responses"]);
    remote["version"] = json!("gpt-9-2026-03-05");
    remote["capabilities"]["family"] = json!("gpt");
    let remote: oc_ai::protocols::copilot::CopilotRemoteModel =
        serde_json::from_value(remote).expect("decode");
    let entry = copilot_model_entry("gpt-9", &remote, "https://api.githubcopilot.com", CopilotPrevious::default());
    // 2026-03-05 UTC in epoch ms.
    assert_eq!(entry.released, 1_772_668_800_000.0, "{}", entry.released);

    // A kept local release time wins.
    let kept = copilot_model_entry(
        "alias",
        &remote,
        "https://api.githubcopilot.com",
        CopilotPrevious { released: Some(1.0), ..Default::default() },
    );
    assert_eq!(kept.released, 1.0);
}

/// The non-messages variant lanes (models.ts:206-217, 224-236): efforts
/// become reasoning variants off the messages lane, and a thinking
/// budget becomes the max/high pair.
#[test]
fn variant_lanes() {
    use oc_ai::protocols::copilot::{copilot_model_entry, CopilotPrevious};

    let build = |endpoints: &[&str], extras: serde_json::Value| {
        let mut remote: serde_json::Value = remote_model("m", endpoints);
        remote["capabilities"]["family"] = json!("f");
        serde_json::Value::Object(
            remote.as_object().unwrap().clone().into_iter().chain(extras.as_object().unwrap().clone()).collect(),
        )
    };
    let decode = |value: serde_json::Value| -> oc_ai::protocols::copilot::CopilotRemoteModel {
        serde_json::from_value(value).expect("decode")
    };

    // Non-messages + efforts → the reasoningEffort variants.
    let mut merged = build(&["/responses"], json!({}));
    merged["capabilities"]["supports"]["reasoning_effort"] = json!(["low", "medium"]);
    let entry = copilot_model_entry("m", &decode(merged), "https://x", CopilotPrevious::default());
    assert_eq!(entry.variants.len(), 2);
    assert_eq!(entry.variants[0].settings["reasoningEffort"], "low");
    assert_eq!(entry.variants[0].settings["include"][0], "reasoning.encrypted_content");

    // No efforts + max_thinking_budget → max/high pair.
    let mut budget = build(&["/v1/messages"], json!({}));
    budget["capabilities"]["supports"]["max_thinking_budget"] = json!(10_000);
    let entry = copilot_model_entry("m", &decode(budget), "https://x", CopilotPrevious::default());
    assert_eq!(entry.variants[0].id, "max");
    assert_eq!(entry.variants[0].settings["thinking"]["budgetTokens"], serde_json::json!(9_999.0));
    assert_eq!(entry.variants[1].id, "high");
    assert_eq!(entry.variants[1].settings["thinking"]["budgetTokens"], serde_json::json!(5_000.0));
}

/// The free utility-title channel (github-copilot.ts:398-475): its own
/// non-streaming `/chat/completions` call against gpt-4o-mini with the
/// utility interaction headers.
#[tokio::test]
async fn utility_title_free_channel() {
    use oc_ai::protocols::copilot::{
        copilot_utility_title, utility_title_from_response, UtilityTitleInput, UtilityTitleMessage,
        UTILITY_TITLE_MODELS,
    };

    assert_eq!(UTILITY_TITLE_MODELS, ["gpt-4o-mini"], "gpt-4o-mini is the small utility model");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    let captured = Arc::new(std::sync::Mutex::new(String::new()));
    tokio::spawn({
        let captured = Arc::clone(&captured);
        async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let captured = Arc::clone(&captured);
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buffer = vec![0u8; 65536];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    *captured.lock().unwrap() = text;
                    let body = r#"{"choices":[{"message":{"content":"  A fine title  "}}]}"#;
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

    let messages = vec![
        UtilityTitleMessage { role: "user".into(), text: "help me port a compiler".into() },
    ];
    let input = UtilityTitleInput {
        base_url: &format!("http://127.0.0.1:{port}"),
        token: "ghu_utility_token",
        model: "gpt-4o-mini",
        user_agent: "opencode-test",
        session_id: "ses_utility",
        system: Some("name the session"),
        messages: &messages,
        max_tokens: Some(64),
    };
    let transport: oc_ai::route::transport::SharedTransport = Arc::new(ReqwestTransport::new());
    let title = copilot_utility_title(transport, &input).await.expect("utility title");
    assert_eq!(title, "A fine title", "the content is trimmed");

    let request = captured.lock().unwrap().clone();
    assert!(request.starts_with("POST /chat/completions "), "{request}");
    let lowered = request.to_ascii_lowercase();
    assert!(lowered.contains("authorization: bearer ghu_utility_token"), "{request}");
    assert!(
        lowered.contains("x-interaction-type: agent-session-name-generation"),
        "{request}"
    );
    assert!(lowered.contains("x-interaction-id: ses_utility"), "{request}");
    assert!(lowered.contains("x-initiator: agent"), "{request}");
    assert!(lowered.contains("x-github-api-version: 2026-08-01"), "{request}");
    let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(body).expect("body is json");
    assert_eq!(parsed["model"], "gpt-4o-mini");
    assert_eq!(parsed["stream"], false);
    assert_eq!(parsed["max_tokens"], 64);
    assert_eq!(parsed["messages"][0]["role"], "system");
    assert_eq!(parsed["messages"][1]["role"], "user");

    // An empty content is a failure so the normal title path still runs.
    assert!(utility_title_from_response(r#"{"choices":[{"message":{"content":"  "}}]}"#)
        .is_err());
    assert!(utility_title_from_response(r#"{"choices":[]}"#).is_err());
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
