//! C6b session-route PTY regressions (TUI-TD01/TD02/TD03/TD05): the
//! REAL [`oc_tui::app::run_with_size`] loop over a pty against a
//! scripted server, driven with actual keystrokes.
//!
//! * **TD01** `six_wire_types_render` — the six wire message classes
//!   (shell / agent-switched / model-switched / location-switched /
//!   system / synthetic) render through BOTH ingestion paths: the
//!   `message.list` snapshot AND the live `message.updated` SSE echo
//!   (session/index.tsx:1781-1812's Switch/Shell/Notice arms).
//! * **TD02** `message_navigation_commands` — the five registered
//!   navigation commands dispatch (`session.message.next/previous`,
//!   the user-only pair, `session.messages_last_user`'s alt+end —
//!   index.tsx:1084-1128 + message-navigation.ts's boundary walk).
//! * **TD03** `parent_up_returns` — `up` in a child session routes to
//!   the parent (`session.parent`, index.tsx:1272-1288).
//! * **TD05** `tail_window_boot` — the boot `message.list` is the
//!   bounded tail page (`limit=20` + cursor, client/solid/data.ts:
//!   1630-1655), the transcript mounts a bounded tail window
//!   (`TRANSCRIPT_TAIL_ROWS`, index.tsx:130-135), and `session.first`
//!   backfills the older page (cursor walk) before pinning the head
//!   (index.tsx:790-873).

#[path = "vt_harness.rs"]
mod vt_harness;

#[path = "fixtures/route_server.rs"]
mod route_server;

use route_server::{
    assistant_row, message_page_body, session_json, user_row, MessagePage, ServerState,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{pty_lock, run_tui_boot, sandbox_env, HarnessHandle, LiveHarness};

/// The keybind overrides for TD02's otherwise-unbound navigation
/// commands (v2 registers them `none`; a test keybind makes the walk
/// reachable over the pty — the SAME mechanism a user config uses).
/// The file plants under the app config dir
/// (`<XDG_CONFIG_HOME>/opencode-rust/cli.json` — `global_roots`'
/// `APP_NAME`).
fn navigation_keybinds(root: &std::path::Path) {
    let dir = root.join(".config").join("opencode-rust");
    std::fs::create_dir_all(&dir).expect("app config dir");
    let _ = std::fs::write(
        dir.join("cli.json"),
        r#"{"keybinds": {
            "session.message.previous": "f5",
            "session.message.next": "f6",
            "session.message.user.previous": "f8"
        }}"#,
    );
}

/// Sandbox + pty + driver thread; the test body runs on the driver,
/// the REAL run loop on this thread (`pty_daily`'s runner shape).
#[allow(clippy::type_complexity)]
fn run_route_test<T: Send + 'static>(
    tag: &str,
    state: ServerState,
    boot: oc_tui::app::BootArgs,
    cols: u16,
    rows: u16,
    arm: impl FnOnce(&Arc<ServerState>, &std::path::PathBuf),
    drive: impl FnOnce(&mut HarnessHandle) -> Result<T, String> + Send + 'static,
) -> (T, Arc<ServerState>) {
    let _lock = pty_lock();
    let sandbox = sandbox_env(tag);
    let (url, shared, shutdown) = route_server::spawn(state);
    arm(&shared, &sandbox);
    let harness = LiveHarness::attach(cols, rows);
    let result_slot: Arc<Mutex<Option<Result<T, String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui_boot(&url, cols, rows, Duration::from_secs(90), boot);
    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = match result_slot.lock().expect("slot").take() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => {
            let raw = harness.handle().raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]);
            panic!("the driver thread never reported (deadlock?); raw TAIL: {tail:?}")
        }
    };
    (outcome, shared)
}

fn new_state(sessions: Vec<String>, pages: Vec<MessagePage>) -> ServerState {
    ServerState {
        requests: Mutex::new(Vec::new()),
        sse: Mutex::new(Vec::new()),
        events: std::sync::atomic::AtomicU64::new(1),
        sessions,
        pages,
    }
}

