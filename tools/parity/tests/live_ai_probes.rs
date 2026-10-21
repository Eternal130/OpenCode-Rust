//! CI-RUNTIME-AI-LIVE — live canary probes for the 3 AI needs-runtime rows
//! (user ruling 2026-10-21: live canary, opt-in trigger; ADR-0006 addendum).
//!
//! Three probes, one per needs-runtime path, each driving the REAL
//! production call surface in `oc-ai` (route composition → protocol body →
//! `prepare_transport` → `ReqwestTransport`) — no parallel HTTP stack:
//!
//! - [`live_ai_vertex_messages_never_api_anthropic`] (AI-VERTEX-V2):
//!   `google-vertex/messages` against the real Vertex publisher endpoint —
//!   the request URL must be
//!   `{host}/v1/projects/{p}/locations/{l}/publishers/anthropic/models/{model}:streamRawPredict`
//!   (NEVER api.anthropic.com), ADC bearer auth (never x-api-key /
//!   x-goog-api-key), and even a 4xx/5xx answer must not carry the generic
//!   Anthropic error envelope (the 401-anthropic shape is the exact
//!   regression this row guards).
//! - [`live_ai_vertex_chat_responses_openapi_url_auth`] (AI-V1): the
//!   `google-vertex/chat` + `google-vertex/responses` aiplatform
//!   `endpoints/openapi` URL and ADC-bearer auth (API keys rejected), plus
//!   the Express-mode (`x-goog-api-key`) live leg the Gemini API key
//!   authorizes — an API key cannot drive the `OpenAPI` lanes (they are
//!   OAuth/ADC only), so those two lanes run live on the
//!   `OC_CI_VERTEX_CREDENTIALS` bearer when present and assert their exact
//!   wire shape otherwise.
//! - [`live_ai_copilot_models_sync_and_endpoint_selection`]
//!   (AI-COPILOT-C6): the real `/models` dynamic sync
//!   (`load_copilot_models` + `derive_copilot_models`) and the endpoint
//!   selection (`/v1/messages` → `{base}/v1`, `/responses`, `/chat`).
//!
//! Gating (`#[ignore]` + runtime):
//!
//! ```text
//! OC_CI_LIVE=1 cargo nextest run -p parity --run-ignored only -E 'test(live_ai_)'
//! ```
//!
//! - default suite: `#[ignore]`d, never runs, never dials;
//! - secrets present (see the contract in the
//!   `.github/workflows/runtime-gates.yml` header): the probe RUNS live;
//! - secrets missing: loud `LIVE-SKIP (secrets missing): <ID>` bail — a
//!   skip is never a pass (§8.5); under `OC_GATE_STRICT=1` the skip FAILS
//!   instead (`parity::gate` semantics);
//! - `OC_CI_LIVE=1` with secrets missing: FAILS — an explicitly requested
//!   live run without credentials is a configuration error, never green.
//!
//! Budgets: ≤60s per probe (`PROBE_BUDGET`), ≤25s per single request.
//!
//! Secret discipline (ADR-0006 recording rule): credential VALUES are
//! never printed, logged, or written to disk — evidence records only the
//! endpoint, elapsed time, status, and a body-shape summary. Header dumps
//! go through [`wire_shape`], which redacts `authorization` /
//! `x-goog-api-key` / `x-api-key` values.
//!
//! Evidence: `/tmp/opencode/parity-evidence/CI-RUNTIME-AI-LIVE/live-runs.jsonl`.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oc_ai::protocols::copilot::{
    copilot_client_headers, derive_copilot_models, load_copilot_models, CopilotEndpoint,
    CopilotModelEntry, COPILOT_API_VERSION, COPILOT_BASE_URL,
};
use oc_ai::protocols::vertex::{
    vertex_chat_route, vertex_responses_route, vertex_route_for_model, VertexConfig,
    VertexEndpointConfig,
};
use oc_ai::protocols::vertex_messages::{
    vertex_messages_route, VertexMessagesConfig, HEADER_VERSION,
};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::read_body_limited;
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::route::transport::{SharedTransport, TransportError, TransportRequest};
use oc_ai::route::Route;
use oc_ai::schema::{GenerationOptions, LLMRequest, Message};

