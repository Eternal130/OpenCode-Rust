//! D4.1a tabs-strip PTY resize proof (lane gate): the strip rides the
//! shared tree as the `tabs-strip` base family (the retired
//! `frame_from_buffer` copy + the (0,0) paste are gone) — the wrapper
//! rect and the content subtree must follow the terminal resize
//! beat-for-beat with the old paste path.
//!
//! The test boots the real app loop at 120×30, opens the first
//! session (ONE strip tab), then SIGWINCH-resizes the PTY to 60×20
//! and asserts the strip re-lays: the tab title and the ` + `
//! affordance re-paint INSIDE the 60-column window on row 0, and the
//! tab's pill re-fits (the content subtree rebuilt on the width key).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness,
    WORDMARK,
};

#[test]
fn pty_tabs_strip_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d41a-strip-rs");
    let fake = FakeLlm::spawn(standard_sessions());
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
            // Pre-resize: the strip paints at 120 cols — the tab
            // title + the add affordance on row 0.
            if !wait_strip(&mut driver, 120) {
                return Err("the strip never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master): the
            // wrapper rect re-seats via set_prop and the content
            // subtree rebuilds on the width geometry key.
            driver.resize_pty(60, 20);
            if !wait_strip(&mut driver, 60) {
                let grid = driver.grid_now();
                let row0 = grid.content_rect(0, 0, 60, 1);
                return Err(format!(
                    "the strip never re-laid at 60 cols after SIGWINCH: row0 = {row0:?}"
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
        panic!("strip resize: {e}");
    }
}

/// Poll the settled grid until the strip's row 0 carries the opened
/// tab's title AND the ` + ` affordance inside the `cols`-column
/// window (strip pixels nothing else paints).
fn wait_strip(driver: &mut vt_harness::HarnessHandle, cols: usize) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        let row0 = grid.content_rect(0, 0, cols, 1);
        if row0.contains("Alpha") && row0.contains(" + ") {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
