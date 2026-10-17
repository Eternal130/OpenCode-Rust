//! Dogfood wave-1 PTY regressions (the restore/open/render chain +
//! the model-selection dead-ends + `<leader>e`): the REAL
//! [`oc_tui::app::run_with_size`] loop over a pty against a scripted
//! FakeLlm-style server, driven with actual keystrokes.
//!
//! * **F19** — a RESTORED session (`-s`, straight from the fixture
//!   "DB") renders its transcript chronologically at the sticky
//!   bottom AND live-renders a new turn's reply (the old bug: the
//!   desc-ordered snapshot put new turns below the fold, frozen).
//! * **F12** — opening a session from the `/sessions` dialog FETCHES
//!   the transcript (50 rows render; the old bug left a blank void —
//!   only the boot resync ever fetched).
//! * **F7** — switch-away-and-back: the transcript tail renders
//!   identically after the round trip (no stale-row contamination).
//! * **F13** — the `/sessions` dialog's open refetches `session.list`.
//! * **F8/F10/F22** — the model-selection chain: pristine home
//!   resolves the catalog head (a submit reaches switch-model +
//!   prompt, not the "No model" dead end), a new tab inherits the
//!   active session's explicit pick, and a restarted session restores
//!   its durable model from the session record.
//! * **F16** — `<leader>e` spawns `$EDITOR` over the focused draft
//!   (the stub writes a marker file; the edited text replaces the
//!   draft).

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// The scripted server
// ---------------------------------------------------------------------------

/// One recorded request (method, path, body).
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    body: String,
}

struct ServerState {
    requests: Mutex<Vec<Recorded>>,
    /// Live SSE connections: (socket clone, id) — the prompt handler
    /// pushes turn events through them.
    sse: Mutex<Vec<Arc<Mutex<std::net::TcpStream>>>>,
    /// Monotonic event + message id seeds.
    events: AtomicU64,
    sessions_created: AtomicU64,
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

    /// Push one bus event frame (JSON-in-string, the `V2EventEncoded`
    /// double-encode) to every live SSE subscriber.
    #[allow(clippy::needless_pass_by_value)] // the json! sites hand over owned values
    fn push_event(&self, type_: &str, data: serde_json::Value) {
        let id = format!("evt_{}", self.events.fetch_add(1, Ordering::SeqCst));
        let event = serde_json::json!({ "id": id, "type": type_, "data": &data });
        let encoded = serde_json::to_string(&event).expect("event serializes");
        let frame = format!("data: {encoded}\n\n");
        let mut streams = self.sse.lock().expect("sse").clone();
        streams.retain(|stream| stream.lock().expect("stream").write_all(frame.as_bytes()).is_ok());
    }

    /// The full live-turn event choreography for one prompt reply
    /// (the engine's publish order, from the real bus shapes).
    fn push_turn(&self, session: &str, text: &str, reply: &str) {
        let user_id = format!("msg_u_live_{}", self.events.fetch_add(1, Ordering::SeqCst));
        let assistant_id = format!("msg_a_live_{}", self.events.fetch_add(1, Ordering::SeqCst));
        self.push_event(
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": session,
                "inboxID": user_id,
                "item": { "type": "user", "delivery": "steer", "payload": { "text": text } }
            }),
        );
        self.push_event(
            "session.step.started",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": assistant_id,
                "agent": "build",
                "model": { "providerID": "parityfake", "id": "fake-model" },
                "started": 0
            }),
        );
        self.push_event(
            "session.text.delta",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": assistant_id,
                "ordinal": 0,
                "delta": reply
            }),
        );
        self.push_event(
            "session.text.ended",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": assistant_id,
                "ordinal": 0,
                "text": reply
            }),
        );
        self.push_event(
            "session.step.ended",
            serde_json::json!({
                "sessionID": session,
                "assistantMessageID": assistant_id,
                "finish": "stop",
                "cost": 0,
                "tokens": { "input": 1, "output": 1 }
            }),
        );
    }
}

