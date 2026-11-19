//! G-457 T2 — the full selection-model chain at the PTY level, from
//! REAL SGR byte sequences through the multi-click detector →
//! behavior (cell/word/line) → endpoint expansion → reverse-video
//! highlight → copy-on-release (`copyOnSelectRelease`,
//! util/selection.ts:44-60) → key routing (`handleSelectionKey`:
//! escape clears).
//!
//! Ref anchors: `SelectionBehavior` (types.d.ts:35, endpoints extend
//! through native selectWord/selectLine), `expandSelectionRange`'s
//! union model (text-buffer-view.zig:871-891, word spans on both
//! endpoints), the renderer's private `nextClickBehavior` click
//! cycle (renderer.d.ts:617). The PTY presses below are real SGR
//! strokes; the multi-click window is the port's 500 ms tracker.

#![allow(unsafe_code)]
#![allow(clippy::too_many_lines)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, LiveHarness, ScriptEvent, WORDMARK,
};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| u64::try_from(since.as_millis()).unwrap_or(1_000_000))
}

#[allow(clippy::needless_pass_by_value)] // mirrors pty_w11_mouse's helper shape
fn bus_event(id: &str, kind: &str, data: serde_json::Value) -> ScriptEvent {
    ScriptEvent {
        delay_ms: 30,
        body: serde_json::json!({
            "id": id,
            "created": now_ms(),
            "type": kind,
            "data": data,
        })
        .to_string(),
    }
}

fn wordy_transcript_script(session: &str) -> Vec<ScriptEvent> {
    let lines = "alpha bravo charlie delta echo foxtrot\n\
                 golf hotel india juliet kilo lima\n\
                 mike november oscar papa quebec romeo"
        .to_owned();
    vec![
        bus_event(
            "evt_inbox",
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": session, "inboxID": "msg_user_1",
                "item": { "type": "user", "payload": { "text": "list words" } },
            }),
        ),
        bus_event(
            "evt_step",
            "session.step.started",
            serde_json::json!({
                "sessionID": session, "assistantMessageID": "msg_asst_1",
                "agent": "build", "model": { "providerID": "anthropic", "id": "claude-sonnet" },
            }),
        ),
        bus_event(
            "evt_text",
            "session.text.delta",
            serde_json::json!({
                "sessionID": session, "assistantMessageID": "msg_asst_1",
                "ordinal": 0, "delta": lines,
            }),
        ),
        bus_event(
            "evt_ended",
            "session.step.ended",
            serde_json::json!({
                "sessionID": session, "assistantMessageID": "msg_asst_1", "finish": "success",
            }),
        ),
        bus_event(
            "evt_idle",
            "session.idle",
            serde_json::json!({ "sessionID": session, "id": "msg_idle_1" }),
        ),
    ]
}

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

/// The number of `word`'s cells whose style differs from `baseline`'s.
fn word_changed(h: &HarnessHandle, baseline: &vt_harness::Grid, row: usize, word: &str) -> usize {
    let grid = h.grid_now();
    let text = grid.row_text(row);
    let Some(byte) = text.find(word) else { return 0 };
    let col = text[..byte].chars().count();
    (0..word.chars().count())
        .filter(|i| grid.cell(col + i, row).style != baseline.cell(col + i, row).style)
        .count()
}

fn col_of(h: &HarnessHandle, row: usize, word: &str) -> Option<usize> {
    let grid = h.grid_now();
    let text = grid.row_text(row);
    text.find(word).map(|byte| text[..byte].chars().count())
}

fn scenario(driver: &mut HarnessHandle) -> Result<(), String> {
    if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(driver, b"Alpha").map_err(|e| format!("session dialog: {e}"))?;
    driver.write(b"list words\r");
    let bravo_row = wait_row(driver, "bravo", Duration::from_secs(10))
        .ok_or_else(|| "the wordy reply never rendered".to_owned())?;

    // -- ① triple-click: the whole LINE flips ------------------------
    let baseline = driver.grid_now();
    driver.sgr_press(10, u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_release(10, u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_press(10, u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_release(10, u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_press(10, u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(400));
    let flipped = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"]
        .iter()
        .map(|w| word_changed(driver, &baseline, bravo_row, w))
        .sum::<usize>();
    if flipped == 0 {
        return Err("triple-click highlighted nothing (line selection dead)".to_owned());
    }
    // A word OUTSIDE the line must be untouched.
    let other_row = wait_row(driver, "golf", Duration::from_secs(2)).unwrap_or(0);
    if word_changed(driver, &baseline, other_row, "golf") > 0 {
        return Err("triple-click leaked outside its line".to_owned());
    }
    // Escape clears (handleSelectionKey).
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    if word_changed(driver, &baseline, bravo_row, "bravo") > 0 {
        return Err("escape did not clear the line selection".to_owned());
    }

    // -- ② double-click: exactly the WORD flips ----------------------
    let baseline = driver.grid_now();
    let col = col_of(driver, bravo_row, "charlie").ok_or("charlie lost")?;
    driver.sgr_press(u16::try_from(col + 2).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_release(u16::try_from(col + 2).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_press(u16::try_from(col + 2).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(400));
    if word_changed(driver, &baseline, bravo_row, "charlie") != "charlie".len() {
        return Err(format!(
            "double-click did not highlight exactly `charlie`: {}",
            word_changed(driver, &baseline, bravo_row, "charlie")
        ));
    }
    if word_changed(driver, &baseline, bravo_row, "bravo") > 0
        || word_changed(driver, &baseline, bravo_row, "delta") > 0
    {
        return Err("double-click leaked past the word endpoints".to_owned());
    }
    // The release resolved copy-on-select: the Copied toast.
    driver.sgr_release(u16::try_from(col + 2).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(400));
    let grid = driver.grid_now();
    let copied = (0..grid.rows).any(|r| grid.row_text(r).contains("Copied"));
    if !copied {
        return Err("the word release did not copy (no Copied toast)".to_owned());
    }

    // -- ③ cross-row double-drag: rows between fully selected -------
    let golf_row = other_row.max(1);
    let baseline = driver.grid_now();
    let col_a = col_of(driver, bravo_row, "alpha").ok_or("alpha lost")?;
    let col_g = col_of(driver, golf_row, "lima").ok_or("lima lost")?;
    driver.sgr_press(u16::try_from(col_a).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_release(u16::try_from(col_a).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_press(u16::try_from(col_a).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(80));
    driver.sgr_drag(u16::try_from(col_g).unwrap_or(79), u16::try_from(golf_row).unwrap_or(29));
    std::thread::sleep(Duration::from_millis(500));
    if word_changed(driver, &baseline, bravo_row, "foxtrot") != "foxtrot".len() {
        return Err("cross-row word drag did not extend the first row to its tail".to_owned());
    }
    if word_changed(driver, &baseline, golf_row, "golf") != "golf".len() {
        return Err("cross-row word drag did not cover the last row's head".to_owned());
    }
    Ok(())
}

#[test]
fn pty_selection_multiclick_full_chain() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("g457-selection");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(wordy_transcript_script("ses_alpha"));
    let harness = LiveHarness::attach(80, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        let outcome = scenario(&mut driver);
        let from = driver.raw_len();
        let _ = assert_exit_ack(&mut driver, from);
        outcome
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 30, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the selection model chain");
    let _ = sandbox;
}
