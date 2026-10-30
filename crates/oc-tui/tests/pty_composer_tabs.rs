//! G-6 — the composer's multi-tab UI + `@`-reference completion, PTY
//! proofs over the REAL app loop (`oc_tui::app::run_with_size`):
//!
//! * `composer_tabs_walk` — `down` opens the picker panel; the
//!   ←/→ keys cycle the three registered views (Subagents / Shell /
//!   Terminals — the ref registration order, composer/index.tsx:
//!   127-131) with each tab's faithful body: the session's running
//!   shell row (the command's first line) + `output`/`kill` hints on
//!   the Shell tab, the `+ New terminal` slot on the Terminals tab;
//!   `return` on the shell row opens the `DialogShellOutput` viewer
//!   (status + command + the captured output), ctrl+d fires the kill
//!   DELETE, and `up` at the top of the Shell tab closes the composer
//!   (shell-tab.tsx:70-79).
//! * `at_completion_lists_reference_sources` — typing `@` opens the
//!   popup over the skills → reference-sources rows (the ref ordering,
//!   autocomplete.tsx:549-551); the reference rows render `@<name>`
//!   with the ` <repo|path>` description (autocomplete.tsx:455-457);
//!   a query filters them; `return` accepts the chip.

#![allow(unsafe_code)] // pty(7) via libc, the shared harness

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::Arc;
use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, WORDMARK,
};

/// Wait until any grid row contains `needle`.
fn wait_row(h: &HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
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

/// Open the first history session with a boot-retry: under a loaded
/// box the dialog's session list can outlive the shared helper's 5 s
/// window — escape folds, a beat passes, and the chord replays.
fn open_session_with_retry(h: &mut HarnessHandle, needle: &[u8]) -> Result<(), String> {
    if open_first_session(h, needle).is_ok() {
        return Ok(());
    }
    h.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(400));
    open_first_session(h, needle)
}