/// Whole-probe budget (lane spec: each probe ≤ 60s, single request budget
/// inside it).
const PROBE_BUDGET: Duration = Duration::from_secs(60);
/// Single-request budget (execute + body-prefix read).
const REQUEST_BUDGET: Duration = Duration::from_secs(25);
/// Response-body prefix read for shape assertions — never the whole stream
/// (SSE answers can outlive the probe; the prefix is all the shape checks
/// need).
const BODY_PREFIX_LIMIT: usize = 8 * 1024;
/// The evidence append log (endpoint/time/status/shape ONLY — ADR-0006).
const EVIDENCE_LOG: &str = "/tmp/opencode/parity-evidence/CI-RUNTIME-AI-LIVE/live-runs.jsonl";

// ---------------------------------------------------------------------------
// Gating + secret discipline
// ---------------------------------------------------------------------------

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn live_requested() -> bool {
    std::env::var("OC_CI_LIVE").ok().as_deref() == Some("1")
}

/// The run/skip/fail gate for one probe. Returns `true` = run live.
///
/// - nothing missing → run;
/// - `OC_CI_LIVE=1` and something missing → FAIL (explicit live request
///   without credentials is a configuration error — never green);
/// - otherwise → loud `LIVE-SKIP` bail (visible, never a pass), which
///   additionally FAILS under `OC_GATE_STRICT=1` (`parity::gate`).
fn gate(id: &str, secrets: &[(&str, Option<String>)]) -> bool {
    let missing: Vec<&str> =
        secrets.iter().filter(|(_, value)| value.is_none()).map(|(name, _)| *name).collect();
    if missing.is_empty() {
        return true;
    }
    assert!(
        !live_requested(),
        "LIVE-FAIL (config error): {id} — OC_CI_LIVE=1 explicitly requests the live battery but \
         these secrets are missing: {}. Set them (contract: the \
         .github/workflows/runtime-gates.yml header) or unset OC_CI_LIVE. Refusing to fake or \
         mute an explicitly requested live run.",
        missing.join(", ")
    );
    eprintln!(
        "LIVE-SKIP (secrets missing): {id} — set {} (contract: the \
         .github/workflows/runtime-gates.yml header). The cassette battery under \
         crates/oc-ai/tests stays the only coverage; this is NOT a live pass.",
        missing.join(", ")
    );
    assert!(
        !parity::gate::strict(),
        "OC_GATE_STRICT=1: LIVE-SKIP for {id} is forbidden in strict mode (a skip is not a \
         pass; PARITY-LOOP.md §8.3 Gate 0)"
    );
    false
}

/// Header-safe wire dump for diagnostics: credential-bearing header VALUES
/// are redacted (name + char count only), everything else prints verbatim.
fn wire_shape(wire: &TransportRequest) -> String {
    let headers: Vec<String> = wire
        .headers
        .iter()
        .map(|(name, value)| {
            let secretish = name.eq_ignore_ascii_case("authorization")
                || name.eq_ignore_ascii_case("x-goog-api-key")
                || name.eq_ignore_ascii_case("x-api-key");
            if secretish {
                format!("{name}=<redacted,{} chars>", value.chars().count())
            } else {
                format!("{name}={value}")
            }
        })
        .collect();
    format!("{} {} [headers: {}]", wire.method, wire.url, headers.join("; "))
}

/// Body prefix for failure messages — bounded, and it only ever holds the
/// ANSWER body (never a request credential).
fn body_prefix_for_log(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= 300 {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(300).collect::<String>())
    }
}

// ---------------------------------------------------------------------------
// Live request execution + verdicts
// ---------------------------------------------------------------------------

/// One executed live request (status + bounded body prefix + header NAMES —
/// header values are never kept).
struct LiveOutcome {
    url: String,
    status: u16,
    elapsed_ms: u128,
    body_prefix: String,
    header_names: Vec<String>,
}

