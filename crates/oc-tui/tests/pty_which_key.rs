//! The which-key panel's live-PTY proof: the full port drives the
//! eleven-id family through real terminal key encoding — the toggle
//! pins the root listing, `layout.toggle` switches dock/overlay via
//! the kitty CSI-u chord, `group.next` cycles the tabs, and a pending
//! chord renders the flattened listing and closes on escape.
//! Field-level twins live in `src/app.rs` (`which_key_tests`).

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness,
    WORDMARK,
};

/// Wait until some row contains `needle`.
fn wait_row(h: &vt_harness::HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        for row in 0..grid.rows {
            if grid.row_text(row).contains(needle) {
                return Some(row);
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

fn wait_row_gone(h: &vt_harness::HarnessHandle, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        if (0..grid.rows).all(|row| !grid.row_text(row).contains(needle)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// ctrl+alt+k / ctrl+alt+shift+k / ctrl+alt+right as kitty CSI-u and
/// CSI-modifier sequences (the PTY's real wire encoding).
const TOGGLE: &[u8] = b"\x1b[107;7u";
const LAYOUT: &[u8] = b"\x1b[107;8u";
const GROUP_NEXT: &[u8] = b"\x1b[1;7C";

#[test]
fn pty_which_key_full_panel_surfaces() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-d31-which-key");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        // Pinned root listing: tabs + footer render at the screen bottom.
        driver.write(TOGGLE);
        wait_row(&driver, "toggle ctrl+alt+k", Duration::from_secs(10))
            .ok_or_else(|| "the pinned which-key panel never rendered".to_owned())?;
        wait_row(&driver, "dock ctrl+alt+shift+k", Duration::from_secs(5))
            .ok_or_else(|| "the footer never named the next layout".to_owned())?;
        // The default tab (Agent) renders its rows.
        wait_row(&driver, "Cycle model variants", Duration::from_secs(5))
            .ok_or_else(|| "the root listing never showed the Agent tab rows".to_owned())?;
        // layout.toggle (ctrl+alt+shift+k): dock -> overlay; the footer
        // names the NEW next layout.
        driver.write(LAYOUT);
        wait_row(&driver, "overlay ctrl+alt+shift+k", Duration::from_secs(5))
            .ok_or_else(|| "layout.toggle never switched to overlay".to_owned())?;
        // group.next (ctrl+alt+right): the selected tab's rows swap.
        driver.write(GROUP_NEXT);
        if !wait_row_gone(&driver, "Cycle model variants", Duration::from_secs(5)) {
            return Err("group.next never left the Agent tab".to_owned());
        }
        // The pending chord: the flattened listing replaces the tabs
        // and the panel never borrows the route's input.
        driver.write(b"\x18");
        wait_row(&driver, "List agents", Duration::from_secs(10))
            .ok_or_else(|| "the pending listing never rendered".to_owned())?;
        if wait_row(&driver, "toggle ctrl+alt+k", Duration::from_secs(1)).is_some() {
            return Err("the pending listing kept the pinned footer".to_owned());
        }
        driver.write(b"\x1b");
        if !wait_row_gone(&driver, "List agents", Duration::from_secs(5)) {
            return Err("escape never cleared the pending listing".to_owned());
        }
        // Unpin before the exit chord (the panel's pinned state must
        // not outlive the interaction).
        driver.write(TOGGLE);
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the which-key PTY cycle");
    let _ = sandbox;
}
