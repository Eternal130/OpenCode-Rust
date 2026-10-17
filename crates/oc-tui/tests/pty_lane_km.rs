//! Lane KM — the interaction layer's live-PTY proofs (auditA #8b/#9/
//! #10):
//!
//! * **tab strip advanced interactions** — a middle press closes the
//!   tab under the pointer, a right press opens v2's context menu
//!   (New session / Rename / Copy session ID / Close — the rows
//!   render at the press), and a press→drag→release across slots
//!   reorders the strip (the moved title lands in the target slot).
//! * **tool-row affordances** — a failed tool row's release reveals
//!   its error text (the errorExpanded branch) and hides it again.
//! * **toast action + home chips** — an MCP server entering a failed
//!   state toasts with the `Open MCP servers` action; the
//!   `toast.activate` chord (ctrl+alt+t) dispatches `mcp.list` (the
//!   MCP dialog opens); the home footer's `1 MCP failed` cell is a
//!   real release target.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::too_many_lines)] // the PTY drivers are one linear scenario each

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, LiveHarness, ScriptEvent, WORDMARK,
};

/// Wait until some row contains `needle`.
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

/// Wait until NO row contains `needle`.
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| u64::try_from(since.as_millis()).unwrap_or(1_000_000))
}

#[allow(clippy::needless_pass_by_value)] // serde_json::Value is the script's unit
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