/// Execute one prepared wire request under the single-request budget and
/// read a bounded body prefix. Transport failures and budget overruns fail
/// the probe with a classifiable message.
async fn execute_probe_request(
    id: &str,
    transport: &SharedTransport,
    wire: TransportRequest,
) -> LiveOutcome {
    let url = wire.url.clone();
    let started = Instant::now();
    let request = async {
        let response = transport.execute(wire).await?;
        let status = response.status;
        let header_names = response.headers.keys().cloned().collect::<Vec<_>>();
        let body = read_body_limited(response.body, BODY_PREFIX_LIMIT).await?;
        Ok::<_, TransportError>((status, header_names, body))
    };
    match tokio::time::timeout(REQUEST_BUDGET, request).await {
        Err(elapsed) => panic!(
            "LIVE-FAIL (budget): {id} — {url} exceeded the {}s single-request budget ({elapsed:?})",
            REQUEST_BUDGET.as_secs()
        ),
        Ok(Err(error)) => panic!(
            "LIVE-FAIL (transport): {id} — {url}: {} (code {:?}); classify connect/DNS/TLS vs \
             timeout before retrying",
            error.message, error.code
        ),
        Ok(Ok((status, header_names, body_prefix))) => LiveOutcome {
            url,
            status,
            elapsed_ms: started.elapsed().as_millis(),
            body_prefix,
            header_names,
        },
    }
}

/// The AI-VERTEX-V2 regression shape: the generic Anthropic API's error
/// envelope. Whatever the status, a body shaped like this means the request
/// was answered by api.anthropic.com instead of the Vertex publisher
/// endpoint — fail with the URL classification.
fn assert_not_anthropic_error_shape(id: &str, status: u16, body: &str) {
    let fingerprints =
        ["\"type\": \"error\"", "\"type\":\"error\"", "authentication_error", "www.anthropic.com"];
    let hit = fingerprints.iter().find(|fingerprint| body.contains(*fingerprint));
    if let Some(hit) = hit {
        panic!(
            "LIVE-FAIL (URL): {id} — status {status} carries the generic Anthropic API error \
             envelope ({hit}): the request was answered by api.anthropic.com's shape, the exact \
             regression this row guards. Body prefix: {}",
            body_prefix_for_log(body)
        );
    }
}

/// The live verdict on one executed request: only 2xx with a parseable
/// SSE/JSON prefix is a pass. Any non-2xx fails the probe — classified
/// (AUTH / URL / UPSTREAM) so the failure names its category; a live
/// canary is green or honestly red, never quietly tolerant.
fn assert_live_status(id: &str, outcome: &LiveOutcome) {
    if !(200..299).contains(&outcome.status) {
        let class = match outcome.status {
            401 | 403 => "AUTH",
            404 => "URL",
            _ => "UPSTREAM",
        };
        panic!(
            "LIVE-FAIL ({class}): {id} — {} answered status {} after {}ms (body prefix: {}). \
             401/403 = credentials rejected, 404 = endpoint/URL composition, 429/5xx = upstream \
             — a red canary is the honest signal, never mute it.",
            outcome.url,
            outcome.status,
            outcome.elapsed_ms,
            body_prefix_for_log(&outcome.body_prefix)
        );
    }
    let trimmed = outcome.body_prefix.trim_start();
    let parseable = trimmed.starts_with("data:")
        || trimmed.starts_with("event:")
        || trimmed.starts_with('{')
        || trimmed.starts_with('[');
    assert!(
        parseable,
        "LIVE-FAIL (SHAPE): {id} — {} answered 2xx but the body prefix is not SSE/JSON \
         ({} byte(s) read): {}",
        outcome.url,
        outcome.body_prefix.len(),
        body_prefix_for_log(&outcome.body_prefix)
    );
}

fn body_shape_summary(body: &str) -> &'static str {
    let trimmed = body.trim_start();
    if trimmed.is_empty() {
        "empty"
    } else if trimmed.starts_with("data:") || trimmed.starts_with("event:") {
        "sse"
    } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
        "json"
    } else {
        "text"
    }
}

/// Append the ADR-0006 evidence record (endpoint/time/status/shape — never
/// credential values). Best-effort: the probe's verdict does not depend on
/// the evidence write.
fn record_live_run(probe: &str, outcome: &LiveOutcome, verdict: &str) {
    let Some(dir) = std::path::Path::new(EVIDENCE_LOG).parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let entry = serde_json::json!({
        "probe": probe,
        "time_unix": seconds,
        "endpoint": outcome.url,
        "status": outcome.status,
        "elapsed_ms": outcome.elapsed_ms,
        "verdict": verdict,
        "body_shape": body_shape_summary(&outcome.body_prefix),
        "response_header_names": outcome.header_names,
        "credentials": "never recorded (ADR-0006)",
    });
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(EVIDENCE_LOG) {
        use std::io::Write as _;
        let _ = writeln!(file, "{entry}");
    }
}

