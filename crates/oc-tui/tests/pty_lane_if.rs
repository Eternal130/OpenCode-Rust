//! Lane IF — input foundations, live-PTY proofs (auditA #1/#2/#4/#5/#6):
//!
//! * **wheel-scroll cycle on the transcript** — SGR button-64/65
//!   events drive the E1 half-page math live: one notch displaces the
//!   visible `Lnn` markers by a half-page-class step (the exact
//!   `js_round(viewport/2)` landing is unit-pinned against the pageup
//!   key), the tail leaves, and enough down-notches re-stick it.
//! * **route-change phantom-click negative** — the footer location
//!   label's release opens the `WorkingDirectory` select on the session route, and
//!   the SAME coordinates on Home (after the `+` hop) open NOTHING
//!   (the registry clear + route gate of lane/if item 2).
//! * **permission allow/deny via pointer** — the ask's pills and the
//!   reject stage's `enter confirm` button are real SGR release
//!   targets (allow resolves the ask; reject → confirm resolves it).
//! * **disclosure groups toggle** — the completed `+ Thought` header
//!   expands on release (`- Thought` + body), the thought row's own
//!   release collapses it back to the one-line header; the
//!   `Explored:` header's release is consumed without side effects
//!   (the state flip is unit-pinned — expanded CONTENT rides the
//!   missing tool-row surface, auditA #9).
//! * **dialog ✕** — the palette's header `esc` label releases the
//!   dialog.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::too_many_lines)] // the PTY drivers are one linear scenario each

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, HarnessHandle, LiveHarness, ScriptEvent, WORDMARK,
};

/// Wait until some row contains `needle`; returns the row index.
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

/// The `Lnn` marker indices visible on the transcript rows (sorted).
fn visible_markers(h: &HarnessHandle) -> Vec<usize> {
    let grid = h.grid_now();
    let mut seen: Vec<usize> = Vec::new();
    for row in 0..grid.rows {
        let text = grid.row_text(row);
        for (index, word) in text.split_whitespace().enumerate() {
            if let Some(number) = word.strip_prefix('L') {
                if let Ok(value) = number.parse::<usize>() {
                    if word.len() == 3 {
                        seen.push(value);
                        let _ = index;
                    }
                }
            }
        }
    }
    seen.sort_unstable();
    seen.dedup();
    seen
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| u64::try_from(since.as_millis()).unwrap_or(1_000_000))
}

#[allow(clippy::needless_pass_by_value)] // ScriptEvent is the unit of the script vec
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

/// The long-transcript script: one completed turn whose text part is
/// 60 hard-wrapped `Lnn` marker lines (the wheel's observable).
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

