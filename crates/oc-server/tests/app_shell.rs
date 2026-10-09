//! App-shell integration tests (process.test.ts + event-feed + pairing +
//! assets + openapi ports). Boots a real engine + app shell on an ephemeral
//! port with a password, drives it over raw HTTP.

mod support;

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use oc_core::bus::PublishOptions;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::app::AppShell;
use oc_server::lifecycle::{serve, ServeHandle, ServeOptions};
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::json;

use support::{request, request_streaming};

struct Server {
    state: Arc<AppState>,
    handle: ServeHandle,
    addr: std::net::SocketAddr,
}

impl Server {
    async fn boot(password: &str, heartbeat: Duration) -> Self {
        Self::boot_with(password, heartbeat, |_| Box::pin(async {})).await
    }

    async fn boot_with(
        password: &str,
        heartbeat: Duration,
        hook: impl Fn(Arc<AppState>) -> futures::future::BoxFuture<'static, ()> + Send + Sync + 'static,
    ) -> Self {
        Self::boot_feed(password, heartbeat, None, hook).await
    }

    /// Boots with an explicit SSE feed capacity (slow-consumer tests).
    async fn boot_feed(
        password: &str,
        heartbeat: Duration,
        feed_capacity: Option<usize>,
        hook: impl Fn(Arc<AppState>) -> futures::future::BoxFuture<'static, ()> + Send + Sync + 'static,
    ) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let engine =
            Engine::boot(EngineOptions::new(tmp.path().to_owned(), tmp.path().to_owned())).await;
        let mut state = AppState::new(
            engine,
            AuthConfig { password: Some(password.to_owned()) },
            "test-version".to_owned(),
            Arc::new(UuidV4IdGen),
            Arc::new(SystemClock),
        );
        state.cors_origins =
            vec!["http://192.168.1.10:3001".to_owned(), "https://example.com".to_owned()];
        state.assets.insert(
            "index.html".to_owned(),
            br#"<html><script id="oc-theme-preload-script">theme()</script><body>shell</body></html>"#.to_vec(),
        );
        state.assets.insert("_assets/app.js".to_owned(), b"console.log(1)".to_vec());
        let state = Arc::new(state);
        let feed = Arc::new(match feed_capacity {
            Some(capacity) => oc_server::sse::EventFeed::with_capacity(capacity),
            None => oc_server::sse::EventFeed::new(),
        });
        let shell = AppShell::with_feed(Arc::clone(&state), feed, heartbeat);
        let router = shell.router();
        let hook_state = Arc::clone(&state);
        let on_listen: oc_server::lifecycle::OnListen = Arc::new(move |_addr| {
            let future = hook(Arc::clone(&hook_state));
            Box::pin(async move {
                future.await;
            })
        });
        let handle = serve(
            &ServeOptions { port: Some(0), ..ServeOptions::default() },
            Arc::clone(&state),
            router,
            Some(on_listen),
        )
        .await
        .expect("serve");
        let addr = handle.addr;
        std::mem::forget(tmp);
        Self { state, handle, addr }
    }

    async fn shutdown(self) {
        self.handle.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(10), self.handle.shutdown_and_join())
            .await
            .expect("server drains")
            .expect("serve ok");
    }

    async fn get(&self, target: &str, headers: &[(&str, &str)]) -> support::RawResponse {
        request(self.addr, "GET", target, headers, &[]).await.expect("request")
    }
}

fn basic(password: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
    )
}

fn auth_headers(password: &str) -> Vec<(&'static str, &'static str)> {
    let value: &'static str = Box::leak(
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
        )
        .into_boxed_str(),
    );
    vec![("authorization", value)]
}