/// Wait until NO grid row contains `needle`.
fn wait_row_gone(h: &HarnessHandle, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let present = (0..h.grid_now().rows).any(|row| h.grid_now().row_text(row).contains(needle));
        if !present {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// The scripted turn that also lands the shell registry row: a short
/// settled reply plus one `shell.created` for the routed session (the
/// Shell tab's data source).
fn tabs_script() -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| u64::try_from(since.as_millis()).unwrap_or(1_000_000));
    let event = |delay: u64, id: &str, type_: &str, data: &str| vt_harness::ScriptEvent {
        delay_ms: delay,
        body: format!(r#"{{"id":"{id}","created":{now_ms},"type":"{type_}","data":{data}}}"#),
    };
    vec![
        event(
            30,
            "evt_u",
            "session.inbox.enqueued",
            r#"{"sessionID":"ses_alpha","inboxID":"msg_user_1","item":{"type":"user","payload":{"text":"PARITY tabs walk"}}}"#,
        ),
        event(
            60,
            "evt_s",
            "session.step.started",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_a","agent":"build","model":{"providerID":"parityfake","id":"fake-model"}}"#,
        ),
        event(
            100,
            "evt_t1",
            "session.text.delta",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_a","ordinal":0,"delta":"TABS WALK REPLY"}"#,
        ),
        event(
            140,
            "evt_t2",
            "session.text.ended",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_a","ordinal":0,"text":"TABS WALK REPLY"}"#,
        ),
        event(
            180,
            "evt_e",
            "session.step.ended",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_a","finish":"stop"}"#,
        ),
        // The Shell tab's row: one running shell owned by the session.
        event(
            220,
            "evt_shell",
            "shell.created",
            r#"{"info":{"id":"sh_1","status":"running","command":"cargo build\n--all-features","cwd":"/tmp","shell":"bash","file":"/tmp/sh_1.out","metadata":{"sessionID":"ses_alpha"},"time":{"started":0}}}"#,
        ),
    ]
}

/// Drive `f`, and on ANY outcome still walk the app to a clean exit.
fn drive_or_exit(h: &mut HarnessHandle, f: impl FnOnce(&mut HarnessHandle) -> Result<(), String>) {
    let outcome = f(h);
    if outcome.is_err() {
        h.write(b"\x03");
        std::thread::sleep(Duration::from_millis(400));
        h.write(b"\x03");
    } else {
        assert_exit_ack(h, h.raw_len()).expect("teardown");
    }
    outcome.expect("the walk failed");
}

/// The ←/→ tab cycle across the three registered views with each
/// tab's body + the shell row's interactions.
#[test]
fn composer_tabs_walk() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("g6-tabs");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_terminals();
    fake.arm_shell_output();
    fake.arm_script(tabs_script());
    let shell_deletes = Arc::clone(&fake.shell_deletes);

    let harness = vt_harness::LiveHarness::attach(110, 30);
    let mut driver = harness.handle();
    let handle = std::thread::spawn(move || {
        drive_or_exit(&mut driver, |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            open_session_with_retry(h, b"Alpha")?;
            h.write(b"PARITY tabs walk\r");
            wait_row(h, "TABS WALK REPLY", Duration::from_secs(8))
                .ok_or_else(|| "the reply never rendered".to_owned())?;
            // The script's `shell.created` rides the same feed; give
            // the ingest a beat (the Shell tab reads the registry).
            std::thread::sleep(Duration::from_millis(400));

            // `down` on the empty composer opens the picker: the
            // three-tab strip in registration order + the Subagents
            // tab's active-filter empty state.
            h.write(b"\x1b[B");
            wait_row(h, "Subagents  Shell  Terminals", Duration::from_secs(5))
                .ok_or_else(|| "the picker strip never rendered".to_owned())?;
            wait_row(h, "No active subagents", Duration::from_secs(3))
                .ok_or_else(|| "the Subagents tab's empty state never rendered".to_owned())?;

            // → Shell: the shell row (the command's FIRST line only)
            // + the output/kill hints (shell-tab.tsx:53-59,139).
            h.write(b"\x1b[C");
            wait_row(h, "cargo build", Duration::from_secs(4))
                .ok_or_else(|| "the Shell tab never listed the shell".to_owned())?;
            let grid = h.grid_now();
            let shell_row = (0..grid.rows)
                .find_map(|row| {
                    let text = grid.row_text(row);
                    text.contains("cargo build").then_some(text)
                })
                .unwrap_or_default();
            if shell_row.contains("--all-features") {
                return Err(format!("the shell row rendered past the first line: {shell_row}"));
            }
            wait_row(h, "output return", Duration::from_secs(3))
                .ok_or_else(|| "the Shell tab's output hint never rendered".to_owned())?;
            wait_row(h, "kill ctrl+d", Duration::from_secs(3))
                .ok_or_else(|| "the Shell tab's kill hint never rendered".to_owned())?;

            // → Terminals: the add slot (terminals-tab.tsx:105).
            h.write(b"\x1b[C");
            wait_row(h, "+ New terminal", Duration::from_secs(4))
                .ok_or_else(|| "the Terminals tab never listed the add slot".to_owned())?;

            // → wraps to Subagents (the three-tab ring).
            h.write(b"\x1b[C");
            wait_row(h, "No active subagents", Duration::from_secs(4))
                .ok_or_else(|| "the ring never wrapped to Subagents".to_owned())?;

            // ← Terminals, ← Shell: both directions cycle.
            h.write(b"\x1b[D");
            wait_row(h, "+ New terminal", Duration::from_secs(4))
                .ok_or_else(|| "← never returned to Terminals".to_owned())?;
            h.write(b"\x1b[D");
            wait_row(h, "output return", Duration::from_secs(4))
                .ok_or_else(|| "← never returned to Shell".to_owned())?;

            // return — the shell output viewer (shell-tab.tsx:29-32):
            // the title, the status, the command, the captured tail.
            h.write(b"\r");
            wait_row(h, "Shell output", Duration::from_secs(6))
                .ok_or_else(|| "the shell output viewer never opened".to_owned())?;
            wait_row(h, "Running", Duration::from_secs(4))
                .ok_or_else(|| "the viewer's status never rendered".to_owned())?;
            wait_row(h, "build finished", Duration::from_secs(4))
                .ok_or_else(|| "the captured output tail never rendered".to_owned())?;

            // escape folds the viewer; the picker stays on Shell.
            h.write(b"\x1b");
            if !wait_row_gone(h, "Shell output", Duration::from_secs(4)) {
                return Err("the shell output viewer never closed".to_owned());
            }
            wait_row(h, "output return", Duration::from_secs(4))
                .ok_or_else(|| "the Shell tab never re-rendered after the viewer".to_owned())?;

            // ctrl+d — the kill DELETE (shell-tab.tsx:98-109).
            h.write(b"\x04");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                let recorded = shell_deletes.lock().expect("deletes").clone();
                if recorded.iter().any(|path| path.starts_with("/api/shell/sh_1")) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            let recorded = shell_deletes.lock().expect("deletes").clone();
            if !recorded.iter().any(|path| path.starts_with("/api/shell/sh_1")) {
                return Err(format!("ctrl+d never DELETEd the shell: {recorded:?}"));
            }

            // up at the TOP of the Shell tab closes the composer
            // (shell-tab.tsx:70-79's close branch).
            h.write(b"\x1b[A");
            if !wait_row_gone(h, "Subagents  Shell  Terminals", Duration::from_secs(4)) {
                return Err("up at the top of the Shell tab never closed the composer".to_owned());
            }
            Ok(())
        });
    });
    run_tui(&fake.url, 110, 30, Duration::from_secs(90));
    fake.stop();
    handle.join().expect("driver thread");
}

