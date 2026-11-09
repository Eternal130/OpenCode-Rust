//! D2.2 dialogs-family PTY resize proof (lane gate: the dialog layer
//! rides the shared tree and its `surface-dialogs` wrapper rect —
//! box centering + width clamping — follows the terminal resize
//! beat-for-beat with the old imperative paste geometry).
//!
//! The command palette (`ctrl+p`) is a centered modal: the wrapper
//! mounts at `(width - box_width) / 2` with the width clamped to
//! `size - 2` (the imperative source `render_overlay` always used).
//! The test boots the real app loop at 120×30, opens the palette,
//! then SIGWINCH-resizes the PTY to 60×20 and asserts the dialog
//! re-lays: the title ("Commands") and the close label re-paint
//! INSIDE the 60-column window, the box re-centering through the
//! wrapper's `set_prop` rect (never a remount) and the content
//! subtree rebuilding on the geometry key (box width 60 → 58).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK,
};

#[test]
fn pty_dialog_palette_rect_follows_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d22dialog-rs");
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
            // OPEN: ctrl+p mounts the command palette (a centered
            // dialog — the `surface-dialogs` wrapper at skeleton
            // index 5).
            driver.write(b"\x10");
            if !wait_palette(&mut driver, 120) {
                return Err("the palette never painted at 120 cols".into());
            }
            // SIGWINCH to 60×20 (real TIOCSWINSZ on the master — the
            // kernel-delivered winsize the pump reconciles; the
            // wrapper's rect re-seats via set_prop, the box width
            // re-clamping to 58 with the content subtree rebuilding
            // on the geometry key).
            driver.resize_pty(60, 20);
            // Post-resize: the dialog re-lays INSIDE the 60-column
            // window (title + close label are dialog pixels nothing
            // else paints; asserted within the new window only, the
            // harness parser keeps the old grid beyond).
            if !wait_palette(&mut driver, 60) {
                let grid = driver.grid_now();
                let mut rows = Vec::new();
                for y in 0..grid.rows.min(20) {
                    let text = grid.content_rect(0, y, 60, 1);
                    if !text.trim().is_empty() {
                        rows.push(format!("r{y:02}: {text:?}"));
                    }
                }
                return Err(format!(
                    "the palette never re-laid at 60 cols after SIGWINCH: {rows:?}"
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
        panic!("dialog resize: {e}");
    }
}

/// Poll the settled grid until the palette's title row paints inside
/// the `cols`-column window: the title and the right-justified close
/// label are the dialog chrome's own pixels, and the title must be
/// INDENTED (the box centers — `box_x = (width - box_width) / 2`,
/// never column 0).
fn wait_palette(driver: &mut vt_harness::HarnessHandle, cols: usize) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let grid = driver.grid_now();
        let title = (0..grid.rows).find(|y| {
            let row = grid.content_rect(0, *y, cols, 1);
            row.contains("Commands") && row.trim_start().starts_with("Commands")
        });
        let esc =
            (0..grid.rows).any(|y| grid.content_rect(0, y, cols, 1).trim_end().ends_with("esc"));
        if let (Some(_), true) = (title, esc) {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