// ---------------------------------------------------------------------------
// process.test.ts — dispatch/readiness/auth gates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn server_process_info_starting_then_ready() {
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&observed);
    let server = Server::boot_with("secret", Duration::from_secs(15), move |state| {
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            // During the Starting window /api/info is auth-gated and carries
            // the info payload with a 503 + retry-after (v2 infoResponse).
            let addr = state.local_addr.read().expect("bound");
            let authed = request(
                addr,
                "GET",
                "/api/info",
                &[(
                    "authorization",
                    &format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode("opencode:secret")
                    ),
                )],
                &[],
            )
            .await
            .expect("info during starting");
            seen.lock().unwrap().push((
                authed.status,
                authed.text(),
                authed.header("retry-after").map(str::to_owned),
            ));

            let denied = request(addr, "GET", "/api/info", &[], &[]).await.expect("unauth");
            seen.lock().unwrap().push((denied.status, denied.text(), None));
        })
    })
    .await;

    let (authed_status, authed_body, retry_after) = observed.lock().unwrap()[0].clone();
    assert_eq!(authed_status, 503);
    let body = serde_json::from_str::<serde_json::Value>(&authed_body).unwrap();
    assert_eq!(body["version"], "test-version");
    assert_eq!(body["pid"], json!(std::process::id()));
    assert_eq!(retry_after.as_deref(), Some("1"));
    let (denied_status, denied_body, _) = observed.lock().unwrap()[1].clone();
    assert_eq!(denied_status, 401);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&denied_body).unwrap(),
        json!({"_tag": "UnauthorizedError", "message": "Authentication required"})
    );

    // Ready: 200 with the full info payload.
    let response = server.get("/api/info", &auth_headers("secret")).await;
    assert_eq!(response.status, 200);
    let body = response.json();
    assert_eq!(body["version"], "test-version");
    assert_eq!(body["pid"], json!(std::process::id()));
    assert_eq!(body["urls"], json!([format!("http://{}", server.addr)]));
    assert_eq!(
        body["paths"]["tmp"].as_str().map(str::to_owned),
        Some(oc_util::global_roots::resolve().tmp.display().to_string())
    );

    server.shutdown().await;
}

#[tokio::test]
async fn server_process_unavailable_envelopes_per_readiness() {
    // stopping: everything except info → 503 code envelope + retry-after.
    let server = Server::boot("secret", Duration::from_secs(15)).await;
    oc_server::readiness::begin_stopping(&server.state.readiness);
    let response = server.get("/api/event", &auth_headers("secret")).await;
    assert_eq!(response.status, 503);
    assert_eq!(response.json(), json!({"code": "service_stopping"}));
    assert_eq!(response.header("retry-after"), Some("1"));
    server.shutdown().await;

    // failed: /api/info → 500; other routes → 503 service_failed envelope.
    let server = Server::boot_with("secret", Duration::from_secs(15), |state| {
        Box::pin(async move {
            oc_server::readiness::fail(&state.readiness);
        })
    })
    .await;
    let info = server.get("/api/info", &auth_headers("secret")).await;
    assert_eq!(info.status, 500);
    assert_eq!(info.json()["version"], "test-version");
    let other = server.get("/api/session", &auth_headers("secret")).await;
    assert_eq!(other.status, 503);
    assert_eq!(
        other.json(),
        json!({
            "code": "service_failed",
            "message": "The background service could not start.",
            "action": "Run `opencode service restart` after checking the service logs."
        })
    );
    assert_eq!(other.header("retry-after"), None);
    server.shutdown().await;
}

#[tokio::test]
async fn server_process_api_surface_requires_auth() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;
    for target in ["/api", "/api/missing", "/openapi.json"] {
        let response = request(server.addr, "GET", target, &[], &[]).await.expect("target");
        assert_eq!(response.status, 401, "{target}");
        assert_eq!(
            response.json(),
            json!({"_tag": "UnauthorizedError", "message": "Authentication required"}),
            "{target}"
        );
        assert_eq!(
            response.header("www-authenticate"),
            Some("Basic realm=\"Secure Area\""),
            "{target}"
        );
    }
    // sec-fetch-mode: cors gets no challenge.
    let response = request(server.addr, "GET", "/api/info", &[("sec-fetch-mode", "cors")], &[])
        .await
        .expect("fetch mode");
    assert_eq!(response.status, 401);
    assert_eq!(response.header("www-authenticate"), None);

    // Unmatched API routes with auth → empty 404.
    let response = server.get("/api/definitely/not/a/route", &auth_headers("secret")).await;
    assert_eq!(response.status, 404);
    assert_eq!(response.body, [] as [u8; 0]);

    // auth_token query credential (browser WS path).
    let token = base64::engine::general_purpose::STANDARD.encode("opencode:secret");
    let response = server.get(&format!("/api/info?auth_token={token}"), &[]).await;
    assert_eq!(response.status, 200);

    server.shutdown().await;
}

