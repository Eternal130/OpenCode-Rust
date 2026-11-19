//! diff-ext C3 — real-corpus recording driver (MANUAL, env-gated).
//!
//! Boots the REAL oc-tui production loop (`run_with_size` over a PTY,
//! exactly like the vt suites) against the `FakeLlm` scripted server,
//! with `TUI_RECORD_DIR` armed, and drives three longer real-chain
//! sessions the channel-8 corpus needs (C1's bootstrap recording was
//! a 1-event boot smoke; these are the first content-bearing ones):
//!
//! * `c3_record_deep_transcript` — multi-turn deep transcript
//!   (>10 messages: user, reasoning, markdown-code text parts, tool
//!   call/success) with mid-stream resizes 80×24 → 120×30 → 237×54.
//! * `c3_record_dense_interactions` — dialog/palette/leader-chord
//!   dense session (30+ keys: session dialog, queued-prompt manager
//!   undo, command palette filter, navigation family).
//! * `c3_record_interrupt_and_new_session` — a held stream
//!   interrupted with ESC, then `session.new` (`<leader>n`) with a
//!   fresh submission.
//!
//! Manual invocation (each test IS one recorded session — one process
//! each, per the recorder's per-arm file boundary):
//!
//! ```sh
//! TUI_RECORD_DIR=/tmp/opencode/c3-recs \
//!   tools/dev-gate.sh 'cargo test -p oc-tui --test tui_record_sessions \
//!     c3_record_deep_transcript -- --ignored --exact --nocapture'
//! ```
//!
//! Then `python3 tools/diff/tui_record_compile.py /tmp/opencode/c3-recs`
//! (privacy iron law: recordings are real-session data; raw files
//! never enter a commit — see crates/oc-util/src/recorder.rs).

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;

use vt_harness::{
    exit_chord, open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm,
    FakeSession, HarnessHandle, LiveHarness, ScriptEvent, ALT_SCREEN_LEAVE, WORDMARK,
};

fn probe(driver: &HarnessHandle, needle: &[u8]) -> bool {
    let hit = driver.wait_for(needle, Duration::from_secs(5));
    if !hit {
        dump_grid(driver, &String::from_utf8_lossy(needle));
    }
    hit
}

fn dump_grid(h: &HarnessHandle, tag: &str) {
    let grid = h.grid_now();
    eprintln!("--- grid @ {tag} ---");
    for row in 0..grid.rows.min(14) {
        eprintln!("c3-grid{row:02}: {:?}", grid.row_text(row));
    }
}

fn guard() -> std::path::PathBuf {
    let Some(dir) = std::env::var_os("TUI_RECORD_DIR") else {
        panic!("C3 manual recorder: TUI_RECORD_DIR must be set (see the module doc)");
    };
    std::fs::create_dir_all(&dir).expect("record dir");
    std::path::PathBuf::from(dir)
}

