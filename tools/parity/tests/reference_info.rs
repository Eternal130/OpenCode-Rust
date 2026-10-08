//! End-to-end sample proving the whole parity chain: boot the reference
//! server, hit `GET /api/info`, normalize, golden-compare, exercise
//! `httpdiff` and `sse`, and shut down cleanly.
//!
//! Skips (pass, with a note) when bun or the reference checkout is missing.

use std::time::Duration;

use parity::httpdiff;
use parity::{golden, normalize, reference, sse};

const TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn reference_api_info_and_first_event_shape() -> anyhow::Result<()> {
    if reference::skip_if_no_bun() {
        eprintln!("skipping: bun or reference checkout not available (see tools/parity/README.md)");
        return Ok(());
    }
    let server = reference::spawn().await?;
    let checks = Box::pin(run_checks(&server)).await;
    let shutdown = server.stop().await;
    checks?;
    shutdown?;
    Ok(())
}

async fn run_checks(server: &reference::ReferenceServer) -> anyhow::Result<()> {
    let env = server.norm_env();
    eprintln!("[parity] server ready at {}", server.url());

    // 1. Same request twice through normalize + deep_diff: stable shape.
    let first = httpdiff::get_json(server.url(), server.password(), "/api/info", TIMEOUT).await?;
    eprintln!("[parity] first api/info: HTTP {}", first.status);
    let second = httpdiff::get_json(server.url(), server.password(), "/api/info", TIMEOUT).await?;
    eprintln!("[parity] second api/info: HTTP {}", second.status);
    let a = normalize::normalize_value(&first.value, &env);
    let b = normalize::normalize_value(&second.value, &env);
    assert_eq!(first.status, 200, "api/info status");
    if let Some(divergence) = httpdiff::deep_diff(&a, &b) {
        panic!("api/info not stable across two GETs: {divergence}");
    }

    // 2. Cross-run stability via golden (pid/port/url/version rewritten per
    //    NORMALIZATION.md).
    golden::assert_json("reference_api_info.json", &a)?;

    // 3. httpdiff against the same base (two sides, identical request):
    //    exercises the two-sided compare path; later phases point the second
    //    side at the Rust port.
    eprintln!("[parity] httpdiff self-compare");
    let side = httpdiff::Side { base: server.url(), password: server.password(), env: &env };
    let comparison = Box::pin(httpdiff::compare_requests(
        httpdiff::Side { ..side },
        httpdiff::Side { ..side },
        "GET",
        "/api/info",
        None,
        TIMEOUT,
    ))
    .await?;
    if let Some(divergence) = &comparison.divergence {
        panic!("self-compare diverged: {divergence}");
    }

    // 4. SSE chain: subscribe, collect the first data frame (the
    //    server.connected replay), normalize the sequence, golden it.
    eprintln!("[parity] SSE subscribe");
    let mut stream =
        sse::SseStream::subscribe(server.url(), server.password(), "/api/event", TIMEOUT).await?;
    eprintln!("[parity] SSE collecting");
    let frames = stream.collect(1, Duration::from_secs(20)).await?;
    eprintln!("[parity] SSE got {} frame(s)", frames.len());
    let shape = sse::frames_to_json(&frames, &env);
    let type_at =
        shape.get(0).and_then(|entry| entry.get("data")).and_then(|data| data.get("type")).cloned();
    assert_eq!(type_at, Some(serde_json::json!("server.connected")), "first SSE frame type");
    golden::assert_json("reference_sse_first_frame.json", &shape)?;

    Ok(())
}