#[tokio::test]
async fn server_process_gate_exemptions() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;

    // Pairing connect URLs skip auth ALWAYS (unmatched code → pairing 401).
    let response = server.get("/auth/connect/unknown-code", &[]).await;
    assert_eq!(response.status, 401);
    let body = response.json();
    assert_eq!(body["_tag"], "UnauthorizedError");
    assert_eq!(body["message"], "Pairing link expired or already used");

    // PTY connect with ticket query skips the gate when ready. Since the
    // Phase 5 mount, the handler table answers this route: an unknown pty
    // without websocket upgrade headers reaches the handler's extractor
    // rejection (400) / not-found path — anything but the gate's 401.
    let response = server.get("/api/pty/pty_1/connect?ticket=abc", &[]).await;
    assert_ne!(response.status, 401, "ticketed pty connect must not 401");
    let response =
        server.get("/api/experimental/persistent-pty/term_1/connect?ticket=abc", &[]).await;
    assert_ne!(response.status, 401, "ticketed persistent-pty connect must not 401");

    // Without a ticket the gate applies.
    let response = server.get("/api/pty/pty_1/connect", &[]).await;
    assert_eq!(response.status, 401);

    // Pre-ready the ticket exemption is OFF; pairing connect still passes
    // the gate but hits the unavailable envelope. Both observed inside the
    // Starting window (serve flips Ready right after the hook).
    let observed: Arc<std::sync::Mutex<Vec<(u16, String)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&observed);
    let _starting = Server::boot_with("secret", Duration::from_secs(15), move |state| {
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            let addr = state.local_addr.read().expect("bound");
            let ticketed = request(addr, "GET", "/api/pty/pty_1/connect?ticket=abc", &[], &[])
                .await
                .expect("ticketed pre-ready");
            seen.lock().unwrap().push((ticketed.status, String::new()));
            let pairing = request(addr, "GET", "/auth/connect/whatever", &[], &[])
                .await
                .expect("pairing pre-ready");
            seen.lock().unwrap().push((pairing.status, pairing.text()));
        })
    })
    .await;
    let statuses = observed.lock().unwrap().clone();
    assert_eq!(statuses[0].0, 401, "ticket exemption requires ready");
    assert_eq!(statuses[1].0, 503, "pairing connect passes the gate but is unavailable");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&statuses[1].1).unwrap(),
        json!({"code": "service_starting"})
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// process.test.ts — CORS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn server_process_cors_allowlist() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;

    // Localhost preflight: 204 + echo.
    let preflight = request(
        server.addr,
        "OPTIONS",
        "/api/info",
        &[
            ("origin", "http://localhost:3000"),
            ("access-control-request-method", "GET"),
            ("access-control-request-headers", "authorization"),
        ],
        &[],
    )
    .await
    .expect("preflight");
    assert_eq!(preflight.status, 204);
    assert_eq!(preflight.header("access-control-allow-origin"), Some("http://localhost:3000"));
    assert_eq!(preflight.header("access-control-allow-headers"), Some("authorization"));
    assert_eq!(preflight.header("access-control-max-age"), Some("86400"));

    for origin in
        ["http://192.168.1.10:3001", "https://example.com", "https://untrusted.example.com"]
    {
        let allowed = (origin != "https://untrusted.example.com").then_some(origin);
        let preflight = request(
            server.addr,
            "OPTIONS",
            "/api/info",
            &[
                ("origin", origin),
                ("access-control-request-method", "GET"),
                ("access-control-request-headers", "authorization"),
            ],
            &[],
        )
        .await
        .expect("preflight");
        assert_eq!(preflight.status, 204);
        assert_eq!(preflight.header("access-control-allow-origin"), allowed);

        let status = server
            .get("/api/info", &[("origin", origin), ("authorization", &basic("secret"))])
            .await;
        assert_eq!(status.status, 200);
        assert_eq!(status.header("access-control-allow-origin"), allowed);

        let denied = server.get("/api/info", &[("origin", origin)]).await;
        assert_eq!(denied.status, 401);
        assert_eq!(denied.header("access-control-allow-origin"), allowed);
    }

    server.shutdown().await;
}