fn type_keys(h: &mut HarnessHandle, bytes: &[u8]) {
    for key in bytes {
        h.write(&[*key]);
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Every scripted body must be valid JSON (a malformed frame is a
/// client-side decode error → the reconnect loop eats the script).
fn check_script(script: &[ScriptEvent]) {
    for event in script {
        serde_json::from_str::<serde_json::Value>(&event.body)
            .unwrap_or_else(|error| panic!("bad script body: {error}: {}", event.body));
    }
}

// ---------------------------------------------------------------------------
// case a — deep multi-turn transcript + mid-stream resizes
// ---------------------------------------------------------------------------

/// The deep-transcript script (case a): two turns with reasoning,
/// markdown code blocks and a tool call, ending in a title update.
// one declarative `step(...)` line per scripted event — a table, not
// logic; splitting it would hurt the recording's readability.
#[allow(clippy::too_many_lines)]
#[must_use]
fn deep_script() -> Vec<ScriptEvent> {
    let session = "ses_alpha";
    let markdown_part1 = concat!(
        "# Build Report\n\n",
        "The workspace analysis found three issues.\n\n",
        "```rust\n",
        "fn main() {\n",
        "    println!(\"hello build\");\n",
        "}\n",
        "```\n\n",
        "- first: the linker args drift\n",
        "- second: the cache key omits the profile\n"
    );
    let markdown_part2 = concat!(
        "## Summary\n\n",
        "The fix lands in two commits:\n\n",
        "```diff\n",
        "+ let key = profile.cache_key();\n",
        "```\n\n",
        "Watch the CI rerun after the second commit.\n"
    );
    let mut script = Vec::new();
    let mut t = 30u64;
    // one scripted event per call (absolute delay t, sequential id)
    let mut step = |t: u64, mut data: serde_json::Value| {
        let n = script.len();
        let kind = data.as_object_mut().expect("object").remove("type").expect("type");
        script.push(ScriptEvent {
            delay_ms: t,
            body: serde_json::json!({
                "id": format!("evt_{n:03}"), "created": t,
                "type": kind, "data": data
            })
            .to_string(),
        });
    };
    // turn 1 — user message arrives through the inbox
    step(
        t,
        serde_json::json!({
            "type": "session.inbox.enqueued",
            "sessionID": session, "inboxID": "msg_user_1",
            "item": { "id": "msg_user_1", "type": "user",
                      "payload": { "text": "please analyze the workspace and report the build failures" },
                      "time": { "created": 1 } }
        }),
    );
    let asst1 = |data: serde_json::Value| {
        let mut d = data;
        d["sessionID"] = serde_json::json!(session);
        d["assistantMessageID"] = serde_json::json!("msg_asst_1");
        d
    };
    t += 40;
    step(
        t,
        serde_json::json!({
            "type": "session.execution.started", "sessionID": session
        }),
    );
    step(
        t,
        asst1(serde_json::json!({
            "type": "session.step.started", "agent": "build", "started": 1,
            "model": { "providerID": "anthropic", "id": "claude-sonnet" }
        })),
    );
    // reasoning block
    t += 40;
    step(t, asst1(serde_json::json!({ "type": "session.reasoning.started", "ordinal": 0 })));
    for (advance, delta) in
        [(40u64, "Inspecting the tree"), (60, " the Cargo profile"), (50, " and the cache key")]
    {
        t += advance;
        step(
            t,
            asst1(
                serde_json::json!({ "type": "session.reasoning.delta", "ordinal": 0, "delta": delta }),
            ),
        );
    }
    t += 60;
    step(
        t,
        asst1(serde_json::json!({
            "type": "session.reasoning.ended", "ordinal": 0,
            "text": "Inspecting the tree the Cargo profile and the cache key"
        })),
    );
    // markdown text part (built from deltas, then settled verbatim)
    t += 80;
    step(t, asst1(serde_json::json!({ "type": "session.text.started", "ordinal": 0 })));
    for chunk in markdown_part1.split_inclusive('\n').collect::<Vec<_>>().chunks(2).flatten() {
        t += 45;
        step(
            t,
            asst1(
                serde_json::json!({ "type": "session.text.delta", "ordinal": 0, "delta": chunk }),
            ),
        );
    }
    t += 80;
    step(
        t,
        asst1(
            serde_json::json!({ "type": "session.text.ended", "ordinal": 0, "text": markdown_part1 }),
        ),
    );
    // tool call: read the build log
    t += 100;
    step(
        t,
        asst1(
            serde_json::json!({ "type": "session.tool.input.started", "id": "tool_read_1", "name": "read" }),
        ),
    );
    t += 60;
    step(
        t,
        asst1(
            serde_json::json!({ "type": "session.tool.called", "id": "tool_read_1", "input": { "filePath": "build.log", "line": 1 } }),
        ),
    );
    t += 120;
    step(
        t,
        asst1(
            serde_json::json!({ "type": "session.tool.progress", "id": "tool_read_1", "metadata": { "bytes": 2048 } }),
        ),
    );
    t += 200;
    step(
        t,
        asst1(serde_json::json!({
            "type": "session.tool.success", "id": "tool_read_1",
            "content": [ { "type": "text", "text": "error: linking with cc failed\nnote: ld: library not found" } ]
        })),
    );
    t += 120;
    step(
        t,
        serde_json::json!({
            "type": "session.step.ended", "sessionID": session, "assistantMessageID": "msg_asst_1"
        }),
    );
    step(
        t,
        serde_json::json!({
            "type": "session.execution.succeeded", "sessionID": session
        }),
    );
    // turn 2 — second user message
    t += 200;
    step(
        t,
        serde_json::json!({
            "type": "session.inbox.enqueued",
            "sessionID": session, "inboxID": "msg_user_2",
            "item": { "type": "user", "payload": { "text": "now summarize the fix in two commits" } }
        }),
    );
    let asst2 = |data: serde_json::Value| {
        let mut d = data;
        d["sessionID"] = serde_json::json!(session);
        d["assistantMessageID"] = serde_json::json!("msg_asst_2");
        d
    };
    t += 60;
    step(
        t,
        serde_json::json!({
            "type": "session.execution.started", "sessionID": session
        }),
    );
    step(
        t,
        asst2(serde_json::json!({
            "type": "session.step.started", "agent": "build", "started": 2,
            "model": { "providerID": "anthropic", "id": "claude-sonnet" }
        })),
    );
    t += 40;
    step(t, asst2(serde_json::json!({ "type": "session.reasoning.started", "ordinal": 0 })));
    t += 60;
    step(
        t,
        asst2(
            serde_json::json!({ "type": "session.reasoning.delta", "ordinal": 0, "delta": "Splitting the linker drift from the cache key" }),
        ),
    );
    t += 60;
    step(
        t,
        asst2(
            serde_json::json!({ "type": "session.reasoning.ended", "ordinal": 0, "text": "Splitting the linker drift from the cache key" }),
        ),
    );
    t += 60;
    step(t, asst2(serde_json::json!({ "type": "session.text.started", "ordinal": 0 })));
    for chunk in markdown_part2.split_inclusive('\n').collect::<Vec<_>>().chunks(2).flatten() {
        t += 45;
        step(
            t,
            asst2(
                serde_json::json!({ "type": "session.text.delta", "ordinal": 0, "delta": chunk }),
            ),
        );
    }
    t += 80;
    step(
        t,
        asst2(
            serde_json::json!({ "type": "session.text.ended", "ordinal": 0, "text": markdown_part2 }),
        ),
    );
    t += 120;
    step(
        t,
        serde_json::json!({
            "type": "session.step.ended", "sessionID": session, "assistantMessageID": "msg_asst_2"
        }),
    );
    step(
        t,
        serde_json::json!({
            "type": "session.execution.succeeded", "sessionID": session
        }),
    );
    // the completion marker the driver waits for: the tab title
    t += 200;
    step(
        t,
        serde_json::json!({
            "type": "session.updated", "sessionID": session,
            "info": { "id": session, "title": "C3 DONE",
                      "time": { "created": 0, "updated": 9 },
                      "location": { "directory": "/tmp" }, "projectID": "p", "cost": 0,
                      "tokens": { "cache": { "read": 0, "write": 0 }, "input": 0, "output": 0,
                                  "read": 0, "write": 0, "reasoning": 0 } }
        }),
    );

    // FakeLlm semantics: delay_ms is the increment FROM the previous
    // event (the anchor advances per frame); `t` above is absolute —
    // convert to deltas here.
    let mut prev = 0u64;
    for event in &mut script {
        let at = event.delay_ms;
        event.delay_ms = at.saturating_sub(prev);
        prev = at;
    }
    script
}

#[test]
#[ignore = "C3 manual recording driver: set TUI_RECORD_DIR (module doc)"]
fn c3_record_deep_transcript() {
    let _dir = guard();
    let _lock = pty_lock();
    let _sandbox = sandbox_env("c3-deep");

    let script = deep_script();
    check_script(&script);
    let fake = FakeLlm::spawn(vec![
        FakeSession { id: "ses_alpha", title: "Alpha" },
        FakeSession { id: "ses_long", title: vt_harness::LONG_TITLE },
    ]);
    fake.arm_script(script);

    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let url = fake.url.clone();
    let runner = std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(300)));
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(10)), "boot wordmark");
    open_first_session(&mut driver, b"Alpha").expect("open Alpha");
    type_keys(&mut driver, b"please analyze the workspace and report");
    std::thread::sleep(Duration::from_millis(200));
    driver.write(b"\r");
    // Resize strategy (starved-ingest finding, see the module doc):
    // the live loop is a single-threaded runtime — at 120 cols in a
    // debug build each transcript frame costs O(100ms), the scripted
    // stream lands minutes behind its nominal schedule, and the 45s
    // idle watchdog then reconnects and the fake REPLAYS the script
    // (duplicate events). So the stream runs at the boot 80×24 (the
    // cheapest frames), and the size history grows AFTER the marker —
    // the recorded `sizes` ledger is identical either way (the replay
    // harness reads the size history, not its timing).
    // wait for the completion marker chain, dumping the grid at the
    // first miss (starved-ingest forensics)
    let markers: &[&[u8]] = &[
        b"Build Report",
        b"linker args drift",
        // (the tool card renders collapsed: its output content is NOT
        // a screen marker — skip "library not found")
        b"two commits",
        b"Watch the CI", // the final text delta of turn 2
                         // (the tab STRIP title does not repaint from session.updated
                         // alone — titles refresh on resync; the session.updated event
                         // IS recorded, which is what the corpus needs)
    ];
    // let the script tail land (text.ended + step.ended + updated)
    std::thread::sleep(Duration::from_millis(2500));
    for marker in markers {
        if !driver.wait_for(marker, Duration::from_secs(60)) {
            let grid = driver.grid_now();
            for row in 0..grid.rows {
                eprintln!("c3-grid{:02}: {:?}", row, grid.row_text(row));
            }
            panic!("marker {:?} never painted", String::from_utf8_lossy(marker));
        }
    }
    driver.resize_pty(120, 30);
    std::thread::sleep(Duration::from_millis(500));
    driver.resize_pty(237, 54);
    std::thread::sleep(Duration::from_millis(600));
    let from = driver.raw_len();
    exit_chord(&mut driver);
    assert!(driver.wait_for_after(ALT_SCREEN_LEAVE, from, Duration::from_secs(8)), "clean exit");
    fake.stop();
    runner.join().expect("run_tui");
}