/// Typing `@` opens the popup over the skills → reference-sources
/// rows; the reference rows carry the ` <detail>` description; a
/// query filters; `return` accepts the chip.
#[test]
fn at_completion_lists_reference_sources() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("g6-refs");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_skills();
    fake.arm_references();

    let harness = vt_harness::LiveHarness::attach(110, 30);
    let mut driver = harness.handle();
    let handle = std::thread::spawn(move || {
        drive_or_exit(&mut driver, |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            open_session_with_retry(h, b"Alpha")?;
            // The boot's server-list chain fetches the reference
            // registry LAST; give the worker a beat so the popup's
            // rows are complete when `@` opens it (the ref's solid
            // memo re-runs on arrival; the port re-attaches per key).
            std::thread::sleep(Duration::from_millis(700));

            // `see @` — the empty-query popup lists EVERY row in the
            // ref order: skills first (@greet @lint), then the
            // reference sources (@docs @vendor).
            h.write(b"see @");
            if let Some(row) = wait_row(h, "@greet", Duration::from_secs(6)) {
                let _ = row;
            } else {
                let grid = h.grid_now();
                let mut rows = Vec::new();
                for r in 0..grid.rows {
                    let text = grid.row_text(r);
                    if !text.trim().is_empty() {
                        rows.push(format!("{r}: {text}"));
                    }
                }
                return Err(format!(
                    "the popup never listed the skill row; grid:\n{}",
                    rows.join("\n")
                ));
            }
            wait_row(h, "/tmp/docs", Duration::from_secs(4))
                .ok_or_else(|| "the reference row's local detail never rendered".to_owned())?;
            wait_row(h, "github.com/acme/vendor", Duration::from_secs(4))
                .ok_or_else(|| "the reference row's git repository never rendered".to_owned())?;
            // The reference rows' structured labels.
            wait_row(h, "@docs", Duration::from_secs(2))
                .ok_or_else(|| "the @docs row never rendered".to_owned())?;
            wait_row(h, "@vendor", Duration::from_secs(2))
                .ok_or_else(|| "the @vendor row never rendered".to_owned())?;

            // `d` filters: @docs keeps the prefix boost, @vendor still
            // contains the needle — both stay.
            h.write(b"d");
            wait_row(h, "@docs", Duration::from_secs(4))
                .ok_or_else(|| "the query dropped the @docs row".to_owned())?;
            if !wait_row_gone(h, "@greet", Duration::from_secs(4)) {
                return Err("the query kept the non-matching skill row".to_owned());
            }

            // backspace to the empty query restores every row.
            h.write(b"\x7f");
            wait_row(h, "@greet", Duration::from_secs(4))
                .ok_or_else(|| "backspace never restored the full list".to_owned())?;

            // `ve` narrows to the git reference; `return` accepts the
            // chip (the draft shows `@vendor`, the popup folds).
            h.write(b"ve");
            if !wait_row_gone(h, "@docs", Duration::from_secs(4)) {
                return Err("the query kept the non-matching reference".to_owned());
            }
            wait_row(h, "@vendor", Duration::from_secs(4))
                .ok_or_else(|| "the query dropped the @vendor row".to_owned())?;
            h.write(b"\r");
            wait_row(h, "see @vendor", Duration::from_secs(4))
                .ok_or_else(|| "the accepted chip never landed in the draft".to_owned())?;
            if !wait_row_gone(h, "@greet", Duration::from_secs(4)) {
                return Err("the popup never closed on accept".to_owned());
            }
            Ok(())
        });
    });
    run_tui(&fake.url, 110, 30, Duration::from_secs(90));
    fake.stop();
    handle.join().expect("driver thread");
}
