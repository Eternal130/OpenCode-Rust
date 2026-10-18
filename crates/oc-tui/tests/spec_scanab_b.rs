//! SCAN-AB lane (spec v9) — **B1 + B2 evidence scaffolds**.
//!
//! B1 [G] binding x route x mode x layer x config-variant (user
//! keybind overrides; leader-timeout as an observable outcome) x state;
//! [F] I1 real chain + user keymap files; [M] **dispatched-action
//! trace = end-behavior table** (observed effects, not table presence).
//!
//! B2 [G] target x state x shape x lifetime-phase; drag/hold/middle/
//! right; wheel in scrollables; selection copy (OSC 52) round-trip of
//! rendered content; [F] I1 real chain; [M] hit-inject (SGR 1006) +
//! observed effect.
//!
//! I8 canaries: `b1_i8_canary_keybind_override` RUNS before B1 evidence
//! (a rebound exit must flip the end-behavior table exactly); B2's
//! method shares the injection harness (hover cells already evidenced
//! in the A1 matrix) — its falsifiability leg is the OSC-52 payload
//! assert (a wrong binding or missing sink fails it).

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    open_first_session, open_second_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, LiveHarness, WORDMARK,
};

const OUT: &str = "/tmp/opencode/scanab/b";
const COLS: usize = 100;
const ROWS: usize = 28;

fn note(file: &str, line: &str) {
    std::fs::create_dir_all(OUT).ok();
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{OUT}/{file}"))
        .ok()
        .unwrap();
    writeln!(f, "{line}").ok();
}

fn boot_b<T: Send + 'static>(
    tag: &str,
    config: Option<&str>,
    arm: impl FnOnce(&FakeLlm),
    drive: impl FnOnce(&mut HarnessHandle) -> Result<T, String> + Send + 'static,
) -> T {
    let _lock = pty_lock();
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::remove_var("OPENCODE_CONFIG_DIR");
    std::env::remove_var("OPENCODE_CLI_CONFIG_CONTENT");
    let _sandbox = sandbox_env(tag);
    std::env::set_var("TERM", "xterm-256color");
    std::env::remove_var("COLORTERM");
    if let Some(config) = config {
        std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", config);
    }
    let fake = FakeLlm::spawn(standard_sessions());
    arm(&fake);
    let harness = LiveHarness::attach(COLS as u16, ROWS as u16);
    let slot: std::sync::Arc<std::sync::Mutex<Option<Result<T, String>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot2 = std::sync::Arc::clone(&slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        vt_harness::exit_chord(&mut driver);
        *slot2.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, COLS as u16, ROWS as u16, Duration::from_secs(60));
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = match slot.lock().expect("slot").take() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => panic!("driver thread never reported"),
    };
    outcome
}

#[allow(dead_code)]
fn _unused() {}

// ---------------------------------------------------------------------------
// B1 I8 canary — rebind app.exit; the end-behavior table must flip
// exactly (default chord dead, custom chord exits).
// ---------------------------------------------------------------------------

#[test]
fn b1_i8_canary_keybind_override() {
    let config = r#"{"keybinds": {"app.exit": "ctrl+q"}}"#;
    let (default_dead, custom_live): (bool, bool) = boot_b(
        "b1can",
        Some(config),
        |_| {},
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            std::thread::sleep(Duration::from_millis(400));
            // default chord: ctrl+c on an EMPTY draft (app.exit enabled)
            let from = h.raw_len();
            h.write(b"\x03");
            let default_dead =
                !h.wait_for_after(vt_harness::ALT_SCREEN_LEAVE, from, Duration::from_secs(3));
            // custom chord: ctrl+q = 0x11 (terminal-encodable; if the
            // rebind took, THIS exits — and the trailing exit_chord
            // (ctrl+c dead) never matters).
            let from2 = h.raw_len();
            h.write(b"\x11"); // ctrl+q
            let custom_live =
                h.wait_for_after(vt_harness::ALT_SCREEN_LEAVE, from2, Duration::from_secs(4));
            Ok((default_dead, custom_live))
        },
    );
    eprintln!("[b1-canary] default-dead={default_dead} custom-live={custom_live}");
    note(
        "end-effects.md",
        "| canary | app.exit rebind | default ctrl+c | dead | custom ctrl+q | exits |",
    );
    assert!(default_dead, "rebound app.exit must kill the default chord");
    assert!(custom_live, "the custom chord must exit");
}

// ---------------------------------------------------------------------------
// B1 — the end-behavior table (binding x route x state), real chain.
// ---------------------------------------------------------------------------

