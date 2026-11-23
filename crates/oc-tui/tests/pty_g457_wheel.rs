//! G-457 T1 regression — the real-machine wheel dead-path. A REAL
//! terminal differs from the stock PTY harness in two ways: it
//! answers the capability probe (DA1, kitty `CSI ? u`, DECRPM
//! 2026/2004/1004, OSC 11) BEFORE the user scrolls, and — the root
//! cause — `?1003` any-motion mode queues a motion report for every
//! pointer move. The pre-fix loop dispatched ONE event per full pump
//! pass (~9 ms), so a moving mouse grew an unbounded backlog and the
//! wheel behind it never visibly fired (beta.20 symptom 1). This
//! scenario replays a real terminal's boot replies + a 200-report
//! motion flood, then wheels, and pins the bounded-apply contract.

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

fn long_transcript_script(session: &str) -> Vec<ScriptEvent> {
    let lines = (0..60).map(|index| format!("L{index:02}")).collect::<Vec<_>>().join("\n");
    vec![
        bus_event(
            "evt_inbox",
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": session, "inboxID": "msg_user_1",
                "item": { "type": "user", "payload": { "text": "list the lines" } },
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

fn scenario(driver: &mut HarnessHandle) -> Result<(), String> {
    if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    // -- the REAL terminal's probe replies (WezTerm/Ghostty-like) ---
    // DECRPM: sync 2026 supported, paste 2004, focus 1004.
    driver.write(b"\x1b[?2026;2$y");
    driver.write(b"\x1b[?2004;1$y");
    driver.write(b"\x1b[?1004;1$y");
    // kitty keyboard reply + DA1 (the pair that finishes the probe and
    // triggers the rust input thread's KITTY_PUSH on real machines).
    driver.write(b"\x1b[?1;2u");
    driver.write(b"\x1b[?62;1;2;6;9;15;22c");
    // OSC 11 dark theme answer.
    driver.write(b"\x1b]11;rgb:1010/1010/1010\x07");
    // Focus-in (a real terminal sends ?1004 focus on click focus).
    driver.write(b"\x1b[I");
    std::thread::sleep(Duration::from_millis(400));

    open_first_session(driver, b"Alpha").map_err(|e| format!("session dialog: {e}"))?;
    driver.write(b"list the lines\r");
    if wait_row(driver, "L59", Duration::from_secs(10)).is_none() {
        return Err("the long reply never rendered its tail".to_owned());
    }
    let sticky = visible_markers(driver);
    if sticky.len() < 8 {
        return Err(format!("viewport too small: {sticky:?}"));
    }
    // a real any-motion (1003) flood — a real hand ALWAYS moves the
    // pointer across the transcript before the wheel turns; every
    // move is a separate SGR report in one read chunk.
    for i in 0..200u16 {
        let report = format!("\x1b[<35;{};{}M", 10 + (i % 60), 2 + (i % 25));
        driver.write(report.as_bytes());
    }
    // the wheel notch right behind the flood.
    let wheeled_at = std::time::Instant::now();
    driver.sgr_scroll_up(40, 15);
    // G-457 regression: the notch must VISIBLY apply within a
    // bounded window even behind a full any-motion backlog. Before
    // the event-batch fix this took ~1.7 s for 200 queued reports
    // (one full pump pass per event) — on a real machine's
    // continuous 60-120 Hz motion stream the backlog never drained
    // and the wheel read as dead.
    let applied = (|| -> Option<usize> {
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        let sticky0 = sticky.clone();
        while std::time::Instant::now() < deadline {
            let freed = visible_markers(driver);
            // The wheel-up signature is the first visible marker's
            // line number DECREASING. A transient sample where the
            // first marker moved the other way (CI geometry /
            // mid-drain re-render) is "not applied yet", never an
            // arithmetic underflow panic (CI: `attempt to subtract
            // with overflow` at this line) — keep polling.
            let step = freed
                .first()
                .zip(sticky0.first())
                .map_or(0, |(f, s)| s.checked_sub(*f).unwrap_or(0));
            if step != 0 {
                return Some(step);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    })();
    match applied {
        Some(step) if step <= sticky.len() => Ok(()),
        _ => Err(format!(
            "wheel lagged out behind the motion flood (applied={applied:?}, {} ms): {sticky:?}",
            wheeled_at.elapsed().as_millis()
        )),
    }
}

#[test]
fn pty_wheel_after_real_terminal_probe_replies() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("g457-wheel-forensics");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(long_transcript_script("ses_alpha"));
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
    result.expect("the wheel pipeline with real-terminal replies");
    let _ = sandbox;
}
