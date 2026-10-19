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
}

#[derive(Clone, Copy, PartialEq)]
enum PromptMode {
    PlainReply,
    PermissionAsk,
}

async fn start_stub(mode: PromptMode) -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let port = listener.local_addr().expect("addr").port();
    let (events_tx, _) = tokio::sync::broadcast::channel::<String>(64);
    let output = Arc::new(Mutex::new(Vec::new()));
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(Vec::new()));
    let mode_storage = Arc::new(Mutex::new(mode));

    let server = Stub { port, prompts: Arc::clone(&prompts), replies: Arc::clone(&replies) };

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
            let mode = Arc::clone(&mode_storage);
            tokio::spawn(async move {
                serve_connection(&mut socket, sub_tx, &output, &prompts, &replies, &mode).await;
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

async fn serve_connection(
    socket: &mut tokio::net::TcpStream,
    sub_tx: tokio::sync::broadcast::Sender<String>,
    output: &Arc<Mutex<Vec<u8>>>,
    prompts: &Arc<Mutex<Vec<String>>>,
    replies: &Arc<Mutex<Vec<String>>>,
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
    if method == "GET" && path_only == format!("/api/session/{SESSION_ID}/message") {
        respond_json(socket, r#"{"cursor":{},"data":[]}"#).await;
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
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
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
        PromptMode::PlainReply => {
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
            // Make the pty the child's controlling terminal: crossterm
            // reads the cursor-position reply from /dev/tty, so the
            // slave must BE /dev/tty (setsid + TIOCSCTTY).
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
            .arg(SESSION_ID)
            .arg("--no-replay")
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
                            // position query (`ESC[6n`, the inline
                            // viewport's boot probe) — emulate it once.
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
