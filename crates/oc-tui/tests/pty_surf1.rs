//! SURF-1 PTY proofs — the transcript detail states (auditB #5) and
//! the sidebar's live content (auditB #3) against the REAL app loop
//! over a pty, asserted on the parsed CELL GRID:
//!
//! * the ungrouped shell tool renders its detail cell (`$ echo hi` +
//!   the collapsed output preview) — message/list.rs:288 rendered
//!   NOTHING before the lane;
//! * the skill row (`→ Skill commitlint`), the queued-compaction
//!   divider (`◇ Compaction queued`), the completed compaction
//!   divider with usage (`1,200 in`, `200 out`) + summary markdown;
//! * the execute card's open→scroll→close dialog cycle
//!   (`DialogExecute`: click opens, arrows scroll, escape closes);
//! * the revert banner cycle: stage hides the boundary tail + paints
//!   "1 message reverted", the banner's release dismisses it and the
//!   hidden rows return;
//! * the sidebar's live sections: the context token meter (`Context`
//!   plus `60,400 tokens`), the MCP rows (`Connected` and `Error`
//!   dots, served by the `/api/mcp` fixture) and the `{dir}:{branch}`
//!   location row (`:main`).

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
#![allow(clippy::too_many_lines)] // one linear scenario per proof

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, LiveHarness, WORDMARK,
};

/// Find `(row, col)` of the first grid row containing `needle`.
fn find_row(h: &HarnessHandle, needle: &str) -> Option<(usize, usize)> {
    let grid = h.grid_now();
    for row in 0..grid.rows {
        let text = grid.row_text(row);
        if let Some(col) = text.find(needle) {
            return Some((row, col));
        }
    }
    None
}