fn transport() -> SharedTransport {
    Arc::new(ReqwestTransport::new())
}

fn tiny_request(route: &Route, model: &str) -> TransportRequest {
    let language = route
        .model(model.to_string(), None)
        .unwrap_or_else(|error| panic!("LIVE-FAIL (route): model {model} does not bind: {error}"));
    let mut request = LLMRequest::new(language);
    request.messages = vec![Message::user("Reply with the single word: ok")];
    request.generation =
        Some(GenerationOptions { max_tokens: Some(16.0), ..GenerationOptions::default() });
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap_or_else(|error| {
        panic!("LIVE-FAIL (body): the protocol body does not build: {error}")
    });
    let (wire, _) = route.prepare_transport(&prepared, &body).unwrap_or_else(|error| {
        panic!("LIVE-FAIL (wire): the wire request does not render: {error}")
    });
    wire
}

/// Unwrap the [`PROBE_BUDGET`] result, failing with the budget
/// classification.
async fn within_probe_budget<T>(id: &str, future: impl std::future::Future<Output = T>) -> T {
    match tokio::time::timeout(PROBE_BUDGET, future).await {
        Ok(value) => value,
        Err(elapsed) => panic!(
            "LIVE-FAIL (budget): {id} exceeded the {}s whole-probe budget ({elapsed:?})",
            PROBE_BUDGET.as_secs()
        ),
    }
}

// ---------------------------------------------------------------------------
// Probe 1 — AI-VERTEX-V2 (google-vertex/messages, never api.anthropic.com)
// ---------------------------------------------------------------------------

/// AI-VERTEX-V2 live canary. Credentials: `OC_CI_VERTEX_PROJECT` +
/// `OC_CI_VERTEX_CREDENTIALS` (a fresh `OAuth2` access token with the
/// cloud-platform scope — see the `runtime-gates.yml` header contract).
/// Optional overrides: `OC_CI_VERTEX_MODEL` (default `claude-sonnet-4-5`),
/// `OC_CI_VERTEX_LOCATION` (default `us-central1`).
#[tokio::test]
#[ignore = "live canary: OC_CI_LIVE=1 + OC_CI_VERTEX_PROJECT/OC_CI_VERTEX_CREDENTIALS (contract: runtime-gates.yml header)"]
async fn live_ai_vertex_messages_never_api_anthropic() {
    let project = env_non_empty("OC_CI_VERTEX_PROJECT");
    let credentials = env_non_empty("OC_CI_VERTEX_CREDENTIALS");
    if !gate(
        "AI-VERTEX-V2",
        &[
            ("OC_CI_VERTEX_PROJECT", project.clone()),
            ("OC_CI_VERTEX_CREDENTIALS", credentials.clone()),
        ],
    ) {
        return;
    }
    let project = project.unwrap_or_else(|| panic!("gate guarantees the project"));
    let token = credentials.unwrap_or_else(|| panic!("gate guarantees the credentials"));
    let location =
        env_non_empty("OC_CI_VERTEX_LOCATION").unwrap_or_else(|| "us-central1".to_string());
    let model =
        env_non_empty("OC_CI_VERTEX_MODEL").unwrap_or_else(|| "claude-sonnet-4-5".to_string());

    let outcome = within_probe_budget("AI-VERTEX-V2", async {
        let transport = transport();
        let route = vertex_messages_route(
            Arc::clone(&transport),
            VertexMessagesConfig {
                base_url: None,
                project: Some(project.clone()),
                location: Some(location.clone()),
                access_token: Some(Secret::Value(token.clone())),
                api_key: None,
            },
        )
        .unwrap_or_else(|error| panic!("LIVE-FAIL (route): the messages route does not bind: {error}"));
        let wire = tiny_request(&route, &model);

        // Pre-flight wire-shape assertions (the regression this row guards):
        // the exact Vertex publisher :streamRawPredict URL, ADC bearer, no
        // API-key headers, the static anthropic-version.
        let host = if location == "global" {
            "aiplatform.googleapis.com".to_string()
        } else {
            format!("{location}-aiplatform.googleapis.com")
        };
        let expected = format!(
            "https://{host}/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{model}:streamRawPredict"
        );
        assert_eq!(
            wire.url, expected,
            "LIVE-FAIL (URL): the composed URL must be the Vertex publisher :streamRawPredict \
             form — wire: {}",
            wire_shape(&wire)
        );
        assert!(
            !wire.url.contains("api.anthropic.com"),
            "LIVE-FAIL (URL): the messages lane must NEVER hit api.anthropic.com — wire: {}",
            wire_shape(&wire)
        );
        let authorization = wire.header("authorization").unwrap_or_default();
        assert!(
            authorization.starts_with("Bearer ") && authorization.len() > "Bearer ".len() + 8,
            "LIVE-FAIL (AUTH): the ADC access token must ride Authorization as a bearer — wire: {}",
            wire_shape(&wire)
        );
        assert!(
            wire.header("x-api-key").is_none() && wire.header("x-goog-api-key").is_none(),
            "LIVE-FAIL (AUTH): the messages lane rejects API keys (x-api-key / x-goog-api-key \
             are foreign here) — wire: {}",
            wire_shape(&wire)
        );
        assert_eq!(
            wire.header("anthropic-version"),
            Some(HEADER_VERSION),
            "LIVE-FAIL (AUTH): the static anthropic-version header is missing — wire: {}",
            wire_shape(&wire)
        );

        eprintln!("LIVE-REQ: AI-VERTEX-V2 -> {}", wire.url);
        execute_probe_request("AI-VERTEX-V2", &transport, wire).await
    })
    .await;

    assert_not_anthropic_error_shape("AI-VERTEX-V2", outcome.status, &outcome.body_prefix);
    assert_live_status("AI-VERTEX-V2", &outcome);
    record_live_run("AI-VERTEX-V2", &outcome, "live-ok");
    eprintln!(
        "LIVE-OK: AI-VERTEX-V2 — {} status {} in {}ms, body {}, response headers {:?}",
        outcome.url,
        outcome.status,
        outcome.elapsed_ms,
        body_shape_summary(&outcome.body_prefix),
        outcome.header_names
    );
}

