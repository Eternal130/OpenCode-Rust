//! D2.2 footer-family PTY resize proof (lane gate: the geometry
//! family's blocker body rides the shared tree and its wrapper rect
//! follows the terminal resize beat-for-beat with the old imperative
//! paste site).
//!
//! The session route's PERMISSION body is a geometry-reactive footer
//! surface (the `footer` wrapper mounts under `surface-footer` in
//! the shared tree at the old `(0, size.1 - mount_height)` paste
//! site; D2.2). The test boots the real app loop at 120×30, opens a
//! session, submits a turn whose SSE script fires a
//! `permission.asked` (the blocker that holds the footer), then
//! SIGWINCH-resizes the PTY to 60×20 and asserts the body re-lays:
//! the title ("Permission required") and the controls row
//! ("Allow once") re-paint INSIDE the 60-column window and INSIDE
//! the bottom 16 rows (the wrapper's rect tracks `app.size`, the
//! content subtree rebuilding on the geometry key).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness,
    ScriptEvent, WORDMARK,
};

#[test]
fn pty_footer_permission_rect_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d22footer-rs");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(permission_ask_script("ses_alpha"));
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
            // Submit: the armed SSE script fires the permission ask
            // (the blocker that holds the footer family's wrapper).
            driver.write(b"read the resize probe\r");
            // Pre-resize: the body paints inside the 120-column frame
            // (the wrapper's imperative rect = the old paste site).
            if !wait_body(&mut driver, 120, 30) {
                return Err("the permission body never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master — the
            // kernel-delivered winsize the pump reconciles; the
            // wrapper's rect re-seats via set_prop and the content
            // subtree rebuilds on the geometry key).
            driver.resize_pty(60, 20);
            // Post-resize: the body re-lays INSIDE the 60-column
            // window and the bottom 16 rows of the 20-row screen
            // (mount rows 4..20; asserted within the new window
            // only, the harness parser keeps the old grid beyond).
            if !wait_body(&mut driver, 60, 20) {
                let grid = driver.grid_now();
                let mut rows = Vec::new();
                for y in 0..grid.rows.min(20) {
                    let text = grid.content_rect(0, y, 60, 1);
                    if !text.trim().is_empty() {
                        rows.push(format!("r{y:02}: {text:?}"));
                    }
                }
                return Err(format!(
                    "the permission body never re-laid at 60 cols after SIGWINCH: {:?}",
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
        panic!("footer resize: {e}");
    }
}

/// The ask script: on the first prompt POST (the submitted turn), a
/// single `permission.asked` for a read of `src/index.ts`.
fn permission_ask_script(session: &str) -> Vec<ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    vec![ScriptEvent {
        delay_ms: 50,
        body: format!(
            r#"{{"id":"evt_ask","created":{now_ms},"type":"permission.asked","data":{{"id":"per_d22","sessionID":"{session}","action":"read","resources":["src/index.ts"],"save":[]}}}}"#
        ),
    }]
}

/// Poll the settled grid until the permission body paints inside the
/// `cols`-column window and the footer family's bottom-docked mount
/// rows: the title and the controls row are footer-body pixels
/// nothing else paints.
fn wait_body(driver: &mut vt_harness::HarnessHandle, cols: usize, rows: usize) -> bool {
    let mount_top = rows.saturating_sub(16);
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        let title = (mount_top..grid.rows.min(rows))
            .any(|y| grid.content_rect(0, y, cols, 1).contains("Permission required"));
        let controls = (mount_top..grid.rows.min(rows))
            .any(|y| grid.content_rect(0, y, cols, 1).contains("Allow once"));
        if title && controls {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