/// The model catalog: three models so the chain steps are
/// distinguishable (the catalog head `fake-model`, an explicit-pick
/// target `picked-model`, a durable-restore target `recorded-model`).
fn model_catalog_json() -> String {
    let model = |id: &str| {
        format!(
            r#"{{"providerID":"parityfake","modelID":"{id}","id":"{id}","name":"{id}",
                "status":"active","enabled":true,"cost":[],"variants":[],
                "capabilities":{{"input":["text"],"output":["text"],"tools":false}},
                "limit":{{"context":1000,"output":100}},
                "time":{{"released":1.0}}}}"#
        )
    };
    format!(
        r#"{{"data":[{},{},{}],"location":{{"directory":"/tmp"}}}}"#,
        model("fake-model"),
        model("picked-model"),
        model("recorded-model")
    )
}

/// One session row (`SessionInfo` wire shape, the `pty_tabs` fixture
/// family + the durable `model` field).
fn session_json(id: &str, title: &str, model: Option<&str>) -> String {
    let model_field = model
        .map(|model| format!(r#","model":{{"providerID":"parityfake","id":"{model}"}}"#))
        .unwrap_or_default();
    let tokens =
        r#"{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}"#;
    format!(
        r#"{{"id":"{id}","title":{title}{model_field},
            "time":{{"created":1,"updated":2,"idle":3,"viewed":3}},
            "location":{{"directory":"/tmp"}},"projectID":"p","cost":0,"tokens":{tokens}}}"#
    )
}

/// `ses_restore`'s transcript: 50 chronological turns, served
/// NEWEST-FIRST (`order: "desc"` — the wire default the client must
/// reverse).
fn restore_messages_desc() -> String {
    let mut rows = Vec::new();
    for index in (0..50usize).rev() {
        rows.push(format!(
            r#"{{"type":"user","id":"msg_u_{index}","text":"turn-{index}-ask","time":{{"created":{index}}}}}"#
        ));
        rows.push(format!(
            r#"{{"type":"assistant","id":"msg_a_{index}","agent":"build",
                "model":{{"providerID":"parityfake","id":"fake-model"}},
                "time":{{"created":{},"completed":{}}},
                "content":[{{"type":"text","text":"turn-{index}-reply"}}]}}"#,
            index + 1,
            index + 2
        ));
    }
    format!(r#"{{"cursor":{{}},"data":[{}]}}"#, rows.join(","))
}

/// `ses_dialog`'s transcript: two turns, distinct markers.
fn dialog_messages_desc() -> String {
    r#"{"cursor":{},"data":[
        {"type":"user","id":"msg_d_u2","text":"s-ask-2","time":{"created":4}},
        {"type":"assistant","id":"msg_d_a2","agent":"build",
         "model":{"providerID":"parityfake","id":"fake-model"},
         "time":{"created":5,"completed":6},
         "content":[{"type":"text","text":"s-reply-2"}]},
        {"type":"user","id":"msg_d_u1","text":"s-ask-1","time":{"created":1}},
        {"type":"assistant","id":"msg_d_a1","agent":"build",
         "model":{"providerID":"parityfake","id":"fake-model"},
         "time":{"created":2,"completed":3},
         "content":[{"type":"text","text":"s-reply-1"}]}
    ]}"#
    .to_owned()
}