// ---------------------------------------------------------------------------
// case b — dialog / palette / leader-chord dense interaction session
// ---------------------------------------------------------------------------

#[test]
#[ignore = "C3 manual recording driver: set TUI_RECORD_DIR (module doc)"]
fn c3_record_dense_interactions() {
    let _dir = guard();
    let _lock = pty_lock();
    let _sandbox = sandbox_env("c3-dense");

    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let url = fake.url.clone();
    let runner = std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(300)));
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(10)), "boot wordmark");
    std::thread::sleep(Duration::from_millis(400));

    // 1. command palette at home (the pty_interactions proven cycle):
    // ctrl+p → filter "theme" → browse → apply → theme list → esc
    driver.write(b"\x10");
    probe(&driver, b"Commands");
    driver.write(b"\x1b[B\x1b[B");
    type_keys(&mut driver, b"theme");
    probe(&driver, b"Switch theme");
    std::thread::sleep(Duration::from_millis(300));
    driver.write(b"\r");
    probe(&driver, b"nightowl");
    std::thread::sleep(Duration::from_millis(300));
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(250));

    // 2. session dialog: <leader>l, walk the list, open Alpha
    driver.write(b"\x18l");
    probe(&driver, b"Alpha");
    driver.write(b"\x1b[B\x1b[B\x1b[A");
    driver.write(b"\r");
    probe(&driver, b"Alpha");

    // 3. composer draft: type, backspace-edit, retype
    type_keys(&mut driver, b"draft with edits");
    std::thread::sleep(Duration::from_millis(300));
    driver.write(b"\x7f\x7f\x7f");
    type_keys(&mut driver, b"ing");

    // 4. model dialog: <leader>m, filter, esc
    driver.write(b"\x18m");
    probe(&driver, b"fake-model");
    type_keys(&mut driver, b"fake");
    std::thread::sleep(Duration::from_millis(250));
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(250));

    // 5. navigation family over the transcript + tabs
    for key in [b"\x1b[A".as_slice(), b"\x1b[B", b"\x1b[5~", b"\x1b[6~", b"\x1b[H", b"\x1b[F"] {
        driver.write(key);
        std::thread::sleep(Duration::from_millis(120));
    }
    driver.write(b"\t");
    std::thread::sleep(Duration::from_millis(150));
    driver.write(b"\x1b[1;3C"); // alt+right
    std::thread::sleep(Duration::from_millis(150));
    driver.write(b"\x1b[1;3D"); // alt+left
    std::thread::sleep(Duration::from_millis(150));

    // 6. session dialog again; walk; close with escape (no commit)
    driver.write(b"\x18l");
    probe(&driver, b"Alpha");
    driver.write(b"\x1b[B\x1b[B");
    std::thread::sleep(Duration::from_millis(200));
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(250));

    // 7. palette once more, in-session: filter, escape out
    driver.write(b"\x10");
    probe(&driver, b"Commands");
    type_keys(&mut driver, b"session");
    std::thread::sleep(Duration::from_millis(250));
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(250));

    // 8. session.new: <leader>n routes home
    driver.write(b"\x18n");
    probe(&driver, b"sk anything");

    // 9. clear any draft, final esc sweep
    driver.write(b"\x15");
    std::thread::sleep(Duration::from_millis(150));
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));

    let from = driver.raw_len();
    exit_chord(&mut driver);
    assert!(driver.wait_for_after(ALT_SCREEN_LEAVE, from, Duration::from_secs(8)), "clean exit");
    fake.stop();
    runner.join().expect("run_tui");
}

