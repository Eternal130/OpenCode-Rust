//! AI-TIMEOUT regression tests (ledger: `03:5.1` / `04:B-6`, cluster C7a).
//!
//! v2 enforces a three-part HTTP timeout budget on every AI provider
//! request (`packages/ai/src/route/transport/http.ts:100-143` +
//! `packages/core/src/provider.ts:155-170`):
//!
//! * `headerTimeout` — wait for response headers (default 300s, `false`
//!   disables);
//! * `chunkTimeout` — idle wait between streamed body chunks (default 300s,
//!   `false` disables);
//! * `timeout` — whole-request budget from send until the response
//!   completes (**no default**; also bounds the header wait).
//!
//! Expired budgets surface as typed transport errors (`operation`
//! `request`/`read`, `code: "Timeout"`) instead of hanging forever. The
//! budget resolution is pinned to the v2 constants; the integration cases
//! below use short synthetic budgets so the suite stays fast — an outer
//! guard (`GUARD`) makes a missing budget fail deterministically instead of
//! hanging the suite (which is exactly the pre-fix symptom).
//!
//! Every case runs against a local stall server bound to `127.0.0.1:0`
//! through the production `ReqwestTransport` + `Route` pipeline (ADR-0006:
//! no real network, no real providers).

use std::sync::Arc;
use std::time::{Duration, Instant};

use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::route::{Endpoint, Route};
use oc_ai::schema::{
    AiError, AiErrorReason, HttpOptions, HttpTimeout, LLMRequest, TransportOperation,
};
use oc_ai::test_support::echo_route;

/// Outer guard: far above the synthetic budgets, far below the v2 default —
/// proves "fails within budget instead of hanging" without ever risking a
/// 300s stall in CI.
const GUARD: Duration = Duration::from_secs(12);

/// Synthetic budgets/delays: budgets generously above scheduling noise,
/// delays comfortably above the budgets so the disabled-timeout cases
/// cannot win by accident.
const BUDGET_MS: f64 = 600.0;
const DELAY: Duration = Duration::from_millis(1_500);

fn ms(value: f64) -> HttpTimeout {
    HttpTimeout::Ms(value)
}

/// Response head shared by every SSE-speaking behavior.
const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n";

// ---------------------------------------------------------------------------
// Local stall server (127.0.0.1:0, in-test; no fixed ports, never 49374)
// ---------------------------------------------------------------------------

/// What the server does once it has read a full request.
#[derive(Clone, Copy)]
enum Behavior {
    /// Accept the request, then never write anything (hanging headers).
    StallHeaders,
    /// Write response headers + one SSE chunk, then stall mid-body (the
    /// declared content-length is never satisfied).
    StallAfterFirstChunk,
    /// Hold the headers for `DELAY`, then answer a complete SSE body.
    DelayedHeaders,
    /// Write one SSE chunk, pause `DELAY`, then finish the body.
    ChunkPauseFinish,
}

struct StallServer {
    addr: std::net::SocketAddr,
    accept: tokio::task::JoinHandle<()>,
}

impl StallServer {
    async fn start(behavior: Behavior) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let accept = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { return };
                tokio::spawn(serve(socket, behavior));
            }
        });
        Ok(Self { addr, accept })
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for StallServer {
    fn drop(&mut self) {
        // Own handle only (§2.1): abort the accept loop we spawned; open
        // connections exit when the client side is dropped or their GC cap
        // elapses. No name/pattern-based process killing.
        self.accept.abort();
    }
}

/// Hard cap for one parked connection so a leaked connection task can never
/// outlive the test binary meaningfully.
const CONNECTION_GC: Duration = Duration::from_secs(60);