#[tokio::test]
async fn server_process_compression_respects_accept_encoding() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;
    let response = server
        .get("/api/info", &[("accept-encoding", "gzip"), ("authorization", &basic("secret"))])
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-encoding"), Some("gzip"));

    let identity = server.get("/api/info", &auth_headers("secret")).await;
    assert_eq!(identity.header("content-encoding"), None);

    // SSE is never compressed.
    let (response, mut body) = request_streaming(
        server.addr,
        "GET",
        "/api/event",
        &[("accept-encoding", "gzip"), ("authorization", &basic("secret"))],
        &[],
    )
    .await
    .expect("sse");
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-encoding"), None);
    assert_eq!(response.header("content-type"), Some("text/event-stream"));
    body.read_until("server.connected", Duration::from_secs(5)).await.expect("initial frame");
    body.ignore();
    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// process.test.ts — pairing round trip
// ---------------------------------------------------------------------------

async fn pair(server: &Server) -> serde_json::Value {
    let response =
        request(server.addr, "POST", "/api/pair", &[("authorization", &basic("secret"))], &[])
            .await
            .expect("pair");
    assert_eq!(response.status, 200);
    response.json()
}

#[tokio::test]
async fn server_process_pairing_round_trip() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;
    let base = format!("http://{}", server.addr);

    // Unauthenticated pair is challenged.
    let rejected = request(server.addr, "POST", "/api/pair", &[], &[]).await.expect("pair");
    assert_eq!(rejected.status, 401);
    assert_eq!(rejected.header("www-authenticate"), Some("Basic realm=\"Secure Area\""));

    // Browser pairing: 302 + cookie, then cookie auth.
    let browser = pair(&server).await;
    assert_eq!(browser["expires_in"], json!(300));
    let code = browser["code"].as_str().expect("code").to_owned();
    let redirect = server.get(&format!("/auth/connect/{code}"), &[("accept", "text/html")]).await;
    assert_eq!(redirect.status, 302);
    assert_eq!(redirect.header("location"), Some("/"));
    let set_cookie = redirect.header("set-cookie").expect("cookie").to_owned();
    let port = server.addr.port();
    assert!(set_cookie.contains(&format!("opencode_session_{port}=")), "cookie {set_cookie}");
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    let cookie = set_cookie.split(';').next().expect("pair").to_owned();

    // Reuse fails with the browser text.
    let reused = server.get(&format!("/auth/connect/{code}"), &[("accept", "text/html")]).await;
    assert_eq!(reused.status, 401);
    assert!(reused.text().contains("opencode pair"), "body {}", reused.text());

    let cookie_ok = server.get("/api/info", &[("cookie", &cookie)]).await;
    assert_eq!(cookie_ok.status, 200);
    let same_origin =
        server.get("/api/info", &[("cookie", &cookie), ("origin", base.as_str())]).await;
    assert_eq!(same_origin.status, 200);
    let cross_origin =
        server.get("/api/info", &[("cookie", &cookie), ("origin", "http://127.0.0.1:1")]).await;
    assert_eq!(cross_origin.status, 401);
    let tampered = server.get("/api/info", &[("cookie", &format!("{cookie}x"))]).await;
    assert_eq!(tampered.status, 401);

    // API pairing: JSON token usable as a Basic password.
    let client = pair(&server).await;
    let code = client["code"].as_str().expect("code").to_owned();
    let redeemed = server.get(&format!("/auth/connect/{code}"), &[]).await;
    assert_eq!(redeemed.status, 200);
    let token = redeemed.json()["token"].as_str().expect("token").to_owned();
    let via_token = request(
        server.addr,
        "GET",
        "/api/info",
        &[(
            "authorization",
            &format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("opencode:{token}"))
            ),
        )],
        &[],
    )
    .await
    .expect("token auth");
    assert_eq!(via_token.status, 200);
    assert_eq!(server.get(&format!("/auth/connect/{code}"), &[]).await.status, 401);
    assert_eq!(server.get("/auth/connect/unknown", &[]).await.status, 401);

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// SSE event feed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn server_sse_initial_frame_live_events_and_heartbeat() {
    let server = Server::boot("secret", Duration::from_millis(150)).await;
    let (response, mut body) = request_streaming(
        server.addr,
        "GET",
        "/api/event",
        &[("authorization", &basic("secret"))],
        &[],
    )
    .await
    .expect("sse");

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/event-stream"));
    assert_eq!(response.header("cache-control"), Some("no-cache, no-transform"));
    assert_eq!(response.header("x-accel-buffering"), Some("no"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));

    let initial = body
        .read_until("\"type\":\"server.connected\"", Duration::from_secs(5))
        .await
        .expect("connected frame");
    assert!(initial.contains("\"data\":{}"), "initial: {initial}");

    // A public bus event flows through.
    server
        .state
        .engine
        .bus
        .publish(
            oc_core::bus::registry::config_updated(),
            json!({ "generation": 1 }),
            PublishOptions { global: true, ..PublishOptions::default() },
        )
        .await
        .expect("publish");
    body.read_until("config.updated", Duration::from_secs(5)).await.expect("live frame");

    // Heartbeat comments arrive on the short cadence.
    body.read_until(": heartbeat", Duration::from_secs(5)).await.expect("heartbeat");

    body.ignore();
    server.shutdown().await;
}

