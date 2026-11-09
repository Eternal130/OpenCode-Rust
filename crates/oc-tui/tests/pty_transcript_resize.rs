//! D2.2 transcript-family PTY resize proof (lane gate: the geometry
//! family's second surface rides the shared tree and its wrapper rect
//! follows the terminal resize beat-for-beat with the old imperative
//! paste site).
//!
//! The session route's TRANSCRIPT is a geometry-reactive family (the
//! `transcript` wrapper mounts under `surface-transcript` in the
//! shared tree at the old `(shift, content_top)` paste site; D2.2).
//! The test boots the real app loop at 120×30, opens a session, arms
//! the streaming-thinking script (a running "Thinking" row held open
//! in the transcript), then SIGWINCH-resizes the PTY to 60×20 and
//! asserts the transcript re-lays: the streamed row re-paints INSIDE
//! the 60-column window (the wrapper's rect tracks `app.size`, the
//! content subtree rebuilding on the geometry key), and the composer
//! mount still re-lays under it.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    streaming_thinking_script, FakeLlm, LiveHarness, WORDMARK,
};

#[test]
fn pty_transcript_rect_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d22transcript-rs");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(streaming_thinking_script("ses_alpha", "count slowly for the resize probe"));
    let harness = LiveHarness::attach(120, 30);
    let mut driver = harness.handle();
    let done = Arc::new(Mutex::new(None::<Result<(), String>>));
    let slot = Arc::clone(&done);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".into());
            }
            open_first_session(&mut driver, b"Alpha")
                .map_err(|e| format!("session never opened: {e}"))?;
            // Submit: the armed SSE script holds a running "Thinking"
            // row open in the transcript (the streaming turn that
            // never ends).
            driver.write(b"count slowly for the resize probe\r");
            // Pre-resize: the transcript's streamed row paints inside
            // the 120-column frame (the wrapper's imperative rect =
            // the old paste site).
            if !wait_transcript(&mut driver, 120, 30) {
                return Err("the transcript never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master — the
            // kernel-delivered winsize the pump reconciles; the
            // wrapper's rect re-seats via set_prop and the content
            // subtree rebuilds on the geometry key).
            driver.resize_pty(60, 20);
            // Post-resize: the transcript re-lays INSIDE the 60-column
            // frame (asserted within the new window only; the harness
            // parser keeps the old 120-col grid, so cells beyond 60
            // are stale by design).
            if !wait_transcript(&mut driver, 60, 20) {
                let grid = driver.grid_now();
                let mut rows = Vec::new();
                for y in 0..grid.rows.min(20) {
                    let text = grid.content_rect(0, y, 60, 1);
                    if !text.trim().is_empty() {
                        rows.push(format!("r{y:02}: {text:?}"));
                    }
                }
                return Err(format!(
                    "the transcript never re-laid at 60 cols after SIGWINCH: {rows:?}"
                ));
            }
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("done") = Some(out);
    });
    run_tui(&fake.url, 120, 30, Duration::from_secs(60));
    fake.stop();
    let outcome = done.lock().expect("done").take();
    if let Some(Err(e)) = outcome {
        panic!("transcript resize: {e}");
    }
}

/// Poll the settled grid until the transcript's streamed content
/// paints inside the `cols`-column window: the user's probe text (the
/// turn's user-message box) or the held-open "Thinking" row — both
/// are transcript-region pixels nothing else paints.
fn wait_transcript(driver: &mut vt_harness::HarnessHandle, cols: usize, rows: usize) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        let probe = (0..grid.rows.min(rows))
            .any(|y| grid.content_rect(0, y, cols, 1).contains("count slowly for the resize"));
        let thinking =
            (0..grid.rows.min(rows)).any(|y| grid.content_rect(0, y, cols, 1).contains("Thinking"));
        if probe && thinking {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
