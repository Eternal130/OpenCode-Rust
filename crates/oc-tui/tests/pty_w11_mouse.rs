//! W11 / G-447 — the mouse pipeline (wheel + selection highlight) at the
//! PTY level, from REAL SGR byte sequences (`\x1b[<64;x;yM` wheel,
//! button-0 press / button-32 drag / `m` release) injected at the PTY
//! master through the full decode → route → store → render stack.
//!
//! The beta.18 symptom wave reported both faces dead on a real machine;
//! this file pins the in-tree pipeline end-to-end so a regression in
//! either half (the decoder's SGR mapping, the four-level routing, the
//! G-440 post-grid reverse-video pass) fails here rather than on a user
//! terminal:
//!
//! * **wheel + drag + highlight in one scenario** — a long transcript,
//!   one up-notch displaces the `Lnn` markers; a left press→drag over
//!   the marker rows flips exactly the dragged cells' style digest
//!   (reverse video), a control cell outside the rect keeps its style;
//!   the release resolves the copy-on-select (highlight clears +
//!   `Copied to clipboard` toast).
//!
//! The app-level semantics (E1 half-page math, the rect model, the
//! G-440 fg/bg swap) stay unit-pinned in `app.rs` / `selection.rs`;
//! this file owns the LIVE byte-stream proof.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::too_many_lines)] // the PTY driver is one linear scenario

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

#[allow(clippy::needless_pass_by_value)] // the json! literal reads best by value
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

