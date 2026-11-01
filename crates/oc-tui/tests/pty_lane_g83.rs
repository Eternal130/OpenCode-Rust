//! Lane F — G-83's live-PTY proof: the which-key panel (the bounded
//! port of the `@opentui/keymap` addon's pending-chord help surface)
//! renders on a prefix stroke and dismisses on completion/escape.
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

/// Press the leader (ctrl+x): the panel must render the `<leader>`
/// prefix and the chord's continuation rows (the table's labels),
/// then close on escape.
#[test]
fn pty_g83_which_key_panel_on_leader_prefix() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-f-g83");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        // The leader stroke: the pending-chord panel mounts with the
        // prefix riding the top border and the continuations listed.
        driver.write(b"\x18");
        let panel = wait_row(&driver, "<leader>", Duration::from_secs(10))
            .ok_or_else(|| "the which-key panel never rendered its prefix".to_owned())?;
        let grid = driver.grid_now();
        let screen: Vec<String> = (0..grid.rows).map(|row| grid.row_text(row)).collect();
        assert!(
            screen.iter().any(|row| row.contains("List agents")),
            "the <leader>a continuation lists with its label: {screen:?}"
        );
        assert!(
            screen.iter().any(|row| row.contains("Toggle sidebar")),
            "the <leader>b continuation lists with its label: {screen:?}"
        );
        let _ = panel;
        // Escape cancels the chord — the panel leaves the screen.
        driver.write(b"\x1b");
        let gone = {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut gone = false;
            while std::time::Instant::now() < deadline {
                let grid = driver.grid_now();
                if (0..grid.rows).all(|row| !grid.row_text(row).contains("<leader>")) {
                    gone = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            gone
        };
        assert!(gone, "escape never dismissed the which-key panel");
        // Chord completion: re-arm the leader, complete <leader>n —
        // the panel closes with the sequence (the session list
        // dialog's surface answers instead).
        driver.write(b"\x18");
        wait_row(&driver, "<leader>", Duration::from_secs(10))
            .ok_or_else(|| "the panel never re-armed".to_owned())?;
        driver.write(b"n");
        let closed = {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut closed = false;
            while std::time::Instant::now() < deadline {
                let grid = driver.grid_now();
                if (0..grid.rows).all(|row| !grid.row_text(row).contains("<leader>")) {
                    closed = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            closed
        };
        assert!(closed, "chord completion never dismissed the panel");
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
