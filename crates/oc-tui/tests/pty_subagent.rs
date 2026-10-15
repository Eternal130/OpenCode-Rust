//! PTY live checks for the subagent UI surfaces (#40/#44/#45/#46 —
//! fix-wave E2): the REAL [`oc_tui::app::run_with_size`] loop runs
//! over a pty against the `FakeLlm` server with a scripted subagent
//! flow, driven by real keystrokes and asserted on the parsed CELL
//! GRID:
//!
//! * **#40** the tool card: the completed `subagent` tool part renders
//!   `✓ General Subagent — Check the fixture` between the user text
//!   and the reply;
//! * **#44** the picker: `down` on the empty composer opens the panel
//!   (`Subagents  Shell  Terminals` + `No active subagents` +
//!   `show inactive ctrl+a`), `ctrl+a` flips the filter to reveal the
//!   completed run, `enter` (`composer.subagent.select`) navigates in;
//! * **#45** the child-session view renders (its own transcript) with
//!   the picker mounted (the `parentID` composer) and — at 160 cols —
//!   NO sidebar pane (session-frame.tsx:106-111);
//! * **#46** the exit farewell: `ctrl+c` from the session leaves the
//!   alternate screen and prints the wordmark + resume hint epilogue.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, ALT_SCREEN_LEAVE, WORDMARK,
};

