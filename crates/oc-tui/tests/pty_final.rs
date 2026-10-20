//! Lane FINAL PTY proofs — the stub-dispatch family's real surfaces,
//! asserted on the parsed CELL GRID against the real app loop over a
//! pty:
//!
//! * `/connect` (provider.connect) opens the REAL integration list
//!   over the BE-3 host (`integration.list`), the row submit opens
//!   the API-key prompt, the typed key POSTs `integration.connect.
//!   key`, and the result lands as the "Connected {name}" toast —
//!   the first-run connect flow end to end;
//! * `/settings` (opencode.settings) opens the Settings rows;
//! * `/share` (session.share) mirrors v2's unavailable toast.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
#![allow(clippy::too_many_lines)] // one linear scenario per proof

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK,
};

fn find_row(h: &vt_harness::HarnessHandle, needle: &str) -> Option<usize> {
    let grid = h.grid_now();
    (0..grid.rows).find(|row| grid.row_text(*row).contains(needle))
}

fn wait_row(h: &vt_harness::HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Some(row) = find_row(h, needle) {
            return Some(row);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

/// The slash-command path: type `/x` into the composer and submit.
fn slash(h: &mut vt_harness::HarnessHandle, command: &str) {
    h.write(format!("/{command}").as_bytes());
    std::thread::sleep(Duration::from_millis(150));
    h.write(b"\r");
}

/// BLOCKER #1: `/connect` → the real integration list → the API-key
/// prompt → the connect.key POST → the "Connected" toast. No "Not
/// wired in this build" row anywhere in the flow.
#[test]
fn pty_connect_flow_list_key_entry_connected() {
    let _guard = pty_lock();
    let sandbox = sandbox_env("final-connect");
    let server = FakeLlm::spawn(standard_sessions());
    server.arm_integrations();
    let url = server.url.clone();
    let harness = LiveHarness::attach(100, 30);
    let mut driver = harness.handle();
    std::thread::spawn(move || run_tui(&url, 100, 30, Duration::from_secs(90)));
    drop(sandbox);
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(20)), "the wordmark rendered");

    // 1. `/connect` opens the integration list (the refetched rows,
    //    not a stub).
    slash(&mut driver, "connect");
    assert!(
        wait_row(&driver, "Connect an integration", Duration::from_secs(8)).is_some(),
        "the integration list title"
    );
    let list_row = wait_row(&driver, "Anthropic", Duration::from_secs(8))
        .expect("the fetched integration row (integration.list resolved)");
    let _ = list_row;

    // 2. Enter on the row → the API-key prompt (the single key
    //    method opens directly — v2's selectMethod shortcut).
    driver.write(b"\r");
    assert!(
        wait_row(&driver, "API key", Duration::from_secs(5)).is_some(),
        "the API-key prompt mounted"
    );

    // 3. Type the key + submit → the connect.key POST resolves →
    //    the "Connected Anthropic" toast (the dialog cleared).
    driver.write(b"sk-pty-proof-key");
    std::thread::sleep(Duration::from_millis(120));
    driver.write(b"\r");
    assert!(
        wait_row(&driver, "Connected Anthropic", Duration::from_secs(8)).is_some(),
        "the connected toast landed (the key POST resolved)"
    );

    // No stub row survives anywhere on the grid.
    let grid = driver.grid_now();
    for row in 0..grid.rows {
        let text = grid.row_text(row);
        assert!(!text.contains("Not wired in this build"), "no stub text: {text}");
    }

    server.stop();
}

/// `/settings` (opencode.settings) opens the Settings rows (the
/// four settings this build reads + persists); `/share` mirrors
/// v2's unavailable toast.
#[test]
fn pty_settings_and_share() {
    let _guard = pty_lock();
    let sandbox = sandbox_env("final-settings");
    let server = FakeLlm::spawn(standard_sessions());
    let url = server.url.clone();
    let harness = LiveHarness::attach(100, 30);
    let mut driver = harness.handle();
    std::thread::spawn(move || run_tui(&url, 100, 30, Duration::from_secs(60)));
    drop(sandbox);
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(20)), "the wordmark rendered");

    slash(&mut driver, "settings");
    // TUI-23: the FULL table's initial window (the scroll viewport is
    // the pinned dialog geometry — "Animations" rides the first
    // window; "Tool grouping" now scrolls).
    assert!(
        wait_row(&driver, "Animations", Duration::from_secs(5)).is_some(),
        "the settings rows mounted"
    );

    // esc → the dialog closes (wait for the rows to clear) →
    // `/share` → v2's exact unavailable toast.
    driver.write(b"\x1b");
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut gone = false;
        while std::time::Instant::now() < deadline {
            if find_row(&driver, "Animations").is_none() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(gone, "the settings dialog closed");
    }
    // The share command through the PALETTE path (the default slash
    // table predates the session commands' wiring — golden-pinned;
    // the palette row reaches every command) → v2's exact
    // unavailable toast.
    driver.write(b"\x10"); // ctrl+p — the command palette
    std::thread::sleep(Duration::from_millis(400));
    driver.write(b"share");
    std::thread::sleep(Duration::from_millis(400));
    driver.write(b"\r");
    let found =
        wait_row(&driver, "Sharing is not implemented for V2 sessions yet", Duration::from_secs(5));
    if found.is_none() {
        let grid = driver.grid_now();
        let mut dump = String::new();
        for row in 0..grid.rows {
            let text = grid.row_text(row);
            if !text.trim().is_empty() {
                use std::fmt::Write as _;
                let _ = writeln!(dump, "[{row}] {text}");
            }
        }
        panic!("the share toast mirrored v2 — grid:\n{dump}");
    }

    server.stop();
}