/// Wait until some grid row contains `needle`, returning its row.
fn wait_row(h: &HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Some((row, _)) = find_row(h, needle) {
            return Some(row);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

/// Wait until NO grid row contains `needle`.
fn wait_gone(h: &HarnessHandle, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if find_row(h, needle).is_none() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// The SURF-1 scenario: one turn carrying the shell + execute tools,
/// a skill message, the compaction lifecycle (queued + completed) and
/// — last — the staged revert. `session.step.ended` carries the token
/// payload that feeds the sidebar's context meter.
fn surf1_script(session: &str) -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    let event = |delay: u64, body: String| vt_harness::ScriptEvent { delay_ms: delay, body };
    vec![
        event(
            30,
            format!(
                r#"{{"id":"evt_inbox","created":{now_ms},"type":"session.inbox.enqueued","data":{{"sessionID":"{session}","inboxID":"msg_user_1","item":{{"type":"user","payload":{{"text":"run things"}}}}}}}}"#
            ),
        ),
        event(
            60,
            format!(
                r#"{{"id":"evt_step","created":{now_ms},"type":"session.step.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","agent":"build","model":{{"providerID":"anthropic","id":"claude-sonnet"}}}}}}"#
            ),
        ),
        event(
            90,
            format!(
                r#"{{"id":"evt_t1","created":{now_ms},"type":"session.tool.input.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","id":"tool_sh","name":"bash"}}}}"#
            ),
        ),
        event(
            120,
            format!(
                r#"{{"id":"evt_t2","created":{now_ms},"type":"session.tool.called","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","id":"tool_sh","input":{{"command":"echo hi","workdir":"."}}}}}}"#
            ),
        ),
        event(
            180,
            format!(
                r#"{{"id":"evt_t3","created":{now_ms},"type":"session.tool.success","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","id":"tool_sh","metadata":{{}},"content":[{{"type":"text","text":"hi\nsecond line"}}]}}}}"#
            ),
        ),
        event(
            210,
            format!(
                r#"{{"id":"evt_t4","created":{now_ms},"type":"session.tool.input.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","id":"tool_ex","name":"execute"}}}}"#
            ),
        ),
        event(
            240,
            format!(
                r#"{{"id":"evt_t5","created":{now_ms},"type":"session.tool.called","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","id":"tool_ex","input":{{"description":"run tests"}}}}}}"#
            ),
        ),
        event(
            300,
            format!(
                r#"{{"id":"evt_t6","created":{now_ms},"type":"session.tool.failed","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","id":"tool_ex","metadata":{{"toolCalls":[{{"tool":"read","status":"completed","input":{{"path":"x.rs"}}}}]}},"content":[{{"type":"text","text":"boom line"}}],"error":{{"type":"tool","message":"exit 1"}}}}}}"#
            ),
        ),
        event(
            340,
            format!(
                concat!(
                    r#"{{"id":"evt_skill","created":{now_ms},"type":"message.updated","data":{{"#,
                    r#""sessionID":"{session}","id":"msg_skill","type":"skill","name":"commitlint","#,
                    r#""text":"","time":{{"created":{now_ms}}}}}}}"#
                ),
                now_ms = now_ms,
                session = session
            ),
        ),
        // A queued compaction that never starts — the `◇ Compaction
        // queued` divider stays.
        event(
            380,
            format!(
                r#"{{"id":"evt_cq","created":{now_ms},"type":"session.inbox.enqueued","data":{{"sessionID":"{session}","inboxID":"inbox_comp","item":{{"type":"compaction"}}}}}}"#
            ),
        ),
        event(
            420,
            format!(
                r#"{{"id":"evt_cs","created":{now_ms},"type":"session.compaction.started","data":{{"sessionID":"{session}","inputID":"msg_comp","reason":"manual"}}}}"#
            ),
        ),
        event(
            520,
            format!(
                r#"{{"id":"evt_ce","created":{now_ms},"type":"session.compaction.ended","data":{{"sessionID":"{session}","text":"Condensed the early turns.","tokens":{{"input":1000,"output":200,"reasoning":0,"cache":{{"read":200,"write":0}}}},"providerContext":{{"used":100}}}}}}"#
            ),
        ),
        // The first turn ends (footer; no token payload — the
        // compaction below resets the context meter).
        event(
            560,
            format!(
                r#"{{"id":"evt_se","created":{now_ms},"type":"session.step.ended","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","finish":"stop"}}}}"#
            ),
        ),
        // The next turn's step carries the usage the sidebar's
        // context meter reads (`usage_total` — post-compaction).
        event(
            620,
            format!(
                r#"{{"id":"evt_s2","created":{now_ms},"type":"session.step.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_2","agent":"build","model":{{"providerID":"anthropic","id":"claude-sonnet"}}}}}}"#
            ),
        ),
        event(
            650,
            format!(
                r#"{{"id":"evt_se2","created":{now_ms},"type":"session.step.ended","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_2","finish":"stop","tokens":{{"input":9000,"output":1200,"reasoning":100,"cache":{{"read":50000,"write":100}}}}}}}}"#
            ),
        ),
        // LAST: stage the revert at the user message — everything
        // with id >= `msg_user_1` hides (only the user row itself).
        event(
            720,
            format!(
                r#"{{"id":"evt_rv","created":{now_ms},"type":"session.revert.staged","data":{{"sessionID":"{session}","revert":{{"messageID":"msg_user_1","files":[{{"file":"a.py","status":"modified","additions":3,"deletions":1}}]}}}}}}"#
            ),
        ),
    ]
}

/// Item 1 + 2 in one live walk (the scenario is one scripted turn):
/// the detail states render, the dialog cycles, the revert banner
/// dismisses, and the sidebar's live sections paint against the
/// served `/api/mcp` + vcs fixtures.
#[test]
fn pty_surf1_detail_states_and_sidebar_live_content() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-surf1");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_vcs();
    fake.arm_mcp();
    fake.arm_script(surf1_script("ses_alpha"));
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        driver.write(b"run things\r");

        // -- the shell tool's detail cell (ungrouped tools rendered
        //    NOTHING before SURF-1) ------------------------------------
        wait_row(&driver, "$ echo hi", Duration::from_secs(10))
            .ok_or_else(|| "the shell cell's command row never rendered".to_owned())?;
        wait_row(&driver, "second line", Duration::from_secs(5))
            .ok_or_else(|| "the shell cell's output preview never rendered".to_owned())?;

        // -- the skill row -------------------------------------------------
        wait_row(&driver, "Skill commitlint", Duration::from_secs(5))
            .ok_or_else(|| "the skill row never rendered".to_owned())?;

        // -- the execute card + its detail dialog's open→scroll→close ---
        let ex_row = wait_row(&driver, "execute", Duration::from_secs(5))
            .ok_or_else(|| "the execute card never rendered".to_owned())?;
        // The failed execute card's error output preview (4 lines).
        wait_row(&driver, "boom line", Duration::from_secs(5))
            .ok_or_else(|| "the execute error preview never rendered".to_owned())?;
        // Release on the card opens DialogExecute (title `Execute` —
        // the cell's own label is lowercase `execute`).
        driver.sgr_press(5, u16::try_from(ex_row).unwrap_or(u16::MAX));
        driver.sgr_release(5, u16::try_from(ex_row).unwrap_or(u16::MAX));
        wait_row(&driver, "Execute", Duration::from_secs(5))
            .ok_or_else(|| "the execute dialog never opened".to_owned())?;
        wait_row(&driver, "description: run tests", Duration::from_secs(5))
            .ok_or_else(|| "the dialog's input rows never rendered".to_owned())?;
        // Scroll: three arrow-downs move the select's cursor.
        driver.write(b"\x1b[B\x1b[B\x1b[B");
        std::thread::sleep(Duration::from_millis(250));
        // Close: escape dismisses; the card stays.
        driver.write(b"\x1b");
        if !wait_gone(&driver, "description: run tests", Duration::from_secs(5)) {
            return Err("the execute dialog never closed".to_owned());
        }
        if find_row(&driver, "execute").is_none() {
            return Err("the execute card vanished after the dialog closed".to_owned());
        }

        // -- the compaction lifecycle --------------------------------------
        wait_row(&driver, "Compaction queued", Duration::from_secs(5))
            .ok_or_else(|| "the queued-compaction divider never rendered".to_owned())?;
        let divider = wait_row(&driver, "1,200 in", Duration::from_secs(5))
            .ok_or_else(|| "the completed compaction usage never rendered".to_owned())?;
        let line = {
            let grid = driver.grid_now();
            grid.row_text(divider)
        };
        // `providerContext` in the fixture flips the label to
        // "Provider compaction" (index.tsx:2044-2047).
        if !line.contains("200 out") || !line.contains("Provider compaction") {
            return Err(format!("the compaction divider row is malformed: {line}"));
        }
        wait_row(&driver, "Condensed the early turns.", Duration::from_secs(5))
            .ok_or_else(|| "the compaction summary markdown never rendered".to_owned())?;

        // -- the sidebar's live content (docked at 160) --------------------
        wait_row(&driver, "Context", Duration::from_secs(5))
            .ok_or_else(|| "the sidebar's Context section never rendered".to_owned())?;
        wait_row(&driver, "60,400 tokens", Duration::from_secs(5))
            .ok_or_else(|| "the context token meter never rendered".to_owned())?;
        wait_row(&driver, "MCP", Duration::from_secs(5))
            .ok_or_else(|| "the sidebar's MCP section never rendered".to_owned())?;
        // Three servers (the fixture) — the collapse header renders
        // open by default: per-server rows with their statuses.
        wait_row(&driver, "Connected", Duration::from_secs(5))
            .ok_or_else(|| "the MCP server rows never rendered".to_owned())?;
        wait_row(&driver, "Error", Duration::from_secs(5))
            .ok_or_else(|| "the failed MCP server's status never rendered".to_owned())?;
        // The `{dir}:{branch}` footer location row.
        wait_row(&driver, ":main", Duration::from_secs(5))
            .ok_or_else(|| "the sidebar location row never rendered".to_owned())?;

        // -- the revert banner cycle ---------------------------------------
        // (Sticky at the tail: the banner renders after the last row.
        // The user row itself hides under the boundary filter — the
        // unit suite pins the filter; here the banner + dismiss
        // cycle is the live surface.)
        let banner = wait_row(&driver, "1 message reverted", Duration::from_secs(5))
            .ok_or_else(|| "the revert banner never rendered".to_owned())?;
        wait_row(&driver, "+3", Duration::from_secs(2))
            .ok_or_else(|| "the banner's file row never rendered".to_owned())?;
        // Release on the banner dismisses it (revert.clear + the local
        // state drops) — the hidden rows return.
        driver.sgr_press(5, u16::try_from(banner).unwrap_or(u16::MAX));
        driver.sgr_release(5, u16::try_from(banner).unwrap_or(u16::MAX));
        if !wait_gone(&driver, "message reverted", Duration::from_secs(5)) {
            return Err("the revert banner never dismissed".to_owned());
        }
        // `home` (`session.first`) jumps to the top: the reverted
        // user row is back above the fold.
        driver.write(b"\x1b[H");
        wait_row(&driver, "run things", Duration::from_secs(5))
            .ok_or_else(|| "the reverted rows never returned after the dismiss".to_owned())?;

        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 160, 30, Duration::from_secs(150));
    });
    let result = span.join().expect("driver thread");
    if let Err(error) = &result {
        eprintln!("pty_surf1 driver error: {error}");
    }
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the surf1 scenario");
    let _ = sandbox;
}
