//! Dogfood lane 3 — the three user-reported fixes, live-PTY proofs:
//!
//! * **onboarding dismiss + persistence**: the getting-started card's
//!   `✕` is a REAL pointer target — SGR hover + release dismisses the
//!   card, the flip lands in `<state>/<ch>/tui/getting-started.json`
//!   (v2 footer.tsx:11-51's storage doc), and a SECOND boot in the
//!   same sandbox never shows the card (the boot read).
//! * **mouse-mode selection semantics**: the emitted byte stream
//!   carries EXACTLY v2's four-mode set (`?1000 ?1002 ?1003 ?1006`,
//!   no `?1015`) under the default config, and NOTHING under
//!   `"mouse": false` — the terminal keeps its native selection.
//! * **`--prompt`**: booting with the flag auto-submits the first
//!   turn through the stub flow (the prompt POST + the scripted
//!   reply render with ZERO keystrokes).

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, run_tui_boot, sandbox_env,
    standard_sessions, streaming_thinking_script, FakeLlm, LiveHarness, WORDMARK,
};

/// The sidebar pane's left edge at the 160-col test size
/// (`width − SESSION_SIDEBAR_WIDTH`).
const PANE_X: usize = 118;
/// The ✕ dismiss cell's column-local x inside the pane
/// (`stack_x + CARD_TEXT_WIDTH − 1` = 6 + 32 − 1).
const DISMISS_LOCAL_X: usize = 37;

fn wait_pane_row(h: &vt_harness::HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
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

fn pane_gone(h: &vt_harness::HarnessHandle, needle: &str, timeout: Duration) -> bool {
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

/// Item 1 — the PTY cycle: hover the `✕`, release, the card
/// disappears and the dismissal lands in the `getting-started`
/// document (the restart read is covered unit-side — see the note at
/// the end of the test).
#[test]
fn pty_onboarding_dismiss_writes_the_doc() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("dogfood3-onboarding");

    // ---- boot 1: dismiss through the pointer ----------------------
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        let card_row = wait_pane_row(&driver, "Getting started", Duration::from_secs(5))
            .ok_or_else(|| "the onboarding card never rendered".to_owned())?;
        // The ✕ sits on the card's HEADER row (title left, ✕ right
        // edge) — the same row the title text landed on.
        let dismiss_x = PANE_X + DISMISS_LOCAL_X;
        let cell = driver.grid_now().cell(dismiss_x, card_row);
        if cell.ch != '✕' {
            return Err(format!(
                "the dismiss target is not the ✕ glyph at ({dismiss_x}, {card_row}): {:?}",
                cell.ch
            ));
        }
        let (x, row) = (
            u16::try_from(dismiss_x).unwrap_or(u16::MAX),
            u16::try_from(card_row).unwrap_or(u16::MAX),
        );
        driver.sgr_motion(x, row); // hover
        driver.sgr_press(x, row);
        driver.sgr_release(x, row); // v2 acts on mouseUp
        if !pane_gone(&driver, "Getting started", Duration::from_secs(5)) {
            return Err("the card did not dismiss on the ✕ release".to_owned());
        }
        if !pane_gone(&driver, "Connect provider", Duration::from_secs(1)) {
            return Err("the whole card (connect row included) left the pane".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let teardown = std::thread::spawn(move || {
        run_tui(&fake.url, 160, 30, Duration::from_secs(45));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    result.expect("boot 1: the dismiss cycle");
    drop(harness);

    // The flip persisted: v2's storage doc shape
    // (`<state>/<ch>/tui/getting-started.json`, `{"dismissed":true}`).
    let doc = sandbox.join(".local/state/opencode-rust/stable/tui/getting-started.json");
    let body = std::fs::read_to_string(&doc)
        .unwrap_or_else(|error| panic!("the getting-started doc at {}: {error}", doc.display()));
    assert!(body.contains("\"dismissed\":true"), "the doc carries the v2 dismissal shape: {body}");

    // Boot 2 (the restart read) is covered unit-side: the doc this
    // test wrote is the exact input `run_with_size`'s boot read feeds
    // `restore_sidebar_onboarding` (session.rs's gate test proves the
    // hidden card from a restored dismissal) — a second live boot in
    // ONE process cannot re-arm crossterm's global input source.
}

/// Item 2 — default config (mouse true): the boot pushes EXACTLY
/// v2's four-mode set and teardown pops it (no `?1015` anywhere).
#[test]
fn pty_mouse_modes_default_pushes_v2_set() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("dogfood3-mouse-on");
    let enable = b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";
    let disable = b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(100, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        let raw = driver.raw_mirror();
        assert!(
            raw.windows(enable.len()).any(|window| window == enable),
            "the default boot pushes v2's exact four-mode set"
        );
        assert!(
            !raw.windows(6).any(|window| window == b"?1015h"),
            "the urxvt ?1015 mode never rides the stream (auditC P9)"
        );
        assert!(
            raw.windows(disable.len()).any(|window| window == disable),
            "teardown pops the same set in reverse"
        );
        Ok(())
    });
    let teardown = std::thread::spawn(move || {
        run_tui(&fake.url, 100, 30, Duration::from_secs(45));
    });
    span.join().expect("driver thread").expect("default boot cycle");
    teardown.join().expect("run thread");
    drop(harness);
}

/// Item 2 (the escape hatch) — `"mouse": false`: NO mode push at all,
/// so the terminal keeps its native text selection (shift-selection
/// pass-through without any capture to bypass). Its own #[test] (its
/// own process): crossterm's input source is process-global, a second
/// boot in one process would be deaf.
#[test]
fn pty_mouse_modes_off_pushes_nothing() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("dogfood3-mouse-off");
    // The config root is `<XDG_CONFIG_HOME>/<APP_NAME>` — opencode-rust.
    let config_dir = sandbox.join(".config/opencode-rust");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("cli.json"), r#"{"mouse": false}"#).expect("cli.json");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(100, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        let from = driver.raw_len();
        if let Err(error) = assert_exit_ack(&mut driver, from) {
            let raw = driver.raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]).into_owned();
            return Err(format!("teardown: {error}; raw tail: {tail:?}"));
        }
        let raw = driver.raw_mirror();
        for mode in ["?1000h", "?1002h", "?1003h", "?1006h", "?1015h"] {
            assert!(
                !raw.windows(mode.len()).any(|window| window == mode.as_bytes()),
                "mouse:false must not push {mode} — the terminal keeps native selection"
            );
        }
        Ok(())
    });
    let teardown = std::thread::spawn(move || {
        run_tui(&fake.url, 100, 30, Duration::from_secs(45));
    });
    span.join().expect("driver thread").expect("mouse-off boot cycle");
    teardown.join().expect("run thread");
    drop(harness);
}