// ---------------------------------------------------------------------------
// Probe 2 — AI-V1 (google-vertex/chat + /responses openapi URL/auth, plus
// the Express leg the Gemini API key authorizes)
// ---------------------------------------------------------------------------

/// The `endpoints/openapi` wire for one lane (chat or responses) under an
/// explicit bearer — used both for the live legs (real token) and the
/// prepare-only shape assertions.
fn openapi_wire(
    transport: &SharedTransport,
    lane: &str,
    project: &str,
    token: &str,
    model: &str,
) -> TransportRequest {
    let config = VertexEndpointConfig {
        base_url: None,
        location: Some("global".to_string()),
        project: Some(project.to_string()),
        access_token: Some(Secret::Value(token.to_string())),
        api_key: None,
    };
    let route = match lane {
        "chat" => vertex_chat_route(Arc::clone(transport), config).unwrap_or_else(|error| {
            panic!("LIVE-FAIL (route): the chat lane does not bind: {error}")
        }),
        _ => vertex_responses_route(Arc::clone(transport), config).unwrap_or_else(|error| {
            panic!("LIVE-FAIL (route): the responses lane does not bind: {error}")
        }),
    };
    tiny_request(&route, model)
}

/// AI-V1 live canary. Credential: `OC_CI_GEMINI_API_KEY` (drives the
/// Express-mode `x-goog-api-key` leg — an API key CANNOT drive the `OpenAPI`
/// lanes, they are OAuth/ADC bearer only, which is itself asserted). When
/// `OC_CI_VERTEX_PROJECT` + `OC_CI_VERTEX_CREDENTIALS` are also present (the
/// CI live=true battery always has them) the two `endpoints/openapi` lanes
/// run LIVE under the real bearer; their exact URL/auth shape is asserted
/// either way. Optional overrides: `OC_CI_GEMINI_MODEL` (default
/// `gemini-2.5-flash`), `OC_CI_VERTEX_CHAT_MODEL` /
/// `OC_CI_VERTEX_RESPONSES_MODEL`.
/// The `endpoints/openapi` wire for one lane (chat or responses) under an
/// explicit bearer — used both for the live legs (real token) and the
/// prepare-only shape assertions. Asserts the row's namesake: the exact
/// aiplatform `endpoints/openapi` URL and the ADC-bearer auth shape.
fn assert_openapi_lane_wire(
    transport: &SharedTransport,
    lane: &str,
    project: &str,
    token: &str,
    model: &str,
) -> TransportRequest {
    let wire = openapi_wire(transport, lane, project, token, model);
    assert_eq!(
        wire.url,
        format!(
            "https://aiplatform.googleapis.com/v1/projects/{project}/locations/global/endpoints/openapi{}",
            if lane == "chat" { "/chat/completions" } else { "/responses" }
        ),
        "LIVE-FAIL (URL): the {lane} lane must compose the aiplatform endpoints/openapi root — \
         wire: {}",
        wire_shape(&wire)
    );
    assert!(
        wire.header("authorization").unwrap_or_default().starts_with("Bearer "),
        "LIVE-FAIL (AUTH): the {lane} lane authenticates through the ADC bearer — wire: {}",
        wire_shape(&wire)
    );
    wire
}

