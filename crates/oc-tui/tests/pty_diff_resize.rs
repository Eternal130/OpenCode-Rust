//! D3.5 diff-family PTY resize proof: the fullscreen `diff` route
//! rides the shared tree's BASE family (`surface-diff`, skeleton index
//! 0), so its layout follows the terminal's SIGWINCH beat-for-beat —
//! the file tree appears/disappears at the 90-column breakpoint and
//! the patch column re-clamps to `width - 4` when the tree drops.
//!
//! The fixture (`/api/vcs/diff`, `FakeLlm::arm_vcs`) serves one
//! modified `src/lib.rs`. At 120×30 the tree pane renders the `▾ src`
//! / `│ ≡ file.txt` rows; after the SIGWINCH to 60×20 the tree is
//! below the breakpoint, so the viewer re-lays WITHOUT those rows
//! while the file header and the added patch row stay inside the
//! 60-column window.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK,
};

fn find_in_window(driver: &vt_harness::HarnessHandle, needle: &str, cols: usize) -> Option<usize> {
    let grid = driver.grid_now();
    (0..grid.rows).find(|row| grid.content_rect(0, *row, cols, 1).contains(needle))
}

fn wait_in_window(
    driver: &mut vt_harness::HarnessHandle,
    needle: &str,
    cols: usize,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if find_in_window(driver, needle, cols).is_some() {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[test]
fn pty_diff_viewer_relays_on_terminal_resize() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("d35diff-resize");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_vcs();
    let harness = LiveHarness::attach(120, 30);
    let mut driver = harness.handle();
    let done = Arc::new(Mutex::new(None::<Result<(), String>>));
    let slot = Arc::clone(&done);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            if !driver.wait_for(WORDMARK, Duration::from_secs(10)) {
                return Err("wordmark never rendered".into());
            }
            // Open /diff.
            driver.write(b"/diff");
            std::thread::sleep(Duration::from_millis(150));
            driver.write(b"\r");
            if !wait_in_window(&mut driver, "src/lib.rs", 120, Duration::from_secs(5)) {
                return Err("the diff file header never painted at 120".into());
            }
            // The tree pane (>=90 cols): its dir + file rows.
            let tree_before = find_in_window(&driver, "▾", 120).is_some()
                && find_in_window(&driver, "≡", 120).is_some();
            if !tree_before {
                let grid = driver.grid_now();
                let rows: Vec<String> =
                    (0..grid.rows).map(|y| grid.content_rect(0, y, 120, 1)).collect();
                return Err(format!("the tree pane never painted at 120: {rows:?}"));
            }
            if !wait_in_window(&mut driver, "let new = 1;", 120, Duration::from_secs(3)) {
                return Err("the added patch row never painted at 120".into());
            }
            // SIGWINCH 120×30 → 60×20: below FILE_TREE_BREAKPOINT (90)
            // the tree drops; the patch column re-clamps to width - 4.
            driver.resize_pty(60, 20);
            if !wait_in_window(&mut driver, "src/lib.rs", 60, Duration::from_secs(8)) {
                let grid = driver.grid_now();
                let rows: Vec<String> =
                    (0..grid.rows.min(20)).map(|y| grid.content_rect(0, y, 60, 1)).collect();
                return Err(format!("the diff header never re-laid at 60: {rows:?}"));
            }
            if find_in_window(&driver, "▾", 60).is_some()
                || find_in_window(&driver, "≡", 60).is_some()
            {
                return Err("the file tree must drop below the 90-col breakpoint".into());
            }
            if !wait_in_window(&mut driver, "let new = 1;", 60, Duration::from_secs(3)) {
                return Err("the patch body never re-laid inside the 60-column window".into());
            }
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("done") = Some(out);
    });
    run_tui(&fake.url, 120, 30, Duration::from_secs(90));
    fake.stop();
    let outcome = done.lock().expect("done").take();
    if let Some(Err(error)) = outcome {
        panic!("diff resize: {error}");
    }
}