/// Item 3 — `--prompt`: the boot seeds the composer and auto-submits
/// the first turn (zero keystrokes): the prompt POST reaches the stub
/// (`prompt_posts`) and the scripted streaming reply renders.
#[test]
fn pty_boot_prompt_auto_submits_the_first_turn() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("dogfood3-prompt");
    let fake = FakeLlm::spawn(Vec::new()); // no history — the home boot
    fake.arm_script(streaming_thinking_script("ses_alpha", "kick off dogfood3"));
    let prompt_posts = Arc::clone(&fake.prompt_posts);
    let fake_url = fake.url.clone();
    let harness = LiveHarness::attach(100, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        // NO typing: the only path to the scripted reply is the boot's
        // auto-submit. Wait for the user text (the inbox echo of OUR
        // submission) and the streaming reasoning row.
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let (mut saw_user, mut saw_reply) = (false, false);
        while std::time::Instant::now() < deadline {
            let text = {
                let grid = driver.grid_now();
                (0..grid.rows).map(|row| grid.row_text(row)).collect::<Vec<_>>().join("\n")
            };
            if text.contains("kick off dogfood3") {
                saw_user = true;
            }
            if text.contains("Thinking") {
                saw_reply = true; // the running step row (vt_temporal's marker)
            }
            if saw_user && saw_reply {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !saw_user {
            return Err("the boot prompt never submitted (no user row)".to_owned());
        }
        if !saw_reply {
            return Err("the scripted turn never opened its Thinking row".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let teardown = std::thread::spawn(move || {
        run_tui_boot(
            &fake_url,
            100,
            30,
            Duration::from_secs(45),
            oc_tui::app::BootArgs {
                prompt: Some("kick off dogfood3".to_owned()),
                ..oc_tui::app::BootArgs::default()
            },
        );
    });
    let result = span.join().expect("driver thread");
    // Report the driver's own verdict BEFORE the run thread's
    // timeout panic can mask it.
    if let Err(error) = &result {
        eprintln!("dogfood3 driver verdict: {error}");
    }
    if let Err(panic) = teardown.join() {
        // The app outlived its cap: surface the redirected stderr log.
        let log = sandbox.join(".local/state/opencode-rust/tui.log");
        if let Ok(body) = std::fs::read_to_string(&log) {
            eprintln!("tui.log tail: {}", &body[body.len().saturating_sub(3000)..]);
        }
        std::panic::resume_unwind(panic);
    }
    result.expect("the --prompt boot cycle");
    assert!(
        prompt_posts.load(Ordering::SeqCst) >= 1,
        "the auto-submit reached the stub's prompt endpoint"
    );
    drop(harness);
    fake.stop();
}