/// API keys are structurally rejected on the `OpenAPI` lanes — the
/// auth-shape half of the row.
fn assert_openapi_api_keys_rejected(transport: &SharedTransport, project: &str) {
    let rejected = VertexEndpointConfig {
        base_url: None,
        location: Some("global".to_string()),
        project: Some(project.to_string()),
        access_token: None,
        api_key: Some(Secret::Value("express-key".to_string())),
    };
    let error = vertex_chat_route(Arc::clone(transport), rejected)
        .expect_err("the chat lane must reject API keys");
    assert!(
        error.to_string().contains("does not support API keys"),
        "LIVE-FAIL (AUTH): the OpenAPI lanes must reject API keys, got: {error}"
    );
}

/// The Express-mode wire the Gemini API key DOES authorize: the
/// Gemini-native lane under `x-goog-api-key` on the aiplatform publishers
/// root — asserted shape, returned for the live execution.
fn assert_express_wire(
    transport: &SharedTransport,
    gemini_key: &str,
    project: &str,
    gemini_model: &str,
) -> TransportRequest {
    let route = vertex_route_for_model(
        Arc::clone(transport),
        VertexConfig {
            project: Some(project.to_string()),
            location: Some("us-central1".to_string()),
            access_token: None,
            api_key: Some(Secret::Value(gemini_key.to_string())),
            base_url: None,
        },
        gemini_model,
    );
    let wire = tiny_request(&route, gemini_model);
    let expected_prefix = format!(
        "https://aiplatform.googleapis.com/v1/publishers/google/models/{gemini_model}:streamGenerateContent"
    );
    assert!(
        wire.url.starts_with(&expected_prefix) && wire.url.contains("alt=sse"),
        "LIVE-FAIL (URL): the Express lane must hit the aiplatform publishers root with the \
         model on :streamGenerateContent — wire: {}",
        wire_shape(&wire)
    );
    assert_eq!(
        wire.header("x-goog-api-key"),
        Some(gemini_key),
        "LIVE-FAIL (AUTH): the Gemini API key must ride x-goog-api-key — wire: {}",
        wire_shape(&wire)
    );
    assert!(
        wire.header("authorization").is_none(),
        "LIVE-FAIL (AUTH): the Express lane is header-auth, not bearer — wire: {}",
        wire_shape(&wire)
    );
    wire
}

