//! TUI-4 — the embedded `TerminalPane`'s PTY proofs
//! (`component/terminal-pane.tsx` + `context/session-terminals.tsx`):
//!
//! * `terminal_pane_roundtrip` — `<leader>t` mounts the pane: the
//!   capability probe (`server.info` `capabilities.persistentPty`),
//!   the registry create (`persistentPty.create`), the snapshot
//!   checkpoint fast-forward, the WS attach (connect-token → the
//!   §3 frame protocol) and the replay/live output all land in the
//!   pane's VT grid at the right half of the session frame;
//!   `<leader>up` unmounts it.
//! * `terminal_pane_input_frames` — the typed-input + resize control
//!   frames (input protocol 1): `#[ignore]`d pending a harness-level
//!   input-delivery investigation (the byte-proven encoder/frame
//!   contracts are unit-pinned in `terminal_pane.rs`).
//! * `terminal_select_tab` — `<leader>down` opens the composer's
//!   Terminals tab with the registry rows + `+ New terminal`.

#![allow(unsafe_code)] // pty(7) via libc, the shared harness

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, HarnessHandle, LiveHarness,
    TerminalFixture, WORDMARK,
};

/// The shared driver body: boot → session → the pane lifecycle.
fn drive_pane(driver: &mut HarnessHandle, fixture: &Arc<TerminalFixture>) -> Result<(), String> {
    // Boot: home wordmark → open the first session.
    if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    vt_harness::open_first_session(driver, b"Alpha")?;
    // No pane yet: the scripted terminal text is absent.
    if driver.wait_for(b"CHECKPOINT-OUT", Duration::from_millis(400)) {
        return Err("terminal output painted before the pane mounted".to_owned());
    }
    // `<leader>t` — toggle the pane on: the create + select + attach
    // chain lands the snapshot checkpoint, the WS replay and the live
    // output in the pane's VT grid.
    driver.write(b"\x18t");
    if !driver.wait_for(b"CHECKPOINT-OUT", Duration::from_secs(8)) {
        return Err("the snapshot checkpoint never painted".to_owned());
    }
    if !driver.wait_for(b"LIVE-OUT", Duration::from_secs(8)) {
        return Err("the WS live output never painted".to_owned());
    }
    // The REST chain: one create, at least one completed WS attach.
    if fixture.created.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        return Err("the toggle never created a terminal".to_owned());
    }
    if fixture.ws_attached.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        return Err("the WS attach never completed".to_owned());
    }
    // The pane occupies the RIGHT half — the terminal text sits past
    // the session column (x ≥ 60 at 120 cols).
    let grid = driver.grid_now();
    let pane_col = (60..120).find(|&x| (0..30).any(|y| grid.cell(x, y).ch == 'C'));
    if pane_col.is_none() {
        return Err("the terminal output painted outside the right pane".to_owned());
    }
    // `<leader>up` — close: the pane unmounts (the terminal text
    // leaves the grid after the next repaint).
    driver.write(b"\x18\x1b[A");
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    while std::time::Instant::now() < deadline {
        let grid = driver.grid_now();
        let still = (0..grid.rows).any(|y| grid.row_text(y).contains("CHECKPOINT-OUT"));
        if !still {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    Err("the terminal pane never closed".to_owned())
}

/// The pane-over-PTY harness: sandbox + fixture + driver thread.
fn run_pane_test<T: Send + 'static>(
    tag: &str,
    drive: impl FnOnce(&mut HarnessHandle, &Arc<TerminalFixture>) -> Result<T, String> + Send + 'static,
) -> T {
    let _lock = pty_lock();
    let _sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_terminals();
    let harness = LiveHarness::attach(120, 30);
    let result_slot: Arc<Mutex<Option<Result<T, String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let mut driver = harness.handle();
    let fixture = Arc::clone(&fake.terminals);
    std::thread::spawn(move || {
        let outcome = drive(&mut driver, &fixture);
        // Defocus the pane first (`<leader>left` — pane.focus.left):
        // a FOCUSED terminal eats ctrl+c exactly like v2's
        // intercept (terminal-pane.tsx:140-150), which would starve
        // the standard exit chord (app.exit = ctrl+c/ctrl+d).
        driver.write(b"\x18\x1b[D");
        std::thread::sleep(Duration::from_millis(150));
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, 120, 30, Duration::from_secs(90));
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = result_slot.lock().expect("slot").take();
    match outcome {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("{error}"),
        None => {
            let raw = harness.handle().raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]);
            panic!("the driver thread never reported (deadlock?); raw TAIL: {tail:?}");
        }
    }
}

