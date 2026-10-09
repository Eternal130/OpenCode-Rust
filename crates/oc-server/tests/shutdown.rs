//! Gate-6 F2 regression: graceful shutdown is not held open by infinite
//! stream bodies.
//!
//! 1. In-process — an open `/api/event` SSE client sees the body END once
//!    the shutdown watch fires (the stream selects on it), and the serve
//!    task joins promptly.
//! 2. Real-process — the actual `oc-server` bin, one open SSE client,
//!    SIGTERM: the process must exit 0 in under 5s (the bin's bounded
//!    overall drain backstops anything still in flight).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::AsyncBufReadExt as _;

use oc_core::engine::{Engine, EngineOptions};
use oc_server::app::AppShell;
use oc_server::lifecycle::{serve, ServeHandle, ServeOptions};
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

use support::request_streaming;

// ---------------------------------------------------------------------------
// In-process: the shutdown watch ends open stream bodies
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_watch_ends_open_sse_body_and_drains_serve() {
    let tmp = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let engine =
        Engine::boot(EngineOptions::new(tmp.path().join("data"), tmp.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let shell = AppShell::with_heartbeat(Arc::clone(&state), Duration::from_secs(15));
    let handle: ServeHandle = serve(
        &ServeOptions { port: Some(0), ..ServeOptions::default() },
        Arc::clone(&state),
        shell.router(),
        None,
    )
    .await
    .expect("serve");
    std::mem::forget(tmp);

    // One SSE client that keeps its connection open, heartbeats idle.
    let (response, mut body) =
        request_streaming(handle.addr, "GET", "/api/event", &[], &[]).await.expect("sse");
    assert_eq!(response.status, 200);
    body.read_until("server.connected", Duration::from_secs(5)).await.expect("initial frame");

    // Begin shutdown (the same latch SIGTERM flips in the bin).
    let started = Instant::now();
    handle.begin_shutdown();

    // The body must END (EOF), not hang on the infinite feed.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let budget = deadline.saturating_duration_since(Instant::now());
        assert!(!budget.is_zero(), "SSE body did not end within 5s of shutdown");
        match body.read_some(budget).await {
            Ok(chunk) if chunk.is_empty() => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }
    }
    // And the serve loop joins (graceful drain completes).
    tokio::time::timeout(Duration::from_secs(5), handle.shutdown_and_join())
        .await
        .expect("serve task drained")
        .expect("serve ok");
    assert!(started.elapsed() < Duration::from_secs(5), "drain took {:?}", started.elapsed());
}

// ---------------------------------------------------------------------------
// Real-process: SIGTERM with an open SSE client exits < 5s
// ---------------------------------------------------------------------------

#[tokio::test]
async fn serve_bin_sigterm_with_open_sse_exits_promptly() {
    let home = tempfile::tempdir_in("/tmp/opencode").expect("home tempdir");
    let project = tempfile::tempdir_in("/tmp/opencode").expect("project tempdir");

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_oc-server"))
        .args(["serve", "--port", "0"])
        .env("OPENCODE_TEST_HOME", home.path())
        .env_remove("OPENCODE_PASSWORD")
        .current_dir(project.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn oc-server");

    // Read the ready line ("server listening on http://127.0.0.1:PORT").
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut line = String::new();
    let url = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            line.clear();
            let read = stdout.read_line(&mut line).await.expect("stdout");
            assert!(read != 0, "server exited before printing its URL");
            if let Some(rest) = line.trim().strip_prefix("server listening on ") {
                return rest.to_owned();
            }
        }
    })
    .await
    .expect("server ready line");
    let addr: std::net::SocketAddr =
        url.trim_start_matches("http://").parse().unwrap_or_else(|_| {
            // SocketAddr::from_str is infallible-shaped for host:port; keep
            // a clear failure if the line carried anything else.
            url.trim_start_matches("http://").to_string().parse().expect("listen addr")
        });

    // One open SSE client (headers + the initial frame).
    let (response, mut body) =
        request_streaming(addr, "GET", "/api/event", &[], &[]).await.expect("sse");
    assert_eq!(response.status, 200);
    body.read_until("server.connected", Duration::from_secs(5)).await.expect("initial frame");

    // SIGTERM — the production shutdown trigger.
    let pid = child.id().expect("pid");
    let started = Instant::now();
    let signalled = std::process::Command::new("kill")
        .args(["-s", "TERM", &pid.to_string()])
        .status()
        .expect("kill -TERM");
    assert!(signalled.success(), "failed to deliver SIGTERM");

    let status = tokio::time::timeout(Duration::from_secs(15), child.wait())
        .await
        .expect("process exited (no SIGTERM hang)")
        .expect("wait");
    let elapsed = started.elapsed();
    assert!(status.success(), "exit status {status:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "SIGTERM with an open SSE client must exit < 5s, took {elapsed:?}"
    );
}