async fn serve(mut socket: tokio::net::TcpStream, behavior: Behavior) {
    use tokio::io::AsyncWriteExt;
    if read_request(&mut socket).await.is_err() {
        return;
    }
    match behavior {
        Behavior::StallHeaders => park(&mut socket).await,
        Behavior::StallAfterFirstChunk => {
            // content-length deliberately larger than what is written: the
            // body stalls mid-stream with the connection still open.
            let head = format!("{SSE_HEAD}content-length: 4096\r\nconnection: keep-alive\r\n\r\n");
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(b"data: hello\n\n").await;
            let _ = socket.flush().await;
            park(&mut socket).await;
        }
        Behavior::DelayedHeaders => {
            tokio::time::sleep(DELAY).await;
            let body = "data: hello\n\n";
            let head =
                format!("{SSE_HEAD}content-length: {}\r\nconnection: close\r\n\r\n", body.len());
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
        }
        Behavior::ChunkPauseFinish => {
            let first = "data: hello\n\n";
            let rest = "data: world\n\n";
            let head = format!(
                "{SSE_HEAD}content-length: {}\r\nconnection: close\r\n\r\n",
                first.len() + rest.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(first.as_bytes()).await;
            let _ = socket.flush().await;
            tokio::time::sleep(DELAY).await;
            let _ = socket.write_all(rest.as_bytes()).await;
        }
    }
}

/// Read one full HTTP/1.1 request (head + content-length body).
async fn read_request(socket: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "client gone"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_head_end(&buffer) {
            break position;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);
    let mut received = buffer.len() - head_end - 4;
    while received < content_length {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "client gone"));
        }
        received += read;
    }
    Ok(())
}

fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// Hold the connection open without writing (the stall). Returns when the
/// client disconnects or the GC cap elapses.
async fn park(socket: &mut tokio::net::TcpStream) {
    use tokio::io::AsyncReadExt;
    let _ = tokio::time::timeout(CONNECTION_GC, socket.read(&mut [0u8; 64])).await;
}

// ---------------------------------------------------------------------------
// Route fixture: production transport + echo route against the stall server
// ---------------------------------------------------------------------------

fn stall_route(server: &StallServer) -> Route {
    let mut route = echo_route(Arc::new(ReqwestTransport::new()) as Arc<_>);
    route.endpoint = Endpoint::path("/echo").with_base_url(server.base_url());
    route
}

fn request(route: &Route, http: HttpOptions) -> LLMRequest {
    let mut request =
        LLMRequest::new(route.model("test-model", None).expect("model builds")).with_prompt("hi");
    request.http = Some(http);
    request
}

/// `route.generate` under the outer guard; returns (elapsed, outcome).
async fn guarded_generate(
    route: &Route,
    request: LLMRequest,
) -> (Duration, Result<oc_ai::LLMResponse, AiError>) {
    let started = Instant::now();
    let outcome = tokio::time::timeout(GUARD, route.generate(request))
        .await
        .expect("GUARD elapsed: the request hung past every budget — timeout budget not applied");
    (started.elapsed(), outcome)
}