// ---------------------------------------------------------------------------
// case c — streaming interrupt (ESC) + new-session switch
// ---------------------------------------------------------------------------

#[test]
#[ignore = "C3 manual recording driver: set TUI_RECORD_DIR (module doc)"]
fn c3_record_interrupt_and_new_session() {
    let _dir = guard();
    let _lock = pty_lock();
    let _sandbox = sandbox_env("c3-interrupt");

    // the held stream: a turn that starts and NEVER ends — the ESC
    // interrupts it mid-flight (the real interrupt shape: a POST the
    // recording's fetch map must carry or replay stub-gaps).
    let session = "ses_alpha";
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
    let script = vec![
        ScriptEvent {
            delay_ms: 30,
            body: format!(
                r#"{{"id":"evt_inbox","created":{now_ms},"type":"session.inbox.enqueued","data":{{"sessionID":"{session}","inboxID":"msg_user_1","item":{{"id":"msg_user_1","type":"user","payload":{{"text":"run the long build and wait"}},"time":{{"created":1}}}}}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 70,
            body: format!(
                r#"{{"id":"evt_step","created":{now_ms},"type":"session.step.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","agent":"build","started":1,"model":{{"providerID":"anthropic","id":"claude-sonnet"}}}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 90,
            body: format!(
                r#"{{"id":"evt_exec","created":{now_ms},"type":"session.execution.started","data":{{"sessionID":"{session}"}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 150,
            body: format!(
                r#"{{"id":"evt_r1","created":{now_ms},"type":"session.reasoning.delta","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","ordinal":0,"delta":"Pondering the long build"}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 700,
            body: format!(
                r#"{{"id":"evt_t1","created":{now_ms},"type":"session.text.delta","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","ordinal":0,"delta":"cargo build --release takes a while"}}}}"#
            ),
        },
        // then the stream HOLDS (no step.ended — interrupted live)
    ];
    let fake = FakeLlm::spawn(standard_sessions());
    check_script(&script);
    fake.arm_script(script);

    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let url = fake.url.clone();
    let runner = std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(300)));
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(10)), "boot wordmark");
    open_first_session(&mut driver, b"Alpha").expect("open Alpha");
    type_keys(&mut driver, b"run the long build and wait");
    std::thread::sleep(Duration::from_millis(200));
    driver.write(b"\r");
    // the stream is live: the text delta paints (the reasoning row
    // renders collapsed — its text is not a screen marker)
    assert!(driver.wait_for(b"cargo build", Duration::from_secs(10)), "streaming text painted");
    std::thread::sleep(Duration::from_millis(600));
    // ESC ×3 — the 2-press interrupt machine (one spare press in case
    // a stray surface eats the first; arm → fire POSTs the interrupt)
    for _ in 0..3 {
        driver.write(b"\x1b");
        std::thread::sleep(Duration::from_millis(300));
    }
    std::thread::sleep(Duration::from_millis(500));
    // <leader>n — session.new: home with a fresh composer. (NO fresh
    // submission after this: the ref leg MINTS its own session id on
    // a home submit — unstubbed route probes are a stub_gap by SCHEMA,
    // so the recorded switch covers the route change only.)
    driver.write(b"\x18n");
    probe(&driver, b"sk anything");
    std::thread::sleep(Duration::from_millis(600));

    let from = driver.raw_len();
    exit_chord(&mut driver);
    assert!(driver.wait_for_after(ALT_SCREEN_LEAVE, from, Duration::from_secs(8)), "clean exit");
    fake.stop();
    runner.join().expect("run_tui");
}