#[tokio::test]
#[ignore = "live canary: OC_CI_LIVE=1 + OC_CI_GEMINI_API_KEY (contract: runtime-gates.yml header)"]
async fn live_ai_vertex_chat_responses_openapi_url_auth() {
    let gemini_key = env_non_empty("OC_CI_GEMINI_API_KEY");
    if !gate("AI-V1", &[("OC_CI_GEMINI_API_KEY", gemini_key.clone())]) {
        return;
    }
    let gemini_key = gemini_key.unwrap_or_else(|| panic!("gate guarantees the key"));
    let project =
        env_non_empty("OC_CI_VERTEX_PROJECT").unwrap_or_else(|| "ai-v1-live-probe".to_string());
    let bearer = env_non_empty("OC_CI_VERTEX_CREDENTIALS");
    let gemini_model =
        env_non_empty("OC_CI_GEMINI_MODEL").unwrap_or_else(|| "gemini-2.5-flash".to_string());
    let chat_model = env_non_empty("OC_CI_VERTEX_CHAT_MODEL")
        .unwrap_or_else(|| "google/gemini-2.5-flash".to_string());
    let responses_model = env_non_empty("OC_CI_VERTEX_RESPONSES_MODEL")
        .unwrap_or_else(|| "google/gemini-2.5-flash".to_string());

    let outcomes = within_probe_budget("AI-V1", async {
        let transport = transport();
        let mut outcomes: Vec<(&'static str, LiveOutcome)> = Vec::new();

        // (1) The OpenAPI lanes' URL + auth shape — the ledger row's
        //     namesake. With a real bearer: a LIVE request each; without:
        //     prepare-only assertions (placeholder bearer, no network).
        let token = bearer.clone().unwrap_or_else(|| "ai-v1-probe-bearer-placeholder".to_string());
        let live_openapi = bearer.is_some();
        for (lane, model) in
            [("chat", chat_model.as_str()), ("responses", responses_model.as_str())]
        {
            let wire = assert_openapi_lane_wire(&transport, lane, &project, &token, model);
            if live_openapi {
                eprintln!("LIVE-REQ: AI-V1 openapi/{lane} -> {}", wire.url);
                let outcome = execute_probe_request("AI-V1", &transport, wire).await;
                outcomes.push((lane, outcome));
            } else {
                eprintln!(
                    "LIVE-PARTIAL: AI-V1 openapi/{lane} URL/auth asserted at prepare time only — \
                     the live {lane} leg needs OC_CI_VERTEX_CREDENTIALS (an API key cannot drive \
                     the OpenAPI lanes)"
                );
            }
        }
        assert_openapi_api_keys_rejected(&transport, &project);

        // (2) The Express-mode live leg (always live: the Gemini key is
        //     this probe's contract credential).
        let wire = assert_express_wire(&transport, &gemini_key, &project, &gemini_model);
        eprintln!("LIVE-REQ: AI-V1 express -> {}", wire.url);
        outcomes.push(("express", execute_probe_request("AI-V1", &transport, wire).await));
        outcomes
    })
    .await;

    for (lane, outcome) in outcomes {
        assert_not_anthropic_error_shape("AI-V1", outcome.status, &outcome.body_prefix);
        assert_live_status(&format!("AI-V1 {lane}"), &outcome);
        record_live_run("AI-V1", &outcome, "live-ok");
        eprintln!(
            "LIVE-OK: AI-V1 ({lane}) — {} status {} in {}ms, body {}, response headers {:?}",
            outcome.url,
            outcome.status,
            outcome.elapsed_ms,
            body_shape_summary(&outcome.body_prefix),
            outcome.header_names
        );
    }
}

// ---------------------------------------------------------------------------
// Probe 3 — AI-COPILOT-C6 (real /models sync + endpoint selection)
// ---------------------------------------------------------------------------

/// AI-COPILOT-C6 live canary. Credential: `OC_CI_COPILOT_TOKEN` (a GitHub
/// Copilot token with /models visibility — see the `runtime-gates.yml`
/// header contract). Optional override: `OC_CI_COPILOT_BASE_URL` (default
/// `https://api.githubcopilot.com`).
/// Endpoint selection over the derived entries (models.ts `build`):
/// `/v1/messages` wins, then `/responses`, then `/chat/completions`; the
/// messages lane gains the `/v1` baseURL overlay. Returns the counts
/// `(routed, messages, responses, chat)`; models advertising no known
/// endpoint are logged (LIVE-NOTE) without failing the probe.
fn summarize_endpoint_selection(
    base_url: &str,
    entries: &[CopilotModelEntry],
) -> (usize, usize, usize, usize) {
    let mut routed = 0usize;
    let mut messages = 0usize;
    let mut responses = 0usize;
    let mut chat = 0usize;
    for entry in entries {
        let Some(endpoint) = entry.endpoint else {
            eprintln!(
                "LIVE-NOTE: model {} advertises no recognized endpoint (supported_endpoints has \
                 none of /v1/messages, /responses, /chat/completions)",
                entry.id
            );
            continue;
        };
        routed += 1;
        match endpoint {
            CopilotEndpoint::Messages => {
                messages += 1;
                assert_eq!(
                    entry.base_url,
                    format!("{base_url}/v1"),
                    "LIVE-FAIL (SHAPE): the messages lane's baseURL overlay must be {{base}}/v1 \
                     for {}",
                    entry.id
                );
            }
            CopilotEndpoint::Responses => responses += 1,
            CopilotEndpoint::Chat => chat += 1,
        }
    }
    (routed, messages, responses, chat)
}

#[tokio::test]
#[ignore = "live canary: OC_CI_LIVE=1 + OC_CI_COPILOT_TOKEN (contract: runtime-gates.yml header)"]
async fn live_ai_copilot_models_sync_and_endpoint_selection() {
    let token = env_non_empty("OC_CI_COPILOT_TOKEN");
    if !gate("AI-COPILOT-C6", &[("OC_CI_COPILOT_TOKEN", token.clone())]) {
        return;
    }
    let token = token.unwrap_or_else(|| panic!("gate guarantees the token"));
    let base_url =
        env_non_empty("OC_CI_COPILOT_BASE_URL").unwrap_or_else(|| COPILOT_BASE_URL.to_string());

    let (remote_count, entry_count, endpoint_counts) =
        within_probe_budget("AI-COPILOT-C6", async {
            let transport = transport();
            // The credential header set the production sync sends: the
            // bearer + the static Copilot client headers (github-copilot.ts
            // applyHeaders) + Accept.
            let mut headers: Vec<(String, String)> =
                vec![("Authorization".to_string(), format!("Bearer {token}"))];
            for (name, value) in copilot_client_headers() {
                headers.push((name, value));
            }
            headers.push(("Accept".to_string(), "application/json".to_string()));
            assert_eq!(
                headers
                    .iter()
                    .find(|(name, _)| name == "X-GitHub-Api-Version")
                    .map(|(_, v)| v.as_str()),
                Some(COPILOT_API_VERSION),
                "the client header policy must pin the Copilot API version"
            );
            eprintln!("LIVE-REQ: AI-COPILOT-C6 -> {base_url}/models");

            let started = Instant::now();
            let remote = load_copilot_models(Arc::clone(&transport), &base_url, headers)
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "LIVE-FAIL (AUTH/UPSTREAM): AI-COPILOT-C6 — the /models fetch failed: \
                         {error} (401 = token rejected, timed out after 5s = network)"
                    )
                });
            let outcome = LiveOutcome {
                url: format!("{base_url}/models"),
                status: 200,
                elapsed_ms: started.elapsed().as_millis(),
                body_prefix: String::new(),
                header_names: Vec::new(),
            };
            assert!(
                !remote.is_empty(),
                "LIVE-FAIL (SHAPE): AI-COPILOT-C6 — /models returned 0 USABLE models (fetch was \
                 2xx; either the listing is empty or every entry failed the usable filter — the \
                 token may lack Copilot access)"
            );

            // Endpoint selection + the /v1 baseURL overlay for the
            // messages lane.
            let entries = derive_copilot_models(&base_url, &remote, &[]);
            assert!(
                !entries.is_empty(),
                "LIVE-FAIL (SHAPE): AI-COPILOT-C6 — derive() produced 0 entries from {} remote \
                 models",
                remote.len()
            );
            let (routed, messages, responses, chat) =
                summarize_endpoint_selection(&base_url, &entries);
            assert!(
                routed >= 1,
                "LIVE-FAIL (SHAPE): AI-COPILOT-C6 — no derived model selected an endpoint from \
                 {} entries",
                entries.len()
            );
            record_live_run("AI-COPILOT-C6", &outcome, "live-ok");
            eprintln!(
                "LIVE-OK: AI-COPILOT-C6 — {} usable remote models, {} derived entries, endpoint \
                 selection: {routed} routed (messages={messages}, responses={responses}, \
                 chat={chat}), {}ms",
                remote.len(),
                entries.len(),
                outcome.elapsed_ms
            );
            (remote.len(), entries.len(), (messages, responses, chat))
        })
        .await;
    let _ = (remote_count, entry_count, endpoint_counts);
}
