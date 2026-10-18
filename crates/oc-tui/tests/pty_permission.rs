//! PTY permission regressions (C6a / TUI-TD18 + TUI-TD19): the REAL
//! [`oc_tui::app::run_with_size`] loop over a pty against a scripted
//! server, driven with actual keystrokes.
//!
//! * **TD18** — the session-route TWO-STAGE machine
//!   (`routes/session/permission.tsx:20` `PermissionStage =
//!   "permission" | "reject"`): `always` replies on a SINGLE enter
//!   (no confirm stage), escape on a top-level session rejects
//!   immediately, and the free-text reject stage opens only on CHILD
//!   sessions (`session()?.parentID`, permission.tsx:249-254).
//! * **TD19** — the edit ask renders its diff preview
//!   (`metadata.files[0].patch || .diff || metadata.diff`, else
//!   `input.patchText` — util/permission.ts:32-43): the user no
//!   longer blind-approves file edits.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// The scripted server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    body: String,
}

struct ServerState {
    requests: Mutex<Vec<Recorded>>,
    sse: Mutex<Vec<Arc<Mutex<std::net::TcpStream>>>>,
    events: AtomicU64,
    /// The permission ask the next `session.prompt` POST arms.
    next_ask: Mutex<Option<serde_json::Value>>,
    /// Fire the armed ask on the SSE handshake instead of a prompt
    /// POST (child sessions mount the picker — no composer to submit
    /// through).
    auto_fire: AtomicBool,
    /// Extra events to push once the SSE stream is live (session
    /// registry seeds, e.g. the child session's parentID).
    seeded: AtomicBool,
}

impl ServerState {
    fn record(&self, method: &str, path: &str, body: &str) {
        self.requests.lock().expect("requests").push(Recorded {
            method: method.to_owned(),
            path: path.to_owned(),
            body: body.to_owned(),
        });
    }

    fn paths(&self, needle: &str) -> Vec<Recorded> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|request| request.path.contains(needle))
            .cloned()
            .collect()
    }

    #[allow(clippy::needless_pass_by_value)] // the json! sites hand over owned values
    fn push_event(&self, type_: &str, data: serde_json::Value) {
        let id = format!("evt_{}", self.events.fetch_add(1, Ordering::SeqCst));
        let event = serde_json::json!({ "id": id, "type": type_, "data": &data });
        let encoded = serde_json::to_string(&event).expect("event serializes");
        let frame = format!("data: {encoded}\n\n");
        let mut streams = self.sse.lock().expect("sse").clone();
        streams.retain(|stream| stream.lock().expect("stream").write_all(frame.as_bytes()).is_ok());
    }

    /// The permission ask pushed once (the seeded single-shot shape).
    fn arm(&self, ask: serde_json::Value) {
        *self.next_ask.lock().expect("next ask") = Some(ask);
    }

    fn fire(&self) {
        let ask = self.next_ask.lock().expect("next ask").take();
        if let Some(ask) = ask {
            self.push_event("permission.asked", ask);
        }
    }
}

fn model_catalog_json() -> String {
    let model = r#"{"providerID":"parityfake","modelID":"fake-model","id":"fake-model","name":"fake-model",
        "status":"active","enabled":true,"cost":[],"variants":[],
        "capabilities":{"input":["text"],"output":["text"],"tools":false},
        "limit":{"context":1000,"output":100},
        "time":{"released":1.0}}"#;
    format!(r#"{{"data":[{model}],"location":{{"directory":"/tmp"}}}}"#)
}

fn session_json(id: &str) -> String {
    let tokens =
        r#"{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}"#;
    format!(
        r#"{{"id":"{id}","title":"Session {id}","time":{{"created":1,"updated":2,"idle":3,"viewed":3}},
            "location":{{"directory":"/tmp"}},"projectID":"p","cost":0,"tokens":{tokens}}}"#
    )
}