/// The scripted subagent flow: the parent's turn calls the `subagent`
/// tool, the child session runs and completes, the tool settles, and
/// the parent's continuation replies (the corpus3 flow stub's event
/// shape — attribution-free, straight SSE).
#[allow(clippy::too_many_lines)] // the scripted corpus3 flow, one event per arm
fn subagent_flow_script() -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    let event = |delay: u64, id: &str, type_: &str, data: &str| vt_harness::ScriptEvent {
        delay_ms: delay,
        body: format!(r#"{{"id":"{id}","created":{now_ms},"type":"{type_}","data":{data}}}"#),
    };
    vec![
        // The parent's user echo + assistant step.
        event(
            30,
            "evt_u",
            "session.inbox.enqueued",
            r#"{"sessionID":"ses_alpha","inboxID":"msg_user_1","item":{"type":"user","payload":{"text":"PARITY delegate to the researcher now"}}}"#,
        ),
        event(
            60,
            "evt_step",
            "session.step.started",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","agent":"build","model":{"providerID":"parityfake","id":"fake-model"}}"#,
        ),
        event(80, "evt_exec_a", "session.execution.started", r#"{"sessionID":"ses_alpha"}"#),
        // The subagent tool call: streaming → called (parsed input) →
        // progress (the child sessionID metadata).
        event(
            100,
            "evt_t1",
            "session.tool.input.started",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","id":"tool_1","name":"subagent"}"#,
        ),
        event(
            140,
            "evt_t2",
            "session.tool.called",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","id":"tool_1","input":{"agent":"general","description":"Check the fixture","prompt":"PARITY-SUBAGENT-TASK inspect the seeded fixture and report."},"executed":false}"#,
        ),
        // The child session spawns (title = the description, agent
        // general, parent the running session).
        event(
            170,
            "evt_child",
            "session.created",
            r#"{"sessionID":"ses_child","parentID":"ses_alpha","agent":"general","title":"Check the fixture","slug":"check","version":"2","projectID":"p1","location":{"directory":"/tmp"},"subpath":""}"#,
        ),
        event(
            200,
            "evt_t3",
            "session.tool.progress",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","id":"tool_1","metadata":{"sessionID":"ses_child","status":"running"}}"#,
        ),
        // The child's own turn.
        event(
            230,
            "evt_c1",
            "session.inbox.enqueued",
            r#"{"sessionID":"ses_child","inboxID":"msg_user_c","item":{"type":"user","payload":{"text":"You are a subagent spawned by another session.\nPARITY-SUBAGENT-TASK inspect the seeded fixture and report."}}}"#,
        ),
        event(260, "evt_c2", "session.execution.started", r#"{"sessionID":"ses_child"}"#),
        event(
            290,
            "evt_c3",
            "session.step.started",
            r#"{"sessionID":"ses_child","assistantMessageID":"msg_asst_c","agent":"general","model":{"providerID":"parityfake","id":"fake-model"}}"#,
        ),
        event(
            340,
            "evt_c4",
            "session.text.delta",
            r#"{"sessionID":"ses_child","assistantMessageID":"msg_asst_c","ordinal":0,"delta":"Subagent fixture done."}"#,
        ),
        event(
            380,
            "evt_c5",
            "session.text.ended",
            r#"{"sessionID":"ses_child","assistantMessageID":"msg_asst_c","ordinal":0,"text":"Subagent fixture done."}"#,
        ),
        event(
            410,
            "evt_c6",
            "session.step.ended",
            r#"{"sessionID":"ses_child","assistantMessageID":"msg_asst_c","finish":"stop"}"#,
        ),
        event(430, "evt_c7", "session.execution.succeeded", r#"{"sessionID":"ses_child"}"#),
        // The tool settles; the parent's continuation replies.
        event(
            470,
            "evt_t4",
            "session.tool.success",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","id":"tool_1","content":[{"type":"text","text":"Subagent fixture done."}],"metadata":{"sessionID":"ses_child"},"executed":false}"#,
        ),
        event(
            520,
            "evt_p1",
            "session.text.delta",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","ordinal":0,"delta":"Main flow final reply."}"#,
        ),
        event(
            560,
            "evt_p2",
            "session.text.ended",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","ordinal":0,"text":"Main flow final reply."}"#,
        ),
        event(
            590,
            "evt_p3",
            "session.step.ended",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","finish":"stop"}"#,
        ),
        event(610, "evt_p4", "session.execution.succeeded", r#"{"sessionID":"ses_alpha"}"#),
    ]
}

/// Every scripted body must decode as a bus event (a malformed frame
/// would end the SSE stream mid-script and reconnect-loop the app —
/// the exact failure this test caught in development).
#[test]
fn subagent_flow_script_bodies_decode() {
    for event in subagent_flow_script() {
        let value: serde_json::Value = serde_json::from_str(&event.body).unwrap_or_else(|error| {
            panic!("{} body malformed: {error}\n{}", event.delay_ms, event.body)
        });
        assert!(value.get("id").and_then(|v| v.as_str()).is_some(), "{}", event.body);
        assert!(value.get("type").and_then(|v| v.as_str()).is_some(), "{}", event.body);
        assert!(value.get("data").is_some(), "{}", event.body);
    }
}

/// Wait until the session route is mounted and the dialog layer is
/// GONE: the composer's `╹` transition row paints under the dialog
/// overlay too, so the settle needs the quiet gap after the submit
/// (the temporal suite's boot contract: paint, then 500 ms). The
/// row carries the composer's 2-col inset — trim before matching
/// (`pty_sidebar`'s `transition_extent` convention).
fn wait_session_mounted(h: &HarnessHandle) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        if (0..grid.rows)
            .any(|row| grid.row_text(row).trim_start_matches(' ').starts_with('\u{2579}'))
        {
            std::thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err("the session route never painted its composer".to_owned())
}

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

/// Wait until NO grid row contains `needle`.
fn wait_row_gone(h: &HarnessHandle, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        let present = (0..grid.rows).any(|row| grid.row_text(row).contains(needle));
        if !present {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Drive `f`, and on ANY outcome still walk the app to a clean exit
/// (a failed mid-walk assertion must not strand the loop — the first
/// ctrl+c folds a live picker, the second exits).
fn drive_or_exit(
    h: &mut HarnessHandle,
    f: impl FnOnce(&mut HarnessHandle) -> Result<(), String>,
) -> Result<(), String> {
    let outcome = f(h);
    if outcome.is_err() {
        h.write(b"\x03");
        std::thread::sleep(Duration::from_millis(400));
        h.write(b"\x03");
    } else {
        vt_harness::exit_chord(h);
    }
    outcome
}

/// The full E2 surface walk at 80×24: card → picker → child → back →
/// farewell.
fn drive_subagent_surfaces(h: &mut HarnessHandle) -> Result<(), String> {
    if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(h, b"Alpha")?;
    wait_session_mounted(h)?;
    // Submit: the armed script plays the whole flow.
    h.write(b"PARITY delegate to the researcher now\r");

    // #40 — the completed tool card between the user text and the
    // reply (the corpus3 ref shape byte-for-byte).
    wait_row(h, "✓ General Subagent — Check the fixture", Duration::from_secs(8))
        .ok_or_else(|| "the subagent tool card never rendered".to_owned())?;
    wait_row(h, "Main flow final reply.", Duration::from_secs(5))
        .ok_or_else(|| "the continuation reply never rendered".to_owned())?;

    // #44 — `down` on the empty composer opens the picker.
    h.write(b"\x1b[B");
    wait_row(h, "Subagents  Shell  Terminals", Duration::from_secs(5))
        .ok_or_else(|| "`down` never opened the subagent picker".to_owned())?;
    wait_row(h, "No active subagents", Duration::from_secs(3))
        .ok_or_else(|| "the active-filter empty state never rendered".to_owned())?;
    wait_row(h, "show inactive ctrl+a", Duration::from_secs(3))
        .ok_or_else(|| "the hint row never rendered".to_owned())?;
    // The panel replaced the prompt: no `╹▀` transition row remains.
    if (0..h.grid_now().rows)
        .any(|row| h.grid_now().row_text(row).trim_start_matches(' ').starts_with('╹'))
    {
        return Err("the prompt's transition row still renders under the picker".to_owned());
    }

    // ctrl+a — the inactive filter reveals the completed run.
    h.write(b"\x01");
    wait_row(h, "General: Check the fixture", Duration::from_secs(3))
        .ok_or_else(|| "ctrl+a never revealed the completed subagent".to_owned())?;
    wait_row(h, "show active ctrl+a", Duration::from_secs(3))
        .ok_or_else(|| "the filter hint never flipped".to_owned())?;

    // enter — navigate into the child session (#45): the child's own
    // transcript renders; the picker STAYS mounted (the parentID
    // composer); the parent's tab stays the strip's only tab.
    h.write(b"\r");
    wait_row(h, "You are a subagent spawned by another session.", Duration::from_secs(5))
        .ok_or_else(|| "enter never navigated into the child session".to_owned())?;
    wait_row(h, "Subagent fixture done.", Duration::from_secs(3))
        .ok_or_else(|| "the child reply never rendered".to_owned())?;
    // The strip keeps the parent's tab (the root) — no new tab opened.
    let strip = h.grid_now().row_text(0);
    if !strip.contains("Alpha") {
        return Err(format!("the strip lost the root's tab on child navigation: {strip}"));
    }

    // escape — the child's composer close navigates back to the
    // parent and folds the panel.
    h.write(b"\x1b");
    wait_row(h, "PARITY delegate to the researcher now", Duration::from_secs(5))
        .ok_or_else(|| "escape never returned to the parent session".to_owned())?;
    if !wait_row_gone(h, "Subagents  Shell  Terminals", Duration::from_secs(3)) {
        return Err("the picker panel never folded on the parent".to_owned());
    }

    // #46 — ctrl+c from the session: teardown + the farewell screen.
    let from = h.raw_len();
    assert_exit_ack(h, from).map_err(|error| format!("teardown: {error}"))?;
    if !h.wait_for_after(b"opencode -s ses_alpha", from, Duration::from_secs(5)) {
        return Err("the exit epilogue's resume hint never printed".to_owned());
    }
    // The wordmark glyphs are SGR-wrapped per character (presentation
    // ts `draw()`), so the contiguous byte probe lives in the GRID —
    // the parsed cells carry the post-exit primary screen.
    if wait_row(h, "█▀▀█", Duration::from_secs(2)).is_none() {
        return Err("the exit epilogue's wordmark never painted".to_owned());
    }
    if wait_row(h, "Session   Alpha", Duration::from_secs(2)).is_none() {
        return Err("the exit epilogue's session row never painted".to_owned());
    }
    Ok(())
}

/// #45 at 160 cols: the parent docks the sidebar, the child session
/// does NOT (session-frame.tsx:106-111).
fn drive_child_sidebar_suppression(h: &mut HarnessHandle) -> Result<(), String> {
    if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(h, b"Alpha")?;
    wait_session_mounted(h)?;
    // The parent docks the pane above 120 cols.
    h.write(b"PARITY delegate to the researcher now\r");
    wait_row(h, "✓ General Subagent — Check the fixture", Duration::from_secs(8))
        .ok_or_else(|| "the subagent tool card never rendered".to_owned())?;
    let parent_has_pane =
        (0..h.grid_now().rows).any(|row| h.grid_now().row_text(row).contains("Getting started"));
    if !parent_has_pane {
        return Err("the parent session never docked the sidebar at 160".to_owned());
    }
    // The picker walk into the child.
    h.write(b"\x1b[B\x01\r");
    wait_row(h, "You are a subagent spawned by another session.", Duration::from_secs(5))
        .ok_or_else(|| "enter never navigated into the child session".to_owned())?;
    let grid = h.grid_now();
    if (0..grid.rows).any(|row| grid.row_text(row).contains("Getting started")) {
        return Err("the CHILD session docked the sidebar — it must be absent".to_owned());
    }
    // The child's picker spans the full width (the `esc` label lands
    // past the 118-col pane edge).
    let tabs = (0..grid.rows)
        .find_map(|row| {
            let text = grid.row_text(row);
            text.contains("Subagents  Shell  Terminals").then_some(text)
        })
        .ok_or_else(|| "the child's picker never rendered".to_owned())?;
    let esc_col = tabs.find("esc").map(|index| tabs[..index].chars().count());
    if esc_col.is_none_or(|col| col < 118) {
        return Err(format!("the child's picker did not span the full width: {tabs:?}"));
    }
    // Return to the parent first (v2's composer close on a child
    // NAVIGATES — ctrl+c there closes the panel, it does not exit),
    // then the standard exit chord.
    h.write(b"\x1b");
    wait_row(h, "PARITY delegate to the researcher now", Duration::from_secs(5))
        .ok_or_else(|| "escape never returned to the parent session".to_owned())?;
    let from = h.raw_len();
    assert_exit_ack(h, from).map_err(|error| format!("teardown: {error}"))?;
    let _ = ALT_SCREEN_LEAVE;
    Ok(())
}

/// Lane M6 (retired): the picker panel + farewell render through
/// their opentui-solid trees — the only renderers since the
/// retirement lane deleted the `OC_SUBAGENT_OPENTUI` flag pair.
#[test]
fn pty_subagent_surfaces_card_picker_child_farewell() {
    pty_subagent_surfaces_walk();
}

/// The shared walk body (see the module doc).
fn pty_subagent_surfaces_walk() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("subagent-e2");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(subagent_flow_script());
    let posts = std::sync::Arc::clone(&fake.prompt_posts);
    let url = fake.url.clone();
    let harness = vt_harness::LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || drive_or_exit(&mut driver, drive_subagent_surfaces));
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(60));
    });
    let result = span.join().expect("driver");
    let _ = teardown.join();
    let post_count =
        std::sync::atomic::AtomicUsize::load(&posts, std::sync::atomic::Ordering::SeqCst);
    fake.stop();
    if let Err(error) = &result {
        panic!("subagent surfaces (prompt POSTs seen by the fake: {post_count}): {error}");
    }
    result.expect("subagent surfaces");
}

/// The M6 new-path interaction cycle: the picker's keymap family on
/// the opentui-solid tree — `down` opens, `up` at the top CLOSES,
/// `down` reopens, `ctrl+a` flips the filter, the arrows move the
/// selection (bold focus), `left`/`right` cycle the tabs, `esc`
/// closes back to the prompt.
#[test]
fn pty_picker_keymap_cycle_solid_path() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("subagent-m6-cycle");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(subagent_flow_script());
    let url = fake.url.clone();
    let harness = vt_harness::LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || drive_or_exit(&mut driver, drive_picker_cycle));
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(60));
    });
    let result = span.join().expect("driver");
    let _ = teardown.join();
    fake.stop();
    result.expect("picker keymap cycle (solid path)");
}

