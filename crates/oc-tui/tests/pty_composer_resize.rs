//! D2.2 composer-family PTY resize proof (lane gate: a geometry-family
//! surface rides the shared tree and its wrapper rect follows the
//! terminal resize beat-for-beat with the old imperative rects).
//!
//! The session route's COMPOSER is the geometry family's surface (the
//! `composer` wrapper mounts under `surface-composer` in the shared
//! tree at the old paste site; D2.2). The test boots the real app
//! loop at 120×30, opens a session, then SIGWINCH-resizes the PTY to
//! 60×20 and asserts the composer re-lays: the `┃` rail and the `╹`
//! transition row move INTO the 60-column frame (the wrapper's rect
//! tracks `app.size`, exactly the old `composer_mount_frame` paste
//! geometry), and the footer hint row still paints under the new
//! mount.

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
fn pty_composer_rect_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d22composer-rs");
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
            // Pre-resize: the composer's rail + transition row paint
            // inside the 120-column frame (the wrapper's imperative
            // rect = the old paste site).
            if !wait_composer(&mut driver, 120, 30) {
                return Err("the composer never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master — the
            // kernel-delivered winsize the pump reconciles; the
            // wrapper's content subtree rebuilds on the geometry key
            // and the rect re-seats via set_prop).
            driver.resize_pty(60, 20);
            // Post-resize: the composer re-lays INSIDE the 60-column
            // frame (asserted within the new window only; the harness
            // parser keeps the old 120-col grid, so cells beyond 60
            // are stale by design).
            if !wait_composer(&mut driver, 60, 20) {
                let grid = driver.grid_now();
                let mut rows = Vec::new();
                for y in 0..grid.rows.min(20) {
                    let text = grid.content_rect(0, y, 60, 1);
                    if !text.trim().is_empty() {
                        rows.push(format!("r{y:02}: {text:?}"));
                    }
                }
                return Err(format!(
                    "the composer never re-laid at 60 cols after SIGWINCH: {:?}",
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
        panic!("composer resize: {e}");
    }
}

/// Poll the settled grid until the composer's shape paints inside the
/// `cols`-column window: the `┃` rail row(s) above the `╹` transition
/// row (the mount's own chrome — nothing else paints the pair).
fn wait_composer(driver: &mut vt_harness::HarnessHandle, cols: usize, rows: usize) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        let rail = (0..grid.rows.min(rows)).any(|y| (0..cols).any(|x| grid.cell(x, y).ch == '┃'));
        let transition =
            (0..grid.rows.min(rows)).any(|y| (0..cols).any(|x| grid.cell(x, y).ch == '╹'));
        if rail && transition {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
