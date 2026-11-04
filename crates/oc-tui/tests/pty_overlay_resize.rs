//! D2.2 overlays-family PTY resize proof (lane gate: the renderer
//! console strip rides the shared tree and its `surface-console`
//! wrapper rect — full-width top rows, the per-width line clip —
//! follows the terminal resize beat-for-beat with the old
//! imperative paste geometry).
//!
//! The console is a full-width strip at row 0 (the family's lowest
//! seat, skeleton index 6): the bordered box with the " console "
//! title riding the top border. The test boots the real app loop at
//! 120×30, opens the console through the command palette
//! (`app.console`), then SIGWINCH-resizes the PTY to 60×20 and
//! asserts the strip re-lays: the title + top border re-paint
//! INSIDE the 60-column window, the wrapper's rect re-seating via
//! `set_prop` (never a remount) and the content subtree rebuilding
//! on the geometry key (width 120 → 60).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK};

#[test]
fn pty_console_strip_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d22overlays-rs");
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
            // OPEN: ctrl+p mounts the command palette; typing
            // "console" filters to the `app.console` toggle, enter
            // dispatches it (toggles the strip + clears the dialog).
            driver.write(b"\x10");
            if !driver.wait_for(b"Commands", Duration::from_secs(8)) {
                return Err("the palette never opened".into());
            }
            driver.write(b"console\r");
            if !wait_console(&mut driver, 120) {
                return Err("the console strip never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master — the
            // kernel-delivered winsize the pump reconciles; the
            // wrapper's rect re-seats via set_prop, the box re-laying
            // full-width at 60 with the content subtree rebuilding on
            // the geometry key).
            driver.resize_pty(60, 20);
            // Post-resize: the strip re-lays INSIDE the 60-column
            // window (the bordered box is the console's own pixels;
            // asserted within the new window only, the harness
            // parser keeps the old grid beyond).
            if !wait_console(&mut driver, 60) {
                let grid = driver.grid_now();
                let mut rows = Vec::new();
                for y in 0..grid.rows.min(20) {
                    let text = grid.content_rect(0, y, 60, 1);
                    if !text.trim().is_empty() {
                        rows.push(format!("r{y:02}: {text:?}"));
                    }
                }
                return Err(format!(
                    "the console strip never re-laid at 60 cols after SIGWINCH: {:?}",
                    rows
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
        panic!("console resize: {e}");
    }
}

/// Poll the settled grid until the console strip paints inside the
/// `cols`-column window: the top border closes (`┌` at column 0,
/// `┐` at the window's last column) with the " console " title
/// riding it — console pixels nothing else paints at row 0.
fn wait_console(driver: &mut vt_harness::HarnessHandle, cols: usize) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        let row0 = grid.content_rect(0, 0, cols, 1);
        let closed = row0.starts_with('┌') && row0.trim_end().ends_with('┐');
        if closed && row0.contains(" console ") {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