fn json_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[allow(unused_assignments)] // the head-bound reassignment is the read loop's accumulator
#[allow(clippy::too_many_lines)] // one scripted-response match over the endpoint alphabet
fn handle(mut socket: std::net::TcpStream, state: &Arc<ServerState>) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = [0u8; 16384];
    let mut head = String::new();
    let mut content_length = 0usize;
    loop {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                head.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if let Some(rest) = head.split_once("\r\n\r\n") {
                    content_length = rest
                        .0
                        .lines()
                        .find_map(|line| {
                            let lower = line.to_ascii_lowercase();
                            lower
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if head.len() >= rest.0.len() + 4 + content_length {
                        break;
                    }
                }
            }
        }
    }
    let (head_text, body) = head.split_once("\r\n\r\n").expect("request head");
    let body = body.to_owned();
    socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let mut parts = head_text.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let raw_path = parts.next().unwrap_or_default().to_owned();
    let path = raw_path.split('?').next().unwrap_or_default().to_owned();
    state.record(&method, &path, &body);

    if path == "/api/event" {
        let response = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
        if socket.write_all(response).is_err() {
            return;
        }
        let shared = Arc::new(Mutex::new(socket));
        state.sse.lock().expect("sse").push(Arc::clone(&shared));
        // The boot handshake: the real server greets every subscriber
        // with `server.connected` — without it the store stays
        // Connecting and the Reconnecting scrim covers the screen
        // after its 5 s threshold.
        state.push_event("server.connected", serde_json::json!({}));
        if !state.seeded.swap(true, Ordering::SeqCst) {
            state.push_event(
                "session.created",
                serde_json::json!({ "sessionID": "ses_child", "parentID": "ses_perm" }),
            );
        }
        if state.auto_fire.load(Ordering::SeqCst) {
            state.fire();
        }
        // The session registry seed: mark ses_child as a CHILD of
        // ses_perm (the free-text reject stage's gate).
        if !state.seeded.swap(true, Ordering::SeqCst) {
            state.push_event(
                "session.created",
                serde_json::json!({ "sessionID": "ses_child", "parentID": "ses_perm" }),
            );
        }
        if state.auto_fire.load(Ordering::SeqCst) {
            state.fire();
        }
        {
            let stream = shared.lock().expect("stream");
            let _ = stream.set_read_timeout(Some(Duration::from_millis(25)));
        }
        let mut sink = [0u8; 256];
        loop {
            let result = shared.lock().expect("stream").read(&mut sink);
            match result {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
        return;
    }

    let response = if path == "/api/model" {
        json_response(&model_catalog_json())
    } else if path == "/api/session" {
        json_response(&format!(
            r#"{{"cursor":{{}},"data":[{},{}]}}"#,
            session_json("ses_perm"),
            session_json("ses_child")
        ))
    } else if path.ends_with("/message") {
        json_response(r#"{"cursor":{},"data":[]}"#)
    } else if path.ends_with("/model") && method == "POST" {
        // session.switchModel — 204 No Content (the wire success
        // shape); a 200-with-body fails the client and aborts the
        // prompt submit ("Failed to switch model").
        "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_owned()
    } else if path.ends_with("/prompt") && method == "POST" {
        state.fire();
        // The wire's `data`-wrapped inbox-user echo (the client
        // deserializes this — a bare `{}` fails and arms the modal
        // send-failure toast that swallows keys).
        let inbox_id = format!("inbox_{}", state.events.fetch_add(1, Ordering::SeqCst));
        json_response(&format!(
            r#"{{"data":{{"id":"{inbox_id}","delivery":"steer","type":"user","sessionID":"ses_perm","time":{{"created":1}},"payload":{{"text":""}}}}}}"#
        ))
    } else if path.contains("/permission/") && method == "POST" {
        // session.permission.reply — settle the ask over SSE.
        let request_id = path
            .trim_end_matches("/reply")
            .rsplit("/permission/")
            .next()
            .unwrap_or_default()
            .to_owned();
        let decision: String = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("decision").and_then(|d| d.as_str()).map(str::to_owned))
            .unwrap_or_default();
        state.push_event(
            "permission.replied",
            serde_json::json!({ "requestID": request_id, "sessionID": "ses_perm" }),
        );
        let _ = decision;
        "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_owned()
    } else {
        json_response("{}")
    };
    let _ = socket.write_all(response.as_bytes());
}

fn spawn_scripted_server() -> (String, Arc<ServerState>, Arc<AtomicBool>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let state = Arc::new(ServerState {
        requests: Mutex::new(Vec::new()),
        sse: Mutex::new(Vec::new()),
        events: AtomicU64::new(1),
        next_ask: Mutex::new(None),
        auto_fire: AtomicBool::new(false),
        seeded: AtomicBool::new(false),
    });
    let flag = Arc::clone(&shutdown);
    let server_state = Arc::clone(&state);
    std::thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking accept");
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let _ = socket.set_nodelay(true);
                    let state = Arc::clone(&server_state);
                    std::thread::spawn(move || handle(socket, &state));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}"), state, shutdown)
}

