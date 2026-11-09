//! D2.2 chrome-family PTY resize proof (lane gate: a geometry-family
//! surface rides the shared chrome tree and its wrapper rect follows
//! the terminal resize beat-for-beat with the old imperative rects).
//!
//! The session route's VERTICAL RAIL is a chrome-family surface (the
//! `tab_rail`/`tab_rail_tooltip` wrappers mount under `surface-chrome`
//! in the shared tree; D2.2). The test boots the real app loop at
//! 120×30 with `tabs.layout = "vertical"`, opens a session, then
//! SIGWINCH-resizes the PTY to 60×20 and asserts the rail re-lays:
//! the tab row still paints at the left edge inside the NEW width and
//! the composer border moves into the 60-column frame (the rail
//! wrapper's rect tracks `app.size`, exactly the old
//! `paint_live_rail`'s imperative geometry).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness,
    WORDMARK,
};

const RAIL_WIDTH: usize = 42;

#[test]
fn pty_chrome_rail_rect_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d22chrome-rs");
    std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", r#"{"tabs":{"layout":"vertical"}}"#);
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
            // Pre-resize: the rail's tab row at the left edge, within
            // the 42-wide rail (the chrome wrapper's imperative rect).
            let settled = wait_rail_row(&mut driver, 120);
            if !settled {
                return Err("the rail tab row never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master — the
            // kernel-delivered winsize the pump reconciles).
            driver.resize_pty(60, 20);
            // Post-resize: the FULL repaint re-lays the rail — the tab
            // row still starts at col 0 (asserted inside the new
            // 60-column window only; the harness parser keeps the old
            // 120-col grid, so cells beyond 60 are stale by design).
            if !wait_rail_row(&mut driver, 60) {
                let grid = driver.grid_now();
                let mut rows = Vec::new();
                for y in 0..grid.rows.min(20) {
                    let text = grid.content_rect(0, y, 60, 1);
                    if !text.trim().is_empty() {
                        rows.push(format!("r{y:02}: {text:?}"));
                    }
                }
                return Err(format!(
                    "the rail tab row never re-laid at 60 cols after SIGWINCH: {:?}",
                    rows
                ));
            }
            // The composer border re-flowed INTO the 60-column frame
            // (the content column is `width − rail`; at 120 the border
            // sits at x ≥ 44, at 60 it must sit strictly inside 60).
            let deadline = std::time::Instant::now() + Duration::from_secs(6);
            loop {
                let grid = driver.grid_now();
                let found = (0..20).any(|y| (0..60).any(|x| grid.cell(x, y).ch == '┃'));
                if found {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    return Err("the composer border never re-flowed into the 60-col frame".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("done") = Some(out);
    });
    run_tui(&fake.url, 120, 30, Duration::from_secs(60));
    fake.stop();
    std::env::remove_var("OPENCODE_CLI_CONFIG_CONTENT");
    let outcome = done.lock().expect("done").take();
    if let Some(Err(e)) = outcome {
        panic!("chrome rail resize: {e}");
    }
}

/// Poll the settled grid until the rail's tab row paints `" 1 Alpha"`
/// from col 0 (the first `wait_for`-style paint and the post-resize
/// re-layout share it).
fn wait_rail_row(driver: &mut vt_harness::HarnessHandle, cols: usize) -> bool {
    let width = RAIL_WIDTH.min(cols);
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        if (0..grid.rows).any(|y| grid.content_rect(0, y, width, 1).starts_with(" 1 Alpha")) {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