/// The solid-path cycle body (see the test above).
fn drive_picker_cycle(h: &mut HarnessHandle) -> Result<(), String> {
    if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(h, b"Alpha")?;
    wait_session_mounted(h)?;
    h.write(b"PARITY delegate to the researcher now\r");
    wait_row(h, "✓ General Subagent — Check the fixture", Duration::from_secs(8))
        .ok_or_else(|| "the subagent tool card never rendered".to_owned())?;

    // down opens the picker on the empty composer.
    h.write(b"\x1b[B");
    wait_row(h, "Subagents  Shell  Terminals", Duration::from_secs(5))
        .ok_or("`down` never opened the picker")?;
    // up at the top CLOSES (subagents-tab.tsx:145-151).
    h.write(b"\x1b[A");
    if !wait_row_gone(h, "Subagents  Shell  Terminals", Duration::from_secs(3)) {
        return Err("`up` at the top never closed the picker".to_owned());
    }
    // down reopens; ctrl+a reveals the completed run.
    h.write(b"\x1b[B");
    wait_row(h, "Subagents  Shell  Terminals", Duration::from_secs(3))
        .ok_or("the picker never reopened")?;
    h.write(b"\x01");
    wait_row(h, "General: Check the fixture", Duration::from_secs(3))
        .ok_or("ctrl+a never revealed the completed run")?;
    // right cycles to the Shell tab (empty body), left back.
    h.write(b"\x1b[C");
    wait_row(h, "show active ctrl+a", Duration::from_secs(3))
        .ok_or("the Shell tab never mounted")?;
    h.write(b"\x1b[D");
    wait_row(h, "General: Check the fixture", Duration::from_secs(3))
        .ok_or("the tab never cycled back to Subagents")?;
    // esc closes back to the prompt (the parent route).
    h.write(b"\x1b");
    if !wait_row_gone(h, "Subagents  Shell  Terminals", Duration::from_secs(3)) {
        return Err("`esc` never closed the picker".to_owned());
    }
    wait_session_mounted(h)?;
    Ok(())
}

#[test]
fn pty_child_session_suppresses_the_sidebar_at_160() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("subagent-sidebar");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(subagent_flow_script());
    let url = fake.url.clone();
    let harness = vt_harness::LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span =
        std::thread::spawn(move || drive_or_exit(&mut driver, drive_child_sidebar_suppression));
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 160, 30, Duration::from_secs(60));
    });
    let result = span.join().expect("driver");
    let _ = teardown.join();
    fake.stop();
    result.expect("child sidebar suppression");
}

/// #46 negative: exiting from HOME prints NO epilogue (the corpus's
/// identical post-exit blanks).
#[test]
fn pty_home_exit_prints_no_epilogue() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("subagent-home");
    let fake = FakeLlm::spawn(standard_sessions());
    let url = fake.url.clone();
    let harness = vt_harness::LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        std::thread::sleep(Duration::from_millis(300));
        if driver.wait_for_after(b"opencode -s", from, Duration::from_millis(200)) {
            return Err("the home exit printed a session epilogue".to_owned());
        }
        Ok(())
    });
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(30));
    });
    let result = span.join().expect("driver");
    let _ = teardown.join();
    fake.stop();
    result.expect("home exit has no epilogue");
}
