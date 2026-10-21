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
//! `OC_CI_VERTEX_CREDENTIALS` accepts TWO shapes (resolved by
//! [`resolve_vertex_credentials`]):
//! - a **service-account key-file JSON** (recommended — static, no
//!   refresh-before-dispatch): the probe routes it through the PRODUCTION
//!   CRED-V3 chain (`oc-core` `IntegrationHost::vertex_binding` — RS256
//!   self-signed JWT grant at the key's `token_uri`) to mint the access
//!   token, which is itself a live check of CRED-V3;
//! - a raw `OAuth2` access token (`ya29.…`): passed straight through.
//!
//! Secret discipline (ADR-0006 recording rule): credential VALUES are
//! never printed, logged, or written to evidence. The one disk touch is
//! the SA-JSON leg: the CRED-V3 entry point consumes an ADC FILE
//! (`GOOGLE_APPLICATION_CREDENTIALS`), so the JSON is staged to a 0600
//! file under `/tmp/opencode/lane-live-<pid>-adc/` for the duration of
//! the token grant only and unlinked immediately after (Drop guard, also
//! on panic) — never logged, never in evidence. Header dumps go through
//! [`wire_shape`], which redacts `authorization` / `x-goog-api-key` /
//! `x-api-key` values.
//!
//! Evidence: `/tmp/opencode/parity-evidence/CI-RUNTIME-AI-LIVE/live-runs.jsonl`.

use std::os::unix::fs::PermissionsExt as _;
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
use oc_ai::route::transport::{
    FailingTransport, SharedTransport, TransportError, TransportRequest,
};
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

// ---------------------------------------------------------------------------
// Credential resolution: OC_CI_VERTEX_CREDENTIALS auto-detects the shape
// ---------------------------------------------------------------------------

/// The staged ADC file + its `GOOGLE_APPLICATION_CREDENTIALS` env export,
/// undone on drop (also during a panic unwind): the CRED-V3 entry point
/// consumes an ADC FILE, so the SA JSON touches disk only as this 0600
/// temp file, only for the duration of the token grant, and never in a
/// log or evidence record.
struct AdcStage {
    path: std::path::PathBuf,
}

impl AdcStage {
    fn stage(id: &str, content: &str) -> Self {
        let dir =
            std::path::PathBuf::from(format!("/tmp/opencode/lane-live-{}-adc", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|error| {
            panic!(
                "LIVE-FAIL (credentials): {id} — cannot stage the ADC dir {}: {error}",
                dir.display()
            )
        });
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        let path = dir.join("application_default_credentials.json");
        std::fs::write(&path, content).unwrap_or_else(|error| {
            panic!("LIVE-FAIL (credentials): {id} — cannot stage the ADC file: {error}")
        });
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", &path);
        Self { path }
    }
}

impl Drop for AdcStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(self.path.parent().unwrap_or(std::path::Path::new("/")));
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
    }
}

/// A minimal `IntegrationHost` for the CRED-V3 credential chain
/// (in-memory database, non-persisting bus — nothing durable, nothing
/// network-bound except the grant the chain itself performs).
async fn cred_v3_host() -> Arc<oc_core::integration::IntegrationHost> {
    let database = Arc::new(
        oc_core::database::Database::open_in_memory()
            .await
            .expect("in-memory database for the CRED-V3 host"),
    );
    let bus = Arc::new(oc_core::bus::Bus::new(
        (*database).clone(),
        oc_core::bus::BusOptions { persist: false, ..oc_core::bus::BusOptions::deterministic() },
    ));
    oc_core::integration::IntegrationHost::new(
        database,
        bus,
        oc_core::provider_catalog::catalog::Catalog::embedded(),
        "opencode-parity-live-probe",
    )
}