fn expect_transport_error(
    outcome: Result<oc_ai::LLMResponse, AiError>,
    operation: TransportOperation,
    message: &str,
    expect_http_status: Option<u16>,
) {
    let error = outcome.expect_err("expected the budget to expire");
    match &error.reason {
        AiErrorReason::Transport { fields, operation: actual, code, url, .. } => {
            assert_eq!(*actual, operation, "operation for {message}: {}", error.message());
            assert_eq!(code.as_deref(), Some("Timeout"), "code for {message}");
            assert_eq!(fields.message, message, "v2 timeout message");
            assert!(
                url.as_deref().is_some_and(|u| u.starts_with("http://127.0.0.1:")),
                "url captured: {url:?}"
            );
            if let Some(status) = expect_http_status {
                let context = fields.http.as_ref().expect("http context on the reason");
                assert_eq!(context.status, status, "http status for {message}");
            }
        }
        other => panic!(
            "expected Transport({operation:?}) Timeout, got {}: {}",
            other.tag(),
            error.message()
        ),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Ledger test: hanging response headers and stalled mid-body chunks must
/// fail within their budgets with typed transport errors, never hang.
#[tokio::test]
async fn stalled_headers_chunks() {
    // -- stalled response headers: headerTimeout budget -------------------
    let server = StallServer::start(Behavior::StallHeaders).await.expect("bind 127.0.0.1:0");
    let route = stall_route(&server);
    let (elapsed, outcome) = guarded_generate(
        &route,
        request(&route, HttpOptions { header_timeout: Some(ms(BUDGET_MS)), ..Default::default() }),
    )
    .await;
    eprintln!("stalled_headers_chunks: header case failed after {elapsed:?}");
    expect_transport_error(
        outcome,
        TransportOperation::Request,
        "Timed out waiting for response headers",
        None,
    );
    drop(server);

    // -- stalled chunk mid-body: chunkTimeout budget ----------------------
    let server =
        StallServer::start(Behavior::StallAfterFirstChunk).await.expect("bind 127.0.0.1:0");
    let route = stall_route(&server);
    let (elapsed, outcome) = guarded_generate(
        &route,
        request(&route, HttpOptions { chunk_timeout: Some(ms(BUDGET_MS)), ..Default::default() }),
    )
    .await;
    eprintln!("stalled_headers_chunks: chunk case failed after {elapsed:?}");
    expect_transport_error(
        outcome,
        TransportOperation::Read,
        "Timed out waiting for response data",
        Some(200),
    );
}

/// The whole-request budget fails a stream that keeps trickling past
/// `timeout` even when the chunk timeout is disabled (v2: "Timed out
/// waiting for the response to complete").
#[tokio::test]
async fn total_budget_fails_a_stalled_stream() {
    let server =
        StallServer::start(Behavior::StallAfterFirstChunk).await.expect("bind 127.0.0.1:0");
    let route = stall_route(&server);
    let (elapsed, outcome) = guarded_generate(
        &route,
        request(
            &route,
            HttpOptions {
                timeout: Some(ms(BUDGET_MS)),
                chunk_timeout: Some(HttpTimeout::Disabled),
                ..Default::default()
            },
        ),
    )
    .await;
    eprintln!("total_budget_fails_a_stalled_stream: failed after {elapsed:?}");
    expect_transport_error(
        outcome,
        TransportOperation::Read,
        "Timed out waiting for the response to complete",
        Some(200),
    );
}

/// A shorter whole-request budget bounds the header wait even when the
/// header timeout itself is disabled (v2: `Duration.min(header, total)`).
#[tokio::test]
async fn total_budget_bounds_the_header_wait() {
    let server = StallServer::start(Behavior::StallHeaders).await.expect("bind 127.0.0.1:0");
    let route = stall_route(&server);
    let (elapsed, outcome) = guarded_generate(
        &route,
        request(
            &route,
            HttpOptions {
                timeout: Some(ms(BUDGET_MS)),
                header_timeout: Some(HttpTimeout::Disabled),
                ..Default::default()
            },
        ),
    )
    .await;
    eprintln!("total_budget_bounds_the_header_wait: failed after {elapsed:?}");
    expect_transport_error(
        outcome,
        TransportOperation::Request,
        "Timed out waiting for response headers",
        None,
    );
}

/// `headerTimeout: false` / `chunkTimeout: false` disable their budgets:
/// slow headers and a slow chunk still complete (v2 `false` semantics —
/// no false-positive aborts).
#[tokio::test]
async fn disabled_timeouts_still_complete() {
    // headers arrive after DELAY with the header timeout disabled
    let server = StallServer::start(Behavior::DelayedHeaders).await.expect("bind 127.0.0.1:0");
    let route = stall_route(&server);
    let (elapsed, outcome) = guarded_generate(
        &route,
        request(
            &route,
            HttpOptions {
                header_timeout: Some(HttpTimeout::Disabled),
                chunk_timeout: Some(ms(10_000.0)),
                ..Default::default()
            },
        ),
    )
    .await;
    let response = outcome.expect("disabled header timeout waits for slow headers");
    assert_eq!(response.text(), "hello");
    eprintln!("disabled_timeouts_still_complete: header case completed in {elapsed:?}");
    drop(server);

    // a chunk pauses for DELAY with the chunk timeout disabled
    let server = StallServer::start(Behavior::ChunkPauseFinish).await.expect("bind 127.0.0.1:0");
    let route = stall_route(&server);
    let (elapsed, outcome) = guarded_generate(
        &route,
        request(
            &route,
            HttpOptions {
                header_timeout: Some(ms(10_000.0)),
                chunk_timeout: Some(HttpTimeout::Disabled),
                ..Default::default()
            },
        ),
    )
    .await;
    let response = outcome.expect("disabled chunk timeout waits for a slow chunk");
    assert_eq!(response.text(), "helloworld");
    eprintln!("disabled_timeouts_still_complete: chunk case completed in {elapsed:?}");
}
