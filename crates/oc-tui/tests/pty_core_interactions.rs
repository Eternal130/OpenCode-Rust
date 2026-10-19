//! PTY live checks for the Wave A interaction-contract lane
//! (oc-tui-core): the dead keys the audit §1b flagged, now wired —
//!
//! * **escape-interrupt** — v2's 2-press machine
//!   (prompt/index.tsx:500-529): a streaming turn (the scripted
//!   FakeLlm-style server drips `message.updated` chunks with no
//!   finish) shows `esc interrupt`; the first escape arms
//!   (`esc again to interrupt`); the second POSTs
//!   `/api/session/{id}/interrupt`.
//! * **shift+tab agent cycle / f2 model cycle** — the footer hint
//!   flips visibly (`local.agent.move`, `local.model.cycle`).
//! * **scroll keys** — the six commands + first/last over a long
//!   transcript (sticky release + re-stick at bottom).
//! * **`<leader>return` queued prompts** — a second prompt mid-turn
//!   lands in the local serial queue (the dock row); only ONE prompt
//!   POST reaches the server.
//! * **undo/redo** — `ctrl+-` / `ctrl+.` restore the draft text.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// -- the pty harness (shared with the sibling PTY suites) --------------------

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

/// The post-merge CI regression patch (run 39317949352): the settle
/// cap bounds ONE `screen_after_toggle` capture while a starved
/// 2-core runner's TUI catches up on its input queue (see the
/// helper's doc for the race it closes).
const SETTLE_CAP: Duration = Duration::from_secs(8);

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
        let winsize = libc::winsize { ws_row: 24, ws_col: 80, ws_ypixel: 0, ws_xpixel: 0 };
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

fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-core-{tag}-{}-{}",
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

#[derive(Clone)]
struct Boot {
    output: Arc<Mutex<Vec<u8>>>,
}

