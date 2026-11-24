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

/// The va12 selection-delta tests' runner (the multiclick chain's
/// wiring shape: sandbox + `FakeLlm` wordy transcript + real SGR strokes
/// on the PTY master).
fn run_selection_delta_pty(
    tag: &str,
    drive: impl FnOnce(&mut HarnessHandle) -> Result<(), String> + Send + 'static,
) {
    let _lock = pty_lock();
    let sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(wordy_transcript_script("ses_alpha"));
    let harness = LiveHarness::attach(80, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        let outcome = drive(&mut driver);
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
    result.expect("the selection delta");
    let _ = sandbox;
}

fn boot_to_words(driver: &mut HarnessHandle) -> Result<usize, String> {
    if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(driver, b"Alpha").map_err(|e| format!("session dialog: {e}"))?;
    driver.write(b"list words\r");
    wait_row(driver, "bravo", Duration::from_secs(10))
        .ok_or_else(|| "the wordy reply never rendered".to_owned())
}

/// va12 T1① — the multi-click CAP (renderer.ts:4983,
/// `Math.min(last.count + 1, 3)`): the FOURTH rapid press stays on
/// line — the whole line flips again. The pre-va12 port cycled
/// `count % 3`, so the fourth press degraded to a bare cell click and
/// flipped nothing.
#[test]
fn pty_selection_fourth_click_caps_at_line() {
    run_selection_delta_pty("g457-cap", |driver| {
        let bravo_row = boot_to_words(driver)?;
        let baseline = driver.grid_now();
        // Four rapid press/release cycles on one cell.
        for _ in 0..4 {
            driver.sgr_press(10, u16::try_from(bravo_row).unwrap_or(0));
            std::thread::sleep(Duration::from_millis(80));
            driver.sgr_release(10, u16::try_from(bravo_row).unwrap_or(0));
            std::thread::sleep(Duration::from_millis(80));
        }
        // The fourth press's line selection: EVERY word of the row
        // flips (line coverage — a cell/word degenerate would leave
        // most words untouched).
        for word in ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"] {
            let flipped = word_changed(driver, &baseline, bravo_row, word);
            if flipped < word.len() {
                return Err(format!(
                    "the fourth click did not stay line: `{word}` flipped {flipped}/{}",
                    word.len()
                ));
            }
        }
        // Nothing leaked off the line.
        let golf_row = wait_row(driver, "golf", Duration::from_secs(2)).unwrap_or(0);
        if word_changed(driver, &baseline, golf_row, "golf") > 0 {
            return Err("the fourth click leaked off its line".to_owned());
        }
        // The release copied (line text, copy-on-select).
        driver.sgr_release(10, u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(300));
        let grid = driver.grid_now();
        if !(0..grid.rows).any(|r| grid.row_text(r).contains("Copied")) {
            return Err("the fourth click's line release did not copy".to_owned());
        }
        Ok(())
    });
}

/// va12 T1② — the multi-click HIT TOLERANCE (renderer.ts:4982,
/// `max(|dx|, |dy|) <= 1`): a press one cell off the last press
/// (within 500 ms) CONTINUES the count — the second press lands a
/// word selection. The pre-va12 port required exact-cell equality, so
/// the offset press reset to 1 and selected nothing.
#[test]
fn pty_selection_adjacent_press_continues_the_count() {
    run_selection_delta_pty("g457-tolerance", |driver| {
        let bravo_row = boot_to_words(driver)?;
        let baseline = driver.grid_now();
        let col = col_of(driver, bravo_row, "charlie").ok_or("charlie lost")?;
        // Bare press on the word's first cell.
        driver.sgr_press(u16::try_from(col).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(80));
        driver.sgr_release(u16::try_from(col).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(80));
        // Adjacent press (chebyshev 1, inside the same word): count
        // continues to 2 — the word arms on the press itself.
        driver
            .sgr_press(u16::try_from(col + 1).unwrap_or(0), u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(300));
        let flipped = word_changed(driver, &baseline, bravo_row, "charlie");
        if flipped != "charlie".len() {
            return Err(format!(
                "the adjacent press did not continue the count to word: `charlie` \
                 flipped {flipped}/{}",
                "charlie".len()
            ));
        }
        if word_changed(driver, &baseline, bravo_row, "bravo") > 0
            || word_changed(driver, &baseline, bravo_row, "delta") > 0
        {
            return Err("the offset double-click leaked past the word".to_owned());
        }
        driver.sgr_release(
            u16::try_from(col + 1).unwrap_or(0),
            u16::try_from(bravo_row).unwrap_or(0),
        );
        std::thread::sleep(Duration::from_millis(300));
        let grid = driver.grid_now();
        if !(0..grid.rows).any(|r| grid.row_text(r).contains("Copied")) {
            return Err("the offset double-click's release did not copy".to_owned());
        }
        Ok(())
    });
}

/// va12 T1③ — the CTRL+CLICK extend (renderer.ts:3734-3740): over a
/// KEPT selection the ctrl press moves the focus endpoint to the
/// pressed cell (consumed), the highlight grows, and the release
/// re-copies over the union. The pre-va12 port treated the ctrl press
/// as a fresh click: the kept selection dropped and nothing new armed.
#[test]
fn pty_selection_ctrl_press_extends_kept_selection() {
    run_selection_delta_pty("g457-ctrl-extend", |driver| {
        let bravo_row = boot_to_words(driver)?;
        let baseline = driver.grid_now();
        // Drag-select cols 5..20 on the row, release (copy + keep).
        driver.sgr_press(5, u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(100));
        driver.sgr_drag(20, u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(300));
        driver.sgr_release(20, u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(300));
        let grid = driver.grid_now();
        if !(0..grid.rows).any(|r| grid.row_text(r).contains("Copied")) {
            return Err("the drag release never copied".to_owned());
        }
        let style_at = |g: &vt_harness::Grid, x: usize, y: usize| g.cell(x, y).style;
        let kept = driver.grid_now();
        let kept_flipped = (5..=20)
            .filter(|&x| style_at(&kept, x, bravo_row) != style_at(&baseline, x, bravo_row))
            .count();
        if kept_flipped == 0 {
            return Err(
                "the kept highlight vanished after the release (copy must retain)".to_owned()
            );
        }
        // ctrl+press at col 30: the highlight EXTENDS over 21..30.
        driver.sgr_ctrl_press(30, u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(300));
        let extended = driver.grid_now();
        let grew = (21..=30)
            .filter(|&x| style_at(&extended, x, bravo_row) != style_at(&baseline, x, bravo_row))
            .count();
        if grew == 0 {
            return Err("the ctrl press did not extend the highlight over cols 21..30".to_owned());
        }
        let held = (5..=20)
            .filter(|&x| style_at(&extended, x, bravo_row) != style_at(&baseline, x, bravo_row))
            .count();
        if held == 0 {
            return Err("the ctrl press dropped the kept anchor's highlight".to_owned());
        }
        // The release re-copies over the union (the toast refreshes).
        driver.sgr_release(30, u16::try_from(bravo_row).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(300));
        let grid = driver.grid_now();
        if !(0..grid.rows).any(|r| grid.row_text(r).contains("Copied")) {
            return Err("the extended release did not copy".to_owned());
        }
        Ok(())
    });
}