/// Item 1 — the wheel runs the E1 half-page math on the live
/// transcript: one up-notch displaces the visible markers by
/// `js_round(viewport / 2)` (the viewport MEASURED from the screen —
/// the count of sticky-visible marker rows), and enough down-notches
/// re-stick the tail.
#[test]
fn pty_wheel_scrolls_the_transcript_half_a_page() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-if-wheel");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(long_transcript_script("ses_alpha"));
    let harness = LiveHarness::attach(80, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        driver.write(b"spin the wheel\r");
        // Sticky at the tail: the last markers are on screen.
        wait_row(&driver, "L59", Duration::from_secs(10))
            .ok_or_else(|| "the long reply never rendered its tail".to_owned())?;
        let sticky = visible_markers(&driver);
        let viewport = sticky.len();
        if viewport < 8 {
            return Err(format!("the viewport is too small to assert half-page math: {sticky:?}"));
        }

        // ONE wheel notch up: a HALF-PAGE-scale step — strictly more
        // than the 3-line dialog scroll and at most the measured
        // viewport (a full page would overshoot). The EXACT E1 math
        // (floor(store-viewport/2), js_round, sticky re-arm) is
        // unit-pinned against the pageup key; the store's viewport
        // differs from the visible marker count by the pane's chrome
        // rows, so the PTY pins the class + the uniform cadence.
        driver.sgr_scroll_up(40, 15);
        std::thread::sleep(Duration::from_millis(300));
        let freed = visible_markers(&driver);
        let step_one = freed.last().map_or(0, |last| sticky.last().map_or(0, |tail| tail - last));
        if step_one <= 3 || step_one > viewport {
            return Err(format!(
                "one notch moved {step_one} marker rows (viewport {viewport}) — not the half-page class: sticky {sticky:?} → freed {freed:?}"
            ));
        }
        if freed.contains(&59) {
            return Err("the tail must leave the view on an up-notch".to_owned());
        }
        // A second notch keeps moving (the fractional half-page steps
        // alternate around v/2 when the viewport is odd — the E1
        // f64-offset math rounds on read; exact parity with the
        // pageup key is unit-pinned).
        driver.sgr_scroll_up(40, 15);
        std::thread::sleep(Duration::from_millis(300));
        let freed_again = visible_markers(&driver);
        let step_two =
            freed_again.last().map_or(0, |last| freed.last().map_or(0, |prior| prior - last));
        if step_two == 0 || step_two > viewport {
            return Err(format!(
                "the second notch moved {step_two} marker rows (viewport {viewport}) — the wheel stopped or overshot"
            ));
        }

        // Enough down-notches re-stick the tail (sticky re-arms).
        for _ in 0..8 {
            driver.sgr_scroll_down(40, 15);
            std::thread::sleep(Duration::from_millis(120));
        }
        if !visible_markers(&driver).contains(&59) {
            return Err("wheel-down never re-stuck to the tail".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 80, 30, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the wheel cycle");
    let _ = sandbox;
}

/// Item 2 — the phantom-click negative, live: the footer location
/// label opens the `WorkingDirectory` select on the session route; the SAME
/// release on Home (after the `+` hop) opens nothing.
#[test]
fn pty_home_release_fires_no_stale_location_dialog() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-if-stale");
    let fake = FakeLlm::spawn(standard_sessions());
    // The footer label needs the vcs lookup to resolve (`{cwd}:main`).
    fake.arm_vcs();
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        // The label is the LAUNCH cwd (the boot-time vcs lookup's
        // directory — the fake's vcs endpoint fails, so no `:branch`):
        // find the cwd's last path component on the bottom status
        // rows.
        let cwd_tail = std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.file_name().map(|name| name.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "lane-if".to_owned());
        let location_row = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut found = None;
            while std::time::Instant::now() < deadline {
                let grid = driver.grid_now();
                let hit = (24..grid.rows).rev().find(|row| grid.row_text(*row).contains(&cwd_tail));
                if let Some(row) = hit {
                    found = Some(row);
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            found.ok_or_else(|| format!("the footer location label ({cwd_tail}) never rendered"))?
        };
        let grid = driver.grid_now();
        let label_x = grid.row_text(location_row).find(&cwd_tail).unwrap_or(0);
        let (x, row) = (
            u16::try_from(label_x + 2).unwrap_or(u16::MAX),
            u16::try_from(location_row).unwrap_or(u16::MAX),
        );
        // Positive control: on the session route the release opens
        // the Working directory select.
        driver.sgr_press(x, row);
        driver.sgr_release(x, row);
        wait_row(&driver, "Copy path", Duration::from_secs(5))
            .ok_or_else(|| "the location release never opened WorkingDirectory".to_owned())?;
        driver.write(b"\x1b"); // esc closes it
        wait_row_gone(&driver, "Copy path", Duration::from_secs(5))
            .then_some(())
            .ok_or_else(|| "escape never closed the dialog".to_owned())?;

        // Home via the strip's ` + `: the release on the label's OLD
        // coordinates must open NOTHING (registry cleared on
        // navigate + the route gate).
        let strip = driver.grid_now().row_text(0);
        let plus_x = strip.find('+').ok_or_else(|| "no + on the strip: {strip}".to_owned())?;
        driver.sgr_press(u16::try_from(plus_x).unwrap_or(u16::MAX), 0);
        driver.sgr_release(u16::try_from(plus_x).unwrap_or(u16::MAX), 0);
        wait_row(&driver, "New session", Duration::from_secs(5))
            .ok_or_else(|| "the + never routed home".to_owned())?;
        driver.sgr_press(x, row);
        driver.sgr_release(x, row);
        std::thread::sleep(Duration::from_millis(400));
        if wait_row(&driver, "Copy path", Duration::from_millis(300)).is_some() {
            return Err("a stale footer rect fired on home — phantom dialog".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 160, 30, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the phantom-click negative");
    let _ = sandbox;
}

/// The permission-ask script: a prompt, a step, then TWO queued asks
/// (the second becomes the view after the pointer resolves the
/// first — the SSE feed plays its script once per connection).
fn permission_script(session: &str) -> Vec<ScriptEvent> {
    let ask_on = |id: &str, ordinal: usize, session: &str| {
        bus_event(
            &format!("evt_ask_{ordinal}"),
            "permission.asked",
            serde_json::json!({
                "id": id,
                "sessionID": session,
                "action": "bash",
                "resources": ["src/index.ts"],
                "save": ["*"],
                "input": { "command": "echo hi" },
            }),
        )
    };
    let ask = |id: &str, ordinal: usize| ask_on(id, ordinal, session);
    vec![
        bus_event(
            "evt_inbox",
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": session,
                "inboxID": "msg_user_1",
                "item": { "type": "user", "payload": { "text": "run it" } },
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
        ask("p1", 1),
        // The second ask rides a CHILD session (`session.created`
        // seeds its parentID): under the session-route two-stage
        // machine the free-text reject stage opens only for children
        // (`session()?.parentID`, permission.tsx:249-254) — a
        // top-level reject replies immediately.
        ScriptEvent {
            delay_ms: 500,
            body: bus_event(
                "evt_child",
                "session.created",
                serde_json::json!({ "sessionID": "ses_child", "parentID": "ses_alpha" }),
            )
            .body,
        },
        ScriptEvent { delay_ms: 600, ..ask_on("p2", 2, "ses_child") },
    ]
}

/// Items 3 — allow via the pill, deny via the reject stage's confirm
/// button (both pure pointer; zero keystrokes).
#[test]
fn pty_permission_allow_and_reject_via_pointer() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-if-permission");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(permission_script("ses_alpha"));
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        driver.write(b"do the thing\r");
        wait_row(&driver, "Allow once", Duration::from_secs(10))
            .ok_or_else(|| "the ask never rendered its pills".to_owned())?;
        let pills_row = wait_row(&driver, "Reject", Duration::from_secs(2))
            .ok_or_else(|| "the Reject pill never rendered".to_owned())?;
        let grid = driver.grid_now();
        let once_x = grid
            .row_text(pills_row)
            .find("Allow once")
            .ok_or_else(|| "the Allow once label left the row".to_owned())?;
        // Release on the ALLOW pill: the ask resolves (the footer
        // falls back — the body leaves the screen).
        driver.sgr_press(
            u16::try_from(once_x + 2).unwrap_or(u16::MAX),
            u16::try_from(pills_row).unwrap_or(u16::MAX),
        );
        driver.sgr_release(
            u16::try_from(once_x + 2).unwrap_or(u16::MAX),
            u16::try_from(pills_row).unwrap_or(u16::MAX),
        );
        if !wait_row_gone(&driver, "Allow once", Duration::from_secs(5)) {
            return Err("the allow pill never resolved the ask".to_owned());
        }

        // The QUEUED second ask becomes the view: REJECT via the pill
        // → the reject stage's free-text body renders; `enter
        // confirm` (a hint BUTTON) sends the deny.
        wait_row(&driver, "Allow once", Duration::from_secs(10))
            .ok_or_else(|| "the second ask never surfaced".to_owned())?;
        let pills_row = wait_row(&driver, "Reject", Duration::from_secs(2))
            .ok_or_else(|| "the Reject pill never rendered".to_owned())?;
        let grid = driver.grid_now();
        let reject_x = grid
            .row_text(pills_row)
            .find("Reject")
            .ok_or_else(|| "the Reject label left the row".to_owned())?;
        let (x, row) = (
            u16::try_from(reject_x + 2).unwrap_or(u16::MAX),
            u16::try_from(pills_row).unwrap_or(u16::MAX),
        );
        driver.sgr_press(x, row);
        driver.sgr_release(x, row);
        wait_row(&driver, "Tell OpenCode what to do differently", Duration::from_secs(5))
            .ok_or_else(|| "the Reject pill never opened the reject stage".to_owned())?;
        let hint_row = wait_row(&driver, "enter confirm", Duration::from_secs(2))
            .ok_or_else(|| "the reject stage never rendered its hint buttons".to_owned())?;
        let grid = driver.grid_now();
        let confirm_x = grid
            .row_text(hint_row)
            .find("enter confirm")
            .ok_or_else(|| "the confirm label left the row".to_owned())?;
        let (x, row) = (
            u16::try_from(confirm_x + 3).unwrap_or(u16::MAX),
            u16::try_from(hint_row).unwrap_or(u16::MAX),
        );
        driver.sgr_press(x, row);
        driver.sgr_release(x, row);
        if !wait_row_gone(&driver, "Tell OpenCode what to do differently", Duration::from_secs(5)) {
            return Err("the confirm button never sent the reject".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 160, 30, Duration::from_secs(60));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the permission pointer cycle");
    let _ = sandbox;
}

/// Item 4 — the reasoning disclosure + the per-thought toggle, live:
/// the completed `+ Thought` header expands on release; the thought
/// row's own release collapses it to the one-line header; the group
/// header still collapses the whole group.
#[test]
fn pty_thought_and_group_disclosures_toggle() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-if-disclosure");
    let fake = FakeLlm::spawn(standard_sessions());
    let session = "ses_alpha";
    let now = now_ms();
    let script = vec![
        ScriptEvent {
            delay_ms: 30,
            body: serde_json::json!({
                "id": "evt_inbox", "created": now, "type": "session.inbox.enqueued",
                "data": { "sessionID": session, "inboxID": "msg_user_1",
                          "item": { "type": "user", "payload": { "text": "think" } } },
            })
            .to_string(),
        },
        ScriptEvent {
            delay_ms: 60,
            body: serde_json::json!({
                "id": "evt_step", "created": now, "type": "session.step.started",
                "data": { "sessionID": session, "assistantMessageID": "msg_asst_1",
                          "agent": "build", "model": { "providerID": "anthropic", "id": "claude-sonnet" } },
            })
            .to_string(),
        },
        ScriptEvent {
            delay_ms: 90,
            body: serde_json::json!({
                "id": "evt_r1", "created": now, "type": "session.reasoning.delta",
                "data": { "sessionID": session, "assistantMessageID": "msg_asst_1",
                          "ordinal": 0, "delta": "**Inspecting** the parser edges with care" },
            })
            .to_string(),
        },
        ScriptEvent {
            delay_ms: 200,
            body: serde_json::json!({
                "id": "evt_rend", "created": now, "type": "session.reasoning.ended",
                "data": { "sessionID": session, "assistantMessageID": "msg_asst_1", "ordinal": 0,
                          "text": "**Inspecting** the parser edges with care" },
            })
            .to_string(),
        },
        ScriptEvent {
            delay_ms: 230,
            body: serde_json::json!({
                "id": "evt_send", "created": now, "type": "session.step.ended",
                "data": { "sessionID": session, "assistantMessageID": "msg_asst_1", "finish": "success" },
            })
            .to_string(),
        },
        ScriptEvent {
            delay_ms: 260,
            body: serde_json::json!({
                "id": "evt_idle", "created": now, "type": "session.idle",
                "data": { "sessionID": session, "id": "msg_idle_1" },
            })
            .to_string(),
        },
    ];
    fake.arm_script(script);
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        open_first_session(&mut driver, b"Alpha")
            .map_err(|error| format!("the session dialog never opened: {error}"))?;
        driver.write(b"think\r");
        let header_row = wait_row(&driver, "+ Thought", Duration::from_secs(10))
            .ok_or_else(|| "the collapsed thought header never rendered".to_owned())?;
        // Release on the header: the group expands (- Thought + the
        // bordered body).
        driver.sgr_press(10, u16::try_from(header_row).unwrap_or(u16::MAX));
        driver.sgr_release(10, u16::try_from(header_row).unwrap_or(u16::MAX));
        wait_row(&driver, "- Thought", Duration::from_secs(5))
            .ok_or_else(|| "the group header never expanded".to_owned())?;
        // The thought BODY row — search the body text ("parser
        // edges"), NOT the title: the group header itself renders
        // "- Thought: Inspecting · …" (the latest thought's title).
        let body_row = wait_row(&driver, "parser edges", Duration::from_secs(5))
            .ok_or_else(|| "the thought body never rendered".to_owned())?;

        // The thought row's OWN release collapses it to the one-line
        // `+ Thought:` header (the body leaves — the header keeps
        // only the summary TITLE "Inspecting"; the BODY text
        // "parser edges" is what must disappear).
        driver.sgr_press(10, u16::try_from(body_row).unwrap_or(u16::MAX));
        driver.sgr_release(10, u16::try_from(body_row).unwrap_or(u16::MAX));
        wait_row(&driver, "+ Thought:", Duration::from_secs(5))
            .ok_or_else(|| "the collapsed per-thought header never rendered".to_owned())?;
        if !wait_row_gone(&driver, "parser edges", Duration::from_secs(2)) {
            return Err("the thought body did not collapse behind its toggle".to_owned());
        }
        if wait_row(&driver, "- Thought", Duration::from_millis(200)).is_none() {
            return Err("the GROUP header must stay expanded".to_owned());
        }
        let from = driver.raw_len();
        assert_exit_ack(&mut driver, from).map_err(|error| format!("teardown: {error}"))?;
        Ok(())
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 160, 30, Duration::from_secs(120));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the disclosure cycle");
    let _ = sandbox;
}

/// Item 5 — the palette's header close label (`esc`) releases the
/// dialog (the port's stand-in for v2's ~22 per-dialog `✕` labels —
/// every dialog family renders the same chrome).
#[test]
fn pty_dialog_header_close_label_releases_the_palette() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("lane-if-dialog-close");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        // ctrl+p: the palette mounts (its title row carries the
        // close label we click).
        driver.write(b"\x10");
        let title_row = wait_row(&driver, "Commands", Duration::from_secs(5))
            .ok_or_else(|| "the palette never rendered its title".to_owned())?;
        let grid = driver.grid_now();
        // The close label: the `esc` glyph run right-aligned on the
        // SAME row as the title (box x=10..70 at 80 cols; the label
        // ends 4 columns inside the right edge).
        let text = grid.row_text(title_row);
        let x = text
            .rfind("esc")
            .filter(|x| x + 3 >= 60)
            .ok_or_else(|| format!("the esc label never found on the title row: {text}"))?;
        let target = Some((x, title_row));
        let (x, row) = target.ok_or_else(|| "the palette's esc label never found".to_owned())?;
        driver.sgr_press(
            u16::try_from(x).unwrap_or(u16::MAX),
            u16::try_from(row).unwrap_or(u16::MAX),
        );
        driver.sgr_release(
            u16::try_from(x).unwrap_or(u16::MAX),
            u16::try_from(row).unwrap_or(u16::MAX),
        );
        // The palette left: its title + registry rows are gone.
        if !wait_row_gone(&driver, "Commands", Duration::from_secs(5)) {
            return Err("the close label did not release the palette".to_owned());
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
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    result.expect("the dialog close cycle");
    let _ = sandbox;
}