/// The long-transcript feed (the same shape as `pty_lane_if`'s): 60
/// hard-wrapped `Lnn` marker lines — both the wheel's and the drag's
/// observable.
fn long_transcript_script(session: &str) -> Vec<ScriptEvent> {
    let lines = (0..60).map(|index| format!("L{index:02}")).collect::<Vec<_>>().join("\n");
    vec![
        bus_event(
            "evt_inbox",
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": session,
                "inboxID": "msg_user_1",
                "item": { "type": "user", "payload": { "text": "list the lines" } },
            }),
        ),
        bus_event(
            "evt_step",
            "session.step.started",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": "msg_asst_1",
                "agent": "build",
                "model": { "providerID": "anthropic", "id": "claude-sonnet" },
            }),
        ),
        bus_event(
            "evt_text",
            "session.text.delta",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": "msg_asst_1",
                "ordinal": 0,
                "delta": lines,
            }),
        ),
        bus_event(
            "evt_ended",
            "session.step.ended",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": "msg_asst_1",
                "finish": "success",
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

fn wait_row_gone(h: &HarnessHandle, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        if (0..grid.rows).all(|row| !grid.row_text(row).contains(needle)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// The visible `Lnn` marker numbers (sorted, deduped).
fn visible_markers(h: &HarnessHandle) -> Vec<usize> {
    let grid = h.grid_now();
    let mut seen: Vec<usize> = Vec::new();
    for row in 0..grid.rows {
        let text = grid.row_text(row);
        for word in text.split_whitespace() {
            if let Some(number) = word.strip_prefix('L') {
                if word.len() == 3 {
                    if let Ok(value) = number.parse::<usize>() {
                        seen.push(value);
                    }
                }
            }
        }
    }
    seen.sort_unstable();
    seen.dedup();
    seen
}

/// G-447 — the live mouse pipeline: the wheel scrolls the transcript,
/// a real SGR press→drag paints reverse video over exactly the dragged
/// cells, and the release resolves copy-on-select (highlight clears +
/// the Copied toast). One scenario, both beta.18 symptom faces.
#[test]
fn pty_wheel_and_drag_highlight_pipeline() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("w11-mouse");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(long_transcript_script("ses_alpha"));
    let harness = LiveHarness::attach(80, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        // The scenario ALWAYS ends with the exit chord (an early Err must
        // not strand the TUI past run_tui's deadline).
        let outcome = mouse_scenario(&mut driver);
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
    result.expect("the mouse pipeline");
    let _ = sandbox;
}

/// The G-447 scenario body (see the test below for the harness wiring).
fn mouse_scenario(driver: &mut HarnessHandle) -> Result<(), String> {
    {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        // The prompt arms the scripted turn (the FakeLlm feed delivers on
        // the client prompt).
        driver.write(b"list the lines\r");
        // Sticky at the tail first.
        let tail_row = wait_row(driver, "L59", Duration::from_secs(10))
            .ok_or_else(|| "the long reply never rendered its tail".to_owned())?;

        // -- ① the wheel: one up-notch displaces the visible markers ---
        let sticky = visible_markers(driver);
        if sticky.len() < 8 {
            return Err(format!("viewport too small: {sticky:?}"));
        }
        driver.sgr_scroll_up(40, 15);
        std::thread::sleep(Duration::from_millis(300));
        let freed = visible_markers(driver);
        let step = freed.first().zip(sticky.first()).map_or(0, |(f, s)| s - f);
        if step == 0 || step > sticky.len() {
            return Err(format!(
                "one wheel notch moved {step} marker rows (viewport {}) — the wheel is dead: {sticky:?} → {freed:?}",
                sticky.len()
            ));
        }
        // Back to the tail for the selection half.
        for _ in 0..8 {
            driver.sgr_scroll_down(40, 15);
            std::thread::sleep(Duration::from_millis(120));
        }
        if !visible_markers(driver).contains(&59) {
            return Err("wheel-down never re-stuck to the tail".to_owned());
        }

        // -- ② the drag: reverse video over exactly the dragged cells --
        // Anchor on the `L59` row (re-found: the re-stick repaints).
        let anchor_row = wait_row(driver, "L59", Duration::from_secs(5))
            .ok_or_else(|| "the tail left the view before the drag".to_owned())?;
        let _ = tail_row;
        // The marker's first cell (find its column in the row).
        let grid = driver.grid_now();
        let row_text = grid.row_text(anchor_row);
        let marker_col = row_text
            .find("L59")
            .map(|byte| row_text[..byte].chars().count())
            .ok_or_else(|| format!("L59 lost from the row: {row_text}"))?;
        let (x0, y0) = (marker_col, anchor_row);
        let (x1, y1) = (marker_col + 10, anchor_row + 2);
        // Baseline styles BEFORE the press (the inside cells + a control
        // cell far outside the coming rect).
        let baseline = driver.grid_now();
        let control = (70usize, 0usize);
        let style_at = |g: &vt_harness::Grid, x: usize, y: usize| g.cell(x, y).style;
        let inside_cells: Vec<(usize, usize)> =
            (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).collect();
        let control_before = style_at(&baseline, control.0, control.1);

        // Real SGR strokes: press, drag across cells, hold — the LIVE
        // selection paints before any release.
        driver.sgr_press(u16::try_from(x0).unwrap_or(0), u16::try_from(y0).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(100));
        driver.sgr_drag(u16::try_from(x1).unwrap_or(79), u16::try_from(y1).unwrap_or(29));
        std::thread::sleep(Duration::from_millis(400));

        let dragged = driver.grid_now();
        let flipped = inside_cells
            .iter()
            .filter(|&&(x, y)| style_at(&dragged, x, y) != style_at(&baseline, x, y))
            .count();
        if flipped == 0 {
            return Err(format!(
                "the drag painted no reverse video: baseline vs dragged styles at {inside_cells:?} unchanged"
            ));
        }
        // real-machine ⑧ residue (W15)— the selection is a CHARACTER range (ref
        // text-buffer-view.zig setLocalSelectionStyle: cell coords →
        // char offsets, highlight over the occupied cells only): a row
        // whose text ends before the rect's right edge keeps its
        // background blanks UNSELECTED. The scripted lines are 3-4
        // cells wide against a 11-cell drag — the tail cells must not
        // flip.
        for y in y0..=y1 {
            let extent = grid.row_text(y).trim_end().chars().count().max(x0);
            for x in (extent + 1)..=x1 {
                if style_at(&dragged, x, y) != style_at(&baseline, x, y) {
                    return Err(format!(
                        "cell ({x},{y}) beyond the row's text extent ({extent}) flipped — \
                         the selection must clamp to actual characters (真机⑧)"
                    ));
                }
            }
        }
        // The control cell outside the rect keeps its style.
        if style_at(&dragged, control.0, control.1) != control_before {
            return Err("a cell outside the selection rect changed style".to_owned());
        }

        // -- ③ the release: copy-on-select clears the highlight + toast -
        driver.sgr_release(u16::try_from(x1).unwrap_or(79), u16::try_from(y1).unwrap_or(29));
        if wait_row(driver, "Copied to clipboard", Duration::from_secs(5)).is_none() {
            return Err("copy-on-select never toasted".to_owned());
        }
        // The highlight clears with the release (the selection ended).
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let cleared = loop {
            let g = driver.grid_now();
            let still = inside_cells
                .iter()
                .filter(|&&(x, y)| style_at(&g, x, y) != style_at(&baseline, x, y))
                .count();
            if still == 0 {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if !cleared {
            return Err("the selection highlight never cleared after the release".to_owned());
        }
        // And the toast eventually retires.
        if !wait_row_gone(driver, "Copied to clipboard", Duration::from_secs(8)) {
            return Err("the Copied toast never retired".to_owned());
        }
        Ok(())
    }
}