/// The strip cycle: middle-click close, right-click menu, drag
/// reorder — over a live two-tab strip at 80 cols (slots 0..32,
/// 32..64).
#[test]
fn pty_km_tab_strip_middle_right_and_drag() {
    eprintln!("lane-km: test start");
    let _lock = pty_lock();
    eprintln!("lane-km: lock held");
    let sandbox = sandbox_env("lane-km-strip");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        // TWO tabs: the first session, then the second as a second
        // tab (slots 0..32 / 32..64 at 80 cols).
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the first session never opened: {error}"))?;
        vt_harness::open_second_session(&mut driver, b"Long")
            .map_err(|error| format!("the second tab never opened: {error}"))?;
        wait_row(&driver, "Long Session", Duration::from_secs(5))
            .ok_or_else(|| "the strip never painted the second tab".to_owned())?;

        // RIGHT press on the second tab's body (x=40): the context
        // menu opens with v2's four rows.
        driver.sgr_right_press(40, 0);
        wait_row(&driver, "Copy session ID", Duration::from_secs(3))
            .ok_or_else(|| "the context menu never rendered its rows".to_owned())?;
        if wait_row(&driver, "New session", Duration::from_millis(300)).is_none() {
            return Err("the menu's New session row is missing".to_owned());
        }
        driver.write(b"\x1b");
        if !wait_row_gone(&driver, "Copy session ID", Duration::from_secs(2)) {
            return Err("escape never closed the menu".to_owned());
        }

        // MIDDLE press on the second tab's body: closes on the press
        // (the long title leaves the strip).
        driver.sgr_middle_press(40, 0);
        if !wait_row_gone(&driver, "Long Session", Duration::from_secs(3)) {
            return Err("the middle press never closed the second tab".to_owned());
        }

        // Reopen the second tab (closed by the middle press); drag
        // the FIRST slot over the second: press at x=5, drag to x=50,
        // release — the dragged title lands in the second slot.
        vt_harness::open_second_session(&mut driver, b"Long")
            .map_err(|error| format!("the reopen failed: {error}"))?;
        wait_row(&driver, "Long Session", Duration::from_secs(5))
            .ok_or_else(|| "the strip never re-painted the second tab".to_owned())?;
        std::thread::sleep(Duration::from_millis(400));
        let before = driver.grid_now().row_text(0);
        driver.sgr_press(5, 0);
        std::thread::sleep(Duration::from_millis(150));
        driver.sgr_drag(50, 0);
        std::thread::sleep(Duration::from_millis(250));
        driver.sgr_release(50, 0);
        std::thread::sleep(Duration::from_millis(600));
        let after = driver.grid_now().row_text(0);
        if before == after {
            return Err(format!("the drag never reordered the strip: {before:?}"));
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    if let Err(error) = &result {
        eprintln!("lane-km driver failed: {error}");
    }
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the strip cycle");
    let _ = sandbox;
}

/// The failed tool row's error reveal (auditA #9): a failed `read`
/// renders the `InlineTool` row; a release on it reveals the error
/// text; a second release hides it.
#[test]
fn pty_km_failed_tool_error_reveal() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-km-toolrow");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(vec![
        bus_event(
            "evt_inbox",
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": "ses_alpha",
                "inboxID": "msg_user_1",
                "item": { "type": "user", "payload": { "text": "read it" } },
            }),
        ),
        bus_event(
            "evt_step",
            "session.step.started",
            serde_json::json!({ "sessionID": "ses_alpha", "stepID": "step_1" }),
        ),
        bus_event(
            "evt_t1",
            "session.tool.input.started",
            serde_json::json!({
                "sessionID": "ses_alpha",
                "assistantMessageID": "msg_asst_1",
                "id": "tool_probe_1",
                "name": "probe",
            }),
        ),
        bus_event(
            "evt_t2",
            "session.tool.called",
            serde_json::json!({
                "sessionID": "ses_alpha",
                "assistantMessageID": "msg_asst_1",
                "id": "tool_probe_1",
                "input": { "path": "/gone/file.rs" },
            }),
        ),
        bus_event(
            "evt_t3",
            "session.tool.failed",
            serde_json::json!({
                "sessionID": "ses_alpha",
                "assistantMessageID": "msg_asst_1",
                "id": "tool_probe_1",
                "metadata": {},
                "error": { "message": "ENOENT: no such file" },
            }),
        ),
        bus_event(
            "evt_step_end",
            "session.step.ended",
            serde_json::json!({ "sessionID": "ses_alpha", "stepID": "step_1" }),
        ),
        bus_event(
            "evt_idle",
            "session.idle",
            serde_json::json!({ "sessionID": "ses_alpha", "id": "msg_idle_1" }),
        ),
    ]);
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session never opened: {error}"))?;
        driver.write(b"read it\r");
        let tool_row = wait_row(&driver, "probe [path=/gone/file.rs]", Duration::from_secs(10))
            .ok_or_else(|| "the failed probe row never rendered".to_owned())?;
        // The error text is hidden until the release.
        if wait_row(&driver, "ENOENT", Duration::from_millis(300)).is_some() {
            return Err("the error revealed without a click".to_owned());
        }
        let tool_row = u16::try_from(tool_row).unwrap_or(u16::MAX);
        driver.sgr_release(10, tool_row);
        wait_row(&driver, "ENOENT: no such file", Duration::from_secs(3))
            .ok_or_else(|| "the release never revealed the error".to_owned())?;
        driver.sgr_release(10, tool_row);
        if !wait_row_gone(&driver, "ENOENT", Duration::from_secs(3)) {
            return Err("the second release never hid the error".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    if let Err(error) = &result {
        eprintln!("lane-km driver failed: {error}");
    }
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the error-reveal cycle");
    let _ = sandbox;
}

/// The toast action + the home MCP chip (auditA #10): arming the MCP
/// fetch (one connected + one failed server) toasts the failure with
/// the `Open MCP servers` action; ctrl+alt+t activates it (the MCP
/// dialog opens); after the dismiss, the home footer's `1 MCP
/// failed` cell opens the same dialog on release.
#[test]
fn pty_km_toast_action_and_home_chip() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-km-toast");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_mcp();
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        // The failed server toasts (app.tsx:522-545).
        wait_row(&driver, "MCP server failed: broken", Duration::from_secs(8))
            .ok_or_else(|| "the MCP failure toast never rendered".to_owned())?;
        // The keyboard activation chord: the action dispatches
        // mcp.list — the servers dialog opens.
        driver.write(b"\x1b[116;7u");
        wait_row(&driver, "MCP servers", Duration::from_secs(3))
            .ok_or_else(|| "the toast action never opened the MCP dialog".to_owned())?;
        driver.write(b"\x1b");
        if !wait_row_gone(&driver, "MCP servers", Duration::from_secs(2)) {
            return Err("escape never closed the MCP dialog".to_owned());
        }
        // The home footer's failure cell: `1 MCP failed` + `/mcps`.
        let chip_row = wait_row(&driver, "1 MCP failed", Duration::from_secs(3))
            .ok_or_else(|| "the home MCP chip never rendered".to_owned())?;
        // Release on the cell (col 2, the `⊙ ` dot): mcp.list again.
        driver.sgr_release(3, u16::try_from(chip_row).unwrap_or(u16::MAX));
        wait_row(&driver, "MCP servers", Duration::from_secs(3))
            .ok_or_else(|| "the chip release never opened the MCP dialog".to_owned())?;
        driver.write(b"\x1b");
        if !wait_row_gone(&driver, "MCP servers", Duration::from_secs(3)) {
            return Err("escape never closed the chip-opened MCP dialog".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 24, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    if let Err(error) = &result {
        eprintln!("lane-km driver failed: {error}");
    }
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the toast + chip cycle");
    let _ = sandbox;
}