// ---------------------------------------------------------------------------
// probe (debugging aid): the deep script against a bare Connection —
// no TUI, no PTY. Logs every ingested event id; a cutoff here means
// the FAKE/script side is at fault, a full pass means the live loop.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "C3 probe: run manually (no TUI_RECORD_DIR needed)"]
fn c3_probe_script_delivery() {
    let script = deep_script();
    check_script(&script);
    let fake = FakeLlm::spawn(vec![FakeSession { id: "ses_alpha", title: "Alpha" }]);
    fake.arm_script(script);

    let client = oc_client::Client::new(&fake.url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let conn = oc_tui::data::Connection::with_config(
        client.clone(),
        oc_tui::data::ConnectionConfig {
            reconnect_delay: Duration::from_millis(10),
            max_attempts: Some(3),
            ..oc_tui::data::ConnectionConfig::default()
        },
    );
    let feeder = oc_tui::data::HttpResync::new(client.clone());
    runtime.spawn(conn.run(tx, feeder));
    // the prompt POST arms the script (the fake's trigger)
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = client
            .session()
            .prompt(
                "ses_alpha",
                &oc_client::types::SessionPromptBody {
                    text: "go".to_owned(),
                    ..Default::default()
                },
            )
            .await;
    });
    let started = std::time::Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut seen = 0usize;
    while let Some(ingest) = runtime
        .block_on(async { tokio::time::timeout_at(deadline, rx.recv()).await.unwrap_or_default() })
    {
        match ingest {
            oc_tui::data::Ingest::Event(event) => {
                seen += 1;
                eprintln!(
                    "c3-probe +{:>6}ms ingest #{seen}: {} {}",
                    started.elapsed().as_millis(),
                    event.id,
                    event.type_
                );
            }
            oc_tui::data::Ingest::Connection(state) => {
                eprintln!("c3-probe connection: {state:?}");
            }
            oc_tui::data::Ingest::Connected => eprintln!("c3-probe connected"),
            _ => {}
        }
    }
    eprintln!("c3-probe total ingested: {seen}");
}