/// The ledger repro: `terminal_pane_roundtrip` — the mount chain, the
/// output stream painted in the right pane, and the close.
#[test]
fn terminal_pane_roundtrip() {
    run_pane_test("tui4-pane", drive_pane);
}

/// The typed-input + resize control frames over the live socket —
/// byte-level roundtrips of `[1][cols u16be][rows u16be][data]` and
/// the kind-0 `[0][cols][rows]` control (input protocol 1,
/// `handlers/persistent_pty.rs` §3).
#[test]
fn terminal_pane_input_frames() {
    run_pane_test("tui4-input", |driver, fixture| {
        if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        vt_harness::open_first_session(driver, b"Alpha")?;
        driver.write(b"\x18t");
        if !driver.wait_for(b"LIVE-OUT", Duration::from_secs(8)) {
            return Err("the WS live output never painted".to_owned());
        }
        // Typed keys travel as interaction frames (the pane is focused
        // on mount — session-terminals' autoFocus).
        driver.write(b"hi");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let frames = fixture.input_frames.lock().expect("frames").clone();
            let typed: Vec<Vec<u8>> =
                frames.iter().filter(|frame| !frame.is_empty() && frame[0] == 1).cloned().collect();
            if typed.len() >= 2
                && typed[typed.len() - 2].ends_with(b"h")
                && typed[typed.len() - 1].ends_with(b"i")
                && typed.iter().all(|frame| frame.len() >= 5 && frame[1] == 0 && frame[3] == 0)
            {
                break;
            }
            if std::time::Instant::now() > deadline {
                return Err(format!("typed keys did not reach the pty framed: {frames:?}"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // A pane resize emits the kind-0 control frame with the new
        // size (the box's onSizeChange → interact()).
        let before = fixture.input_frames.lock().expect("frames").len();
        driver.resize_pty(100, 28);
        let deadline = std::time::Instant::now() + Duration::from_secs(6);
        let mut control = None;
        while std::time::Instant::now() < deadline {
            let frames = fixture.input_frames.lock().expect("frames").clone();
            if let Some(frame) =
                frames[before.min(frames.len())..].iter().find(|frame| frame[0] == 0)
            {
                control = Some(frame.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        match control {
            Some(frame) if frame.len() >= 5 => {
                let cols = u16::from_be_bytes([frame[1], frame[2]]);
                let rows = u16::from_be_bytes([frame[3], frame[4]]);
                // The pane spans the right half minus the box padding
                // (left 1): 100/2 - 1 = 49 cols max.
                if cols == 0 || rows == 0 || cols > 50 || rows > 28 {
                    return Err(format!("control frame size {cols}x{rows} out of range"));
                }
            }
            other => return Err(format!("no resize control frame arrived: {other:?}")),
        }
        Ok(())
    });
}

/// The plain-list slice: `<leader>down` (`terminal.select`) opens the
/// composer's Terminals tab with the registry rows + the
/// `+ New terminal` slot.
#[test]
fn terminal_select_tab() {
    run_pane_test("tui4-select", |driver, _fixture| {
        if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        vt_harness::open_first_session(driver, b"Alpha")?;
        // `<leader>down` opens the picker on the Terminals tab.
        driver.write(b"\x18\x1b[B");
        if !driver.wait_for(b"+ New terminal", Duration::from_secs(8)) {
            return Err("the Terminals tab never listed the add slot".to_owned());
        }
        Ok(())
    });
}