// ---------------------------------------------------------------------------
// The pty harness (the pty_restore family)
// ---------------------------------------------------------------------------

struct PtyGuard {
    master: File,
    saved_stdin: RawFd,
    saved_stdout: RawFd,
}

impl Drop for PtyGuard {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.saved_stdin, libc::STDIN_FILENO);
            libc::dup2(self.saved_stdout, libc::STDOUT_FILENO);
            libc::close(self.saved_stdin);
            libc::close(self.saved_stdout);
        }
    }
}

fn attach_pty() -> PtyGuard {
    let master = unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(fd >= 0, "posix_openpt");
        libc::grantpt(fd);
        libc::unlockpt(fd);
        File::from_raw_fd(fd)
    };
    let slave_path = unsafe {
        let name = libc::ptsname(master.as_raw_fd());
        assert!(!name.is_null(), "ptsname");
        std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
    };
    let slave =
        std::fs::OpenOptions::new().read(true).write(true).open(&slave_path).expect("open slave");
    unsafe {
        let winsize = libc::winsize { ws_row: 30, ws_col: 100, ws_ypixel: 0, ws_xpixel: 0 };
        libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
    }
    let saved_stdin = unsafe { libc::dup(libc::STDIN_FILENO) };
    let saved_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
    unsafe {
        libc::dup2(slave.as_raw_fd(), libc::STDIN_FILENO);
        libc::dup2(slave.as_raw_fd(), libc::STDOUT_FILENO);
    }
    drop(slave);
    PtyGuard { master, saved_stdin, saved_stdout }
}

#[derive(Clone)]
struct Boot {
    output: Arc<Mutex<Vec<u8>>>,
}

/// Strip ANSI escape sequences (CSI/OSC) — ratatui emits per-cell SGR
/// runs, so multi-glyph needles never sit contiguously in the raw
/// byte stream (e.g. a typed draft paints as `q<SGR>1<SGR>w…`).
fn strip_ansi(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == 0x1b {
            let rest = &input[index..];
            if rest.len() >= 2 && rest[1] == b'[' {
                index += 2;
                while index < input.len() && !input[index].is_ascii_alphabetic() {
                    index += 1;
                }
                index += 1; // the final byte
            } else if rest.len() >= 2 && rest[1] == b']' {
                index += 2;
                while index < input.len() && input[index] != 0x07 && input[index] != 0x1b {
                    index += 1;
                }
                if index < input.len() && input[index] == 0x07 {
                    index += 1;
                }
            } else {
                index += 2; // two-byte escape
            }
        } else {
            out.push(input[index]);
            index += 1;
        }
    }
    out
}

fn wait_for(boot: &Boot, needle: &[u8], timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = strip_ansi(&boot.output.lock().expect("lock").clone());
        if data.windows(needle.len()).any(|window| window == needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-perm-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos())
    ));
    std::fs::create_dir_all(root.join(".config")).expect("config dir");
    std::fs::create_dir_all(root.join(".local/state")).expect("state dir");
    std::env::set_var("OPENCODE_TEST_HOME", &root);
    std::env::set_var("XDG_CONFIG_HOME", root.join(".config"));
    std::env::set_var("XDG_STATE_HOME", root.join(".local/state"));
    root
}

