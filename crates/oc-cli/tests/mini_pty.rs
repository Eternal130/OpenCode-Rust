//! Mini PTY integration: drives the REAL `opencode-rust mini` binary
//! inside a pseudo-terminal (the `pty_boot.rs` pattern from lane TUI)
//! against a scripted stub server (the `tools/parity` `FakeLlm`
//! pattern: a plain loopback HTTP server answering the handful of
//! endpoints the boot path needs). Pins:
//!
//! * the boot splash lands in the native scrollback (no alternate
//!   screen — `?1049h` must NEVER appear);
//! * a prompt typed into the composer posts and the stub's SSE reply
//!   appears in the scrollback (user echo `> ...` + assistant text);
//! * the permission flow: `permission.asked` mounts the footer body,
//!   accepting it posts the reply and the turn continues;
//! * the two-press ctrl+c exit writes the exit splash (session title +
//!   `opencode mini -s <id>` resume command).

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SESSION_ID: &str = "ses_min_stub";

// -- the stub server ---------------------------------------------------------

struct Stub {
    port: u16,
    /// prompts received (text bodies).
    prompts: Arc<Mutex<Vec<String>>>,
    /// permission replies received (request ids).
    replies: Arc<Mutex<Vec<String>>>,
    /// G-272 — catalog fetches received (paths, query-stripped).
    catalog_requests: Arc<Mutex<Vec<String>>>,
    /// G-272 — the SSE event feed (tests push catalog events).
    events: tokio::sync::broadcast::Sender<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum PromptMode {
    PlainReply,
    PermissionAsk,
    /// G-221 — the five display blocks live (inbox queued prompts,
    /// skill, shell, compaction, tool.input.ended) after a prompt.
    G221Live,
    /// G-221 — the replay/hydrate path: the message list serves
    /// skill/shell/compaction messages and the inbox list one queued
    /// row.
    G221Replay,
    /// G-241 — a multi-delta text turn: TWO text deltas for one part
    /// and a final LONGER than the streamed accumulation.
    MultiDelta,
    /// G-272 — the hydrate path with PENDING blockers: the permission
    /// and form lists serve one row each and `session.active` marks
    /// the session running.
    HydrateBlockers,
}

async fn start_stub(mode: PromptMode) -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let port = listener.local_addr().expect("addr").port();
    let (events_tx, _) = tokio::sync::broadcast::channel::<String>(64);
    let output = Arc::new(Mutex::new(Vec::new()));
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(Vec::new()));
    let catalog_requests = Arc::new(Mutex::new(Vec::new()));
    let mode_storage = Arc::new(Mutex::new(mode));

    let server = Stub {
        port,
        prompts: Arc::clone(&prompts),
        replies: Arc::clone(&replies),
        catalog_requests: Arc::clone(&catalog_requests),
        events: events_tx.clone(),
    };

    tokio::spawn(async move {
        // Every /api/event connection subscribes to the broadcast and
        // forwards whatever the script sends.
        let sub_tx = events_tx;
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let sub_tx = sub_tx.clone();
            let output = Arc::clone(&output);
            let prompts = Arc::clone(&prompts);
            let replies = Arc::clone(&replies);
            let catalog_requests = Arc::clone(&catalog_requests);
            let mode = Arc::clone(&mode_storage);
            tokio::spawn(async move {
                serve_connection(
                    &mut socket,
                    sub_tx,
                    &output,
                    &prompts,
                    &replies,
                    &catalog_requests,
                    &mode,
                )
                .await;
            });
        }
    });

    // Warmup: prove the accept loop is actually serving before the child
    // is spawned — under loaded CI runners the spawned task can lag, and
    // the child's first transport failure would otherwise surface as an
    // opaque `error sending request` flake.
    let warm =
        format!("GET /api/info HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let mut ready = false;
    for _ in 0..100 {
        if let Ok(mut probe) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            if probe.write_all(warm.as_bytes()).await.is_ok() {
                let mut buf = [0u8; 256];
                if probe.read(&mut buf).await.unwrap_or(0) > 0 {
                    ready = true;
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "stub server never answered the warmup probe");
    server
}

#[allow(clippy::too_many_lines)] // the stub server's full protocol walk
async fn serve_connection(
    socket: &mut tokio::net::TcpStream,
    sub_tx: tokio::sync::broadcast::Sender<String>,
    output: &Arc<Mutex<Vec<u8>>>,
    prompts: &Arc<Mutex<Vec<String>>>,
    replies: &Arc<Mutex<Vec<String>>>,
    catalog_requests: &Arc<Mutex<Vec<String>>>,
    mode: &Arc<Mutex<PromptMode>>,
) {
    let head = read_request(socket).await;
    let request = head
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (method, path) =
        (request.first().cloned().unwrap_or_default(), request.get(1).cloned().unwrap_or_default());
    let path_only = path.split('?').next().unwrap_or_default().to_owned();

    // -- route -------------------------------------------------------------
    if method == "GET" && path_only == "/api/event" {
        let response = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
        let _ = socket.write_all(response.as_bytes()).await;
        output.lock().expect("lock").extend_from_slice(response.as_bytes());
        let mut subscription = sub_tx.subscribe();
        loop {
            let body = match subscription.recv().await {
                Ok(body) => body,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let frame = format!("data: {body}\n\n");
            if socket.write_all(frame.as_bytes()).await.is_err() {
                break;
            }
        }
        return;
    }
    if method == "GET" && path_only == "/api/info" {
        let body =
            r#"{"version":"2.0.24","pid":1,"urls":[],"paths":{"tmp":"/tmp/opencode"}}"#.to_owned();
        respond_json(socket, &body).await;
        return;
    }
    if method == "GET" && path_only == format!("/api/session/{SESSION_ID}") {
        let body = format!(
            r#"{{"data":{{"id":"{SESSION_ID}","cost":0,"projectID":"prj_stub","location":{{"directory":"/tmp/opencode/mini-pty"}},"time":{{"created":1,"updated":1}},"tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"reasoning":0}},"title":"Stub session"}}}}"#
        );
        respond_json(socket, &body).await;
        return;
    }
    if method == "GET" && path_only == "/api/location" {
        let body =
            r#"{"directory":"/tmp/opencode/mini-pty","project":{"canonical":"/tmp/opencode/mini-pty","directory":"/tmp/opencode/mini-pty","id":"prj_stub"}}"#
                .to_owned();
        respond_json(socket, &body).await;
        return;
    }
    // G-272 — the boot/event catalog fetches (`loadCatalog` +
    // `refreshCatalog`, runtime.ts:601-611, 690-733): the agent list
    // (an id/name/mode-only row the client can decode) and the model
    // catalog (one model with a context limit + two variants).
    if method == "GET" && (path_only == "/api/agent" || path_only == "/api/model") {
        catalog_requests.lock().expect("lock").push(path_only.clone());
        let body = if path_only == "/api/agent" {
            serde_json::json!({ "data": [
                { "id": "build", "name": "Build", "hidden": false, "mode": "primary",
                  "permissions": [],
                  "request": { "body": {}, "headers": {}, "settings": {} } },
            ], "location": { "directory": "/tmp/opencode/mini-pty" } })
        } else {
            serde_json::json!({ "data": [
                { "id": "stub-1", "modelID": "stub-1", "name": "Stub Model",
                  "providerID": "stub", "enabled": true, "status": "active", "cost": [],
                  "variants": [ { "id": "high" }, { "id": "low" } ],
                  "capabilities": { "input": ["text"], "output": ["text"], "tools": true },
                  "limit": { "context": 120_000, "output": 8192 },
                  "time": { "released": 1 } },
            ], "location": { "directory": "/tmp/opencode/mini-pty" } })
        };
        respond_json(socket, &body.to_string()).await;
        return;
    }
    if method == "GET" && path_only == format!("/api/session/{SESSION_ID}/message") {
        // G-221 — the replay fixtures (skill/shell/compaction
        // messages, newest first — the client fetches desc and the
        // runtime reverses).
        if *mode.lock().expect("lock") == PromptMode::G221Replay {
            let body = serde_json::json!({ "cursor": {}, "data": [
                { "id": "msg_rc", "type": "compaction", "status": "completed", "reason": "auto",
                  "summary": "Replay compaction summary.", "recent": "recent text",
                  "time": { "created": 9 } },
                { "id": "msg_rs", "type": "shell", "shellID": "shl_r1", "command": "echo replayed",
                  "status": "exited", "exit": 0,
                  "output": { "output": "replayed shell\n", "cursor": 15, "size": 15, "truncated": false },
                  "time": { "created": 8, "completed": 9 } },
                { "id": "msg_rsk", "type": "skill", "skill": "skl_review", "name": "review",
                  "text": "skill body", "time": { "created": 7 } },
            ] });
            respond_json(socket, &body.to_string()).await;
            return;
        }
        respond_json(socket, r#"{"cursor":{},"data":[]}"#).await;
        return;
    }
    if method == "GET" && path_only == format!("/api/session/{SESSION_ID}/inbox") {
        // G-221 — the hydrate's inbox seed (`session.inbox.list`).
        if *mode.lock().expect("lock") == PromptMode::G221Replay {
            let body = serde_json::json!({ "data": [
                { "id": "msg_rq1", "sessionID": SESSION_ID, "type": "user",
                  "payload": { "text": "queued while away" }, "delivery": "queue",
                  "time": { "created": 1 } },
            ] });
            respond_json(socket, &body.to_string()).await;
            return;
        }
        respond_json(socket, r#"{"data":[]}"#).await;
        return;
    }
    // G-272 — the hydrate's blocker sweep (`hydrate`,
    // stream-v2.transport.ts:952-1004): the parallel permission /
    // form / active lists a reconnect (or boot) issues.
    if method == "GET" && path_only == format!("/api/session/{SESSION_ID}/permission") {
        if *mode.lock().expect("lock") == PromptMode::HydrateBlockers {
            let body = serde_json::json!({ "data": [
                { "id": "prm_hy", "sessionID": SESSION_ID, "action": "bash",
                  "resources": ["bash(echo blocked)"] },
            ] });
            respond_json(socket, &body.to_string()).await;
            return;
        }
        respond_json(socket, r#"{"data":[]}"#).await;
        return;
    }
    if method == "GET" && path_only == format!("/api/session/{SESSION_ID}/form") {
        if *mode.lock().expect("lock") == PromptMode::HydrateBlockers {
            let body = serde_json::json!({ "data": [
                { "id": "frm_hy", "sessionID": SESSION_ID, "title": "Confirm hydrate",
                  "fields": [ { "key": "note", "type": "string", "title": "Note" } ] },
            ] });
            respond_json(socket, &body.to_string()).await;
            return;
        }
        respond_json(socket, r#"{"data":[]}"#).await;
        return;
    }
    if method == "GET" && path_only == "/api/session/active" {
        if *mode.lock().expect("lock") == PromptMode::HydrateBlockers {
            let body = serde_json::json!({ "data": { SESSION_ID: { "time": { "created": 1 } } } });
            respond_json(socket, &body.to_string()).await;
            return;
        }
        respond_json(socket, r#"{"data":{}}"#).await;
        return;
    }
    if method == "POST" && path_only == format!("/api/session/{SESSION_ID}/prompt") {
        // The client may send the body chunked — record the raw
        // request (head + body) and match on content.
        prompts.lock().expect("lock").push(head.clone());
        let payload = serde_json::to_string(&serde_json::json!({ "text": "" }))
            .unwrap_or_else(|_| "\"\"".to_owned());
        let _ = payload;
        let body = format!(
            r#"{{"data":{{"id":"msg_stub_1","delivery":"queue","sessionID":"{SESSION_ID}","payload":{{"text":""}},"time":{{"created":1}},"type":"user"}}}}"#
        );
        respond_json(socket, &body).await;
        let mode = *mode.lock().expect("lock");
        if mode == PromptMode::G221Live {
            // The G-221 script paces its phases so the LIVE dock
            // repaints between them (a burst would collapse the
            // transient footer states before any flush).
            emit_g221_script(&sub_tx).await;
            return;
        }
        emit_turn_script(&sub_tx, mode, None);
        return;
    }
    if method == "POST"
        && path_only.starts_with(&format!("/api/session/{SESSION_ID}/permission/"))
        && path_only.ends_with("/reply")
    {
        replies.lock().expect("lock").push(path_only.clone());
        let _ = socket.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n").await;
        emit_turn_script(&sub_tx, PromptMode::PlainReply, Some("after-permission"));
        return;
    }
    let _ = socket
        .write_all(
            format!(
                "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{{\"error\":{{\"type\":\"NotFound\"}}}}",
                30
            )
            .as_bytes(),
        )
        .await;
}

/// Read one request (head + content-length body) into a string.
async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut buffer = vec![0u8; 16384];
    let mut seen = 0;
    loop {
        match tokio::time::timeout(Duration::from_secs(10), socket.read(&mut buffer[seen..])).await
        {
            Ok(Ok(0) | Err(_)) | Err(_) => break,
            Ok(Ok(count)) => {
                seen += count;
                if let Some(head_end) = buffer[..seen].windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buffer[..head_end]).to_lowercase();
                    let body_needed = content_length(&head);
                    if seen >= head_end + 4 + body_needed {
                        break;
                    }
                }
            }
        }
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0)
}

async fn respond_json(socket: &mut tokio::net::TcpStream, body: &str) {
    // `connection: close`: this stub serves exactly ONE request per
    // connection — a keep-alive response invites reqwest's pool to
    // reuse the (immediately dropped) socket for the next request,
    // a load-dependent flake.
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

/// The G-221 paced event script: one turn carrying the five display
/// blocks, with inter-phase pauses so the footer repaints between the
/// transient states (the queued count, the running shell).
async fn emit_g221_script(tx: &tokio::sync::broadcast::Sender<String>) {
    let event = |kind: &str, data: serde_json::Value| {
        let _ = tx.send(
            serde_json::json!({ "type": kind, "data": data, "created": 1, "id": "evt_stub" })
                .to_string(),
        );
    };
    let session = serde_json::json!({ "sessionID": SESSION_ID });
    let pace = Duration::from_millis(700);
    // Phase 1 — two rows queue: the dock paints "2 pending".
    event("session.execution.started", session.clone());
    event(
        "session.inbox.enqueued",
        serde_json::json!({
            "sessionID": SESSION_ID, "inboxID": "msg_g21",
            "item": { "type": "user", "payload": { "text": "queued one" }, "delivery": "queue" },
        }),
    );
    event(
        "session.inbox.enqueued",
        serde_json::json!({
            "sessionID": SESSION_ID, "inboxID": "msg_g22",
            "item": { "type": "user", "payload": { "text": "queued two" }, "delivery": "queue" },
        }),
    );
    tokio::time::sleep(pace).await;
    // Phase 2 — one cancels, one delivers; a skill activates.
    event(
        "session.inbox.cancelled",
        serde_json::json!({ "sessionID": SESSION_ID, "inboxID": "msg_g21" }),
    );
    event(
        "session.inbox.delivered",
        serde_json::json!({ "sessionID": SESSION_ID, "inboxID": "msg_g22" }),
    );
    event(
        "session.skill.activated",
        serde_json::json!({
            "sessionID": SESSION_ID, "id": "skl_deploy", "name": "deploy", "text": "…",
        }),
    );
    tokio::time::sleep(pace).await;
    // Phase 3 — a direct shell runs and exits 0.
    event(
        "session.shell.started",
        serde_json::json!({
            "sessionID": SESSION_ID, "shell": { "id": "shl_g1", "command": "echo g221 hello", "status": "running" },
        }),
    );
    tokio::time::sleep(pace).await;
    event(
        "session.shell.ended",
        serde_json::json!({
            "sessionID": SESSION_ID,
            "shell": { "id": "shl_g1", "command": "echo g221 hello", "status": "exited", "exit": 0 },
            "output": { "output": "g221 shell output\n", "cursor": 18, "size": 18, "truncated": false },
        }),
    );
    // Phase 4 — a server-driven compaction runs to completion.
    event(
        "session.compaction.started",
        serde_json::json!({
            "sessionID": SESSION_ID, "reason": "auto", "recent": "recent", "inputID": "msg_gcmp",
        }),
    );
    event(
        "session.compaction.delta",
        serde_json::json!({ "sessionID": SESSION_ID, "text": "Distilled " }),
    );
    event(
        "session.compaction.ended",
        serde_json::json!({
            "sessionID": SESSION_ID, "reason": "auto", "text": "Distilled context.", "recent": "recent",
        }),
    );
    tokio::time::sleep(pace).await;
    // Phase 5 — a streaming tool input settles at its final text
    // (the REAL wire shape: id/name, no partID).
    event(
        "session.tool.input.started",
        serde_json::json!({
            "sessionID": SESSION_ID, "assistantMessageID": "msg_stub_1", "id": "call_g1", "name": "bash",
        }),
    );
    event(
        "session.tool.input.ended",
        serde_json::json!({
            "sessionID": SESSION_ID, "assistantMessageID": "msg_stub_1", "id": "call_g1",
            "text": "{\"command\":\"oc fmt --check\"}",
        }),
    );
    tokio::time::sleep(pace).await;
    event("session.execution.succeeded", session);
}

/// The scripted event sequence a prompt triggers (SSE shapes the
/// transport folds).
fn emit_turn_script(
    tx: &tokio::sync::broadcast::Sender<String>,
    mode: PromptMode,
    phase: Option<&str>,
) {
    let event = |kind: &str, data: serde_json::Value| {
        let _ = tx.send(
            serde_json::json!({ "type": kind, "data": data, "created": 1, "id": "evt_stub" })
                .to_string(),
        );
    };
    let session = serde_json::json!({ "sessionID": SESSION_ID });
    if phase == Some("after-permission") {
        event(
            "session.tool.success",
            serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_tool",
                                 "tool": "bash", "input": { "command": "echo hi" } }),
        );
        event("session.execution.succeeded", session);
        return;
    }
    event("session.execution.started", session.clone());
    match mode {
        PromptMode::PermissionAsk => {
            event(
                "session.tool.input.started",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_tool",
                                     "tool": "bash", "input": { "command": "echo hi" } }),
            );
            event(
                "permission.asked",
                serde_json::json!({ "id": "prm_stub", "sessionID": SESSION_ID, "action": "bash",
                                     "resources": ["bash(echo hi)"] }),
            );
        }
        PromptMode::PlainReply | PromptMode::G221Replay => {
            event(
                "session.text.started",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text" }),
            );
            event(
                "session.text.delta",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text", "delta": "Stub says hello." }),
            );
            event(
                "session.text.ended",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text" }),
            );
            event("session.execution.succeeded", session);
        }
        // G-241 — the multi-delta shape every other script lacks: TWO
        // deltas for one part (the second must NOT re-print the
        // first) and a final text LONGER than the streamed chunks
        // (the tail must arrive via the ended suffix). G-272 — the
        // step family rides the same turn: the usage patch (tokens +
        // cost over the step model's context limit) and the
        // step.failed error row.
        PromptMode::MultiDelta => {
            event(
                "session.step.started",
                serde_json::json!({ "sessionID": SESSION_ID, "model": { "providerID": "stub", "id": "stub-1" } }),
            );
            event(
                "session.text.started",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text" }),
            );
            event(
                "session.text.delta",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text", "delta": "Stub streams " }),
            );
            event(
                "session.text.delta",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text", "delta": "twice" }),
            );
            event(
                "session.text.ended",
                serde_json::json!({ "sessionID": SESSION_ID, "messageID": "msg_stub_1", "partID": "prt_text",
                                     "text": "Stub streams twice and finishes." }),
            );
            event(
                "session.step.ended",
                serde_json::json!({ "sessionID": SESSION_ID, "assistantMessageID": "msg_stub_1",
                                     "tokens": { "input": 12000, "output": 300, "reasoning": 0,
                                                 "cache": { "read": 0, "write": 0 } },
                                     "cost": 0.0134 }),
            );
            event(
                "session.step.failed",
                serde_json::json!({ "sessionID": SESSION_ID, "assistantMessageID": "msg_step_fail",
                                     "error": { "type": "Error", "message": "Stub step exploded" } }),
            );
            event("session.execution.succeeded", session);
        }
        // The G-221 live script + the hydrate-blockers script both
        // returned their full payload above.
        PromptMode::G221Live | PromptMode::HydrateBlockers => {}
    }
}

// -- the pty driver ----------------------------------------------------------

struct MiniPty {
    child: std::process::Child,
    master: std::fs::File,
    output: Arc<Mutex<Vec<u8>>>,
}

impl MiniPty {
    fn spawn(port: u16, extra: &[&str]) -> Self {
        // The pinned rendering shape (see spawn_with_cli).
        Self::spawn_with_cli(port, extra, r#"{ "mini": { "tools": "show", "mono": true } }"#)
    }

    fn spawn_with_cli(port: u16, extra: &[&str], cli_json: &str) -> Self {
        Self::spawn_full(port, extra, cli_json, false)
    }

    /// G-221 — a spawn WITH the session replay enabled (no
    /// `--no-replay`): the boot hydrate walks the message list and
    /// the inbox list.
    fn spawn_replay(port: u16, extra: &[&str], cli_json: &str) -> Self {
        Self::spawn_full(port, extra, cli_json, true)
    }

    #[allow(clippy::too_many_lines)] // pty spawn: fd setup + child wiring in one POSIX sequence
    fn spawn_full(port: u16, extra: &[&str], cli_json: &str, replay: bool) -> Self {
        let master = unsafe {
            let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(fd >= 0, "posix_openpt");
            libc::grantpt(fd);
            libc::unlockpt(fd);
            std::fs::File::from_raw_fd(fd)
        };
        let slave_path = unsafe {
            let name = libc::ptsname(master.as_raw_fd());
            assert!(!name.is_null(), "ptsname");
            std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
        };
        let slave =
            std::fs::OpenOptions::new().read(true).write(true).open(&slave_path).expect("slave");
        unsafe {
            let winsize = libc::winsize { ws_row: 30, ws_col: 100, ws_xpixel: 0, ws_ypixel: 0 };
            libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
        }

        // Sandboxed global roots (the `pty_boot.rs` pattern): the
        // child must never see the developer's real service state.
        let sandbox = std::path::Path::new("/tmp/opencode").join(format!(
            "mini-pty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        std::fs::create_dir_all(sandbox.join(".config")).expect("config dir");
        std::fs::create_dir_all(sandbox.join(".local/state")).expect("state dir");
        std::fs::create_dir_all(sandbox.join("work")).expect("work dir");
        // CONFIG-T4: the mini display keys now follow cli.json with
        // v2's defaults (`mini.tools` defaults HIDE — runtime.ts:802,
        // `mini.mono` defaults false); the harness pins the previous
        // rendering shape (tool rows on, mono ASCII) so the flow
        // assertions keep their original scrollback bytes. NOTE the
        // config root is `opencode-rust` (global_roots::resolve's
        // APP_NAME), not the legacy v2 `opencode` directory.
        std::fs::create_dir_all(sandbox.join(".config/opencode-rust"))
            .expect("opencode config dir");
        std::fs::write(sandbox.join(".config/opencode-rust/cli.json"), cli_json).expect("cli.json");

        let binary = env!("CARGO_BIN_EXE_opencode-rust");
        let mut command = Command::new(binary);
        unsafe {
            // Make the pty the child's controlling terminal: the tty
            // device opens /dev/tty, so the slave must BE /dev/tty
            // (setsid + TIOCSCTTY).
            command.pre_exec(|| {
                if libc::setsid() == -1
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM)
                {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command
            .arg("mini")
            .arg("--server")
            .arg(format!("http://127.0.0.1:{port}"))
            .arg("--session")
            .arg(SESSION_ID);
        if !replay {
            command.arg("--no-replay");
        }
        command
            .args(extra)
            .current_dir(sandbox.join("work"))
            .env("OPENCODE_TEST_HOME", &sandbox)
            .env("XDG_CONFIG_HOME", sandbox.join(".config"))
            .env("XDG_STATE_HOME", sandbox.join(".local/state"))
            .env("HOME", &sandbox)
            .env_remove("OPENCODE_SERVER_PASSWORD")
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy")
            .env_remove("HTTPS_PROXY")
            .env_remove("https_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .stdin(slave.try_clone().expect("stdin"))
            .stdout(slave.try_clone().expect("stdout"))
            .stderr(slave);

        let spawned_child = command.spawn().expect("spawn mini");

        let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let output = Arc::clone(&output);
            let mut reader = master.try_clone().expect("clone master");
            let mut reply_writer = master.try_clone().expect("clone master write");
            std::thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                let mut pending: Vec<u8> = Vec::new();
                let mut replied = false;
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            output.lock().expect("lock").extend_from_slice(&chunk[..count]);
                            pending.extend_from_slice(&chunk[..count]);
                            // A real terminal answers the cursor
                            // position query (`ESC[6n`) — emulate it
                            // once for any boot-time probe.
                            if !replied && pending.windows(4).any(|w| w == b"\x1b[6n") {
                                replied = true;
                                let _ = reply_writer.write_all(b"\x1b[1;1R");
                            }
                            if pending.len() > 8192 {
                                pending.drain(..4096);
                            }
                        }
                    }
                }
            });
        }
        Self { child: spawned_child, master, output }
    }

    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("lock").clone()).into_owned()
    }

    fn wait_for(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.output().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// Wait for ANY of the fragments (the dock's diff repaints can
    /// drop glyphs whose cells coincide with the previous frame, so
    /// a single fixed needle is not byte-stable across runs).
    fn wait_for_any(&self, needles: &[&str], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let output = self.output();
            if needles.iter().any(|needle| output.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn type_text(&mut self, text: &str) {
        self.master.write_all(text.as_bytes()).expect("write");
    }

    fn exit_sequence(&mut self) {
        self.master.write_all(b"\x03").expect("ctrl+c");
        std::thread::sleep(Duration::from_millis(250));
        self.master.write_all(b"\x03").expect("ctrl+c again");
    }

    fn finish(mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match self.child.try_wait().expect("wait") {
                Some(status) => return status.code().unwrap_or(-1),
                None if Instant::now() > deadline => {
                    let _ = self.child.kill();
                    return -1;
                }
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

impl Drop for MiniPty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// -- the tests ---------------------------------------------------------------

/// Boot → prompt → SSE reply lands in the native scrollback → exit
/// splash. The alternate screen is NEVER entered (mini is inline).
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_prompt_reply_scrollback_and_exit_splash() {
    let stub = start_stub(PromptMode::PlainReply).await;
    let mut mini = MiniPty::spawn(stub.port, &[]);

    assert!(
        mini.wait_for("oc mini", Duration::from_secs(30)),
        "the entry splash booted: {}",
        mini.output()
    );
    assert!(!mini.output().contains("\x1b[?1049h"), "mini never enters the alternate screen");

    // Type a prompt; the composer submits on Enter.
    mini.type_text("hello stub\r");
    assert!(
        mini.wait_for("Stub says hello.", Duration::from_secs(30)),
        "the stub's reply landed in the scrollback: {}",
        mini.output()
    );
    assert!(mini.output().contains("> hello stub"), "the user echo carries the mono prefix");

    // Two-press ctrl+c exits with the exit splash (shown + session).
    mini.exit_sequence();
    assert!(
        mini.wait_for("opencode mini -s ses_min_stub", Duration::from_secs(30)),
        "the exit splash resume row: {}",
        mini.output()
    );
    let code = mini.finish();
    assert_eq!(code, 0, "clean exit");
    assert!(
        stub.prompts.lock().expect("lock").iter().any(|text| text.contains("hello stub")),
        "the prompt reached the server"
    );
}

/// The permission ask mounts the footer body; accepting posts the
/// reply and the turn continues to completion.
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_permission_ask_allow_continues() {
    let stub = start_stub(PromptMode::PermissionAsk).await;
    let mut mini = MiniPty::spawn(stub.port, &[]);

    assert!(mini.wait_for("oc mini", Duration::from_secs(30)), "boot");
    // Type a prompt; the scripted turn asks permission first.
    mini.type_text("run the tool\r");
    assert!(
        mini.wait_for("Permission required", Duration::from_secs(30)),
        "the permission body mounted: {}",
        mini.output()
    );
    // Enter accepts the default option (allow once) — the footer body
    // owns every key while the blocker is live. The reply releases the
    // blocker and the turn finishes (tool success + execution end).
    mini.type_text("\r");
    assert!(
        mini.wait_for("Bash", Duration::from_secs(30)),
        "the accepted tool completed in the scrollback: {}",
        mini.output()
    );

    mini.exit_sequence();
    mini.wait_for("opencode mini -s ses_min_stub", Duration::from_secs(20));
    let code = mini.finish();
    assert_eq!(code, 0);
    assert!(
        stub.replies.lock().expect("lock").iter().any(|id| id.contains("prm_stub")),
        "the permission reply reached the server"
    );
}

/// CONFIG-T4: the cli.json `mini` section drives the real binary.
/// `mini.tools: "hide"` keeps the tool caption rows out of the
/// scrollback while the turn still completes — the summary row lands
/// (`turn_summary` defaults show, mono defaults off → `·`
/// separators).
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_cli_json_tools_hide_drops_tool_rows() {
    let stub = start_stub(PromptMode::PermissionAsk).await;
    let mut mini = MiniPty::spawn_with_cli(stub.port, &[], r#"{ "mini": { "tools": "hide" } }"#);

    assert!(mini.wait_for("oc mini", Duration::from_secs(30)), "boot");
    mini.type_text("run the tool\r");
    assert!(
        mini.wait_for("Permission required", Duration::from_secs(30)),
        "the permission body mounted: {}",
        mini.output()
    );
    mini.type_text("\r");
    // The turn completes: the summary row lands after the tool would
    // have rendered (ordering — a gated caption can no longer appear).
    assert!(
        mini.wait_for("Build · default ·", Duration::from_secs(30)),
        "the turn summary row: {}",
        mini.output()
    );
    assert!(
        !mini.output().contains("Bash ·"),
        "tools=hide keeps tool caption rows out of the scrollback: {}",
        mini.output()
    );
    assert!(
        stub.replies.lock().expect("lock").iter().any(|id| id.contains("prm_stub")),
        "the permission reply reached the server"
    );
    mini.exit_sequence();
    mini.finish();
}

/// `--demo`: the intro note lists the slash commands and `/fmt` emits
/// its sample offline.
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_demo_slash_commands() {
    let stub = start_stub(PromptMode::PlainReply).await;
    let mut mini = MiniPty::spawn(stub.port, &["--demo"]);

    assert!(
        mini.wait_for("Demo slash commands enabled for interactive mode.", Duration::from_secs(20)),
        "the demo intro: {}",
        mini.output()
    );
    mini.type_text("/fmt text pty demo\r");
    assert!(
        mini.wait_for("pty demo", Duration::from_secs(30)),
        "the /fmt text sample echoed: {}",
        mini.output()
    );
    mini.type_text("/permission shell\r");
    assert!(
        mini.wait_for("Permission required", Duration::from_secs(30)),
        "the demo permission ask mounted: {}",
        mini.output()
    );
    mini.type_text("\r");
    mini.wait_for("Demo · completed", Duration::from_secs(20));
    mini.exit_sequence();
    mini.finish();
}

// -- G-221: the five display blocks -------------------------------------

/// G-221 blocks 1-5 LIVE (stream-v2.transport.ts): a turn carries a
/// server-inbox queue (the footer's `N pending` + the delivered user
/// echo), a skill activation row, the direct-shell rows (`$ command`
/// + the merged output), a server-driven compaction (the divider +
/// the streamed summary), and a streaming tool input that settles at
/// its final caption.
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_g221_inbox_skill_shell_compaction_tool_input() {
    let stub = start_stub(PromptMode::G221Live).await;
    // `mini.shell_output: "show"` — the streaming tool-input caption
    // rows (phase progress, canonical name "shell") stay hidden under
    // the knob's default (CONFIG-T4, tool.ts:1302).
    let mut mini = MiniPty::spawn_with_cli(
        stub.port,
        &[],
        r#"{ "mini": { "tools": "show", "mono": true, "shell_output": "show" } }"#,
    );

    assert!(mini.wait_for("oc mini", Duration::from_secs(30)), "boot");
    mini.type_text("run the blocks\r");

    // Block 1 — the footer's queued-prompts view: two rows queue (the
    // count reaches 2), one cancels, one delivers (the user echo).
    assert!(
        mini.wait_for("2 pending", Duration::from_secs(30)),
        "the footer's queued count: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("> queued two", Duration::from_secs(30)),
        "the delivered row's user echo: {}",
        mini.output()
    );
    assert!(
        !mini.output().contains("> queued one"),
        "the cancelled row never echoes: {}",
        mini.output()
    );
    // Block 3 — the skill row (`→ Skill "name"`,
    // stream-v2.transport.ts:399-408).
    assert!(
        mini.wait_for("→ Skill \"deploy\"", Duration::from_secs(30)),
        "the skill activation row: {}",
        mini.output()
    );
    // Block 2 — the direct-shell rows: `$ command` + the merged
    // output (tool.ts:1282-1294).
    assert!(
        mini.wait_for("$ echo g221 hello", Duration::from_secs(30)),
        "the shell start row: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("g221 shell output", Duration::from_secs(30)),
        "the shell output row: {}",
        mini.output()
    );
    // Block 4 — the server-driven compaction: the divider header + the
    // streamed summary (transport.ts:416-448, 1086-1124). The mono
    // writer is append-only: the delta row and the ended suffix land
    // as separate scrollback rows.
    assert!(
        mini.wait_for("─── Compaction ───", Duration::from_secs(30)),
        "the compaction divider: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("Distilled", Duration::from_secs(30)),
        "the compaction delta row: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("context.", Duration::from_secs(30)),
        "the compaction ended suffix row: {}",
        mini.output()
    );
    // Block 5 — the streaming tool input settles at its final caption
    // (`session.tool.input.ended`, transport.ts:1260-1272).
    assert!(
        mini.wait_for("Bash · oc fmt --check", Duration::from_secs(30)),
        "the finalized tool-input caption: {}",
        mini.output()
    );

    mini.exit_sequence();
    mini.wait_for("opencode mini -s ses_min_stub", Duration::from_secs(20));
    let code = mini.finish();
    assert_eq!(code, 0);
}

/// G-221 replay/hydrate: the boot walk renders the skill, shell, and
/// compaction messages from the session's message list
/// (`renderMessage`, transport.ts:769-841) and the inbox list seeds
/// the footer's pending view (`hydrate`, transport.ts:971-977).
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_g221_replay_hydrates_blocks_and_pending() {
    let stub = start_stub(PromptMode::G221Replay).await;
    let mut mini =
        MiniPty::spawn_replay(stub.port, &[], r#"{ "mini": { "tools": "show", "mono": true } }"#);

    assert!(
        mini.wait_for("→ Skill \"review\"", Duration::from_secs(30)),
        "the replayed skill row: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("$ echo replayed", Duration::from_secs(30)),
        "the replayed shell start row: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("replayed shell", Duration::from_secs(30)),
        "the replayed shell output: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("─── Compaction ───", Duration::from_secs(30)),
        "the replayed compaction divider: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("Replay compaction summary.", Duration::from_secs(30)),
        "the replayed compaction summary: {}",
        mini.output()
    );
    assert!(
        mini.wait_for("1 pending", Duration::from_secs(30)),
        "the hydrate's inbox seed reaches the footer: {}",
        mini.output()
    );

    // The session stays usable: a prompt turn still completes.
    mini.type_text("hello after replay\r");
    assert!(
        mini.wait_for("Stub says hello.", Duration::from_secs(30)),
        "the post-replay turn: {}",
        mini.output()
    );
    mini.exit_sequence();
    mini.wait_for("opencode mini -s ses_min_stub", Duration::from_secs(20));
    let code = mini.finish();
    assert_eq!(code, 0);
}

// -- G-272: the hydrate restores pending blockers -------------------------

/// G-272 — a reconnect/boot hydrate restores the PENDING permission
/// and form (`hydrate`'s parallel `permission.list` + `form.list` +
/// `session.active` → `syncBlockers`, stream-v2.transport.ts:952-
/// 1004): the permission body mounts at boot, and after the reply
/// the queued form takes the footer (permission precedence).
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_hydrate_restores_pending_permission_and_form() {
    let stub = start_stub(PromptMode::HydrateBlockers).await;
    let mut mini =
        MiniPty::spawn_replay(stub.port, &[], r#"{ "mini": { "tools": "show", "mono": true } }"#);

    // The hydrate mounts the pending permission BEFORE any prompt.
    assert!(
        mini.wait_for_any(
            &["Permission required", "ermission required", "Permissio"],
            Duration::from_secs(30)
        ),
        "the hydrated permission body mounted: {}",
        mini.output()
    );
    // Enter allows once — the reply releases the permission and the
    // hydrated form becomes the footer's blocker. (Fragments: the
    // dock's diff repaints can drop glyphs that coincide with the
    // previous frame's cells.)
    mini.type_text("\r");
    assert!(
        mini.wait_for_any(
            &["ydrate", "ydate", "onfirm", "Type your", "enter save"],
            Duration::from_secs(30)
        ),
        "the hydrated form body mounted after the permission reply: {}",
        mini.output()
    );
    // The reply's POST is async (the view falls locally at press
    // time) — poll the stub for it.
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if stub.replies.lock().expect("lock").iter().any(|id| id.contains("prm_hy")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    assert!(
        stub.replies.lock().expect("lock").iter().any(|id| id.contains("prm_hy")),
        "the hydrated permission's reply reached the server"
    );

    // Esc cancels the form (the blocker owns ctrl+c — the two-press
    // exit chord only reaches the composer), then exit. The pause
    // keeps the lone ESC separate from the ctrl+c bytes (a back-to-
    // back ESC+ctrl+c parses as one escape sequence).
    mini.type_text("\x1b");
    std::thread::sleep(Duration::from_millis(300));
    mini.exit_sequence();
    assert!(
        mini.wait_for_any(
            &["-s ses_min_stub", "s ses_min_stub", "ses_min_stub"],
            Duration::from_secs(10)
        ),
        "the exit splash after the form cancel: {}",
        mini.output()
    );
    let _ = mini.finish();
}

// -- G-241: the multi-delta text stream ----------------------------------

/// G-241 — a turn with TWO text deltas for one part and a final
/// LONGER than the streamed accumulation: the full text renders ONCE
/// (the second delta never re-prints the prefix) and the ended suffix
/// completes the tail (the append-only mono writer,
/// stream-v2.transport.ts:1169-1207).
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_multi_delta_streams_once_and_ends_with_the_tail() {
    let stub = start_stub(PromptMode::MultiDelta).await;
    let mut mini = MiniPty::spawn(stub.port, &[]);

    assert!(mini.wait_for("oc mini", Duration::from_secs(30)), "boot");
    mini.type_text("stream it\r");
    assert!(
        mini.wait_for(" and finishes.", Duration::from_secs(30)),
        "the ended suffix completed the server's longer final: {}",
        mini.output()
    );
    let output = mini.output();
    assert_eq!(
        output.matches("Stub streams").count(),
        1,
        "the prefix prints exactly once (a cumulative delta would repeat it): {output}"
    );
    assert_eq!(
        output.matches("twice").count(),
        1,
        "the second delta prints only its own chunk: {output}"
    );
    // G-272 — the usage groups on the statusline (12300 tokens over
    // the stub model's 120000 context = 10%; $0.0134 → $0.01).
    // (Fragments: the dock's diff repaints can drop glyphs that
    // coincide with the previous frame's cells.)
    assert!(
        mini.wait_for_any(&["2.3K (10%)", "(10%)", "2.3K"], Duration::from_secs(30)),
        "the context-usage group: {}",
        mini.output()
    );
    assert!(
        mini.output().contains("0.01") || mini.output().contains(".01"),
        "the cost group: {}",
        mini.output()
    );
    // G-272 — the step.failed error row (once per assistant message).
    assert!(
        mini.wait_for_any(
            &["Stub step exploded", "step exploded", "tub step"],
            Duration::from_secs(30)
        ),
        "the step failure's error row: {}",
        mini.output()
    );

    mini.exit_sequence();
    mini.wait_for("opencode mini -s ses_min_stub", Duration::from_secs(20));
    let code = mini.finish();
    assert_eq!(code, 0);
}

// -- G-272: the footer route family + catalog -----------------------------

/// G-272 — ctrl+p opens the command palette over the composer
/// (`command.palette.show`, config/keybind.ts:57): the panel's title
/// and entries render, the query filters, and esc returns to the
/// composer.
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_ctrl_p_opens_the_command_palette() {
    let stub = start_stub(PromptMode::PlainReply).await;
    let mut mini = MiniPty::spawn(stub.port, &[]);

    assert!(mini.wait_for("oc mini", Duration::from_secs(30)), "boot");
    // A prompt first (the exit splash only prints once a turn
    // rendered — `showExit = shown && sessionID`).
    mini.type_text("hello stub\r");
    assert!(
        mini.wait_for("Stub says hello.", Duration::from_secs(30)),
        "the stub's reply landed: {}",
        mini.output()
    );
    mini.type_text("\x10"); // ctrl+p
    assert!(
        mini.wait_for("Commands", Duration::from_secs(10)),
        "the command palette mounted: {}",
        mini.output()
    );
    assert!(
        mini.output().contains("Switch agent"),
        "the palette lists the agent route entry: {}",
        mini.output()
    );
    // The query filters (PanelShell's search row) — asserted through
    // BEHAVIOR (the byte stream's diff repaints can drop a run's
    // leading glyph, so the count label's "1" is not byte-stable):
    // filter to "exit" and run it — the footer closes with the exit
    // splash. (The query/count rendering itself is unit-pinned.)
    mini.type_text("exit");
    mini.type_text("\r");
    // (Fragments: the dock's diff repaints can drop glyphs that
    // coincide with the previous frame's cells.)
    assert!(
        mini.wait_for_any(
            &["-s ses_min_stub", "s ses_min_stub", "ses_min_stub"],
            Duration::from_secs(20)
        ),
        "the filtered Exit entry ran and closed the footer: {}",
        mini.output()
    );
    let code = mini.finish();
    assert_eq!(code, 0, "clean exit via the palette");
}

/// G-272 — the boot catalog fetch and the `agent.updated` refresh
/// (`loadCatalog` + `refreshCatalog`, runtime.ts:601-611, 690-733):
/// the agent + model lists are fetched at boot and RE-fetched when a
/// catalog event lands for this location.
#[tokio::test(flavor = "multi_thread")]
async fn mini_pty_catalog_fetches_at_boot_and_refreshes_on_events() {
    let stub = start_stub(PromptMode::PlainReply).await;
    let mut mini = MiniPty::spawn(stub.port, &[]);

    assert!(mini.wait_for("oc mini", Duration::from_secs(30)), "boot");
    // The boot fetch hit both lists.
    let boot_deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < boot_deadline {
        let requests = stub.catalog_requests.lock().expect("lock").clone();
        if requests.iter().filter(|path| *path == "/api/agent").count() >= 1
            && requests.iter().filter(|path| *path == "/api/model").count() >= 1
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let boot_requests = stub.catalog_requests.lock().expect("lock").clone();
    assert!(
        boot_requests.iter().any(|path| path == "/api/agent")
            && boot_requests.iter().any(|path| path == "/api/model"),
        "the boot catalog fetched agent + model lists: {boot_requests:?}"
    );
    // A catalog event in THIS location triggers the refresh
    // (transport.ts:1009-1014's location filter).
    let before = stub.catalog_requests.lock().expect("lock").len();
    let _ = stub.events.send(
        serde_json::json!({
            "type": "agent.updated",
            "location": { "directory": "/tmp/opencode/mini-pty" },
        })
        .to_string(),
    );
    let refresh_deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < refresh_deadline {
        if stub.catalog_requests.lock().expect("lock").len() > before {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let after = stub.catalog_requests.lock().expect("lock").clone();
    assert!(after.len() > before, "agent.updated re-fetched the catalogs: {after:?}");
    // A FOREIGN location's event never refreshes.
    let before = after.len();
    let _ = stub.events.send(
        serde_json::json!({
            "type": "agent.updated",
            "location": { "directory": "/tmp/opencode/elsewhere" },
        })
        .to_string(),
    );
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        stub.catalog_requests.lock().expect("lock").len(),
        before,
        "a foreign location's catalog event never refreshes"
    );

    mini.exit_sequence();
    mini.finish();
}
