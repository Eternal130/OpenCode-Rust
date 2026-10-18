//! PTY form regressions (C6a / TUI-TD16 + TUI-TD17): the REAL
//! [`oc_tui::app::run_with_size`] loop over a pty against a scripted
//! server, driven with actual keystrokes.
//!
//! * **TD16** — `form.created` mounts a BLOCKING footer surface: the
//!   form renders (title + fields + hints), keys are taken over while
//!   it is open (typed text lands in the form's answer editor, the
//!   composer underneath cannot type), and reply/cancel restore the
//!   composer (the next submission carries ONLY post-form text).
//! * **TD17** — typed field kinds parse: a form carrying
//!   boolean/number/integer fields is NOT dropped on ingest, answers
//!   validate (number ranges, integer-ness) and the reply POST carries
//!   typed JSON values (`true` / `2.5` / `7`).

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
    /// The form the next `session.prompt` POST asks (None ⇒ plain turn).
    next_form: Mutex<Option<serde_json::Value>>,
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

    /// Push one bus event frame to every live SSE subscriber.
    #[allow(clippy::needless_pass_by_value)] // the json! sites hand over owned values
    fn push_event(&self, type_: &str, data: serde_json::Value) {
        let id = format!("evt_{}", self.events.fetch_add(1, Ordering::SeqCst));
        let event = serde_json::json!({ "id": id, "type": type_, "data": &data });
        let encoded = serde_json::to_string(&event).expect("event serializes");
        let frame = format!("data: {encoded}\n\n");
        let mut streams = self.sse.lock().expect("sse").clone();
        streams.retain(|stream| stream.lock().expect("stream").write_all(frame.as_bytes()).is_ok());
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
        r#"{{"id":"{id}","title":null,"time":{{"created":1,"updated":2,"idle":3,"viewed":3}},
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
        json_response(&format!(r#"{{"cursor":{{}},"data":[{}]}}"#, session_json("ses_form")))
    } else if path.ends_with("/message") {
        json_response(r#"{"cursor":{},"data":[]}"#)
    } else if path.ends_with("/model") && method == "POST" {
        // session.switchModel — 204 No Content (the wire success
        // shape); a 200-with-body fails the client and aborts the
        // prompt submit ("Failed to switch model").
        "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_owned()
    } else if path.ends_with("/prompt") && method == "POST" {
        // session.prompt: ask the armed form (or run a plain turn).
        let form = state.next_form.lock().expect("next form").take();
        if let Some(form) = form {
            state.push_event("form.created", serde_json::json!({ "form": form }));
        }
        // The wire's `data`-wrapped inbox-user echo (the client
        // deserializes this — a bare `{}` fails and arms the modal
        // send-failure toast that swallows keys).
        let inbox_id = format!("inbox_{}", state.events.fetch_add(1, Ordering::SeqCst));
        json_response(&format!(
            r#"{{"data":{{"id":"{inbox_id}","delivery":"steer","type":"user","sessionID":"ses_form","time":{{"created":1}},"payload":{{"text":""}}}}}}"#
        ))
    } else if path.contains("/form/") && method == "POST" {
        // session.form.reply — settle the form over SSE.
        let form_id =
            path.trim_end_matches("/reply").rsplit("/form/").next().unwrap_or_default().to_owned();
        state.push_event(
            "form.replied",
            serde_json::json!({ "id": form_id, "sessionID": "ses_form", "answer": {} }),
        );
        "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_owned()
    } else if path.contains("/form/") && method == "DELETE" {
        let form_id = path.rsplit("/form/").next().unwrap_or_default().to_owned();
        state.push_event(
            "form.cancelled",
            serde_json::json!({ "id": form_id, "sessionID": "ses_form" }),
        );
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
        next_form: Mutex::new(None),
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
        "oc-tui-pty-form-{tag}-{}-{}",
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

fn run_tui<F>(url: &str, choreography: F)
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
                    session: Some("ses_form".to_owned()),
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
                "oc-tui-pty-form-timeout-{}-{}.raw",
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
/// The session-route composer renders NO placeholder (v2 mounts
/// `Prompt` without a placeholder list) — wait for the app's first
/// paint, then probe by typing a distinctive marker and waiting for
/// its draft echo, then clearing it.
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

// ---------------------------------------------------------------------------
// The scenarios
// ---------------------------------------------------------------------------

/// A two-field textual form (the deployment shape): "service"
/// (required) then "notes" (optional) then the review step.
fn deployment_form() -> serde_json::Value {
    serde_json::json!({
        "id": "frm_deploy",
        "sessionID": "ses_form",
        "title": "Deployment",
        "fields": [
            { "type": "string", "key": "service", "title": "Service", "required": true },
            { "type": "string", "key": "notes", "title": "Notes" }
        ]
    })
}

/// TD16: the form mounts, takes over the keys (the composer cannot
/// type while it is open), and reply/cancel restore the composer.
#[test]
fn form_mounts_and_blocks_keys() {
    let _sandbox = sandbox_env("td16");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);
    *state.next_form.lock().expect("next form") = Some(deployment_form());

    run_tui(&url, move |boot, writer| {
        wait_for_composer(&boot, &writer);
        // Ask the form: the prompt POST arms form.created on the server.
        type_keys(&writer, b"go\r");
        assert!(
            wait_for(&boot, b"Deployment", Duration::from_secs(8)),
            "TD16: the form.created event mounted a visible surface"
        );
        assert!(
            wait_for(&boot, b"esc dismiss", Duration::from_secs(4)),
            "the form's hint row rendered"
        );
        // While the form is live the composer must NOT type: typed
        // text lands in the FORM's answer editor (field 1 "service").
        // (The draft's presence is proven by the committed answer in
        // the reply POST below — the terminal diff layer may repaint
        // the draft row's cells style-only, so no byte-level draft
        // assertion here.)
        type_keys(&writer, b"api");
        type_keys(&writer, b"\r");
        type_keys(&writer, b"ship it\r");
        type_keys(&writer, b"\r");
        // The reply POST hit the server with both answers.
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            let replies = state.paths("/form/");
            if replies.iter().any(|r| r.method == "POST" && r.path.ends_with("/reply")) {
                break;
            }
            assert!(
                std::time::Instant::now() <= deadline,
                "the form reply POST never arrived: {:?}",
                state.paths("/form/")
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        let reply = state
            .paths("/form/")
            .into_iter()
            .find(|r| r.method == "POST" && r.path.ends_with("/reply"))
            .expect("reply POST");
        assert!(reply.path.contains("frm_deploy"), "the path names the form: {}", reply.path);
        let body: serde_json::Value = serde_json::from_str(&reply.body).expect("reply body");
        assert_eq!(body["answer"]["service"], "api", "body: {}", reply.body);
        assert_eq!(body["answer"]["notes"], "ship it", "body: {}", reply.body);
        // form.replied cleared the blocker: the composer is back and
        // a fresh submission carries ONLY post-form text (keys typed
        // during the form never leaked into the composer draft).
        wait_for_composer(&boot, &writer);
        type_keys(&writer, b"after form\r");
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            let prompts: Vec<Recorded> =
                state.paths("/prompt").into_iter().filter(|r| r.method == "POST").collect();
            if let Some(last) = prompts.last() {
                if last.body.contains("after form") {
                    assert!(
                        !last.body.contains("api"),
                        "keys typed during the form leaked into the composer: {}",
                        last.body
                    );
                    break;
                }
            }
            assert!(
                std::time::Instant::now() <= deadline,
                "the post-form prompt never arrived: {:?}",
                state.paths("/prompt")
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        // Cancel path: ask another form, esc dismisses it (DELETE).
        *state.next_form.lock().expect("next form") = Some(serde_json::json!({
            "id": "frm_deploy2",
            "sessionID": "ses_form",
            "title": "Again",
            "fields": [
                { "type": "string", "key": "service", "title": "Service", "required": true },
                { "type": "string", "key": "notes", "title": "Notes" }
            ]
        }));
        type_keys(&writer, b"again\r");
        assert!(wait_for(&boot, b"Again", Duration::from_secs(8)), "the second form mounted");
        type_keys(&writer, b"\x1b");
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            let cancels =
                state.paths("/form/").into_iter().filter(|r| r.method == "DELETE").count();
            if cancels >= 1 {
                break;
            }
            assert!(
                std::time::Instant::now() <= deadline,
                "the form cancel DELETE never arrived: {:?}",
                state.paths("/form/")
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        wait_for_composer(&boot, &writer);
        // Exit: escape clears any pending state, then ctrl+c.
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}

/// TD17: boolean/number/integer fields parse — the form mounts, the
/// kinds validate, and the reply carries typed JSON values.
#[test]
fn boolean_field_form_parsed() {
    let _sandbox = sandbox_env("td17");
    let (url, state, shutdown) = spawn_scripted_server();
    let state = Arc::clone(&state);
    *state.next_form.lock().expect("next form") = Some(serde_json::json!({
        "id": "frm_types",
        "sessionID": "ses_form",
        "title": "Typed fields",
        "fields": [
            { "type": "boolean", "key": "flag", "title": "Ship it" },
            { "type": "number", "key": "count", "title": "Count",
              "minimum": 1, "maximum": 10, "required": true },
            { "type": "integer", "key": "items", "title": "Items", "required": true }
        ]
    }));

    run_tui(&url, move |boot, writer| {
        wait_for_composer(&boot, &writer);
        type_keys(&writer, b"go\r");
        // RED today: the whole form is DROPPED (FieldKind lacks
        // boolean/number/integer ⇒ deserialize fails ⇒ footer never
        // mounts ⇒ the composer stays).
        assert!(
            wait_for(&boot, b"Typed fields", Duration::from_secs(8)),
            "TD17: the typed-field form parsed and mounted"
        );
        // Field 1 — boolean: option rows Yes/No; "1" picks Yes.
        assert!(
            wait_for(&boot, b"Yes", Duration::from_secs(4)),
            "the boolean field rendered Yes/No rows"
        );
        type_keys(&writer, b"1");
        // Field 2 — number (1..=10): "2.5" parses as a float.
        assert!(
            wait_for(&boot, b"Count", Duration::from_secs(4)),
            "picking Yes advanced to the number field"
        );
        type_keys(&writer, b"2.5\r");
        // Field 3 — integer: "7" parses as an integer.
        assert!(
            wait_for(&boot, b"Items", Duration::from_secs(4)),
            "the number commit advanced to the integer field"
        );
        type_keys(&writer, b"7\r");
        assert!(
            wait_for(&boot, b"Review", Duration::from_secs(4)),
            "the integer commit landed on the review step"
        );
        type_keys(&writer, b"\r");
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            let replies = state
                .paths("/form/")
                .into_iter()
                .filter(|r| r.method == "POST" && r.path.ends_with("/reply"))
                .collect::<Vec<_>>();
            if let Some(reply) = replies.first() {
                let body: serde_json::Value = serde_json::from_str(&reply.body).expect("body");
                assert_eq!(body["answer"]["flag"], true, "boolean → JSON true: {}", reply.body);
                // The generated client union carries numbers as f64 —
                // the wire's 7.0 IS the JSON number 7.
                assert_eq!(
                    body["answer"]["count"].as_f64(),
                    Some(2.5),
                    "number → JSON 2.5: {}",
                    reply.body
                );
                assert_eq!(
                    body["answer"]["items"].as_f64(),
                    Some(7.0),
                    "integer → JSON 7: {}",
                    reply.body
                );
                break;
            }
            assert!(
                std::time::Instant::now() <= deadline,
                "the typed reply POST never arrived: {:?}",
                state.paths("/form/")
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        type_keys(&writer, b"\x1b");
        type_keys(&writer, b"\x03");
        type_keys(&writer, b"\x03");
    });
    shutdown.store(true, Ordering::SeqCst);
}