fn run_tui<F>(url: &str, session: &str, choreography: F)
where
    F: FnOnce(Boot, File) + Send + 'static,
{
    let guard = attach_pty();
    let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let reader_master = guard.master.try_clone().expect("clone read");
    let writer_master = guard.master.try_clone().expect("clone write");
    {
        let output = Arc::clone(&output);
        let mut reply_writer = guard.master.try_clone().expect("clone reply");
        std::thread::spawn(move || {
            let mut reader = reader_master;
            let mut chunk = [0u8; 4096];
            let mut replied = false;
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        output.lock().expect("output lock").extend_from_slice(&chunk[..count]);
                        if !replied
                            && chunk[..count].windows(6).any(|window| window == b"\x1b]11;?")
                        {
                            replied = true;
                            let _ = reply_writer.write_all(b"\x1b]11;rgb:1717/1717/1717\x1b\\");
                        }
                    }
                }
            }
        });
    }
    let boot = Boot { output: Arc::clone(&output) };
    std::thread::spawn(move || choreography(boot, writer_master));

    let client = oc_client::Client::new(url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let finished = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(45),
            oc_tui::app::run_with_size(
                client,
                Some((100, 30)),
                oc_tui::app::BootArgs {
                    continue_last: false,
                    session: Some(session.to_owned()),
                    ..oc_tui::app::BootArgs::default()
                },
            ),
        )
        .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => {
            let captured = output.lock().expect("output lock").clone();
            let dump = std::env::temp_dir().join(format!(
                "oc-tui-pty-perm-timeout-{}-{}.raw",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.subsec_nanos())
            ));
            let _ = std::fs::write(&dump, &captured);
            let tail = String::from_utf8_lossy(&captured[captured.len().saturating_sub(2000)..])
                .into_owned();
            panic!(
                "the TUI did not exit in time: {elapsed} (dumped {}): tail {tail:?}",
                dump.display()
            );
        }
    }
    drop(guard);
}

fn type_keys(mut writer: &File, keys: &[u8]) {
    let _ = writer.write_all(keys);
    std::thread::sleep(Duration::from_millis(150));
}

/// The session-route composer renders NO placeholder (v2 mounts
/// `Prompt` without a placeholder list) — probe boot readiness by
/// typing a distinctive marker and waiting for its draft echo, then
/// clearing it.
fn wait_for_composer(boot: &Boot, writer: &File) {
    // Wait for the app's FIRST PAINT before typing: the boot owns
    // stdin exclusively until then (the terminal probe's 300 ms read
    // window + the pre-loop setup), and bytes typed earlier are eaten
    // by the probe or echoed by the still-cooked line discipline. The
    // composer's status hints paint on every session route ≥60 cols.
    assert!(
        wait_for(boot, b"shift+tab agents", Duration::from_secs(15)),
        "the session route painted"
    );
    type_keys(writer, b"q1w2e3");
    assert!(
        wait_for(boot, b"q1w2e3", Duration::from_secs(15)),
        "the session composer rendered and took keys"
    );
    type_keys(writer, b"\x7f\x7f\x7f\x7f\x7f\x7f");
}