/// Resolve `OC_CI_VERTEX_CREDENTIALS` into an access token + a source
/// label (the label is logged; the token never is).
///
/// - JSON (a service-account key file — `client_email` / `private_key` /
///   `token_uri`, or an `authorized_user` ADC file) → the PRODUCTION
///   CRED-V3 chain mints the token (`IntegrationHost::vertex_binding`:
///   RS256 self-signed JWT grant at the key's own `token_uri`, else the
///   production token endpoint). Running the real chain is itself a live
///   check of CRED-V3.
/// - anything else → a raw `OAuth2` access token (`ya29.…`), passed
///   through untouched.
async fn resolve_vertex_credentials(id: &str, raw: &str) -> (String, &'static str) {
    if !raw.trim_start().starts_with('{') {
        return (raw.trim().to_string(), "raw access token (passed through)");
    }
    let shape = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => match value.get("type").and_then(serde_json::Value::as_str) {
            Some("service_account") => "service_account",
            Some("authorized_user") => "authorized_user",
            other => panic!(
                "LIVE-FAIL (credentials): {id} — OC_CI_VERTEX_CREDENTIALS is JSON but not an \
                 ADC shape (type={other:?}). Accepted: a service-account key-file JSON \
                 (recommended) or a raw access token. No credential material is logged."
            ),
        },
        Err(error) => panic!(
            "LIVE-FAIL (credentials): {id} — OC_CI_VERTEX_CREDENTIALS starts with '{{' but is \
             not valid JSON ({error}). Accepted: a service-account key-file JSON (recommended) \
             or a raw access token."
        ),
    };
    // Stage as the ADC file the production chain reads; the guard unlinks
    // it (and the env export) the moment the grant returns or panics.
    let stage = AdcStage::stage(id, raw);
    let host = cred_v3_host().await;
    let grant = tokio::time::timeout(REQUEST_BUDGET, host.vertex_binding()).await;
    drop(stage);
    let binding = match grant {
        Err(elapsed) => panic!(
            "LIVE-FAIL (budget): {id} — the {shape} token grant exceeded the {}s request budget \
             ({elapsed:?})",
            REQUEST_BUDGET.as_secs()
        ),
        Ok(None) => panic!(
            "LIVE-FAIL (credentials): {id} — the {shape} token grant returned no token (CRED-V3 \
             chain: check the key validity / refresh token / the service account's permissions; \
             no credential material is logged)"
        ),
        Ok(Some(binding)) => binding,
    };
    (binding.api_key, "service-account JSON via the CRED-V3 RS256 exchange (live check of CRED-V3)")
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
/// `OC_CI_VERTEX_CREDENTIALS` (a service-account key-file JSON —
/// recommended, exchanged to an access token through the PRODUCTION
/// CRED-V3 chain — or a raw access token; see the `runtime-gates.yml`
/// header contract). Optional overrides: `OC_CI_VERTEX_MODEL` (default
/// `claude-sonnet-4-5`), `OC_CI_VERTEX_LOCATION` (default
/// `us-central1`).
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
    let raw_credentials = credentials.unwrap_or_else(|| panic!("gate guarantees the credentials"));
    let location =
        env_non_empty("OC_CI_VERTEX_LOCATION").unwrap_or_else(|| "us-central1".to_string());
    let model =
        env_non_empty("OC_CI_VERTEX_MODEL").unwrap_or_else(|| "claude-sonnet-4-5".to_string());

    let outcome = within_probe_budget("AI-VERTEX-V2", async {
        // Auto-detect the credential shape: SA key-file JSON goes through
        // the production CRED-V3 exchange (itself a live check of
        // CRED-V3); a raw access token passes straight through.
        let (token, source) = resolve_vertex_credentials("AI-VERTEX-V2", &raw_credentials).await;
        eprintln!("LIVE-CRED: AI-VERTEX-V2 — {source}");
        let transport = transport();
        let route = vertex_messages_route(
            Arc::clone(&transport),
            VertexMessagesConfig {
                base_url: None,
                project: Some(project.clone()),
                location: Some(location.clone()),
                access_token: Some(Secret::Value(token)),
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
/// run LIVE under the resolved bearer (SA key-file JSON → the production
/// CRED-V3 exchange; raw token → passthrough); their exact URL/auth shape
/// is asserted either way. Optional overrides: `OC_CI_GEMINI_MODEL`
/// (default `gemini-2.5-flash`), `OC_CI_VERTEX_CHAT_MODEL` /
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
        //     namesake. With credentials: a LIVE request each (an SA
        //     key-file JSON resolves through the production CRED-V3
        //     exchange, a raw token passes through); without:
        //     prepare-only assertions (placeholder bearer, no network).
        let (token, live_openapi) = match bearer.as_deref() {
            Some(raw) => {
                let (token, source) = resolve_vertex_credentials("AI-V1", raw).await;
                eprintln!("LIVE-CRED: AI-V1 openapi legs — {source}");
                (token, true)
            }
            None => ("ai-v1-probe-bearer-placeholder".to_string(), false),
        };
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

// ---------------------------------------------------------------------------
// Hermetic local check: SA key-file JSON is identified and routed through
// the CRED-V3 exchange (throwaway key + stub token endpoint bound to
// 127.0.0.1:0 — ZERO outbound network). This is the always-run regression
// for the credential auto-detection, NOT a live probe: it is not
// `#[ignore]`d and deliberately not part of the `test(live_ai_)` CI
// battery.
// ---------------------------------------------------------------------------

/// A throwaway 2048-bit RSA key (PKCS#8 PEM), generated for this fixture
/// only — never registered anywhere, exists solely so ring can load and
/// sign the CRED-V3 self-signed JWT. The stub endpoint never verifies the
/// signature (verification is Google's server-side job), so any valid
/// signing key exercises the client chain.
const FIXTURE_SA_PRIVATE_KEY: &str = r"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDI81dajy2U/JxD
TrGPPEZkt0xCWU3CAAtR83NJM1lmjjALxb4L5gdgvRlmmwV43TJO5YkptM4nrnan
Gkb9/A0AY6OviOg/mVqlBdn9Li62wnsIkhqEQnHukYZBai6h1ikArUU4TS5v+AlN
Y6qvEOX9M5f/6EwQn6AuPRe6cV2Ywnvs69DFUCtxCnnke6NKvhmFwt+ynl188Z9g
561wI2WFEUvCRtS37WIL5ICuLepm07aXFrK1NfPatHVuDLrQ9w2KG9+qlSwav1Zu
spGyP+WfzzxLTKncfWg4a5K+vLZep7c8fZiUOM7RA5r6M8/E2SfBW3aKTm545QXD
3AomA7VxAgMBAAECggEAOBccWe2/JmgAYhU0IKwNZg133sk4mB8VCdHvaw5HNQYZ
SR/v9ThG9Oyzi11YAK2VXkpZp+eM56yd9/SwvjI9ms81IEkRsrbmg0uH1lCuv1q3
puPJPQOi85lxJQ8zZv71QPgcayO4A47zYcOhH+6Sk8Nz+P1pRfaYam+pEOSV4I5h
eHwS1CwXnme4HYO9Xt3oVIXDxMLc68L+v5TKkt9pSoxs4TKD5LsS8CxY9sAJ18lb
9ImypqKkgCQHeijH2ZqiezOKDFnxKb7j6DgTbkryW+5VBqJ7SQ1NiLg8c6vNAjcJ
T7VQ5gC3tlopNrNebJcxNg9RYLlyLyq45EIhE9n69QKBgQD6Lc5ocksoIWCHYUPu
VnEQbfAJmUK0QXl3DxqNUr6z31DAaaa2ArYJVrsDQTz9dnkw+XN0prJd7hFR0KWg
C0nbeV2tkQ/Zl7IYgs3dQ6xiz6u8vb1WW6uDis97C7qwMfR3YXDZ9G2semU8ND33
+ohh3n5odz0yLrXuW9SCmsb8xwKBgQDNoE43NuRTK5+ctaCnE8xo9nzO+4bvr3v8
HfjtAwrDcu2oBqspCEYIAym51o6XqMiFj60T7cblnJTtFwZRqrsBwko4bTxOY0/G
tyYMt7SuwdrMPi0oCwVFyQACpyauFFlpb8mpLiZwD/n/bmOZda6/kjhgYAZu3vzr
rI9QCtLUBwKBgE7RNZEUSPdIpKeiDgu9cftyF0h+31P4uLK4sG3Lnsb+WOt1nY8E
BaOljl91IyjbNU0nmacriVAsW4qCCFwGkmsxrkCrTeoIb5RnSUInOt0afgmFYWwe
j0du6PTj+hnecx1VNn+EETVgjhpMfyVr6aGjvgFdZ4syyiBhD+9hocUrAoGBALEj
6n2ychwcNpJ//XgxnCTIQ1jkYqhVWtAk2e8Ecd9SQN5BcG4WzI3We7m21nxcUPKO
haHnczMXI0zkSvNLy3KGd/4ITK2S7ITZ/pNyJttt8RmGbsxSQiaOM84VRUMuEKC+
RtHSChb8DHg6Ft96/CHYy2xrDN+yhdxZedSQnnM9AoGAfneALCs+dqBtBn+UHZBu
Q/Sf1yBbEXyFleja7mPOj8STUf1cpmmSQ3KQ/jqt+kPTQsfWRWhef5DvS3gUFchF
NWXlxGposHbrhQeC9/dnqL2MEeCyEXCyp4nQougjBMwIvOcnG0VccCVWOcLl41Yy
Fc4YR4dFPoGcDI+4Ep9HUX0=
-----END PRIVATE KEY-----
";

/// Serve exactly one token grant: capture the request, assert it is the
/// CRED-V3 `jwt-bearer` POST, answer the canned access token. Returns
/// `(stub_base_url, JoinHandle<request_summary>)` — the caller runs its
/// client side while this accepts.
async fn stub_token_endpoint() -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = std::sync::Arc::new(
        tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 127.0.0.1:0"),
    );
    let port = listener.local_addr().expect("local addr").port();
    let service = async move {
        let (mut socket, _) = listener.accept().await.expect("one connection");
        // Read headers + the content-length-delimited body (looped — one
        // read may return a partial request).
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let body_start = loop {
            let text = String::from_utf8_lossy(&buffer);
            if let Some(index) = text.find("\r\n\r\n") {
                let length = text[..index]
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .starts_with("content-length:")
                            .then(|| line.split(':').nth(1))
                            .flatten()
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if buffer.len() >= index + 4 + length {
                    break index + 4;
                }
            }
            let read = socket.read(&mut chunk).await.expect("read the grant request");
            assert!(read > 0, "stub token endpoint: the client closed before sending the grant");
            buffer.extend_from_slice(&chunk[..read]);
        };
        let request = String::from_utf8_lossy(&buffer);
        let request_line = request.lines().next().unwrap_or_default().to_string();
        let grant_body = &request[body_start..];
        let grant_type = grant_body
            .split('&')
            .find_map(|pair| pair.strip_prefix("grant_type="))
            .unwrap_or_default()
            .replace("%3A", ":");
        let assertion_segments = grant_body
            .split('&')
            .find_map(|pair| pair.strip_prefix("assertion="))
            .map_or(0, |value| value.split('.').count());
        let reply = serde_json::json!({ "access_token": "ya29.stub-minted-for-probe" }).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
        socket.write_all(response.as_bytes()).await.expect("write the token reply");
        format!(
            "{request_line} | grant_type={grant_type} | assertion_segments={assertion_segments}"
        )
    };
    (format!("http://127.0.0.1:{port}"), tokio::spawn(service))
}

/// The SA key-file JSON leg of [`resolve_vertex_credentials`]: the JSON is
/// identified, staged as the ADC file, exchanged through the PRODUCTION
/// CRED-V3 chain (the fixture's `token_uri` points at the local stub —
/// zero outbound network), and the minted token flows into the production
/// route's Authorization header. The raw-token passthrough is asserted
/// inline (it must not construct anything — instant, no stub hit).
#[tokio::test]
async fn vertex_credentials_sa_json_routes_through_cred_v3() {
    let (stub_base, stub) = stub_token_endpoint().await;
    let sa_json = serde_json::json!({
        "type": "service_account",
        "project_id": "probe-project",
        "client_email": "probe@probe-project.iam.gserviceaccount.com",
        "private_key": FIXTURE_SA_PRIVATE_KEY,
        "token_uri": format!("{stub_base}/token"),
    })
    .to_string();

    // The service-account grant through the CRED-V3 client.
    let (token, source) = resolve_vertex_credentials("AI-VERTEX-V2(local-fixture)", &sa_json).await;
    assert_eq!(token, "ya29.stub-minted-for-probe", "the stub's canned token must flow through");
    assert!(source.contains("CRED-V3"), "the source label must name the exchange: {source}");
    let stub_summary = stub.await.expect("stub token endpoint service");
    assert!(
        stub_summary.contains("POST /token")
            && stub_summary.contains("grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer"),
        "the grant must be the CRED-V3 jwt-bearer POST against the key file's own token_uri: \
         {stub_summary}"
    );
    assert!(
        stub_summary.contains("assertion_segments=3"),
        "the assertion must be a 3-segment JWT: {stub_summary}"
    );

    // The minted token rides the production messages route (prepare-only,
    // failing transport — still zero network).
    let route = vertex_messages_route(
        Arc::new(FailingTransport { message: "cred_v3 routing test (nothing dials)" }),
        VertexMessagesConfig {
            base_url: None,
            project: Some("probe-project".to_string()),
            location: Some("us-central1".to_string()),
            access_token: Some(Secret::Value(token)),
            api_key: None,
        },
    )
    .expect("the messages route binds with the minted token");
    let wire = tiny_request(&route, "claude-sonnet-4-5");
    assert_eq!(wire.header("authorization"), Some("Bearer ya29.stub-minted-for-probe"));
    assert_eq!(
        wire.url,
        "https://us-central1-aiplatform.googleapis.com/v1/projects/probe-project/locations/us-central1/publishers/anthropic/models/claude-sonnet-4-5:streamRawPredict"
    );

    // The raw-token passthrough (no JSON, no host construction, no stub).
    let (raw, raw_source) =
        resolve_vertex_credentials("AI-VERTEX-V2(local-fixture)", "ya29.c.raw-passthrough-token")
            .await;
    assert_eq!(raw, "ya29.c.raw-passthrough-token");
    assert!(raw_source.contains("raw"), "the source label must name the passthrough: {raw_source}");
}