#[tokio::test]
async fn server_sse_overflow_terminates_slow_consumers() {
    // Small feed queue (64) + fat frames: a client that never reads fills the
    // kernel socket buffers after a handful of frames, the bounded queue
    // overflows, the subscriber is dropped, and the connection aborts
    // instead of streaming everything.
    const TOTAL: u64 = 120;
    const BLOB: &str = "0123456789abcdef"; // repeated to ~100 KiB payloads

    let server =
        Server::boot_feed("secret", Duration::from_secs(15), Some(64), |_| Box::pin(async {}))
            .await;
    let stream = std::net::TcpStream::connect(server.addr).expect("connect");
    stream.set_nodelay(true).expect("nodelay");
    stream.set_nonblocking(true).expect("nonblocking");
    let mut writer = tokio::net::TcpStream::from_std(stream).expect("tokio stream");
    support::write_request_public(
        &mut writer,
        &server.addr.to_string(),
        "GET",
        "/api/event",
        &[("authorization", &basic("secret"))],
        &[],
    )
    .await
    .expect("request");

    // Consume the response head only, then stop reading entirely.
    let mut body = support::StreamingBody::from_stream_after_head(writer, 16384).await;
    let blob = BLOB.repeat(6_400);
    let definition = oc_core::bus::registry::config_updated();
    let bus = Arc::clone(&server.state.engine.bus);
    for index in 0..TOTAL {
        bus.publish(
            definition,
            json!({ "generation": index, "blob": blob }),
            PublishOptions { global: true, ..PublishOptions::default() },
        )
        .await
        .expect("publish");
    }

    // The subscriber is dropped on overflow; the connection aborts (EOF)
    // instead of streaming all 600 frames.
    let mut received = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let budget = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!budget.is_zero(), "stream never terminated; received {} bytes", received.len());
        match body.read_some(budget).await {
            Ok(chunk) if chunk.is_empty() => break, // EOF = terminated
            Ok(chunk) => received.extend_from_slice(&chunk),
            Err(_) => break, // aborted mid-chunk
        }
    }
    let text = String::from_utf8_lossy(&received).into_owned();
    let frames = text.matches("data: ").count();
    assert!(frames < usize::try_from(TOTAL).unwrap(), "terminated early (frames={frames})");
    server.shutdown().await;
}