/// Wait until one recorded POST body for `needle` satisfies `check`.
fn wait_for_reply<F>(state: &Arc<ServerState>, needle: &str, timeout: Duration, check: F)
where
    F: Fn(&Recorded) -> bool,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let hits: Vec<Recorded> =
            state.paths(needle).into_iter().filter(|r| r.method == "POST").collect();
        if hits.iter().any(&check) {
            return;
        }
        assert!(
            std::time::Instant::now() <= deadline,
            "no matching POST for {needle}: {:?}",
            state.paths(needle)
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ---------------------------------------------------------------------------
// The scenarios
// ---------------------------------------------------------------------------

/// TD18: escape on a TOP-LEVEL session rejects immediately — one esc,
/// one `reject` POST, the composer restores (the old three-stage
/// machine opened a free-text prompt instead and never replied).
#[test]
fn esc_rejects_immediately() {
    let _sandbox = sandbox_env("td18-esc");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);
    state.arm(serde_json::json!({
        "id": "per_esc",
        "sessionID": "ses_perm",
        "action": "read",
        "resources": ["src/main.rs"],
        "save": []
    }));

    run_tui(&url, "ses_perm", move |boot, writer| {
        wait_for_composer(&boot, &writer);
        type_keys(&writer, b"go\r");
        assert!(wait_for(&boot, b"Permission required", Duration::from_secs(8)), "the ask mounted");
        type_keys(&writer, b"\x1b");
        wait_for_reply(&state, "/permission/", Duration::from_secs(8), |hit| {
            hit.body.contains("reject")
        });
        wait_for_composer(&boot, &writer);
        // The reject stage NEVER opened for a top-level session.
        let out = strip_ansi(&boot.output.lock().expect("lock").clone());
        assert!(
            !String::from_utf8_lossy(&out).contains("Tell OpenCode what to do differently"),
            "the free-text reject stage must not open on a top-level session"
        );
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}

/// TD18: `always` replies on a SINGLE enter (no confirm stage), with
/// the always-lines preview shown while the pill is selected.
#[test]
fn always_replies_on_a_single_enter() {
    let _sandbox = sandbox_env("td18-always");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);
    state.arm(serde_json::json!({
        "id": "per_always",
        "sessionID": "ses_perm",
        "action": "read",
        "resources": ["src/main.rs"],
        "save": ["src/**"]
    }));

    run_tui(&url, "ses_perm", move |boot, writer| {
        wait_for_composer(&boot, &writer);
        type_keys(&writer, b"go\r");
        assert!(wait_for(&boot, b"Permission required", Duration::from_secs(8)), "the ask mounted");
        // right → "Always allow" selected: the body swaps to the
        // always-lines preview (`body={(option) => ...}`).
        type_keys(&writer, b"l");
        // (Two fragments: the terminal diff layer may repaint a
        // single cell style-only between frames — the composed row
        // never sits byte-contiguous.)
        assert!(
            wait_for(&boot, b"This will", Duration::from_secs(4)),
            "the always preview replaced the presentation"
        );
        assert!(
            wait_for(&boot, b"the following patterns for this project.", Duration::from_secs(4)),
            "the always preview's pattern lines rendered"
        );
        // ONE enter: the reply fires immediately (the old machine
        // staged a Confirm/Cancel pair first).
        type_keys(&writer, b"\r");
        wait_for_reply(&state, "/permission/", Duration::from_secs(8), |hit| {
            hit.body.contains("always")
        });
        wait_for_composer(&boot, &writer);
        let out = strip_ansi(&boot.output.lock().expect("lock").clone());
        assert!(
            !String::from_utf8_lossy(&out).contains("Confirm"),
            "no confirm stage may appear for always"
        );
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}

/// TD18: the free-text reject stage opens only on CHILD sessions
/// (`session()?.parentID`): esc stages, typing builds the message,
/// enter sends the reject WITH the message.
#[test]
fn free_text_reject_opens_on_child_sessions_only() {
    let _sandbox = sandbox_env("td18-child");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);
    // ses_child's parentID was seeded by the SSE handler
    // (session.created with parentID ses_perm). Child sessions mount
    // the subagent picker (v2 index.tsx:1451) — no composer — so the
    // ask fires on the SSE handshake.
    state.arm(serde_json::json!({
        "id": "per_child",
        "sessionID": "ses_child",
        "action": "bash",
        "resources": [],
        "save": [],
        "input": { "command": "cargo test" }
    }));
    state.auto_fire.store(true, Ordering::SeqCst);

    run_tui(&url, "ses_child", move |boot, writer| {
        assert!(
            wait_for(&boot, b"Permission required", Duration::from_secs(10)),
            "the child ask mounted"
        );
        type_keys(&writer, b"\x1b");
        assert!(
            wait_for(&boot, b"Tell OpenCode what to do differently", Duration::from_secs(4)),
            "esc on a CHILD session opens the free-text reject stage"
        );
        type_keys(&writer, b"do less\r");
        wait_for_reply(&state, "/permission/", Duration::from_secs(8), |hit| {
            hit.body.contains("reject") && hit.body.contains("do less")
        });
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}

/// TD19: an edit ask renders its diff preview — the hunk lines from
/// `metadata.files[0].diff` paint in the ask body, not just the
/// `Edit <file>` title.
#[test]
fn edit_diff_rendered() {
    let _sandbox = sandbox_env("td19");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);
    let diff = "@@ -1,2 +1,2 @@\n const old = 1\n-const gone = 2\n+const fresh = 3\n";
    state.arm(serde_json::json!({
        "id": "per_edit",
        "sessionID": "ses_perm",
        "action": "edit",
        "resources": ["src/lib.rs"],
        "save": [],
        "metadata": { "files": [ { "diff": diff } ] }
    }));

    run_tui(&url, "ses_perm", move |boot, writer| {
        wait_for_composer(&boot, &writer);
        type_keys(&writer, b"go\r");
        assert!(
            wait_for(&boot, b"Edit src/lib.rs", Duration::from_secs(8)),
            "the edit ask's title rendered"
        );
        // RED today: `lines` is always empty for edit — the diff body
        // never paints and the user blind-approves.
        assert!(
            wait_for(&boot, b"-const gone = 2", Duration::from_secs(4)),
            "TD19: the removed diff line rendered"
        );
        assert!(
            wait_for(&boot, b"+const fresh = 3", Duration::from_secs(4)),
            "TD19: the added diff line rendered"
        );
        let _ = state;
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}
