//! PTY live check for the session sidebar (`session-frame.tsx` port):
//! the REAL [`oc_tui::app::run_with_size`] loop runs over a pty at a
//! WIDE size (160×30) against the `FakeLlm` server, driven by real
//! keystrokes and asserted on the parsed CELL GRID (the vt-stream
//! harness — positional claims need cells, not raw bytes):
//!
//! * entering a session at 160 docks the pane (`auto` > 120): the
//!   onboarding card renders inside columns 118..160 and the
//!   composer's `╹▀▀…` transition row stays inside the reflowed
//!   118-col column;
//! * `<leader>b` (`session.sidebar.toggle`, keybind.ts:95) HIDES it:
//!   the card region clears and the transition row spans the full
//!   width — the transcript column reflowed to 160;
//! * `<leader>b` again re-docks the pane;
//! * a second boot with a synthetic plugin (`sidebar_boot.ts`,
//!   staged through `OPENCODE_CONFIG`'s `plugins[]`) renders the
//!   plugin's `ctx.ui.sidebar.setSection` content in the live pane
//!   through the host's slot bridge.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, LiveHarness, WORDMARK,
};

/// The sidebar pane's left edge (`width − SESSION_SIDEBAR_WIDTH`,
/// ui/layout.ts:1) at the 160-col test size.
const PANE_X: usize = 118;

/// Wait until the grid's pane region contains `needle`, then return
/// the row it landed on.
fn wait_pane(h: &HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        for row in 0..grid.rows {
            let text = grid.content_rect(PANE_X, row, 42, 1);
            if text.contains(needle) {
                return Some(row);
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

/// Wait until NO grid row's pane region contains `needle`.
fn wait_pane_gone(h: &HarnessHandle, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        let present =
            (0..grid.rows).any(|row| grid.content_rect(PANE_X, row, 42, 1).contains(needle));
        if !present {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// The `╹▀▀…` composer transition row's ▀-run extent (the row that
/// starts with `╹`): the rightmost ▀ column — the transcript column's
/// width fingerprint.
fn transition_extent(h: &HarnessHandle) -> Option<usize> {
    let grid = h.grid_now();
    (0..grid.rows).find_map(|row| {
        let text = grid.row_text(row);
        let trimmed = text.trim_start_matches(' ');
        if !trimmed.starts_with('╹') {
            return None;
        }
        text.chars().enumerate().filter(|(_, ch)| *ch == '▀').map(|(col, _)| col).last()
    })
}

/// Wait until the transition extent satisfies `predicate`.
fn wait_transition(h: &HarnessHandle, timeout: Duration, predicate: fn(usize) -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Some(extent) = transition_extent(h) {
            if predicate(extent) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Phase driver: boot, open the first session, assert the pane
/// geometry, cycle the `<leader>b` toggle, exit cleanly.
fn drive_sidebar_cycle(h: &mut HarnessHandle) -> Result<(), String> {
    if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(h, b"Alpha")?;
    // (a) The docked pane: the onboarding card lands inside
    // cols 118..160 and the composer transition stays under 118.
    let card_row = wait_pane(h, "Getting started", Duration::from_secs(5))
        .ok_or_else(|| "the auto-shown pane's card never rendered".to_owned())?;
    let title_row = wait_pane(h, "Alpha", Duration::from_secs(2))
        .ok_or_else(|| "the pane's session title never rendered".to_owned())?;
    if title_row >= card_row {
        return Err(format!("the title ({title_row}) must sit above the card ({card_row})"));
    }
    if !wait_transition(h, Duration::from_secs(2), |extent| extent < PANE_X) {
        return Err("the docked composer transition exceeds the 118-col column".to_owned());
    }
    // (b) <leader>b: hide — the pane region clears and the transition
    // row reflows to the full width.
    h.write(b"\x18b");
    if !wait_pane_gone(h, "Getting started", Duration::from_secs(5)) {
        return Err("the pane did not hide on <leader>b".to_owned());
    }
    if !wait_transition(h, Duration::from_secs(5), |extent| extent > PANE_X + 10) {
        return Err("hiding did not reflow the transcript column to the full width".to_owned());
    }
    // (c) <leader>b again: the pane re-docks (fresh card + title).
    h.write(b"\x18b");
    wait_pane(h, "Getting started", Duration::from_secs(5))
        .ok_or_else(|| "the pane did not re-show on the second <leader>b".to_owned())?;
    if !wait_transition(h, Duration::from_secs(5), |extent| extent < PANE_X) {
        return Err("re-docking did not shrink the transcript column".to_owned());
    }
    // Teardown proof: the exit chord leaves the alternate screen.
    let from = h.raw_len();
    assert_exit_ack(h, from).map_err(|error| format!("teardown: {error}"))?;
    Ok(())
}

#[test]
fn pty_sidebar_toggles_and_carries_plugin_sections() {
    // ---- phase A: the auto pane + the <leader>b cycle at 160 -------
    let _lock = pty_lock();
    let _sandbox = sandbox_env("sidebar-a");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || drive_sidebar_cycle(&mut driver));
    let teardown = std::thread::spawn(move || {
        // The exit chord runs on the main thread's handle once the
        // driver finishes; keep the app bounded either way.
        run_tui(&fake.url, 160, 30, Duration::from_secs(45));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    result.expect("phase A: the sidebar cycle");
    drop(harness);
}

#[test]
fn pty_sidebar_plugin_section_renders_through_the_host() {
    if !oc_plugin_host::PluginHost::detect().enabled() {
        eprintln!("bun unavailable; skipping the plugin phase");
        return;
    }
    let _lock = pty_lock();
    let sandbox = sandbox_env("sidebar-plugin");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oc-plugin-host/tests/fixtures/sidebar_boot.ts");
    let config_path = sandbox.join("opencode.json");
    std::fs::write(&config_path, format!(r#"{{"plugins": ["{}"]}}"#, fixture.display()))
        .expect("stage config");
    std::env::set_var("OPENCODE_CONFIG", &config_path);

    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")?;
        // The plugin's setup-time section renders in the live pane
        // (the `sidebar.content` slot through the host's UI-request
        // drain): the section's rows land under the pane's title.
        let section_row = wait_pane(&driver, "build · running", Duration::from_secs(15))
            .ok_or_else(|| "the plugin section never rendered in the pane".to_owned())?;
        let title_row = wait_pane(&driver, "Alpha", Duration::from_secs(2))
            .ok_or_else(|| "the pane's session title never rendered".to_owned())?;
        if title_row >= section_row {
            return Err(format!(
                "the section ({section_row}) must sit under the title ({title_row})"
            ));
        }
        // Teardown proof: the exit chord leaves the alternate screen.
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || run_tui(&url, 160, 30, Duration::from_secs(45)));
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    std::env::remove_var("OPENCODE_CONFIG");
    drop(harness);
    result.expect("phase B");
}