fn wait_for_after(boot: &Boot, needle: &[u8], from: usize, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = boot.output.lock().expect("lock").clone();
        if data.len() > from && data[from..].windows(needle.len()).any(|w| w == needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn len(boot: &Boot) -> usize {
    boot.output.lock().expect("lock").len()
}

/// Force ratatui's FULL repaint (a resize invalidates the diff
/// buffer) and return the RECONSTRUCTED, SETTLED screen text — the
/// assertion surface for state that diff rendering would split
/// across escape sequences (the blinking caret cell, shifted
/// statuslines).
///
/// Post-merge CI regression (run 39317949352, deterministic on
/// 2-core runners under 6-way parallel PTY load): the previous fixed
/// 500 ms sleep + ANSI-stripped raw tail raced the starved TUI two
/// ways — (a) the SIGWINCH repaint is NOT input-ordered (a kernel
/// signal can overtake still-queued keystrokes), so the captured
/// tail could straddle states ("engine draft" beside "Ask
/// anything…" — the ctrl+c clear landed mid-window); (b) a starved
/// TUI streams its full repaint over seconds, and the capture cut it
/// mid-paint ("engine d" + "raft" split across partial rows). The
/// vt100 parser replays the stream in order, so its FINAL grid is
/// always the settled state regardless of interleaving: we feed the
/// post-toggle bytes incrementally and poll until two samples
/// ≥300 ms apart AGREE (the cursor cell masked — the caret blink is
/// the only idle animation), bounded by [`SETTLE_CAP`]. Deterministic
/// under any load; the returned text is the settled grid, so
/// `contains`/`!contains` asserts see one coherent frame instead of
/// a byte tail.
fn screen_after_toggle(
    writer: &std::fs::File,
    boot: &Boot,
    _mark: usize,
    rows: &mut u16,
) -> String {
    // Toggling the row count guarantees a real SIGWINCH every call
    // (a same-size TIOCSWINSZ emits no signal → no repaint).
    *rows = if *rows == 25 { 24 } else { 25 };
    unsafe {
        let winsize = libc::winsize { ws_row: *rows, ws_col: 80, ws_ypixel: 0, ws_xpixel: 0 };
        libc::ioctl(writer.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
    }
    std::thread::sleep(Duration::from_millis(500));
    // A fresh parser at the NEW size: the resize repaint is a FULL
    // paint, so no pre-toggle history is needed (pre-resize stragglers
    // land inside the old row range and are overwritten wholesale).
    let mut parser = vt100::Parser::new(*rows, 80, 0);
    let mut fed = 0usize;
    let mut last_screen = String::new();
    let mut last_sample = std::time::Instant::now();
    let deadline = std::time::Instant::now() + SETTLE_CAP;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let data = boot.output.lock().expect("lock").clone();
        parser.process(&data[fed.min(data.len())..]);
        fed = data.len();
        if last_sample.elapsed() >= Duration::from_millis(300) {
            let screen = masked_screen_text(&parser);
            if !screen.trim().is_empty() && screen == last_screen {
                return screen;
            }
            last_screen = screen;
            last_sample = std::time::Instant::now();
        }
        if std::time::Instant::now() > deadline {
            // A TUI that never settles: return the partial grid so the
            // assert fails with the real evidence.
            return masked_screen_text(&parser);
        }
    }
}

/// The parser's final grid as text with the cursor cell masked (the
/// caret's blink glyph would otherwise flip every other sample, so
/// stability could never be detected on an idle composer).
fn masked_screen_text(parser: &vt100::Parser) -> String {
    let screen = parser.screen();
    let (height, width) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let mut text = String::with_capacity(usize::from(height) * (usize::from(width) + 1));
    for row in 0..height {
        for col in 0..width {
            let ch = if (row, col) == (cursor_row, cursor_col) {
                ' '
            } else {
                screen
                    .cell(row, col)
                    .map_or(' ', |cell| cell.contents().chars().next().unwrap_or(' '))
            };
            text.push(ch);
        }
        text.push('\n');
    }
    text
}

/// Strip ANSI escape sequences from a captured byte string.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some(next) = chars.next() {
                match next {
                    '[' => {
                        for inner in chars.by_ref() {
                            if inner.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    }
                    ']' => {
                        for inner in chars.by_ref() {
                            if inner == '\u{7}' {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

// -- the scripted FakeLlm-style server --------------------------------------

struct Shared {
    shutdown: Arc<AtomicBool>,
    prompt_posts: Arc<AtomicUsize>,
    interrupt_posts: Arc<AtomicUsize>,
    /// session.switchModel POSTs (the submit's commitModel step).
    model_posts: Arc<AtomicUsize>,
    /// SURF-2: when > 0, the next that-many /api/event streams are
    /// closed right after the headers (a scripted server loss — the
    /// Reconnecting overlay's trigger).
    sse_drops: Arc<AtomicUsize>,
    /// The SSE broadcast channel: whatever lands here is written on
    /// EVERY open /api/event stream (the scripted turn).
    broadcast: Arc<Mutex<std::collections::VecDeque<String>>>,
    /// Long-transcript mode: /`api/session/ses_live/message` serves a
    /// 20-turn fixture.
    long_transcript: bool,
}

/// One SSE frame (the wire's JSON-in-string double encoding).
fn sse_frame(id: &str, type_: &str, data_json: &str) -> String {
    let event = format!("{{\"id\":\"{id}\",\"type\":\"{type_}\",\"data\":{data_json}}}");
    let escaped = event.replace('\\', "\\\\").replace('"', "\\\"");
    format!("data: \"{escaped}\"\n\n")
}

fn model_list_body() -> String {
    let model = |id: &str, name: &str| {
        format!(
            concat!(
                r#"{{"id":"{id}","modelID":"{id}","name":"{name}","providerID":"pfix","#,
                r#""status":"active","enabled":true,"cost":[],"limit":{{"context":1000,"output":1000}},"#,
                r#""capabilities":{{"input":["text"],"output":["text"],"tools":false}},"#,
                r#""time":{{"released":1.0}},"variants":[]}}"#
            ),
            id = id,
            name = name,
        )
    };
    format!(
        r#"{{"cursor":{{}},"data":[{},{}],"location":{{"directory":"/tmp"}}}}"#,
        model("m-alpha", "Model Alpha"),
        model("m-beta", "Model Beta")
    )
}

fn agent_list_body() -> String {
    // The full Agent.Info wire shape: `permissions` + `request` are
    // required (serde-wise) even for a bare list fixture.
    let agent = |id: &str, name: &str| {
        format!(
            concat!(
                r#"{{"id":"{id}","name":"{name}","mode":"all","hidden":false,"#,
                r#""permissions":[],"#,
                r#""request":{{"body":{{}},"headers":{{}},"settings":{{}}}}}}"#
            ),
            id = id,
            name = name,
        )
    };
    format!(
        r#"{{"cursor":{{}},"data":[{},{}],"location":{{"directory":"/tmp"}}}}"#,
        agent("build", "Build"),
        agent("plan", "Plan")
    )
}

fn session_list_body() -> String {
    concat!(
        r#"{"cursor":{},"data":["#,
        r#"{"id":"ses_live","title":"Live turn","time":{"created":0,"updated":0},"#,
        r#""location":{"directory":"/tmp"},"projectID":"p","cost":0,"#,
        r#""tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}"#,
        r#"]}"#
    )
    .to_owned()
}

/// The long message fixture: 20 user/assistant turns (~90 transcript
/// rows ≫ the 24-row viewport). Wire order is DESC (newest first —
/// the server default; the client reverses into the chronological
/// bucket, v2 data.ts:1633-1637).
fn long_message_list_body() -> String {
    let mut messages = Vec::new();
    for index in (0..20).rev() {
        messages.push(format!(
            r#"{{"id":"u{index}","type":"user","text":"turn {index} of the long fixture","time":{{"created":{index}}}}}"#
        ));
        // The newest assistant (a19 — the scroll test pins the
        // sticky tail at "answer 19") carries tokens — the composer
        // footer's usage arm (`18 (1%)` at the catalog's 1000-token
        // context limit) for the cycle test's stats-row step.
        let tokens = if index == 19 {
            r#","tokens":{"cache":{"read":0,"write":0},"input":10,"output":8,"read":0,"write":0,"reasoning":0}"#
        } else {
            ""
        };
        messages.push(format!(
            concat!(
                r#"{{"id":"a{index}","type":"assistant","agent":"build","#,
                r#""model":{{"providerID":"pfix","id":"m-alpha"}},"finish":"stop","#,
                r#""time":{{"created":{index},"completed":{index}}},"#,
                r#""content":[{{"type":"text","text":"answer {index}"}}]{tokens}}}"#
            ),
            index = index,
            tokens = tokens,
        ));
    }
    format!(r#"{{"cursor":{{}},"data":[{}]}}"#, messages.join(","))
}

fn spawn_scripted_server(long_transcript: bool) -> (String, Arc<Shared>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let broadcast = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    if long_transcript {
        // The long fixture rides the event stream (the boot hydration
        // only fetches messages for tabs open at FIRST connect; this
        // tab opens later through the session list).
        for index in 0..20u32 {
            broadcast.lock().expect("seed").push_back(sse_frame(
                &format!("evt_seed_u{index}"),
                "message.updated",
                &format!(
                    r#"{{"sessionID":"ses_live","id":"u{index}","type":"user","text":"turn {index} of the long fixture","time":{{"created":{index}}}}}"#
                ),
            ));
            broadcast.lock().expect("seed").push_back(sse_frame(
                &format!("evt_seed_a{index}"),
                "message.updated",
                &format!(
                    r#"{{"sessionID":"ses_live","id":"a{index}","type":"assistant","agent":"build","model":{{"providerID":"pfix","id":"m-alpha"}},"finish":"stop","time":{{"created":{index},"completed":{index}}},"content":[{{"type":"text","text":"answer {index}"}}]}}"#
                ),
            ));
        }
    }
    let shared = Arc::new(Shared {
        shutdown: Arc::new(AtomicBool::new(false)),
        prompt_posts: Arc::new(AtomicUsize::new(0)),
        interrupt_posts: Arc::new(AtomicUsize::new(0)),
        model_posts: Arc::new(AtomicUsize::new(0)),
        sse_drops: Arc::new(AtomicUsize::new(0)),
        broadcast,
        long_transcript,
    });
    let shutdown = Arc::clone(&shared.shutdown);
    let accept_shared = Arc::clone(&shared);
    std::thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking accept");
        while !shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let shared = Arc::clone(&accept_shared);
                    std::thread::spawn(move || handle(socket, &shared));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}"), shared)
}

#[allow(clippy::too_many_lines)] // one arm per scripted endpoint
fn handle(mut socket: std::net::TcpStream, shared: &Shared) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = [0u8; 16384];
    let mut head = String::new();
    loop {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                head.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if head.contains("\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let path = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
    // Strip the query string before matching (`?resume=true`).
    let path = path.split('?').next().unwrap_or_default().to_owned();
    let is_post = head.starts_with("POST");
    if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
        eprintln!("[test-server] {} {}", if is_post { "POST" } else { "GET" }, path);
    }
    socket.set_read_timeout(Some(Duration::from_millis(50))).ok();

    if path.starts_with("/api/event") {
        let response =
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n";
        if socket.write_all(response).is_err() {
            return;
        }
        // SURF-2: the scripted server loss — write the headers, then
        // close (the client's stream ends; the reconnect loop kicks
        // in with its backoff).
        if shared.sse_drops.load(Ordering::SeqCst) > 0 {
            shared.sse_drops.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        // Broadcast loop: write whatever the scripted turn queued, then
        // idle-poll for more (read timeouts are the poll tick — the
        // client's pings drain here; a closed socket ends the loop).
        loop {
            while let Some(frame) = shared.broadcast.lock().expect("broadcast").pop_front() {
                if socket.write_all(frame.as_bytes()).is_err() {
                    return;
                }
            }
            let mut sink = [0u8; 256];
            match socket.read(&mut sink) {
                Ok(0) => return,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    }

    let json = |body: String| {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        )
    };

    if is_post && path == "/api/session" {
        // The home submission's optimistic create (the wire wraps the
        // SessionInfo in a `data` envelope).
        let session = r#"{"id":"ses_live","title":"Live turn","time":{"created":0,"updated":0},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}"#;
        let _ = socket.write_all(json(format!(r#"{{"data":{session}}}"#)).as_bytes());
        return;
    }
    if is_post && path.ends_with("/model") {
        // session.switchModel — 204 No Content (the wire success
        // shape; the submit's commitModel step POSTs here before the
        // prompt).
        shared.model_posts.fetch_add(1, Ordering::SeqCst);
        let _ = socket.write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n");
        return;
    }
    if is_post && path.ends_with("/agent") {
        // session.switchAgent — 204 No Content (the wire success
        // shape; a 200-with-body is a client error and its log-skip
        // surfaces an untitled error toast).
        let _ = socket.write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n");
        return;
    }
    if is_post && path.ends_with("/interrupt") {
        shared.interrupt_posts.fetch_add(1, Ordering::SeqCst);
        let _ = socket.write_all(json(r#"{"data":{"interrupted":true}}"#.to_owned()).as_bytes());
        return;
    }
    if is_post && path.ends_with("/prompt") {
        let count = shared.prompt_posts.fetch_add(1, Ordering::SeqCst) + 1;
        // The scripted turn: a user echo, then a STREAMING assistant
        // (no finish, no session.idle — running forever, dripped in
        // three chunks).
        let broadcast = |type_: &str, data: &str| {
            let index = shared.prompt_posts.load(Ordering::SeqCst);
            let frame = sse_frame(&format!("evt_{type_}_{index}"), type_, data);
            shared.broadcast.lock().expect("broadcast").push_back(frame);
        };
        broadcast(
            "message.updated",
            r#"{"sessionID":"ses_live","id":"m_user","type":"user","text":"please stream","time":{"created":1}}"#,
        );
        broadcast(
            "message.updated",
            r#"{"sessionID":"ses_live","id":"m_assistant","type":"assistant","agent":"build","model":{"providerID":"pfix","id":"m-alpha"},"content":[{"type":"text","text":"streaming"}],"time":{"created":2}}"#,
        );
        let _ = count;
        // The wire's `Session.Inbox.User` echo.
        // The wire's `Session.Inbox.User` echo — one inbox id per
        // POST (the cycle test's second submission must paint as a
        // NEW admission, not dedup against the first).
        let _ = socket.write_all(json(format!(
            r#"{{"data":{{"id":"inbox_{count}","delivery":"steer","type":"user","sessionID":"ses_live","time":{{"created":3}},"payload":{{"text":"please stream"}}}}}}"#
        )).as_bytes());
        return;
    }

    let body = if path.starts_with("/api/model") {
        model_list_body()
    } else if path.starts_with("/api/agent") {
        agent_list_body()
    } else if path.starts_with("/api/provider") {
        r#"{"cursor":{},"data":[],"location":{"directory":"/tmp"}}"#.to_owned()
    } else if shared.long_transcript && path.starts_with("/api/session/ses_live/message") {
        long_message_list_body()
    } else if path.starts_with("/api/session/ses_live/message") {
        // Lane M4: the default session carries the THOUGHT-CYCLE turn
        // (a titled reasoning part — the disclosure header + the
        // expandable `┃`-guttered body — plus a settled text reply),
        // served through the OPEN-time fetch (the reliable seam; the
        // SSE push is the documented-flaky one) in the wire's DESC
        // order — newest first, like the long fixture (the client
        // reverses the page into chronological order). The
        // interrupt/queue tests submit their own streaming turn on
        // top — extra transcript rows don't disturb their
        // composer-surface asserts.
        // (The turn's ids are DISTINCT from the live-streamed pair —
        // the snapshot merge drops local rows whose ids it already
        // carries, and the streaming turn's m_user/m_assistant must
        // survive it to keep the session RUNNING.)
        r#"{"cursor":{},"data":[{"id":"th_assistant","type":"assistant","agent":"build","model":{"providerID":"pfix","id":"m-alpha"},"finish":"stop","time":{"created":2,"completed":6},"tokens":{"cache":{"read":0,"write":0},"input":4,"output":9,"read":0,"write":0,"reasoning":3},"content":[{"type":"reasoning","text":"**Planning**\n\nline one of the plan\nline two of the plan"},{"type":"text","text":"the settled thought reply"}]},{"id":"th_user","type":"user","text":"expand the thought please","time":{"created":1}}]}"#.to_owned()
    } else if path.starts_with("/api/session") {
        session_list_body()
    } else {
        "{}".to_owned()
    };
    let _ = socket.write_all(json(body).as_bytes());
}

/// Seed the model-preference ring (`<home>/.local/share/opencode-rust/
/// tui/model.json`) so f2 has two recent entries to cycle.
fn seed_model_ring(root: &std::path::Path) {
    let dir = root.join(".local/share/opencode-rust/tui");
    std::fs::create_dir_all(&dir).expect("ring dir");
    std::fs::write(
        dir.join("model.json"),
        r#"{"recent":[{"providerID":"pfix","modelID":"m-alpha"},{"providerID":"pfix","modelID":"m-beta"}],"favorite":[],"variant":{}}"#,
    )
    .expect("seed ring");
}

/// Boot the app on the pty against `url`; `keys` drives the input.
fn drive(url: &str, keys: impl FnOnce(Boot, std::fs::File) + Send + 'static) {
    let guard = attach_pty();
    let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let reader_master = guard.master.try_clone().expect("clone read");
    {
        let output = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut reader = reader_master;
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        output.lock().expect("lock").extend_from_slice(&chunk[..count]);
                    }
                }
            }
        });
    }
    let writer = guard.master.try_clone().expect("clone keys");
    {
        let keys_output = Arc::clone(&output);
        std::thread::spawn(move || keys(Boot { output: keys_output }, writer));
    }

    let client = oc_client::Client::new(url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let finished = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(25),
            oc_tui::app::run_with_size(client, Some((80, 24)), oc_tui::app::BootArgs::default()),
        )
        .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => {
            let captured =
                String::from_utf8_lossy(&output.lock().expect("lock").clone()).into_owned();
            let tag = std::env::var("PTY_DUMP_TAG").unwrap_or_else(|_| "core".to_owned());
            std::fs::write(format!("/tmp/opencode/pty-core-debug-{tag}.txt"), &captured)
                .expect("dump");
            panic!("the TUI did not exit in time: {elapsed} (dumped /tmp/opencode/pty-core-debug-{{tag}}.txt)");
        }
    }
    drop(guard);
}

/// The streaming-turn harness: boot home, submit a prompt (creating
/// `ses_live`), wait for the running-turn footer.
fn start_streaming_turn(boot: &Boot, writer: &mut std::fs::File) {
    let probe = || Boot { output: Arc::clone(&boot.output) };
    assert!(
        wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
        "the home wordmark rendered"
    );
    std::thread::sleep(Duration::from_millis(700));
    for key in "stream please".bytes() {
        let _ = writer.write_all(&[key]);
        std::thread::sleep(Duration::from_millis(15));
    }
    let _ = writer.write_all(b"\r");
    // The submission creates the session, POSTs the prompt, and the
    // SSE stream projects the streaming assistant (running).
    // The hint paints as two colored spans (`esc ` + label), so the
    // needle stays inside the label's span.
    assert!(
        wait_for_after(&probe(), b"interrupt", len(&probe()), Duration::from_secs(8)),
        "the running turn shows the interrupt hint"
    );
}

/// PTY: the 2-press escape interrupt — armed hint flip + the
/// interrupt POST. The composer renders through its opentui-solid
/// tree (the only renderer since the retirement lane).
#[test]
fn pty_escape_interrupt_fires_on_a_streaming_turn() {
    pty_escape_interrupt_body();
}

fn pty_escape_interrupt_body() {
    let _sandbox = sandbox_env("interrupt");
    let (url, shared) = spawn_scripted_server(false);
    let in_test = Arc::clone(&shared);
    drive(&url, move |boot, mut writer| {
        start_streaming_turn(&boot, &mut writer);
        let shared = Arc::clone(&in_test);
        let probe = || Boot { output: Arc::clone(&boot.output) };
        // First escape: armed.
        let _ = writer.write_all(b"\x1b");
        assert!(
            wait_for_after(&probe(), b"again to interrupt", len(&probe()), Duration::from_secs(5)),
            "the armed hint"
        );
        // Second escape: the interrupt POST.
        let _ = writer.write_all(b"\x1b");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while shared.interrupt_posts.load(Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline, "the interrupt POST never arrived");
            std::thread::sleep(Duration::from_millis(25));
        }
        // Exit (ctrl+c on the now-empty prompt).
        let _ = writer.write_all(b"\x03");
    });
    assert_eq!(shared.interrupt_posts.load(Ordering::SeqCst), 1, "exactly one interrupt");
}

/// PTY: `<leader>return` queues behind the streaming turn — the dock
/// row appears and the queue submit POSTs IMMEDIATELY with the server
/// inbox semantics (TUI-TD07: `session.prompt {delivery: "queue"}` —
/// the queue is the SERVER's pending inbox; the local serial drain
/// that re-POSTed on idle is retired).
#[test]
fn pty_leader_return_queues_behind_the_turn() {
    pty_leader_return_queue_body();
}

fn pty_leader_return_queue_body() {
    let _sandbox = sandbox_env("queue");
    let (url, shared) = spawn_scripted_server(false);
    let in_test = Arc::clone(&shared);
    drive(&url, move |boot, mut writer| {
        start_streaming_turn(&boot, &mut writer);
        let _shared = Arc::clone(&in_test);
        let probe = || Boot { output: Arc::clone(&boot.output) };
        // A second prompt mid-turn: <leader>return queues it.
        for key in "second thought".bytes() {
            let _ = writer.write_all(&[key]);
            std::thread::sleep(Duration::from_millis(15));
        }
        let _ = writer.write_all(b"\x18\r");
        assert!(
            wait_for_after(&probe(), b"1 queued", len(&probe()), Duration::from_secs(5)),
            "the queued-prompt dock row"
        );
        std::thread::sleep(Duration::from_millis(600));
        // Exit.
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
    assert_eq!(
        shared.prompt_posts.load(Ordering::SeqCst),
        2,
        "the streaming turn + the queue submit both POSTed (TUI-TD07: delivery \"queue\", no local drain re-POST)"
    );
}

/// PTY: shift+tab cycles the agents and f2 cycles the recent models —
/// the footer hint flips visibly.
#[test]
fn pty_shift_tab_and_f2_cycle_the_footer_hints() {
    let sandbox = sandbox_env("cycles");
    seed_model_ring(&sandbox);
    let (url, _shared) = spawn_scripted_server(false);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        std::thread::sleep(Duration::from_millis(700));
        // Open the session route (the list → enter).
        let _ = writer.write_all(b"\x18l");
        assert!(
            wait_for_after(&probe(), b"Live turn", len(&probe()), Duration::from_secs(5)),
            "the session list"
        );
        let _ = writer.write_all(b"\r");
        std::thread::sleep(Duration::from_millis(800));
        // shift+tab (kitty CSI-u: crossterm maps the legacy BackTab
        // sequence to the ctrl+shift+tab spec — the tab.previous
        // chord — so the CSI-u form carries the shift+tab binding).
        let before = len(&probe());
        let _ = writer.write_all(b"\x1b[9;2u");
        assert!(
            wait_for_after(&probe(), b"Plan", before, Duration::from_secs(5)),
            "shift+tab cycled the agent hint"
        );
        // f2 cycles the model ring. F8 changed the pre-state: the hint
        // already shows the RESOLVED catalog head (m-alpha) from the
        // moment the session opens (`currentSelection` resolves — no
        // more empty "unset" statusline), so the FIRST press commits
        // the ring head with identical text; the assertion is the
        // full-repaint frame (still m-alpha), then the second press
        // steps to m-beta. ONE toggle counter threads both repaints
        // (a repeated size emits no SIGWINCH).
        let mut rows: u16 = 24;
        let _ = writer.write_all(b"\x1b[12~");
        std::thread::sleep(Duration::from_millis(300));
        let head_frame =
            strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            head_frame.contains("Model Alpha"),
            "the resolved catalog head shows and survives the first f2: {head_frame:?}"
        );
        let before = len(&probe());
        let _ = writer.write_all(b"\x1b[12~");
        // A resize forces ratatui's FULL repaint (diff draws emit only
        // changed cells — a shifted statusline never yields a
        // contiguous `m-beta` run in the byte stream).
        let stepped = strip_ansi(&screen_after_toggle(&writer, &probe(), before, &mut rows));
        assert!(stepped.contains("Model Beta"), "the second f2 stepped the ring: {stepped:?}");
        // Exit.
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
}

/// PTY: the scroll keys move the long transcript — sticky starts at
/// the bottom, pageup releases (the head), end re-sticks (the tail).
/// The transcript renders through its opentui-solid tree (the only
/// renderer since the retirement lane).
#[test]
fn pty_scroll_keys_move_the_transcript() {
    pty_scroll_keys_body();
}

fn pty_scroll_keys_body() {
    let _sandbox = sandbox_env("scroll");
    let (url, _shared) = spawn_scripted_server(true);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        std::thread::sleep(Duration::from_millis(700));
        let _ = writer.write_all(b"\x18l");
        assert!(
            wait_for_after(&probe(), b"Live turn", len(&probe()), Duration::from_secs(5)),
            "the session list"
        );
        let _ = writer.write_all(b"\r");
        // The long fixture loads (sticky: the LAST turn shows).
        assert!(
            wait_for_after(&probe(), b"answer 19", len(&probe()), Duration::from_secs(8)),
            "sticky at the bottom (the tail shows)"
        );
        // pageup ×N: the head surfaces (a page is HALF the
        // viewport; ~40 presses clear the ~180-row fixture).
        for _ in 0..40 {
            let _ = writer.write_all(b"\x1b[5~");
            std::thread::sleep(Duration::from_millis(60));
        }
        let mut rows: u16 = 24;
        let head = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(head.contains("turn 0 of the long fixture"), "page-ups reached the head: {head:?}");
        // end: back to the tail.
        let _ = writer.write_all(b"\x1b[F");
        let tail = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(tail.contains("answer 19"), "end re-stuck to the tail: {tail:?}");
        // Exit.
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
}

/// PTY: undo/redo restores the draft text (`ctrl+-` / `ctrl+.`,
/// keybind.ts:233-234).
#[test]
fn pty_undo_redo_restores_the_draft() {
    let _sandbox = sandbox_env("undo");
    let (url, _shared) = spawn_scripted_server(false);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        std::thread::sleep(Duration::from_millis(700));
        for key in "draft words".bytes() {
            let _ = writer.write_all(&[key]);
            std::thread::sleep(Duration::from_millis(15));
        }
        let mut rows: u16 = 24;
        let typed = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(typed.contains("draft words"), "the draft typed: {typed:?}");
        // ctrl+w deletes the trailing word.
        let _ = writer.write_all(b"\x17");
        let deleted = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            deleted.contains("draft") && !deleted.contains("draft words"),
            "ctrl+w deleted the word: {deleted:?}"
        );
        // ctrl+- (undo; kitty CSI-u — the legacy 0x1f byte maps to
        // crossterm's ctrl+7) restores it.
        let _ = writer.write_all(b"\x1b[45;5u");
        let undone = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(undone.contains("draft words"), "undo restored the word: {undone:?}");
        // ctrl+. (redo; CSI-u 46;5) re-deletes it.
        let _ = writer.write_all(b"\x1b[46;5u");
        let redone = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            redone.contains("draft") && !redone.contains("draft words"),
            "redo re-deleted the word: {redone:?}"
        );
        // Exit: clear (ctrl+c), exit (ctrl+c).
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
}

/// PTY (lane M3, the solid composer path): the composer cycle —
/// type → edit (backspace + retype) → submit → the reply-side
/// admission → the stats row (the footer's usage arm `18 (1%)` once
/// the long fixture's completed turns hydrate the session). The
/// queue arm of the cycle is covered by the parametrized
/// [`pty_leader_return_queue_body`] pair (both renderer paths); this
/// stub's second submission is not observable (the fixture fetch
/// replaces the inbox echo within a frame — verified identical on
/// BOTH composer paths, a harness limitation, not a path delta).
#[test]
fn pty_composer_cycle_type_edit_submit_reply_stats_solid_path() {
    let _sandbox = sandbox_env("cycle");
    let (url, _shared) = spawn_scripted_server(true);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        std::thread::sleep(Duration::from_millis(700));
        // TYPE.
        for key in b"composer cycl" {
            let _ = writer.write_all(&[*key]);
            std::thread::sleep(Duration::from_millis(15));
        }
        // EDIT: drop the dangling "l", retype "e" (the caret seam).
        let _ = writer.write_all(b"\x7f");
        let _ = writer.write_all(b"e");
        std::thread::sleep(Duration::from_millis(300));
        // SUBMIT: creates ses_live, admits the prompt, navigates.
        let after_submit = len(&probe());
        let _ = writer.write_all(b"\r");
        // REPLY: the stub's reply-side admission — the fixed user
        // echo renders and the turn runs (the interrupt arm). ONE
        // shared mark: both paints can land in the same output
        // burst, so the second assert must not re-mark after the
        // first.
        assert!(
            wait_for_after(&probe(), b"please stream", after_submit, Duration::from_secs(8)),
            "the reply-side admission rendered"
        );
        assert!(
            wait_for_after(&probe(), b"interrupt", after_submit, Duration::from_secs(8)),
            "the running footer arm rendered"
        );
        // STATS ROW: the long fixture's messages hydrate the session
        // (the newest assistant carries tokens) — the footer's usage
        // arm lights while no turn runs.
        assert!(
            wait_for_after(&probe(), b"18 (1%)", len(&probe()), Duration::from_secs(8)),
            "the usage arm rendered on the completed turn"
        );
        // Exit.
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
}

/// PTY (lane M4, the solid transcript path): the transcript's own
/// interaction cycle — open the session carrying the thought turn
/// (user box, the `+ Thought: Planning` disclosure header, the text
/// reply, the stats row) → an SGR CLICK on the header expands the
/// thought (the `┃`-guttered body lines) → a second click collapses
/// — the hit-row bridge (E1 #29/#34) driving the store's disclosure
/// through the solid path's geometry. The turn serves through the
/// session-OPEN fetch (the reliable seam — the SSE push is the
/// documented-flaky one). The header sits at screen row 6 (strip row
/// 0, the user box rows 2–4, the marginTop gap row 5), its hit rect
/// from x=5 — the click lands at col 7.
#[test]
fn pty_transcript_cycle_thought_expand_solid_path() {
    let _sandbox = sandbox_env("thought-cycle");
    let (url, _shared) = spawn_scripted_server(false);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        std::thread::sleep(Duration::from_millis(700));
        // OPEN the session (leader+l → the list → enter): the boot
        // hydration fetch paints the thought turn.
        let _ = writer.write_all(b"\x18l");
        assert!(
            wait_for_after(&probe(), b"Live turn", len(&probe()), Duration::from_secs(5)),
            "the session list"
        );
        let _ = writer.write_all(b"\r");
        // The turn's surfaces — ONE shared mark: the hydration paints
        // the whole turn in one output burst (M3's stub-harness trap:
        // a re-marked assert misses content that landed in the same
        // burst). The header assert stays INSIDE one styled run
        // ("Thought: …"; the `+ `/`- ` icon is a separate tint — the
        // M2 diff-serializer gotcha).
        let opened = len(&probe());
        assert!(
            wait_for_after(&probe(), b"Thought: Planning", opened, Duration::from_secs(8)),
            "the collapsed disclosure header rendered"
        );
        assert!(
            wait_for_after(&probe(), b"the settled thought reply", opened, Duration::from_secs(8)),
            "the assistant text part rendered"
        );
        assert!(
            !wait_for_after(&probe(), b"line one of the plan", opened, Duration::from_millis(300)),
            "the thought body stays collapsed before the click"
        );
        // CLICK the disclosure header (press + release at the row's
        // hit rect — the release drives `ToggleGroup`).
        let _ = writer.write_all(b"\x1b[<0;8;7M");
        let _ = writer.write_all(b"\x1b[<0;8;7m");
        // EXPANDED: the guttered body lines mount. The diff writer
        // splits a row's styled runs (and skips unchanged gap cells),
        // so the state asserts read a FORCED FULL REPAINT (the SIGWINCH
        // toggle) instead of the byte stream.
        std::thread::sleep(Duration::from_millis(400));
        let mut rows: u16 = 24;
        let expanded =
            strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            expanded.contains("line one of the plan"),
            "the guttered thought body rendered: {expanded:?}"
        );
        assert!(
            expanded.contains("Thought: Planning"),
            "the header survived the expansion: {expanded:?}"
        );
        // A second click collapses again (the toggle is a cycle).
        let _ = writer.write_all(b"\x1b[<0;8;7M");
        let _ = writer.write_all(b"\x1b[<0;8;7m");
        std::thread::sleep(Duration::from_millis(400));
        let collapsed =
            strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            collapsed.contains("Thought: Planning"),
            "the header survived the second click: {collapsed:?}"
        );
        assert!(
            !collapsed.contains("line one of the plan"),
            "the second click collapsed the group: {collapsed:?}"
        );
        // Exit.
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
}

/// PTY (lane M-engine): the editing-engine + keymap cycle — type →
/// word-delete → undo/redo → leader arm + escape-clear (the K-6
/// semantic, identical on both engines) → prompt.clear. Parametrized
/// over the ENGINE paths (the editing core + the keymap dispatch
/// beneath the composer): the legacy hand-rolled pair and the
/// opentui-core/opentui-keymap pair must drive the SAME observable
/// cycle. (The K-delta behaviors that DIFFER by engine —
/// backspace-pop on a pending sequence, non-leader sequence arming —
/// are pinned by the oc-tui-core suites, never here.)
#[test]
fn pty_engine_cycle_type_worddelete_undo_leader_escape_legacy_path() {
    std::env::set_var("OC_ENGINE_OPENTUI", "0");
    pty_engine_cycle_body();
}

/// The opentui engine path (`OC_ENGINE_OPENTUI=1`).
#[test]
fn pty_engine_cycle_type_worddelete_undo_leader_escape_opentui_path() {
    std::env::set_var("OC_ENGINE_OPENTUI", "1");
    pty_engine_cycle_body();
}

fn pty_engine_cycle_body() {
    let _sandbox = sandbox_env("engine-cycle");
    let (url, _shared) = spawn_scripted_server(false);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        std::thread::sleep(Duration::from_millis(700));
        for key in "engine draft".bytes() {
            let _ = writer.write_all(&[key]);
            std::thread::sleep(Duration::from_millis(15));
        }
        let mut rows: u16 = 24;
        let typed = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(typed.contains("engine draft"), "the draft typed: {typed:?}");
        // ctrl+w deletes the trailing word.
        let _ = writer.write_all(b"\x17");
        let deleted = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            deleted.contains("engine") && !deleted.contains("engine draft"),
            "ctrl+w deleted the word: {deleted:?}"
        );
        // undo restores it.
        let _ = writer.write_all(b"\x1b[45;5u");
        let undone = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(undone.contains("engine draft"), "undo restored the word: {undone:?}");
        // The leader arms and escape CLEARS it without interrupting
        // (no dialog, no navigation, the draft intact).
        let _ = writer.write_all(b"\x18");
        std::thread::sleep(Duration::from_millis(150));
        let _ = writer.write_all(b"\x1b");
        std::thread::sleep(Duration::from_millis(250));
        let cleared = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(
            cleared.contains("engine draft"),
            "escape cleared the pending sequence, the draft intact: {cleared:?}"
        );
        // prompt.clear (ctrl+c on a NON-empty draft) — then exit.
        let _ = writer.write_all(b"\x03");
        let empty = strip_ansi(&screen_after_toggle(&writer, &probe(), len(&probe()), &mut rows));
        assert!(!empty.contains("engine draft"), "ctrl+c cleared the draft: {empty:?}");
        let _ = writer.write_all(b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(b"\x03");
    });
}

/// SURF-2 (auditB #2): the Reconnecting overlay — a scripted server
/// loss (three dropped SSE streams) paints "Connection lost…" over the
/// dimmed frame after the 1 s retry threshold, and clears when the
/// connection recovers (app.tsx:1294-1315 + reconnecting.tsx).
#[test]
fn pty_reconnecting_overlay_on_loss_and_recovery() {
    let sandbox = sandbox_env("reconnect");
    let _ = sandbox;
    let (url, shared) = spawn_scripted_server(false);
    // Drop the first THREE event streams: attempts at +0.5 s, +1.5 s,
    // +3.5 s fail; the 4th (≈ +7.5 s) recovers — the overlay anchors
    // at the last transition (+3.5 s) and shows at +4.5 s.
    shared.sse_drops.store(3, Ordering::SeqCst);
    drive(&url, move |boot, mut writer| {
        let probe = || Boot { output: Arc::clone(&boot.output) };
        assert!(
            wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            "the home wordmark rendered"
        );
        assert!(
            wait_for_after(&probe(), b"Connection lost", 0, Duration::from_secs(20),),
            "the Reconnecting overlay appeared after the retry threshold"
        );
        assert!(
            wait_for_after(
                &probe(),
                b"Reconnecting to the server automatically.",
                0,
                Duration::from_secs(20)
            ),
            "the managed-copy line (unmanaged wording)"
        );
        // Recovery: the overlay's spinner repaints stop — the count of
        // headline occurrences freezes once the stream reattaches.
        let count = |needle: &[u8]| -> usize {
            let data = boot.output.lock().expect("lock").clone();
            data.windows(needle.len()).filter(|window| *window == needle).count()
        };
        let mut stable = 0usize;
        let mut last = count(b"Connection lost");
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(500));
            let now = count(b"Connection lost");
            if now == last {
                stable += 1;
                if stable >= 6 {
                    break;
                }
            } else {
                stable = 0;
                last = now;
            }
        }
        assert!(
            stable >= 6,
            "the overlay stopped repainting (recovered) — last {last}, stable {stable}"
        );
        // The recovered shell still works: exit cleanly.
        let _ = writer.write_all(b"\x03");
    });
}