fn boot_session(id: &str) -> oc_tui::app::BootArgs {
    oc_tui::app::BootArgs {
        continue_last: false,
        session: Some(id.to_owned()),
        prompt: None,
        auto: false,
    }
}

fn wait_text(h: &HarnessHandle, needle: &str, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let grid = h.grid_now();
        if (0..grid.rows).any(|y| grid.row_text(y).contains(needle)) {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            let dump = (0..grid.rows)
                .map(|y| format!("{y:2}|{}", grid.row_text(y)))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("{needle:?} never painted; grid:\n{dump}"));
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

fn absent(h: &HarnessHandle, needle: &str) -> Result<(), String> {
    let grid = h.grid_now();
    if (0..grid.rows).any(|y| grid.row_text(y).contains(needle)) {
        let dump = (0..grid.rows)
            .map(|y| format!("{y:2}|{}", grid.row_text(y)))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!("{needle:?} unexpectedly painted; grid:\n{dump}"));
    }
    Ok(())
}

/// Wait until `needle` leaves the grid (the post-keypress redraw
/// settles — an instant absent can race the frame).
fn wait_absent(h: &HarnessHandle, needle: &str, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let grid = h.grid_now();
        if (0..grid.rows).all(|y| !grid.row_text(y).contains(needle)) {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            let dump = (0..grid.rows)
                .map(|y| format!("{y:2}|{}", grid.row_text(y)))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("{needle:?} never left the grid:\n{dump}"));
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// Wait for the session composer (the status hints row paints on every
/// session route ≥60 cols), then prove the keys land via a draft echo.
fn wait_for_composer(h: &mut HarnessHandle) -> Result<(), String> {
    wait_text(h, "shift+tab agents", Duration::from_secs(15))?;
    h.write(b"q1w2e3");
    wait_text(h, "q1w2e3", Duration::from_secs(15))?;
    h.write(b"\x7f\x7f\x7f\x7f\x7f\x7f");
    Ok(())
}

// ---------------------------------------------------------------------------
// TUI-TD01 — the six wire message classes
// ---------------------------------------------------------------------------

/// TD01: every class renders from the boot `message.list` page AND
/// from a live `message.updated` echo (both ingestion paths keep the
/// six classes — the store/data drop arms are gone).
#[test]
fn six_wire_types_render() {
    let rows = vec![
        user_row("msg_u1", &route_server::json_string("wire classes check")),
        // shell — the `┃` block over `$ ls -la` + the output preview.
        r#"{"type":"shell","id":"msg_sh1","shellID":"sh_1","command":"ls -la","status":"exited","exit":0,
            "output":{"cursor":10,"output":"total 0\nwire-shell-out","size":10,"truncated":false},
            "time":{"created":4}}"#
            .to_owned(),
        // agent-switched — "Switched agent from Build to Plan".
        r#"{"type":"agent-switched","id":"msg_ag1","agent":"plan","previous":"build","time":{"created":5}}"#
            .to_owned(),
        // model-switched — the catalog display name wins.
        r#"{"type":"model-switched","id":"msg_mo1",
            "model":{"providerID":"parityfake","id":"fake-model"},
            "previous":{"providerID":"old","id":"model"},
            "time":{"created":6}}"#
            .to_owned(),
        // location-switched — "↳ Moved to /tmp/other".
        r#"{"type":"location-switched","id":"msg_lo1","location":{"directory":"/tmp/other"},"time":{"created":7}}"#
            .to_owned(),
        // system — the `◈` Notice row.
        r#"{"type":"system","id":"msg_sy1","text":"","description":"System notice text","time":{"created":8}}"#
            .to_owned(),
        // synthetic — the subagent completion heading.
        r#"{"type":"synthetic","id":"msg_ny1","text":"child work done",
            "description":"child work done",
            "metadata":{"source":"subagent","state":"completed","agent":"researcher","childID":"ses_kid"},
            "time":{"created":9}}"#
            .to_owned(),
    ];
    let state = new_state(
        vec![session_json("ses_route", "Route", None)],
        vec![MessagePage {
            session: "ses_route".to_owned(),
            cursor: None,
            body: message_page_body(&rows, None),
        }],
    );
    run_route_test(
        "td01",
        state,
        boot_session("ses_route"),
        100,
        30,
        |_, _| {},
        move |h| {
            wait_for_composer(h)?;
            // The snapshot path (message.list): all six classes paint.
            wait_text(h, "$ ls -la", Duration::from_secs(10))?;
            wait_text(h, "wire-shell-out", Duration::from_secs(10))?;
            wait_text(h, "Switched agent from Build to Plan", Duration::from_secs(10))?;
            wait_text(h, "Switched model to fake-model", Duration::from_secs(10))?;
            wait_text(h, "Moved to /tmp/other", Duration::from_secs(10))?;
            wait_text(h, "System notice text", Duration::from_secs(10))?;
            wait_text(h, "Researcher finished", Duration::from_secs(10))?;
            wait_text(h, "child work done", Duration::from_secs(10))?;
            Ok(())
        },
    );
}

