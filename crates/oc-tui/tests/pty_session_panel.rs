//! PTY live check for the session panel (G-239/G-331 — the
//! session.panel slot + the PanelInput face, panel-host.tsx /
//! context/panel.tsx): the REAL [`oc_tui::app::run_with_size`] loop
//! runs over a pty against the `FakeLlm` server with the
//! `panelclaim.ts` fixture staged through `OPENCODE_CONFIG`:
//!
//! * `ctrl+x o` (the plugin's `ctx.keymap.layer` bind) runs
//!   `ctx.ui.panel.open("board")` — the deferred reply resolves true
//!   on a session route and the DOCKED pane renders the claim's rows
//!   (the right leg at the equal split, 160×30);
//! * `ctrl+x c` runs the owner-scoped `ctx.ui.panel.close()` — the
//!   pane drops and the frame reflows;
//! * a NARROW boot (80×30) is the FULLSCREEN takeover (canSplit =
//!   width > 80 — the session column hides, not an overlay).
//!
//! The exit chord is sent from the TEST thread after the driver ends
//! (either arm): a driver that fails early must not strand the app
//! inside the run loop for the whole teardown budget.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, exit_chord, open_first_session, pty_lock, run_tui, sandbox_env,
    standard_sessions, FakeLlm, HarnessHandle, LiveHarness, WORDMARK,
};

/// The docked pane's left edge at the 160-col test size (the equal
/// split — `defaultPaneWidth`).
const PANE_X: usize = 80;

fn wait_region(
    h: &HarnessHandle,
    needle: &str,
    x: usize,
    width: usize,
    timeout: Duration,
) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        for row in 0..grid.rows {
            let text = grid.content_rect(x, row, width, 1);
            if text.contains(needle) {
                return Some(row);
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

fn wait_region_gone(
    h: &HarnessHandle,
    needle: &str,
    x: usize,
    width: usize,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        let present =
            (0..grid.rows).any(|row| grid.content_rect(x, row, width, 1).contains(needle));
        if !present {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Stage the panelclaim fixture's config (the plugin phase's
/// `OPENCODE_CONFIG` staging).
fn stage_config(tag: &str) -> std::path::PathBuf {
    let sandbox = sandbox_env(tag);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oc-plugin-host/tests/fixtures/panelclaim.ts");
    let config_path = sandbox.join("opencode.json");
    std::fs::write(&config_path, format!(r#"{{"plugins": ["{}"]}}"#, fixture.display()))
        .expect("stage config");
    std::env::set_var("OPENCODE_CONFIG", &config_path);
    sandbox
}

/// The docked open → close cycle at 160×30.
#[test]
fn pty_session_panel_opens_docked_and_closes() {
    if !oc_plugin_host::PluginHost::detect().enabled() {
        eprintln!("bun unavailable; skipping the panel phase");
        return;
    }
    let _lock = pty_lock();
    let _sandbox = stage_config("panel-wide");

    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")?;
        // f9 — the plugin open bind (the deferred reply
        // resolves through the pump): the DOCKED pane renders the
        // claim's rows inside cols 80..160.
        driver.write(b"\x1b[20~"); // f9 — the plugin open bind
                                   // The plugin host may still be mid-boot when the session
                                   // opens; re-stroke the bind (idempotent open) until the pane
                                   // renders (bounded).
        let name_row = {
            let mut found = None;
            for _ in 0..6 {
                if let Some(row) =
                    wait_region(&driver, "name: board", PANE_X, 80, Duration::from_secs(5))
                {
                    found = Some(row);
                    break;
                }
                driver.write(b"\x1b[20~");
            }
            found
        }
        .ok_or_else(|| "the panel's claim rows never rendered".to_owned())?;
        let props_row =
            wait_region(&driver, "presentation: panel", PANE_X, 80, Duration::from_secs(5))
                .ok_or_else(|| "the presentation prop never rendered".to_owned())?;
        if props_row <= name_row {
            return Err("the props rows must sit under the title".to_owned());
        }
        // f8 — the owner-scoped close: the pane drops.
        driver.write(b"\x1b[19~"); // f8 — the plugin close bind
        if !wait_region_gone(&driver, "name: board", PANE_X, 80, Duration::from_secs(10)) {
            return Err("the owner-scoped close did not drop the pane".to_owned());
        }
        // Teardown proof: the exit chord leaves the alternate screen.
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || run_tui(&url, 160, 30, Duration::from_secs(60)));
    let result = span.join().expect("driver thread");
    // A failed driver must not strand the app: the exit chord goes out
    // from the test thread too (idempotent when the driver sent it).
    exit_chord(&mut harness.handle());
    teardown.join().expect("run thread");
    std::env::remove_var("OPENCODE_CONFIG");
    drop(harness);
    result.expect("the docked panel cycle");
}

/// The NARROW boot (80×30): the panel is the FULLSCREEN takeover —
/// the rows paint from the frame's left edge and the presentation
/// prop reflects the effective fullscreen.
#[test]
fn pty_session_panel_narrow_is_fullscreen_takeover() {
    if !oc_plugin_host::PluginHost::detect().enabled() {
        eprintln!("bun unavailable; skipping the panel phase");
        return;
    }
    let _lock = pty_lock();
    let _sandbox = stage_config("panel-narrow");

    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")?;
        driver.write(b"\x1b[20~"); // f9 — the plugin open bind
                                   // Fullscreen: the row starts near the LEFT edge (the session
                                   // column hides — `visible={!fullscreen()}`); re-stroke while
                                   // the plugin host finishes booting (bounded, idempotent).
        let row = {
            let mut found = None;
            for _ in 0..6 {
                if let Some(row) =
                    wait_region(&driver, "name: board", 0, 80, Duration::from_secs(5))
                {
                    found = Some(row);
                    break;
                }
                driver.write(b"\x1b[20~");
            }
            found
        }
        .ok_or_else(|| "the fullscreen panel never rendered".to_owned())?;
        let grid = driver.grid_now();
        let line = grid.row_text(row);
        let col = line
            .find("name: board")
            .ok_or_else(|| "the row disappeared between probes".to_owned())?;
        if col > 8 {
            return Err(format!("the fullscreen takeover must paint from the left edge: {line}"));
        }
        wait_region(&driver, "presentation: fullscreen", 0, 80, Duration::from_secs(5))
            .ok_or_else(|| "the effective presentation prop never rendered".to_owned())?;
        // Close the panel before the teardown chord (the chord's
        // leading ESCs would close it anyway — the explicit close
        // keeps the footprint deterministic).
        driver.write(b"\x1b[19~"); // f8 — the plugin close bind
        wait_region_gone(&driver, "name: board", 0, 80, Duration::from_secs(10))
            .then_some(())
            .ok_or_else(|| "the close did not drop the fullscreen panel".to_owned())?;
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || run_tui(&url, 80, 30, Duration::from_secs(60)));
    let result = span.join().expect("driver thread");
    exit_chord(&mut harness.handle());
    teardown.join().expect("run thread");
    std::env::remove_var("OPENCODE_CONFIG");
    drop(harness);
    result.expect("the narrow fullscreen takeover");
}