#[tokio::test]
async fn server_sse_requires_auth() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;
    let response = server.get("/api/event", &[]).await;
    assert_eq!(response.status, 401);
    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Session log streaming
// ---------------------------------------------------------------------------

#[tokio::test]
async fn server_session_log_validation_and_not_found() {
    let server = Server::boot("secret", Duration::from_secs(15)).await;

    // Invalid session id → 400 InvalidRequestError.
    let response =
        server.get("/api/experimental/session/not-an-id/log", &auth_headers("secret")).await;
    assert_eq!(response.status, 400);
    assert_eq!(response.json()["_tag"], "InvalidRequestError");

    // Valid id, no such session → 404 SessionNotFoundError.
    let response =
        server.get("/api/experimental/session/ses_missing/log", &auth_headers("secret")).await;
    assert_eq!(response.status, 404);
    let body = response.json();
    assert_eq!(body["_tag"], "SessionNotFoundError");
    assert_eq!(body["sessionID"], "ses_missing");

    // Bad query → 400.
    let response = server
        .get("/api/experimental/session/ses_missing/log?after=abc", &auth_headers("secret"))
        .await;
    assert_eq!(response.status, 400);
    let response = server
        .get("/api/experimental/session/ses_missing/log?follow=yes", &auth_headers("secret"))
        .await;
    assert_eq!(response.status, 400);

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Assets + openapi
// ---------------------------------------------------------------------------

#[tokio::test]
async fn server_assets_serve_before_auth() {
    use sha2::Digest;

    let server = Server::boot("secret", Duration::from_secs(15)).await;

    // Any non-API path, unauthenticated and wrong-password alike → the SPA.
    for authorization in [
        None,
        Some(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("opencode:wrong")
        )),
    ] {
        for target in ["/", "/workspace/example", "/sw.js"] {
            let headers: Vec<(&str, String)> =
                authorization.iter().map(|value| ("authorization", value.clone())).collect();
            let refs: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
            let response = server.get(target, &refs).await;
            assert_eq!(response.status, 200, "{target} must serve before auth");
            assert!(response.header("www-authenticate").is_none());
            assert!(response.text().contains("shell"));
        }
    }

    // Exact asset hit with immutable caching.
    let response = server.get("/_assets/app.js", &[]).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/javascript"));
    assert_eq!(response.header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));

    // SPA fallback for unknown non-asset paths (HTML headers + CSP hash).
    let response = server.get("/some/client/route", &[]).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/html"));
    assert_eq!(response.header("cache-control"), Some("no-cache"));
    let csp = response.header("content-security-policy").expect("csp").to_owned();
    assert!(csp.contains("default-src 'self'"), "csp {csp}");
    assert!(csp.contains("'sha256-"), "inline script hash injected: {csp}");
    let expected =
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(b"theme()"));
    assert!(csp.contains(&format!("'sha256-{expected}'")), "csp {csp}");

    // _assets/* and icons/* misses → 404 no-store.
    let response = server.get("/_assets/missing.js", &[]).await;
    assert_eq!(response.status, 404);
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let response = server.get("/icons/icon.svg", &[]).await;
    assert_eq!(response.status, 404);
    assert_eq!(response.header("cache-control"), Some("no-store"));

    // HEAD → 200 with headers, empty body; POST → 405.
    let head = request(server.addr, "HEAD", "/", &[], &[]).await;
    let response = head.expect("head");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, [] as [u8; 0]);
    let response = request(server.addr, "POST", "/", &[], b"x").await.expect("post");
    assert_eq!(response.status, 405);
    assert_eq!(response.body, [] as [u8; 0]);

    server.shutdown().await;
}

#[tokio::test]
async fn server_openapi_serves_pinned_bytes() {
    use sha2::Digest;
    use std::fmt::Write as _;

    let server = Server::boot("secret", Duration::from_secs(15)).await;
    let response = server.get("/openapi.json", &auth_headers("secret")).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let digest = sha2::Sha256::digest(&response.body);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    assert_eq!(hex, oc_server::routes_gen::OPENAPI_SHA256);
    server.shutdown().await;
}