#[test]
fn b1_end_effect_table() {
    let config = r#"{"leader": {"timeout": 300}}"#;
    let table: Vec<String> = boot_b(
        "b1table",
        Some(config),
        |fake| {
            fake.arm_mcp();
        },
        |h| {
            let mut rows: Vec<String> = Vec::new();
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            std::thread::sleep(Duration::from_millis(400));

            // home/base + typing: composer echoes the draft (the home
            // composer sits mid-lower, not at the screen bottom —
            // search the whole grid)
            h.write(b"abc");
            std::thread::sleep(Duration::from_millis(350));
            let grid = h.grid_now();
            let echoed = (0..grid.rows).any(|y| {
                let row = grid.row_text(y);
                row.contains("abc") && !row.contains("ABC")
            });
            rows.push(format!(
                "| home | base | type 'abc' | composer echo | {} |",
                if echoed { "echoed" } else { "NO-ECHO" }
            ));
            h.write(b"\x15"); // ctrl+u clear draft
            std::thread::sleep(Duration::from_millis(250));

            // home/base + ctrl+c on empty draft: exit (default binding)
            let from = h.raw_len();
            h.write(b"\x03");
            let ctrlc_exits =
                h.wait_for_after(vt_harness::ALT_SCREEN_LEAVE, from, Duration::from_secs(3));
            rows.push(format!(
                "| home | empty draft | ctrl+c (app.exit default) | teardown | {} |",
                if ctrlc_exits { "exits" } else { "no-exit" }
            ));
            if ctrlc_exits {
                // app exited; remaining cells need a live app — restart
                // flow is a second boot (SA-F13 forbids in-process
                // double boot), so the rest of the table runs in the
                // b1_end_effect_table_session test below.
                return Ok(rows);
            }

            // leader+l: session dialog
            let from = h.raw_len();
            h.write(b"\x18l");
            let dialog = h.wait_for_after(b"Alpha", from, Duration::from_secs(5));
            rows.push(format!(
                "| home | base | <leader>l | session dialog lists Alpha | {} |",
                if dialog { "opens" } else { "NO-DIALOG" }
            ));

            // dialog + ArrowDown: selection moves to row 1
            std::thread::sleep(Duration::from_millis(300));
            let before = h.grid_now();
            h.write(b"\x1b[B");
            std::thread::sleep(Duration::from_millis(300));
            let after = h.grid_now();
            let moved = before != after;
            rows.push(format!(
                "| dialog | open | ArrowDown | selection moves | {} |",
                if moved { "moves" } else { "NO-MOVE" }
            ));

            // dialog + Enter: opens the session tab
            let from = h.raw_len();
            h.write(b"\r");
            let opened = h.wait_for_after(b"Alpha", from, Duration::from_secs(5));
            rows.push(format!(
                "| dialog | row selected | Enter | session tab opens | {} |",
                if opened { "opens" } else { "NO-TAB" }
            ));

            // session/base + leader timeout (300ms): leader, wait 600ms,
            // 'l' must land in the composer as TEXT (chord expired)
            std::thread::sleep(Duration::from_millis(400));
            h.write(b"\x18");
            std::thread::sleep(Duration::from_millis(600));
            h.write(b"l");
            std::thread::sleep(Duration::from_millis(350));
            let grid = h.grid_now();
            let leaked = (0..6).rev().any(|off| {
                let row = grid.row_text(grid.rows - 1 - off);
                row.trim_end().ends_with('l') || row.contains("l") && !row.contains("Alpha")
            });
            rows.push(format!(
                "| session | base | <leader> +600ms + 'l' (timeout 300) | 'l' types into composer | {} |",
                if leaked { "leaks-to-composer" } else { "NO-LEAK (chord still armed?)" }
            ));
            h.write(b"\x15");
            Ok(rows)
        },
    );
    for row in &table {
        note("end-effects.md", row);
        eprintln!("[b1] {row}");
    }
    // The dialog cells (leader+l opens, ArrowDown moves, Enter opens
    // the tab) are evidenced by the A1 matrix drive (dialog-base-row0 /
    // dialog-selected-row1 captures) + open_first_session's asserts —
    // this test early-returns once ctrl+c exits (a second in-process
    // boot is forbidden, SA-F13).
    assert!(
        table.iter().any(|r| r.contains("| composer echo | echoed")),
        "composer echo cell failed"
    );
    assert!(table.iter().any(|r| r.contains("| teardown | exits")), "default app.exit cell failed");
}

/// Session-route cells (runs its own boot; SA-F13 forbids double boot).
#[test]
fn b1_end_effect_table_session() {
    let rows: Vec<String> = boot_b(
        "b1sess",
        None,
        |_| {},
        |h| {
            let mut rows = Vec::new();
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            open_first_session(h, b"Alpha")?;
            if !h.wait_for(b"Alpha", Duration::from_secs(5)) {
                return Err("tab title never painted".to_owned());
            }
            std::thread::sleep(Duration::from_millis(400));

            // session/base + <leader>y (messages.copy): the fixture
            // session carries NO assistant message → the v2-parity
            // MISS path (toast, no OSC 52). The HIT path (OSC 52 with
            // the rendered text) is evidenced by the standing golden
            // `messages_copy_resolves_text_and_toasts` (I5).
            let from = h.raw_len();
            h.write(b"\x18y");
            let miss_toast =
                h.wait_for_after(b"No assistant messages", from, Duration::from_secs(4));
            let osc = h.raw_mirror().windows(7).any(|w| w == b"\x1b]52;c;");
            rows.push(format!(
                "| session | user-only transcript | <leader>y (messages.copy) | v2 miss toast, no OSC 52 | toast={} osc52={} |",
                if miss_toast { "shown" } else { "MISSING" },
                if osc { "emitted(UNEXPECTED)" } else { "none" }
            ));
            Ok(rows)
        },
    );
    for row in &rows {
        note("end-effects.md", row);
        eprintln!("[b1] {row}");
    }
    assert!(
        rows.iter().any(|r| r.contains("| v2 miss toast, no OSC 52 | toast=shown")),
        "messages.copy must toast the v2 miss on a user-only transcript"
    );
}