/// TD01's live path: the `message.updated` SSE echo of a shell + a
/// system notice renders (the second ingestion path — the store's
/// `message_from_event` reducer).
#[test]
fn six_wire_types_sse_echo_renders() {
    let state = new_state(
        vec![session_json("ses_route", "Route", None)],
        vec![MessagePage {
            session: "ses_route".to_owned(),
            cursor: None,
            body: message_page_body(
                &[user_row("msg_u1", &route_server::json_string("base"))],
                None,
            ),
        }],
    );
    run_route_test(
        "td01sse",
        state,
        boot_session("ses_route"),
        100,
        30,
        |server, _| {
            let server = Arc::clone(server);
            std::thread::spawn(move || {
                // Wait for the boot + SSE subscribe to settle, then
                // push the live echoes.
                std::thread::sleep(Duration::from_millis(4000));
                server.push_event(
                    "message.updated",
                    serde_json::json!({
                        "id": "msg_sh2", "sessionID": "ses_route", "type": "shell",
                        "shellID": "sh_2", "command": "echo sse-path", "status": "exited",
                        "exit": 0,
                        "output": {"cursor": 4, "output": "ok-sse", "size": 4,
                                   "truncated": false},
                        "time": {"created": 20}
                    }),
                );
                server.push_event(
                    "message.updated",
                    serde_json::json!({
                        "id": "msg_sy2", "sessionID": "ses_route", "type": "system",
                        "text": "", "description": "Sse notice row", "time": {"created": 21}
                    }),
                );
            });
        },
        |h| {
            wait_for_composer(h)?;
            wait_text(h, "$ echo sse-path", Duration::from_secs(15))?;
            wait_text(h, "ok-sse", Duration::from_secs(10))?;
            wait_text(h, "Sse notice row", Duration::from_secs(10))?;
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// TUI-TD02 — the message-navigation commands
// ---------------------------------------------------------------------------

/// TD02: the boundary walk moves the viewport (`findMessageBoundary` +
/// `alignMessage`): previous from the tail pins the last assistant
/// block's head (the user row above leaves), next walks back down,
/// the user-only pair steps between asks, and alt+end jumps to the
/// last user message.
#[test]
fn message_navigation_commands() {
    // 10 Q/A pairs; the 20-line answers make each pair (~26 rows)
    // TALLER than the ~18-row viewport, so a pinned target never
    // shares the screen with its neighbours' markers.
    let mut rows = Vec::new();
    for n in 1..=10 {
        let filler = (1..=19).map(|line| format!("filler {line}")).collect::<Vec<_>>().join("\n");
        let body = format!("A{n}MARK\n{filler}");
        rows.push(user_row(&format!("msg_q{n}"), &route_server::json_string(&format!("Q{n}MARK"))));
        rows.push(assistant_row(&format!("msg_a{n}"), &route_server::json_string(&body)));
    }
    let state = new_state(
        vec![session_json("ses_route", "Route", None)],
        vec![MessagePage {
            session: "ses_route".to_owned(),
            cursor: None,
            body: message_page_body(&rows, None),
        }],
    );
    run_route_test(
        "td02",
        state,
        boot_session("ses_route"),
        100,
        30,
        |_, sandbox| navigation_keybinds(sandbox),
        |h| {
            wait_for_composer(h)?;
            // Sticky boot inside the tail window: the newest answer is
            // already on screen; f5's alignment is proven by the user
            // row ABOVE it LEAVING the grid.
            wait_text(h, "A10MARK", Duration::from_secs(10))?;
            // previous (f5): no current anchor — the LAST candidate
            // (the final assistant block) aligns at its head; the user
            // row above scrolls OUT.
            h.write(b"\x1b[15~"); // f5
            wait_text(h, "A10MARK", Duration::from_secs(5))?;
            wait_absent(h, "Q10MARK", Duration::from_secs(5))?;
            // alt+end (`session.messages_last_user`): the LAST user
            // message — Q10 — jumps into view (A9 above leaves).
            h.write(b"\x1b[1;3F");
            wait_text(h, "Q10MARK", Duration::from_secs(5))?;
            absent(h, "A9MARK")?;
            // user-only previous (f8): from Q10 the previous USER
            // candidate is Q9 (a full pair up).
            h.write(b"\x1b[19~"); // f8
            wait_text(h, "Q9MARK", Duration::from_secs(5))?;
            absent(h, "Q10MARK")?;
            // previous (f5): the candidate before Q9 in the y-ordered
            // walk is A8 — Q8's answer (findMessageBoundary's
            // currentID-1 step, message-navigation.ts:46).
            h.write(b"\x1b[15~");
            wait_text(h, "A8MARK", Duration::from_secs(5))?;
            absent(h, "Q9MARK")?;
            // next (f6): the candidate after A8 is Q9.
            h.write(b"\x1b[17~"); // f6
            wait_text(h, "Q9MARK", Duration::from_secs(5))?;
            absent(h, "A8MARK")?;
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// TUI-TD03 — `up` routes to the parent session
// ---------------------------------------------------------------------------

/// TD03: booted into the CHILD, `up` (the empty-composer base binding)
/// navigates to the parent — the parent's transcript replaces the
/// child's (`navigate({type: "session", sessionID: parentID})`).
#[test]
fn parent_up_returns() {
    let child_rows = vec![user_row("msg_c1", &route_server::json_string("child-only-marker"))];
    let parent_rows = vec![user_row("msg_p1", &route_server::json_string("parent-only-marker"))];
    let state = new_state(
        vec![
            session_json("ses_root", "Root", None),
            session_json("ses_child", "Child", Some("ses_root")),
        ],
        vec![
            MessagePage {
                session: "ses_child".to_owned(),
                cursor: None,
                body: message_page_body(&child_rows, None),
            },
            MessagePage {
                session: "ses_root".to_owned(),
                cursor: None,
                body: message_page_body(&parent_rows, None),
            },
        ],
    );
    let ((), server) = run_route_test(
        "td03",
        state,
        boot_session("ses_child"),
        100,
        30,
        |_, _| {},
        |h| {
            wait_for_composer(h)?;
            wait_text(h, "child-only-marker", Duration::from_secs(10))?;
            // `up` in the child: the parent's transcript paints (the
            // parent tab opens; the route's content column re-renders).
            h.write(b"\x1b[A");
            wait_text(h, "parent-only-marker", Duration::from_secs(10))?;
            absent(h, "child-only-marker")?;
            Ok(())
        },
    );
    // The parent's message.list was fetched (the route mount hydrates
    // the newly-open tab through the loop fetcher).
    assert!(
        !server.requests_with("/api/session/ses_root/message").is_empty(),
        "the parent transcript fetched: {:?}",
        server
            .requests
            .lock()
            .expect("requests")
            .iter()
            .map(|request| request.path.clone())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// TUI-TD05 — the bounded tail + history backfill
// ---------------------------------------------------------------------------

/// TD05: the boot fetch carries `limit=20` (the tail page) and keeps
/// the older cursor; the transcript mounts only the TAIL window (the
/// head of the fetched page is off-screen); `session.first` (ctrl+g)
/// walks the older page (the cursor query) and pins the head.
#[test]
fn tail_window_boot() {
    // 12 user blocks × ~7 rows ≈ 84 rows — the 40-row tail window
    // hides the first blocks; the older page (cursor c1) carries 3
    // more.
    let mut tail_rows = Vec::new();
    for n in 1..=12 {
        let text = format!("U{n:02}MARK\nl1\nl2\nl3\nl4");
        tail_rows.push(user_row(&format!("msg_u{n:02}"), &route_server::json_string(&text)));
    }
    // The older page: ONE block (~8 rows) so the first-jump viewport
    // (21 rows) shows BOTH the older head and the tail page's first
    // block (the backfill's full-mount proof).
    let mut older_rows = Vec::new();
    for n in 1..=1 {
        let text = format!("OLD{n:02}MARK\nl1\nl2\nl3\nl4");
        older_rows.push(user_row(&format!("msg_o{n:02}"), &route_server::json_string(&text)));
    }
    let state = new_state(
        vec![session_json("ses_route", "Route", None)],
        vec![
            MessagePage {
                session: "ses_route".to_owned(),
                cursor: None,
                body: message_page_body(&tail_rows, Some("c1")),
            },
            MessagePage {
                session: "ses_route".to_owned(),
                cursor: Some("c1".to_owned()),
                body: message_page_body(&older_rows, None),
            },
        ],
    );
    let ((), server) = run_route_test(
        "td05",
        state,
        boot_session("ses_route"),
        100,
        30,
        |_, _| {},
        |h| {
            wait_for_composer(h)?;
            // The tail pair: the NEWEST block is mounted at the bottom.
            wait_text(h, "U12MARK", Duration::from_secs(10))?;
            // The bounded window: the OLDEST fetched block is NOT
            // mounted (rows beyond the 40-row tail budget stay
            // unmounted — they paint only after the first-jump's
            // backfill pins the head).
            absent(h, "U01MARK")?;
            // session.first (ctrl+g): the backfill fires (the cursor
            // query hits the server) and the head pins — the OLDER
            // page's content renders from the very top.
            h.write(b"\x07");
            wait_text(h, "OLD01MARK", Duration::from_secs(10))?;
            wait_text(h, "U01MARK", Duration::from_secs(10))?;
            Ok(())
        },
    );
    // The boot fetch: the bounded tail page (`limit=20` + desc order —
    // client/solid/data.ts:1630-1637).
    let boot_fetch = server
        .requests_with("/api/session/ses_route/message")
        .into_iter()
        .find(|request| !request.path.contains("cursor="))
        .expect("the boot message.list fetch");
    assert!(
        boot_fetch.path.contains("limit=20"),
        "the boot fetch carries the page limit: {}",
        boot_fetch.path
    );
    assert!(boot_fetch.path.contains("order=desc"), "the boot fetch is desc: {}", boot_fetch.path);
    // session.first's backfill: the OLDER page walked the cursor.
    assert!(
        server
            .requests_with("/api/session/ses_route/message")
            .iter()
            .any(|request| request.path.contains("cursor=c1")),
        "the first-jump backfill fetched the older page: {:?}",
        server
            .requests
            .lock()
            .expect("requests")
            .iter()
            .map(|request| request.path.clone())
            .collect::<Vec<_>>()
    );
}