fn json_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[allow(unused_assignments)] // the head-bound reassignment is the read loop's accumulator
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

    if path.starts_with("/api/event") {
        let response = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
        if socket.write_all(response).is_err() {
            return;
        }
        // Register for live pushes; park until the peer goes away. The
        // read parks on a SHORT timeout and RELEASES the lock between
        // ticks so `push_event` writers always get a window (a
        // blocking read under the lock would deadlock the pusher).
        let shared = Arc::new(Mutex::new(socket));
        state.sse.lock().expect("sse").push(Arc::clone(&shared));
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
    } else if path == "/api/session" && method == "POST" {
        // session.create: a fresh id + title (the wire wraps the
        // SessionInfo in a `data` envelope).
        let id = format!("ses_new_{}", state.sessions_created.fetch_add(1, Ordering::SeqCst));
        json_response(&format!(r#"{{"data":{}}}"#, session_json(&id, "null", None)))
    } else if path == "/api/session" {
        json_response(&format!(
            r#"{{"cursor":{{}},"data":[{},{}]}}"#,
            session_json("ses_restore", r#""Restore target""#, Some("recorded-model")),
            session_json("ses_dialog", r#""Dialog source""#, None)
        ))
    } else if path == "/api/session/ses_restore/message" {
        json_response(&restore_messages_desc())
    } else if path == "/api/session/ses_dialog/message" {
        json_response(&dialog_messages_desc())
    } else if path.ends_with("/model") && method == "POST" {
        // session.switchModel — 204 No Content (the wire success
        // shape); the body was recorded above for the chain
        // assertions.
        "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_owned()
    } else if path.ends_with("/prompt") && method == "POST" {
        // session.prompt: push the live-turn events (the reply text
        // names the session so assertions distinguish turns), then the
        // wire's `data`-wrapped inbox-user echo.
        let text = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("text").and_then(|text| text.as_str()).map(str::to_owned))
            .unwrap_or_default();
        let session = path.trim_start_matches("/api/session/").trim_end_matches("/prompt");
        let reply = format!("Live reply for {session}.");
        state.push_turn(session, &text, &reply);
        let inbox_id = format!("inbox_{}", state.events.fetch_add(1, Ordering::SeqCst));
        let echo = format!(
            r#"{{"data":{{"id":"{inbox_id}","delivery":"steer","type":"user","sessionID":"{session}","time":{{"created":1}},"payload":{{"text":{}}}}}}}"#,
            serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".to_owned())
        );
        json_response(&echo)
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
        sessions_created: AtomicU64::new(1),
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
// The pty harness (the pty_boot/pty_tabs family)
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
        let winsize = libc::winsize { ws_row: 30, ws_col: 100, ws_xpixel: 0, ws_ypixel: 0 };
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

fn wait_for(boot: &Boot, needle: &[u8], timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = boot.output.lock().expect("lock").clone();
        if data.windows(needle.len()).any(|window| window == needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Sandbox state roots (tabs persistence + the model ring read/write
/// for real at boot).
fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-restore-{tag}-{}-{}",
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

/// Run the TUI over the pty while a choreography thread types. The
/// writer closure receives the master writer; steps return false to
/// abort.
fn run_tui<F>(url: &str, boot_args: oc_tui::app::BootArgs, choreography: F)
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
                        // The answering terminal (the pty_boot pattern):
                        // reply to the OSC 11 background query ONCE so
                        // the input thread's probe finishes immediately —
                        // an unanswered probe parks the input reader for
                        // its full timeout and EATS keystrokes typed in
                        // that window.
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
            oc_tui::app::run_with_size(client, Some((100, 30)), boot_args),
        )
        .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => {
            let captured = output.lock().expect("output lock").clone();
            let dump = std::env::temp_dir().join(format!(
                "oc-tui-pty-timeout-{}-{}.raw",
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
            )
        }
    }
    drop(guard);
}

/// Send keys as one atomic chunk (escape sequences must not be split).
fn type_keys(mut writer: &File, keys: &[u8]) {
    let _ = writer.write_all(keys);
    std::thread::sleep(Duration::from_millis(120));
}

// ---------------------------------------------------------------------------
// The scenarios
// ---------------------------------------------------------------------------

/// F19 + F16: the restored session renders chronologically at the
/// sticky bottom, live-renders a new turn, and `<leader>e` runs
/// `$EDITOR` over the draft.
#[test]
fn pty_restored_session_renders_and_live_updates() {
    let _sandbox = sandbox_env("f19");
    let (url, _state, shutdown) = spawn_scripted_server();
    let editor_marker = std::env::temp_dir().join(format!(
        "oc-tui-pty-editor-{}-{}.marker",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos())
    ));
    let marker_display = editor_marker.display().to_string();
    let editor_stub = std::env::temp_dir().join(format!(
        "oc-tui-pty-editor-stub-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos())
    ));
    std::fs::write(
        &editor_stub,
        format!(
            "#!/bin/sh\necho ran > {marker_display}\necho 'EDITED-BY-STUB' >> \"$1\"\nexit 0\n"
        ),
    )
    .expect("write stub");
    let mut permissions = std::fs::metadata(&editor_stub).expect("stub metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&editor_stub, permissions).expect("chmod stub");
    std::env::set_var("EDITOR", &editor_stub);
    let marker_path = editor_marker.clone();

    run_tui(
        &url,
        oc_tui::app::BootArgs {
            continue_last: false,
            session: Some("ses_restore".to_owned()),
            ..oc_tui::app::BootArgs::default()
        },
        move |boot, writer| {
            // (a) The boot resync hydrated the restored transcript IN
            // ORDER: the sticky-bottom viewport shows the LAST turn
            // (turn-49), not the wire-first turn-49-as-newest scramble.
            assert!(
                wait_for(&boot, b"turn-49-reply", Duration::from_secs(8)),
                "the restored transcript's newest turn rendered (chronological + sticky bottom)"
            );
            let early = boot.output.lock().expect("lock").clone();
            assert!(
                !String::from_utf8_lossy(&early).contains("turn-0-reply"),
                "the viewport sits at the bottom: the OLDEST row must be scrolled off"
            );
            // (b) F19 live render: submit a turn; the scripted reply
            // arrives over SSE and must paint.
            type_keys(&writer, b"live turn\r");
            assert!(
                wait_for(&boot, b"Live reply for ses_restore.", Duration::from_secs(8)),
                "the new turn's reply live-rendered"
            );
            // (c) F16: <leader>e spawns $EDITOR over the focused
            // draft — the stub writes the marker and the edit text
            // replaces the draft. (The needle skips the first glyph:
            // the blinking caret inverts the draft's first cell, so
            // the full string is never contiguous in the byte stream.)
            type_keys(&writer, b"\x18e");
            assert!(
                wait_for(&boot, b"DITED-BY-STUB", Duration::from_secs(8)),
                "the editor's content replaced the draft"
            );
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !editor_marker.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(editor_marker.exists(), "the stub $EDITOR ran");
            // Exit: escape clears any pending state, then ctrl+c.
            type_keys(&writer, b"\x1b");
            type_keys(&writer, b"\x03");
            type_keys(&writer, b"\x03");
        },
    );
    shutdown.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_file(&marker_path);
    let _ = std::fs::remove_file(&editor_stub);
}

/// F12 + F7 + F13: the /sessions dialog opens a transcript (the fetch
/// the blank void skipped), the dialog's open refetches session.list,
/// and switch-away-and-back repaints the same tail.
#[test]
fn pty_session_dialog_fetches_transcript_and_switch_is_stable() {
    let _sandbox = sandbox_env("f12");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);

    run_tui(&url, oc_tui::app::BootArgs::default(), move |boot, writer| {
        // (The needle skips the first glyph — the blinking caret
        // inverts the placeholder's first cell, splitting the run.)
        assert!(
            wait_for(&boot, b"sk anything", Duration::from_secs(15)),
            "the home composer rendered"
        );
        // F13: opening the dialog fires a fresh session.list beyond
        // the boot resync's.
        let session_lists = || {
            state
                .paths("/api/session")
                .iter()
                .filter(|request| request.method == "GET" && request.path == "/api/session")
                .count()
        };
        let before = session_lists();
        type_keys(&writer, b"\x18l"); // <leader>l — the session list
        assert!(
            wait_for(&boot, b"Restore target", Duration::from_secs(8)),
            "the session-list dialog rendered the history row"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while session_lists() <= before && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            session_lists() > before,
            "the dialog open refetched session.list ({before} -> {})",
            session_lists()
        );

        // F12: accepting the row FETCHES the transcript — 50 rows
        // render (the boot resync never covered this session).
        type_keys(&writer, b"\r");
        assert!(
            wait_for(&boot, b"turn-49-reply", Duration::from_secs(8)),
            "the dialog-opened session's transcript fetched and rendered"
        );

        // Open the second session through the dialog as well.
        type_keys(&writer, b"\x18l");
        if !wait_for(&boot, b"Dialog source", Duration::from_secs(8)) {
            let captured = boot.output.lock().expect("lock").clone();
            std::fs::write(
                std::env::temp_dir().join(format!("oc-tui-pty-dlg2-{}.raw", std::process::id())),
                &captured,
            )
            .ok();
            panic!("the dialog reopened — output dumped");
        }
        type_keys(&writer, b"\x1b[B"); // down to the second row
        type_keys(&writer, b"\r");
        assert!(
            wait_for(&boot, b"s-reply-2", Duration::from_secs(8)),
            "the second session's transcript rendered"
        );

        // F7: switch back to tab 1 — the SAME tail renders (no stale
        // fragments of the other tab's rows).
        type_keys(&writer, b"\x181"); // <leader>1
        assert!(
            wait_for(&boot, b"turn-49-reply", Duration::from_secs(8)),
            "switching back repainted the first session's tail"
        );
        // Exit.
        // Exit: escape clears any pending leader/panel, then ctrl+c
        // twice (the empty-prompt exit).
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}

/// F22 — the pristine home resolves the catalog head: the first
/// submit commits `fake-model` (switch-model THEN prompt, inside one
/// task) instead of dying on "No model is available".
#[test]
fn pty_pristine_home_submit_commits_the_catalog_head() {
    let _sandbox = sandbox_env("f8-home");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);

    run_tui(&url, oc_tui::app::BootArgs::default(), move |boot, writer| {
        // The composer hint carries the resolved catalog-head model.
        assert!(
            wait_for(&boot, b"fake-model", Duration::from_secs(8)),
            "the footer hint shows the resolved catalog-head model"
        );
        type_keys(&writer, b"from home\r");
        assert!(
            // Lane/chrome (NEW-S3a): plain transcript text renders at
            // the DEFAULT fg — cell-addressed redraws split the line's
            // runs, so byte needles must stay WITHIN one styled run
            // (`ses_new_1.` never crosses a style boundary).
            wait_for(&boot, b"ses_new_1.", Duration::from_secs(8)),
            "the home submission ran its turn"
        );
        // Exit this instance.
        // Exit: escape clears any pending leader/panel, then ctrl+c
        // twice (the empty-prompt exit).
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    let model_posts = state.paths("/model");
    let commits = model_posts
        .iter()
        .filter(|request| request.method == "POST" && request.body.contains("fake-model"))
        .count();
    assert!(commits >= 1, "the catalog head committed before the prompt ({commits} posts)");
    let prompts = state.paths("/prompt").iter().filter(|r| r.method == "POST").count();
    assert_eq!(prompts, 1, "exactly one prompt POST landed");
    shutdown.store(true, Ordering::SeqCst);
}

/// F10 — a new tab inherits the active session's explicit pick: pick
/// `picked-model` on the restored session, `session.new` routes home
/// with the pick staged, and the next submission commits it.
#[test]
fn pty_new_tab_inherits_the_active_pick() {
    let _sandbox = sandbox_env("f8-inherit");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);

    run_tui(&url, oc_tui::app::BootArgs::default(), move |boot, writer| {
        assert!(
            wait_for(&boot, b"fake-model", Duration::from_secs(8)),
            "boot resolved the catalog head"
        );
        // Open the restored session (an explicit session route), pick
        // picked-model through the model dialog (<leader>m + filter).
        type_keys(&writer, b"\x18l");
        assert!(
            wait_for(&boot, b"Restore target", Duration::from_secs(8)),
            "the session list dialog"
        );
        type_keys(&writer, b"\r");
        assert!(
            wait_for(&boot, b"turn-49-reply", Duration::from_secs(8)),
            "the restored transcript rendered"
        );
        type_keys(&writer, b"\x18m"); // <leader>m — model list
        assert!(
            wait_for(&boot, b"picked-model", Duration::from_secs(8)),
            "the model dialog listed the pick target"
        );
        type_keys(&writer, b"picked\r");
        std::thread::sleep(Duration::from_millis(400));
        // session.new inherits: <leader>n routes home with the pick
        // staged; the next submission commits picked-model.
        type_keys(&writer, b"\x18n");
        assert!(wait_for(&boot, b"sk anything", Duration::from_secs(8)), "session.new routed home");
        type_keys(&writer, b"inherited\r");
        assert!(
            // Lane/chrome (NEW-S3a): plain transcript text renders at
            // the DEFAULT fg — cell-addressed redraws split the line's
            // runs, so byte needles must stay WITHIN one styled run
            // (`ses_new_1.` never crosses a style boundary).
            wait_for(&boot, b"ses_new_1.", Duration::from_secs(8)),
            "the inherited submission ran its turn"
        );
        // Exit: escape clears any pending leader/panel, then ctrl+c
        // twice (the empty-prompt exit).
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    let model_posts = state.paths("/model");
    let inherited = model_posts
        .iter()
        .filter(|request| request.method == "POST" && request.body.contains("picked-model"))
        .count();
    assert!(
        inherited >= 2,
        "the pick committed for the session AND the inherited new tab ({inherited} posts)"
    );
    shutdown.store(true, Ordering::SeqCst);
}

/// F8 — restart restores the durable model: the session record carries
/// `recorded-model`; a fresh boot's first submit commits IT (durable
/// beats the catalog head).
#[test]
fn pty_restart_restores_the_durable_model() {
    let _sandbox = sandbox_env("f8-durable");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);

    run_tui(
        &url,
        oc_tui::app::BootArgs {
            continue_last: false,
            session: Some("ses_restore".to_owned()),
            ..oc_tui::app::BootArgs::default()
        },
        move |boot, writer| {
            assert!(
                wait_for(&boot, b"turn-49-reply", Duration::from_secs(8)),
                "the restored transcript rendered"
            );
            type_keys(&writer, b"restart turn\r");
            assert!(
                wait_for(&boot, b"Live reply for ses_restore.", Duration::from_secs(8)),
                "the restart submission ran its turn"
            );
            type_keys(&writer, b"\x03");
            type_keys(&writer, b"\x03");
        },
    );
    let restore_model_posts = state.paths("/api/session/ses_restore/model");
    let restart_commit =
        restore_model_posts.iter().filter(|request| request.method == "POST").collect::<Vec<_>>();
    assert!(
        restart_commit.iter().any(|request| request.body.contains("recorded-model")),
        "the restarted session committed its durable model: {restart_commit:?}"
    );
    shutdown.store(true, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Lane TAIL — the protocol/robustness PTY proofs (auditC P2/P3/P6/P7)
// ---------------------------------------------------------------------------

/// The kitty handshake, byte-asserted end-to-end (TM-1/P2): the boot
/// writes the `CSI ? u` query; a terminal that ANSWERS (`CSI ? 1 u`)
/// gets the `CSI > 5 u` push; teardown pops (`CSI < u`) and resets
/// the cursor (`CSI 0 SP q`). The theme subscriptions (`?2027h` +
/// `?2031h`, TM-2/P6) ride the same boot window.
#[test]
fn pty_kitty_handshake_and_teardown_bytes() {
    let _sandbox = sandbox_env("tail-kitty");
    let (url, _state, shutdown) = spawn_scripted_server();

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
            let mut osc11_replied = false;
            let mut kitty_replied = false;
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let bytes = &chunk[..count];
                        output.lock().expect("output lock").extend_from_slice(bytes);
                        // The answering terminal: OSC 11 + the kitty
                        // capability query — BOTH answered so the boot
                        // probe completes immediately.
                        if !osc11_replied && bytes.windows(6).any(|w| w == b"\x1b]11;?") {
                            osc11_replied = true;
                            let _ = reply_writer.write_all(b"\x1b]11;rgb:1717/1717/1717\x1b\\");
                        }
                        if !kitty_replied && bytes.windows(4).any(|w| w == b"\x1b[?u") {
                            kitty_replied = true;
                            // Advertise DISAMBIGUATE_ESCAPE_CODES.
                            let _ = reply_writer.write_all(b"\x1b[?1u");
                        }
                    }
                }
            }
        });
    }
    let boot_output = Arc::clone(&output);
    let mut exit_writer = writer_master.try_clone().expect("clone exit");
    std::thread::spawn(move || {
        // Wait for the frame, then leave the exit chord.
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let ready =
                boot_output.lock().expect("output lock").windows(8).any(|w| w == b"opencode");
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Exit: escape clears any pending state, ctrl+c twice (the
        // empty-prompt exit — the harness-wide chord).
        let _ = exit_writer.write_all(b"\x1b");
        std::thread::sleep(Duration::from_millis(150));
        let _ = exit_writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(150));
        let _ = exit_writer.write_all(b"\x03");
    });

    let client = oc_client::Client::new(&url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let finished = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(30),
            oc_tui::app::run_with_size(client, Some((100, 30)), oc_tui::app::BootArgs::default()),
        )
        .await
    });
    finished.expect("tui finished within timeout").expect("run_with_size ok");
    shutdown.store(true, Ordering::Relaxed);

    let captured = output.lock().expect("output lock").clone();
    let contains = |needle: &[u8]| captured.windows(needle.len()).any(|w| w == needle);
    // Boot: the subscriptions + the query went out.
    assert!(
        contains(b"\x1b[?2027h\x1b[?2031h"),
        "the theme-flip subscriptions (?2027h + ?2031h) emitted at boot"
    );
    assert!(contains(b"\x1b[?u"), "the kitty query emitted at boot");
    // The ANSWER gated the push (the advertisement arrived → push).
    assert!(contains(b"\x1b[>5u"), "the kitty push followed the advertisement");
    // Teardown: the pop + the cursor reset, at the tail.
    assert!(contains(b"\x1b[<u"), "the kitty pop at teardown");
    assert!(contains(b"\x1b[0 q"), "DECSCUSR reset at teardown");
}

/// The panic-restore finalizer (P3): a thread crashing mid-run while
/// the TUI owns the terminal triggers the chained panic hook, which
/// writes the full restore sequence to the pty — raw mode / alt
/// screen / mouse / kitty can never stay latched after a crash.
#[test]
fn pty_panic_mid_run_restores_the_terminal() {
    let _sandbox = sandbox_env("tail-panic");
    let (url, _state, shutdown) = spawn_scripted_server();

    run_tui(&url, oc_tui::app::BootArgs::default(), move |boot, writer| {
        assert!(
            wait_for(&boot, "█▀▀█".as_bytes(), Duration::from_secs(8)),
            "the home frame rendered before the crash"
        );
        // Baseline occurrence counts (the restore bytes may
        // land in the same reader chunk as the frame — an offset
        // watch would race them; counting does not).
        let count_of = |needle: &[u8]| -> usize {
            boot.output
                .lock()
                .expect("output lock")
                .windows(needle.len())
                .filter(|w| *w == needle)
                .count()
        };
        let baseline =
            [count_of(b"?1049l"), count_of(b"?2004l"), count_of(b"\x1b[<u"), count_of(b"\x1b[0 q")];
        // Crash a worker thread mid-run (the renderer's terminal
        // is UP: alt screen + raw mode + mouse latched).
        std::thread::spawn(|| panic!("mid-render crash (pty proof)"));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut restored = false;
        while std::time::Instant::now() < deadline {
            let now = [
                count_of(b"?1049l"),
                count_of(b"?2004l"),
                count_of(b"\x1b[<u"),
                count_of(b"\x1b[0 q"),
            ];
            if now.iter().zip(baseline).all(|(n, b)| *n > b) {
                restored = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            restored,
            "the panic hook wrote the crash-restore sequence (baseline {baseline:?})"
        );
        // The app itself is unaffected (the crash was contained);
        // leave through the standard chord.
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::Relaxed);
}