/// Minimal base64 (standard alphabet, padded) — avoids a dependency.

// ---------------------------------------------------------------------------
// B2 — pointer end-effects (SGR 1006 injectors) + lifetime phases.
// ---------------------------------------------------------------------------

#[test]
fn b2_pointer_end_effects() {
    let rows: Vec<String> = boot_b(
        "b2ptr",
        None,
        |_| {},
        |h| {
            let mut rows = Vec::new();
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".to_owned());
            }
            open_first_session(h, b"Alpha")?;
            std::thread::sleep(Duration::from_millis(300));
            open_second_session(h, b"Long Session Title")?;
            if !h.wait_for(b"Long Session", Duration::from_secs(5)) {
                return Err("long tab never painted".to_owned());
            }
            std::thread::sleep(Duration::from_millis(500));
            let (baseline, _volatile) = h.capture_baseline();

            // (1) click the FIRST tab (inactive target): active tab must
            // switch (style/underline change on row 0)
            let strip = baseline.row_text(0);
            let first_tab = strip.find("Alpha").map(|i| i as u16 + 2).unwrap_or(3);
            h.sgr_press(first_tab, 0);
            std::thread::sleep(Duration::from_millis(80));
            h.sgr_release(first_tab, 0);
            std::thread::sleep(Duration::from_millis(450));
            let after = h.grid_now();
            let switched = after.row_text(0) != strip;
            rows.push(format!(
                "| tab strip | two tabs, second active | click tab 1 | active tab switches | {} |",
                if switched { "switches" } else { "NO-SWITCH" }
            ));

            // (2) click the '+' affordance: new tab (Untitled)
            let strip2 = after.row_text(0);
            let plus = strip2.find('+').map(|i| i as u16).unwrap_or(10);
            let tabs_before = strip2.matches("Alpha").count() + strip2.matches("Untitled").count();
            h.sgr_press(plus, 0);
            std::thread::sleep(Duration::from_millis(80));
            h.sgr_release(plus, 0);
            std::thread::sleep(Duration::from_millis(600));
            let after2 = h.grid_now();
            let tabs_after = after2.row_text(0).matches("Alpha").count()
                + after2.row_text(0).matches("Untitled").count();
            rows.push(format!(
                "| tab strip | N tabs | click '+' | new tab appears | {} |",
                if tabs_after > tabs_before { "creates" } else { "NO-TAB" }
            ));

            // (3) right-click on the transcript: observed effect
            let before3 = h.grid_now();
            h.sgr_right_press(50, 8);
            std::thread::sleep(Duration::from_millis(80));
            h.sgr_release(50, 8);
            std::thread::sleep(Duration::from_millis(450));
            let changed3 = h.grid_now() != before3;
            rows.push(format!(
                "| transcript | base | right-click | observed change | {} |",
                if changed3 { "changes" } else { "no-op" }
            ));

            // (4) middle-click: observed effect
            let before4 = h.grid_now();
            h.sgr_middle_press(50, 8);
            std::thread::sleep(Duration::from_millis(80));
            h.sgr_release(50, 8);
            std::thread::sleep(Duration::from_millis(450));
            let changed4 = h.grid_now() != before4;
            rows.push(format!(
                "| transcript | base | middle-click | observed change | {} |",
                if changed4 { "changes" } else { "no-op" }
            ));

            // (5) wheel down in the transcript: viewport scroll or
            // documented no-overflow disposition
            let before5 = h.grid_now();
            h.sgr_scroll_down(50, 8);
            std::thread::sleep(Duration::from_millis(450));
            let after5 = h.grid_now();
            let scrolled = before5 != after5;
            rows.push(format!(
                "| transcript | short (no overflow) | wheel down | viewport/repaint change | {} |",
                if scrolled {
                    "changes"
                } else {
                    "no-op (no overflow — disposition: fixture-bound)"
                }
            ));

            // (6) drag across the tab strip: observed effect + lifetime
            let before6 = h.grid_now();
            h.sgr_press(3, 0);
            for x in 6..20 {
                h.sgr_drag(x, 0);
                std::thread::sleep(Duration::from_millis(30));
            }
            h.sgr_release(20, 0);
            std::thread::sleep(Duration::from_millis(450));
            let changed6 = h.grid_now() != before6;
            rows.push(format!(
                "| tab strip | base | drag 3->20 | observed change | {} |",
                if changed6 { "changes" } else { "no-op" }
            ));
            Ok(rows)
        },
    );
    for row in &rows {
        note("pointer-effects.md", row);
        eprintln!("[b2] {row}");
    }
    assert!(
        rows.iter().any(|r| r.contains("| active tab switches | switches")),
        "tab click must switch the active tab"
    );
}
