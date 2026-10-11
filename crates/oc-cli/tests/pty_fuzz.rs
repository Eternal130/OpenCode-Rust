//! PTY invariant fuzzer: a seeded, HUMAN-PLAUSIBLE key driver against
//! the REAL `opencode-rust` TUI binary over a pty (the `mini_pty.rs`
//! spawn pattern: setsid + TIOCSCTTY so the slave is /dev/tty), a
//! scripted FakeLlm-style stub server (the `pty_interactions.rs`
//! session shapes + the `mini_pty.rs` SSE turn script), and a tiny
//! VT100 screen analyzer that reconstructs the 24×80 grid from the
//! captured output stream.
//!
//! The driver does NOT look for any specific bug. After EVERY key it
//! checks cheap INVARIANTS any healthy TUI screen must satisfy:
//!
//! a. **alive** — the process is still running, or exited 0 through an
//!    intended exit chord (ctrl+c / ctrl+d / <leader>q among the last
//!    actions) with no `panicked` in the output;
//! b. **no escape-response garbage** — the harness ANSWERS the boot
//!    queries a real terminal answers (OSC 11 background color →
//!    `11;rgb:…`, CPR `\x1b[6n` → `\x1b[1;1R`); the answer must never
//!    surface as drawn text (`rgb:` on the visible screen / in the
//!    output stream);
//! c. **non-void screen** — after the first 30 keys (boot settled),
//!    the visible screen carries the composer rail `┃` or ≥3 distinct
//!    non-blank rows (the beta.7 blank-void class);
//! d. **recoverability** — at 5 evenly-spaced checkpoints: escape ×3
//!    must, within 2s, land on a known stable shape (home wordmark
//!    `█▀▀█` or composer rail `┃`) or leave a composer that still
//!    responds (a typed char changes the capture);
//! e. **bounded corruption** — a non-navigation key (typing a word /
//!    backspace / space, outside a slash-autocomplete window) must not
//!    flip >60% of the screen cells between consecutive captures.
//!
//! Invariant failure ⇒ a full dump: the key sequence so far (the
//! reproducer), the reconstructed screen, and the raw output tail —
//! written to `/tmp/opencode/pty-fuzz-fail-s<seed>.txt`.
//!
//! Entrypoints:
//! * `pty_fuzz_smoke` — 3 fixed seeds × 120 keys (CI, the serialized
//!   pty job);
//! * `pty_fuzz_long` (`#[ignore]`) — 20 seeds × 250 keys; run
//!   nightly-style locally:
//!
//! ```text
//! cargo nextest run -p oc-cli --run-ignored only -E 'test(pty_fuzz_long)'
//! # custom sweep: seeds + key count ride env vars
//! OC_PTY_FUZZ_SEEDS=41,42,43 OC_PTY_FUZZ_KEYS=400 \
//!   cargo nextest run -p oc-cli --run-ignored only -E 'test(pty_fuzz_long)'
//! ```

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const ROWS: usize = 24;
const COLS: usize = 80;
/// After this many keys the boot has settled and the void-screen
/// invariant switches on.
const BOOT_SETTLE_KEYS: usize = 30;
/// Max fresh child processes per seed when an intended exit chord ends
/// one mid-run (the remaining keys continue in the new child).
const MAX_RESPAWNS: usize = 2;

// -- the stub server (FakeLlm-style) -----------------------------------------

struct Stub {
    port: u16,
    shutdown: Arc<AtomicBool>,
    prompts: Arc<AtomicUsize>,
}

/// Boot + turn endpoints the TUI hits: held-open SSE, a two-session
/// history list, session create, empty transcripts, and a scripted
/// text-delta turn per prompt POST. Everything else answers `{}`.
async fn start_stub() -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let port = listener.local_addr().expect("addr").port();
    let (events_tx, _) = tokio::sync::broadcast::channel::<String>(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    let prompts = Arc::new(AtomicUsize::new(0));
    let created = Arc::new(AtomicUsize::new(0));

    {
        let events_tx = events_tx;
        let shutdown = Arc::clone(&shutdown);
        let prompts = Arc::clone(&prompts);
        let created = Arc::clone(&created);
        tokio::spawn(async move {
            loop {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let events_tx = events_tx.clone();
                let prompts = Arc::clone(&prompts);
                let created = Arc::clone(&created);
                tokio::spawn(async move {
                    serve_connection(&mut socket, events_tx, &prompts, &created).await;
                });
            }
        });
    }

    // Warmup: prove the accept loop serves before the child spawns
    // (the `mini_pty.rs` flake guard).
    let warm =
        format!("GET /api/info HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let mut ready = false;
    for _ in 0..100 {
        if let Ok(mut probe) = TcpStream::connect(("127.0.0.1", port)).await {
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
    Stub { port, shutdown, prompts }
}

fn session_json(id: &str, title: &str) -> String {
    format!(
        concat!(
            r#"{{"id":"{id}","title":"{title}","time":{{"created":1,"updated":1}},"#,
            r#""location":{{"directory":"/tmp/opencode"}},"projectID":"prj_fuzz","cost":0,"#,
            r#""tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}}"#
        ),
        id = id,
        title = title
    )
}

async fn serve_connection(
    socket: &mut TcpStream,
    events_tx: tokio::sync::broadcast::Sender<String>,
    prompts: &AtomicUsize,
    created: &AtomicUsize,
) {
    let head = read_request(socket).await;
    let mut parts = head.lines().next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let path_only = path.split('?').next().unwrap_or_default().to_owned();

    // SSE: headers then park, forwarding whatever the script emits.
    if method == "GET" && path_only == "/api/event" {
        let response = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
        if socket.write_all(response.as_bytes()).await.is_err() {
            return;
        }
        let mut subscription = events_tx.subscribe();
        loop {
            let body = match subscription.recv().await {
                Ok(body) => body,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            if socket.write_all(format!("data: {body}\n\n").as_bytes()).await.is_err() {
                break;
            }
        }
        return;
    }

    if method == "GET" && path_only == "/api/info" {
        respond_json(
            socket,
            r#"{"version":"2.0.24","pid":1,"urls":[],"paths":{"tmp":"/tmp/opencode"}}"#,
        )
        .await;
        return;
    }
    if method == "GET" && path_only == "/api/location" {
        respond_json(
            socket,
            r#"{"directory":"/tmp/opencode","project":{"canonical":"/tmp/opencode","directory":"/tmp/opencode","id":"prj_fuzz"}}"#,
        )
        .await;
        return;
    }
    if method == "GET" && path_only == "/api/session" {
        // History: two sessions for the list dialogs / quick-switch.
        respond_json(
            socket,
            &format!(
                r#"{{"cursor":{{}},"data":[{},{}]}}"#,
                session_json("ses_fuzz_alpha", "Alpha"),
                session_json("ses_fuzz_beta", "Beta")
            ),
        )
        .await;
        return;
    }
    if method == "POST" && path_only == "/api/session" {
        // session.create (first prompt from home opens a session).
        let n = created.fetch_add(1, Ordering::SeqCst);
        respond_json(
            socket,
            &format!(r#"{{"data":{}}}"#, session_json(&format!("ses_dyn_{n}"), "Untitled")),
        )
        .await;
        return;
    }
    if method == "GET"
        && path_only.matches('/').count() == 3
        && path_only.starts_with("/api/session/")
    {
        // GET /api/session/{id}
        let id = path_only.trim_start_matches("/api/session/").to_owned();
        respond_json(socket, &format!(r#"{{"data":{}}}"#, session_json(&id, "Untitled"))).await;
        return;
    }
    if method == "GET" && path_only.ends_with("/message") {
        respond_json(socket, r#"{"cursor":{},"data":[]}"#).await;
        return;
    }
    if method == "POST" && path_only.ends_with("/prompt") {
        let n = prompts.fetch_add(1, Ordering::SeqCst);
        let session_id =
            path_only.trim_start_matches("/api/session/").trim_end_matches("/prompt").to_owned();
        respond_json(
            socket,
            &format!(
                concat!(
                    r#"{{"data":{{"id":"msg_{n}","delivery":"queue","sessionID":"{sid}","#,
                    r#""payload":{{"text":""}},"time":{{"created":1}},"type":"user"}}}}"#
                ),
                n = n,
                sid = session_id
            ),
        )
        .await;
        emit_turn_script(&events_tx, &session_id, n);
        return;
    }
    if method == "POST" && path_only.ends_with("/reply") {
        let _ = socket.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n").await;
        return;
    }
    respond_json(socket, "{}").await;
}

/// The scripted turn a prompt triggers (the `mini_pty.rs`
/// `PlainReply` script, per-prompt distinct text so transcripts grow
/// visibly).
fn emit_turn_script(tx: &tokio::sync::broadcast::Sender<String>, session_id: &str, n: usize) {
    let event = |kind: &str, data: serde_json::Value| {
        let _ = tx.send(
            serde_json::json!({ "type": kind, "data": data, "created": 1, "id": format!("evt_{n}") })
                .to_string(),
        );
    };
    let session = serde_json::json!({ "sessionID": session_id });
    event("session.execution.started", session.clone());
    event(
        "session.text.started",
        serde_json::json!({ "sessionID": session_id, "messageID": format!("msg_{n}"), "partID": format!("prt_{n}") }),
    );
    event(
        "session.text.delta",
        serde_json::json!({ "sessionID": session_id, "messageID": format!("msg_{n}"), "partID": format!("prt_{n}"),
                             "delta": format!("Fuzz reply #{n}: the harness says hello.") }),
    );
    event(
        "session.text.ended",
        serde_json::json!({ "sessionID": session_id, "messageID": format!("msg_{n}"), "partID": format!("prt_{n}") }),
    );
    event("session.execution.succeeded", session);
}

/// Read one request (head + content-length body) into a string.
async fn read_request(socket: &mut TcpStream) -> String {
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
                    let body_needed = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.trim()
                                .eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if seen >= head_end + 4 + body_needed {
                        break;
                    }
                }
            }
        }
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

async fn respond_json(socket: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

// -- the VT screen analyzer ---------------------------------------------------

/// A minimal incremental VT100 parser: reconstructs the visible
/// 24×80 cell grid from the output stream. Handles what a ratatui +
/// crossterm app emits — CUP (`H`/`f`), cursor moves (`A`/`B`/`C`/`D`),
/// ED (`J`), EL (`K`), C0 (`\r`, `\n`, `\b`), UTF-8 text with
/// double-width (CJK) cells; SGR/OSC/mode-set sequences are consumed
/// and dropped. Partial trailing sequences wait for more bytes.
struct Vt {
    cells: Vec<Vec<char>>,
    row: usize,
    col: usize,
    buf: Vec<u8>,
}

impl Vt {
    fn new() -> Self {
        Vt { cells: vec![vec![' '; COLS]; ROWS], row: 0, col: 0, buf: Vec::new() }
    }

    /// Feed the appended output bytes; parse as far as possible.
    fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
        let mut i = 0usize;
        while i < self.buf.len() {
            let byte = self.buf[i];
            match byte {
                0x1b => {
                    let Some(second) = self.buf.get(i + 1) else { break };
                    match second {
                        b'[' => {
                            // CSI: scan for the final byte.
                            let mut j = i + 2;
                            while j < self.buf.len() && !(0x40..=0x7e).contains(&self.buf[j]) {
                                j += 1;
                            }
                            if j >= self.buf.len() {
                                break; // incomplete CSI — wait for more bytes
                            }
                            let params = self.buf[i + 2..j].to_vec();
                            let final_byte = self.buf[j];
                            i = j + 1;
                            self.csi(&params, final_byte);
                        }
                        b']' => {
                            // OSC: BEL or ESC \ terminator.
                            let mut j = i + 2;
                            let mut terminator = None;
                            while j < self.buf.len() {
                                if self.buf[j] == 0x07 {
                                    terminator = Some(j + 1);
                                    break;
                                }
                                if self.buf[j] == 0x1b && self.buf.get(j + 1) == Some(&0x5c) {
                                    terminator = Some(j + 2);
                                    break;
                                }
                                j += 1;
                            }
                            let Some(end) = terminator else { break };
                            i = end;
                        }
                        _ => {
                            i += 2; // two-byte escape (ESC ( B etc.)
                        }
                    }
                }
                b'\r' => {
                    self.col = 0;
                    i += 1;
                }
                b'\n' => {
                    self.row = (self.row + 1).min(ROWS - 1);
                    i += 1;
                }
                0x08 => {
                    self.col = self.col.saturating_sub(1);
                    i += 1;
                }
                byte if byte < 0x20 || byte == 0x7f => {
                    i += 1;
                }
                _ => {
                    let len = utf8_len(byte);
                    if i + len > self.buf.len() {
                        break;
                    }
                    match std::str::from_utf8(&self.buf[i..i + len]) {
                        Ok(text) => {
                            let ch = text.chars().next().unwrap_or(' ');
                            self.put(ch);
                            i += len;
                        }
                        Err(_) => {
                            i += 1;
                        }
                    }
                }
            }
        }
        self.buf.drain(..i);
    }

    fn put(&mut self, ch: char) {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if self.col + width > COLS {
            // autowrap
            self.col = 0;
            self.row = (self.row + 1).min(ROWS - 1);
        }
        self.cells[self.row][self.col] = ch;
        if width > 1 && self.col + 1 < COLS {
            self.cells[self.row][self.col + 1] = '\u{0}'; // occupied by the wide glyph
        }
        self.col += width;
        if self.col >= COLS {
            self.col = 0;
            self.row = (self.row + 1).min(ROWS - 1);
        }
    }

    fn csi(&mut self, params: &[u8], final_byte: u8) {
        let numbers: Vec<usize> = params
            .split(|b| *b == b';')
            .map(|field| {
                String::from_utf8_lossy(field).trim_start_matches(['?', '>', '<', '=']).parse().ok()
            })
            .collect::<Vec<Option<usize>>>()
            .into_iter()
            .map(|n| n.unwrap_or(0))
            .collect();
        // Cursor addressing / movement counts are 1-based; erase modes
        // keep their raw 0/1/2.
        let count = |idx: usize| numbers.get(idx).copied().unwrap_or(1).max(1);
        let mode = |idx: usize| numbers.get(idx).copied().unwrap_or(0);
        match final_byte {
            b'H' | b'f' => {
                self.row = (count(0) - 1).min(ROWS - 1);
                self.col = (count(1) - 1).min(COLS - 1);
            }
            b'A' => self.row = self.row.saturating_sub(count(0)),
            b'B' => self.row = (self.row + count(0)).min(ROWS - 1),
            b'C' => self.col = (self.col + count(0)).min(COLS - 1),
            b'D' => self.col = self.col.saturating_sub(count(0)),
            b'J' => match mode(0) {
                2 | 3 => {
                    self.cells = vec![vec![' '; COLS]; ROWS];
                }
                0 => {
                    for cell in &mut self.cells[self.row][self.col..] {
                        *cell = ' ';
                    }
                    for row in &mut self.cells[self.row + 1..] {
                        for cell in &mut row[..] {
                            *cell = ' ';
                        }
                    }
                }
                _ => {
                    for row in &mut self.cells[..=self.row] {
                        for cell in row.iter_mut() {
                            *cell = ' ';
                        }
                    }
                }
            },
            b'K' => match mode(0) {
                0 => {
                    for cell in &mut self.cells[self.row][self.col..] {
                        *cell = ' ';
                    }
                }
                1 => {
                    for cell in &mut self.cells[self.row][..=self.col.min(COLS - 1)] {
                        *cell = ' ';
                    }
                }
                _ => {
                    for cell in &mut self.cells[self.row][..] {
                        *cell = ' ';
                    }
                }
            },
            _ => {}
        }
    }

    /// The visible text rows (trailing blanks trimmed).
    fn text_rows(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                row.iter().filter(|c| **c != '\u{0}').collect::<String>().trim_end().to_owned()
            })
            .collect()
    }

    fn screen_text(&self) -> String {
        self.text_rows().join("\n")
    }

    fn nonblank_rows(&self) -> usize {
        self.text_rows().iter().filter(|row| !row.is_empty()).count()
    }

    #[allow(clippy::cast_precision_loss)] // a diff ratio, not arithmetic
    fn diff_fraction(&self, other: &Vt) -> f64 {
        let mut changed = 0usize;
        for (a, b) in self.cells.iter().zip(other.cells.iter()) {
            for (x, y) in a.iter().zip(b.iter()) {
                if x != y {
                    changed += 1;
                }
            }
        }
        changed as f64 / (ROWS * COLS) as f64
    }
}

fn utf8_len(byte: u8) -> usize {
    match byte {
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

// -- the pty driver -----------------------------------------------------------

/// The real binary over a pty with a controlling terminal, a sandboxed
/// HOME, and a reader thread that ALSO answers the terminal queries a
/// real terminal answers (OSC 11 background probe, CPR) — instantly,
/// like a local terminal would.
struct FuzzPty {
    child: Child,
    master: std::fs::File,
    output: Arc<Mutex<Vec<u8>>>,
    vt: Vt,
}

impl FuzzPty {
    #[allow(clippy::too_many_lines)] // a spawn ritual, not logic
    fn spawn(port: u16) -> Self {
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
            let winsize = libc::winsize { ws_row: 24, ws_col: 80, ws_ypixel: 0, ws_xpixel: 0 };
            libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
        }

        // Sandboxed global roots (the `pty_boot.rs` pattern): the
        // child must never see the developer's real service state.
        let sandbox = std::path::Path::new("/tmp/opencode").join(format!(
            "pty-fuzz-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        std::fs::create_dir_all(sandbox.join(".config")).expect("config dir");
        std::fs::create_dir_all(sandbox.join(".local/state")).expect("state dir");
        std::fs::create_dir_all(sandbox.join("work")).expect("work dir");

        let binary = env!("CARGO_BIN_EXE_opencode-rust");
        let mut command = Command::new(binary);
        unsafe {
            // The pty is the child's controlling terminal: crossterm
            // and the OSC 11 probe read the tty, so the slave must BE
            // /dev/tty (setsid + TIOCSCTTY — the `mini_pty.rs` pattern).
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
            .arg("--server")
            .arg(format!("http://127.0.0.1:{port}"))
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

        let child = command.spawn().expect("spawn opencode-rust");

        let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let output = Arc::clone(&output);
            let mut reader = master.try_clone().expect("clone master");
            let mut reply_writer = master.try_clone().expect("clone master write");
            std::thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                // Queries the child asks that a real terminal answers.
                // Each occurrence is replied exactly once, instantly.
                let queries: &[(&[u8], &[u8])] = &[
                    // OSC 11 background probe → dark grey.
                    (b"\x1b]11;?", b"\x1b]11;rgb:1c1c/1c1c/1c1c\x07"),
                    // Cursor position report → home.
                    (b"\x1b[6n", b"\x1b[1;1R"),
                ];
                let mut pending: Vec<u8> = Vec::new();
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            output.lock().expect("lock").extend_from_slice(&chunk[..count]);
                            pending.extend_from_slice(&chunk[..count]);
                            loop {
                                let earliest = queries
                                    .iter()
                                    .filter_map(|(pattern, reply)| {
                                        pending
                                            .windows(pattern.len())
                                            .position(|w| w == *pattern)
                                            .map(|at| (at, pattern.len(), *reply))
                                    })
                                    .min_by_key(|(at, _, _)| *at);
                                let Some((at, len, reply)) = earliest else { break };
                                let _ = reply_writer.write_all(reply);
                                pending.drain(..at + len);
                            }
                            if pending.len() > 16_384 {
                                pending.drain(..8192);
                            }
                        }
                    }
                }
            });
        }
        FuzzPty { child, master, output, vt: Vt::new() }
    }

    fn capture(&self) -> Vec<u8> {
        self.output.lock().expect("lock").clone()
    }

    fn wait_boot(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let data = self.capture();
            if data.windows(3).any(|w| w == "█▀▀".as_bytes())
                || data.windows(3).any(|w| w == "┃".as_bytes())
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// Wait for the screen to settle: the run loop re-renders on a
    /// 100 ms tick whenever any clock-driven UI (caret/spinner/toast)
    /// is live, so byte quiescence never happens — settle on CELL
    /// stability instead (unchanged parsed screen for ~240 ms).
    fn settle(&mut self, cap: Duration) {
        let start = Instant::now();
        let mut stable_since = Instant::now();
        self.refresh_screen();
        let mut last_cells = self.vt.cells.clone();
        while start.elapsed() < cap {
            std::thread::sleep(Duration::from_millis(60));
            self.refresh_screen();
            if self.vt.cells == last_cells {
                if stable_since.elapsed() >= Duration::from_millis(240) {
                    return;
                }
            } else {
                last_cells.clone_from(&self.vt.cells);
                stable_since = Instant::now();
            }
        }
    }

    /// Feed appended output into the screen analyzer.
    fn refresh_screen(&mut self) {
        let data = self.capture();
        self.vt.feed(&data);
    }

    fn type_key(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).expect("write key");
    }

    /// Poll the child: `None` while running, `Some(code)` on exit
    /// (signals map to negative codes).
    fn exit_code(&mut self) -> Option<i32> {
        self.child.try_wait().expect("try_wait").map(|status| status.code().unwrap_or(-1))
    }

    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// -- the key generator --------------------------------------------------------

/// One driver action: the bytes to press, a human label for the
/// reproducer dump, and how the invariants treat it.
#[derive(Clone)]
struct Action {
    label: String,
    bytes: Vec<u8>,
    /// Navigation-ish keys legitimately redraw the whole screen
    /// (dialogs, tab switches) — exempt from the corruption bound.
    navigation: bool,
    /// Ctrl+c / ctrl+d / <leader>q: an exit here is intended.
    exitish: bool,
    /// Slash-command text (opens the autocomplete overlay).
    slash: bool,
}

const WORDS: &[&str] = &[
    "hello", "world", "test", "rust", "opencode", "please", "fix", "the", "loading", "bug", "help",
    "me", "write", "docs", "fast", "run",
];
const SLASH: &[&str] = &[
    "/help",
    "/new",
    "/models",
    "/theme",
    "/sessions",
    "/export",
    "/permission",
    "/compact",
    "/status",
    "/quit",
];
const CJK: &[&str] = &["你好", "世界", "测试中文输入", "こんにちは"];
const LEADER_KEYS: &[u8] = b"lnwqtbsxgce123";
const CTRL_KEYS: &[u8] = b"patfdwcbz";

/// Weighted, human-plausible key mix (weights per 100 rolls):
/// text 30 (words 60% / slash commands 32% / CJK 8%), enter 8,
/// escape 12 (humans mash it), backspace 8, arrows 10, tab 4,
/// ctrl-keys 8, leader chords 10 (`DEFAULT_TABLE` pairs), page 3,
/// home/end 2, ctrl+arrows 3, space 2.
fn gen_actions(seed: u64, count: usize) -> Vec<Action> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut actions: Vec<Action> = (0..count).map(|_| gen_action(&mut rng)).collect();
    // A submitted `/quit` / `/exit` command is an intended exit path
    // (the autocomplete's own description says "Exit the app") — the
    // Enter that submits it counts as an exit chord for invariant (a).
    for i in 1..actions.len() {
        let submits_exit = actions[i - 1].bytes.starts_with(b"/quit")
            || actions[i - 1].bytes.starts_with(b"/exit");
        if submits_exit && actions[i].bytes == b"\r" {
            actions[i].exitish = true;
        }
    }
    actions
}

fn gen_action(rng: &mut StdRng) -> Action {
    let roll: u32 = rng.gen_range(0..100);
    let plain = |label: &str, bytes: &[u8], navigation: bool, exitish: bool, slash: bool| Action {
        label: label.to_owned(),
        bytes: bytes.to_vec(),
        navigation,
        exitish,
        slash,
    };
    let text_action = |text: &str, slash: bool| {
        plain(&format!("text({text:?})"), text.as_bytes(), false, false, slash)
    };
    if roll < 30 {
        let pick: u32 = rng.gen_range(0..100);
        if pick < 60 {
            let word = WORDS[rng.gen_range(0..WORDS.len())];
            text_action(word, false)
        } else if pick < 92 {
            let cmd = SLASH[rng.gen_range(0..SLASH.len())];
            text_action(cmd, true)
        } else {
            let cjk = CJK[rng.gen_range(0..CJK.len())];
            text_action(cjk, false)
        }
    } else if roll < 38 {
        plain("enter", b"\r", true, false, false)
    } else if roll < 50 {
        plain("escape", b"\x1b", true, false, false)
    } else if roll < 58 {
        plain("backspace", b"\x7f", false, false, false)
    } else if roll < 68 {
        let arrows: &[(&str, &[u8])] =
            &[("up", b"\x1b[A"), ("down", b"\x1b[B"), ("right", b"\x1b[C"), ("left", b"\x1b[D")];
        let (name, bytes) = arrows[rng.gen_range(0..arrows.len())];
        plain(&format!("arrow_{name}"), bytes, true, false, false)
    } else if roll < 72 {
        if rng.gen_bool(0.5) {
            plain("tab", b"\t", true, false, false)
        } else {
            plain("shift_tab", b"\x1b[Z", true, false, false)
        }
    } else if roll < 80 {
        let key = CTRL_KEYS[rng.gen_range(0..CTRL_KEYS.len())];
        plain(
            &format!("ctrl+{}", key as char),
            &[key - b'a' + 1],
            true,
            key == b'c' || key == b'd',
            false,
        )
    } else if roll < 90 {
        let key = LEADER_KEYS[rng.gen_range(0..LEADER_KEYS.len())];
        let mut bytes = vec![0x18]; // ctrl+x = the leader prefix
        bytes.push(key);
        plain(&format!("leader+{}", key as char), &bytes, true, key == b'q', false)
    } else if roll < 93 {
        if rng.gen_bool(0.5) {
            plain("page_up", b"\x1b[5~", true, false, false)
        } else {
            plain("page_down", b"\x1b[6~", true, false, false)
        }
    } else if roll < 95 {
        if rng.gen_bool(0.5) {
            plain("home", b"\x1b[H", true, false, false)
        } else {
            plain("end", b"\x1b[F", true, false, false)
        }
    } else if roll < 98 {
        let ctrl_arrows: &[(&str, &[u8])] = &[
            ("ctrl+up", b"\x1b[1;5A"),
            ("ctrl+down", b"\x1b[1;5B"),
            ("ctrl+right", b"\x1b[1;5C"),
            ("ctrl+left", b"\x1b[1;5D"),
        ];
        let (name, bytes) = ctrl_arrows[rng.gen_range(0..ctrl_arrows.len())];
        plain(name, bytes, true, false, false)
    } else {
        plain("space", b" ", false, false, false)
    }
}

// -- invariants + the run loop ------------------------------------------------

/// Drive one seed's full key sequence against a fresh child, checking
/// every invariant after every key. `Err` carries the dump path.
#[allow(clippy::too_many_lines)]
async fn fuzz_one(seed: u64, keys: usize) -> Result<(), String> {
    let stub = start_stub().await;
    let actions = gen_actions(seed, keys);
    // 5 evenly-spaced recoverability checkpoints (past boot settle).
    let checkpoints: Vec<usize> = (1..=5).map(|k| keys * k / 6).collect();

    let mut pty = FuzzPty::spawn(stub.port);
    if !pty.wait_boot(Duration::from_secs(30)) {
        let dump = dump_failure(
            seed,
            "boot",
            "the home wordmark never rendered",
            &actions,
            &[],
            &pty.capture(),
        );
        pty.kill();
        stub.shutdown.store(true, Ordering::SeqCst);
        return Err(format!("seed {seed}: BOOT failure — no wordmark in 30s — dump: {dump}"));
    }

    let mut prev_screen: Option<Vt> = None;
    let mut slash_window = false; // autocomplete may be open
    let mut respawns = 0usize;
    let mut exitish_window = 0usize; // actions since an exit-ish key
    let mut scanned: usize = 0; // panic/rgb scan offset into the capture
    let mut corruption_checks = 0usize;
    let mut recover_probes = 0usize;

    let mut index = 0usize;
    while index < actions.len() {
        let action = &actions[index];
        pty.type_key(&action.bytes);
        pty.settle(Duration::from_millis(1500));
        exitish_window = if action.exitish { 0 } else { exitish_window + 1 };

        // (a) alive — running, or an intended, panic-free exit.
        match pty.exit_code() {
            None => {}
            Some(code) => {
                let capture = pty.capture();
                let panicked = capture.windows(8).any(|w| w == b"panicked");
                if code != 0 || panicked || exitish_window > 3 {
                    let why = if code != 0 {
                        format!("exited with code {code}")
                    } else if panicked {
                        "output contains a panic".to_owned()
                    } else {
                        format!("spontaneous exit (code {code}) with no exit chord in the last 3 actions")
                    };
                    let dump = dump_failure(
                        seed,
                        "alive",
                        &why,
                        &actions[..=index],
                        &pty.vt.text_rows(),
                        &capture,
                    );
                    pty.kill();
                    stub.shutdown.store(true, Ordering::SeqCst);
                    return Err(format!(
                        "seed {seed}: INVARIANT a (alive) failed at key {}/{}: {why} — dump: {dump}",
                        index + 1,
                        actions.len()
                    ));
                }
                // Intended exit mid-run: continue the sequence in a
                // fresh child (bounded).
                if respawns >= MAX_RESPAWNS {
                    break;
                }
                respawns += 1;
                pty.kill();
                pty = FuzzPty::spawn(stub.port);
                if !pty.wait_boot(Duration::from_secs(30)) {
                    let dump = dump_failure(
                        seed,
                        "boot",
                        &format!("respawn #{respawns} never reached the wordmark"),
                        &actions[..=index],
                        &[],
                        &pty.capture(),
                    );
                    pty.kill();
                    stub.shutdown.store(true, Ordering::SeqCst);
                    return Err(format!(
                        "seed {seed}: BOOT failure on respawn #{respawns} — dump: {dump}"
                    ));
                }
                prev_screen = None;
                scanned = 0;
                slash_window = false;
                index += 1;
                continue;
            }
        }

        // Scan the newly appended output for panics / leaked replies
        // (the screen was refreshed inside settle).
        let capture = pty.capture();
        let appended = &capture[scanned.min(capture.len())..];
        if appended.windows(8).any(|w| w == b"panicked") {
            let dump = dump_failure(
                seed,
                "alive",
                "output contains a panic",
                &actions[..=index],
                &pty.vt.text_rows(),
                &capture,
            );
            pty.kill();
            stub.shutdown.store(true, Ordering::SeqCst);
            return Err(format!(
                "seed {seed}: INVARIANT a (alive) failed at key {}/{}: panic in output — dump: {dump}",
                index + 1,
                actions.len()
            ));
        }
        // (b) no escape-response garbage: the harness DID answer the
        // OSC 11 probe like a real terminal; the answer must never be
        // drawn as text.
        if appended.windows(4).any(|w| w == b"rgb:") {
            let dump = dump_failure(
                seed,
                "no-rgb-garbage",
                "an OSC 11 reply (rgb:…) surfaced in the output stream — escape-response garbage",
                &actions[..=index],
                &pty.vt.text_rows(),
                &capture,
            );
            pty.kill();
            stub.shutdown.store(true, Ordering::SeqCst);
            return Err(format!(
                "seed {seed}: INVARIANT b (no escape-response garbage) failed at key {}/{}: rgb: leaked into the render — dump: {dump}",
                index + 1,
                actions.len()
            ));
        }
        scanned = capture.len();

        let screen_text = pty.vt.screen_text();

        // (c) non-void screen after boot settles.
        if index >= BOOT_SETTLE_KEYS {
            let has_rail = screen_text.contains('┃');
            let rows = pty.vt.nonblank_rows();
            if !has_rail && rows < 3 {
                let dump = dump_failure(
                    seed,
                    "non-void-screen",
                    &format!("blank void: no composer rail and only {rows} non-blank row(s)"),
                    &actions[..=index],
                    &pty.vt.text_rows(),
                    &capture,
                );
                pty.kill();
                stub.shutdown.store(true, Ordering::SeqCst);
                return Err(format!(
                    "seed {seed}: INVARIANT c (non-void screen) failed at key {}/{}: ≥90% blank while the app runs — dump: {dump}",
                    index + 1,
                    actions.len()
                ));
            }
        }

        // Slash-command autocomplete may be open while slash text is
        // the recent input; navigation keys (enter/escape/arrows)
        // dismiss it, plain typing extends the query and keeps it.
        if action.slash {
            slash_window = true;
        } else if action.navigation {
            slash_window = false;
        }

        // (e) bounded corruption: a plain typing key (word / CJK /
        // space / backspace, outside the autocomplete window) must not
        // flip >60% of the cells.
        let is_text_key = action.label.starts_with("text(") && action.bytes.first() != Some(&b'/');
        let is_edit_key = matches!(action.label.as_str(), "space" | "backspace");
        let plain_typing = (is_text_key || is_edit_key) && !action.navigation;
        if plain_typing && !slash_window {
            if let Some(prev) = prev_screen.as_ref() {
                corruption_checks += 1;
                let fraction = prev.diff_fraction(&pty.vt);
                if fraction > 0.6 {
                    let dump = dump_failure(
                        seed,
                        "bounded-corruption",
                        &format!(
                            "non-navigation key '{}' flipped {:.0}% of the screen cells",
                            action.label,
                            fraction * 100.0
                        ),
                        &actions[..=index],
                        &pty.vt.text_rows(),
                        &capture,
                    );
                    pty.kill();
                    stub.shutdown.store(true, Ordering::SeqCst);
                    return Err(format!(
                        "seed {seed}: INVARIANT e (bounded corruption) failed at key {}/{}: '{}' rewrote >60% of the screen — dump: {dump}",
                        index + 1,
                        actions.len(),
                        action.label
                    ));
                }
            }
        }

        // (d) recoverability checkpoints: escape ×3 must land on a
        // stable shape or leave a responding composer.
        if checkpoints.contains(&(index + 1)) {
            recover_probes += 1;
            if let Err(detail) = probe_recoverable(&mut pty) {
                let dump = dump_failure(
                    seed,
                    "recoverability",
                    &detail,
                    &actions[..=index],
                    &pty.vt.text_rows(),
                    &pty.capture(),
                );
                pty.kill();
                stub.shutdown.store(true, Ordering::SeqCst);
                return Err(format!(
                    "seed {seed}: INVARIANT d (recoverability) failed at key {}/{}: {detail} — dump: {dump}",
                    index + 1,
                    actions.len()
                ));
            }
            slash_window = false;
        }

        prev_screen = Some(Vt { cells: pty.vt.cells.clone(), row: 0, col: 0, buf: Vec::new() });
        index += 1;
    }
    // End of sequence: the documented exit chords must end the process
    // cleanly (the dead-end class: a TUI that will not exit).
    if let Err(detail) = exit_protocol(&mut pty) {
        let dump =
            dump_failure(seed, "exit-path", &detail, &actions, &pty.vt.text_rows(), &pty.capture());
        pty.kill();
        stub.shutdown.store(true, Ordering::SeqCst);
        return Err(format!("seed {seed}: EXIT failure: {detail} — dump: {dump}"));
    }
    let capture = pty.capture();
    if capture.windows(8).any(|w| w == b"panicked") {
        let dump = dump_failure(
            seed,
            "exit-path",
            "panic during exit",
            &actions,
            &pty.vt.text_rows(),
            &capture,
        );
        pty.kill();
        stub.shutdown.store(true, Ordering::SeqCst);
        return Err(format!("seed {seed}: EXIT failure: panic during exit — dump: {dump}"));
    }
    pty.kill();
    stub.shutdown.store(true, Ordering::SeqCst);
    eprintln!(
        "[pty_fuzz] seed {seed}: {keys} keys OK — {corruption_checks} corruption checks, \
         {recover_probes} recover probes, {respawns} respawns, {} prompts",
        stub.prompts.load(Ordering::SeqCst)
    );
    Ok(())
}

/// Invariant (d): escape ×3, then within 2s a known stable shape
/// (wordmark / composer rail) or a composer that still responds
/// (typing a char changes the capture).
fn probe_recoverable(pty: &mut FuzzPty) -> Result<(), String> {
    for _ in 0..3 {
        pty.type_key(b"\x1b");
        std::thread::sleep(Duration::from_millis(250));
    }
    pty.settle(Duration::from_millis(2000));
    let screen = pty.vt.screen_text();
    if screen.contains('┃') || screen.contains("█▀▀█") {
        return Ok(());
    }
    // Responsiveness probe: the typed char must show up on screen
    // (the composer echoed it) — a live clock alone doesn't count.
    let had_probe = screen.contains('Z');
    let cells_before = pty.vt.cells.clone();
    pty.type_key(b"Z");
    let deadline = Instant::now() + Duration::from_millis(2000);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(120));
        pty.refresh_screen();
        let screen = pty.vt.screen_text();
        let echoed = screen.contains('Z') && !had_probe;
        let cells_moved = had_probe && pty.vt.cells != cells_before;
        if echoed || cells_moved {
            pty.type_key(b"\x7f"); // undo the probe char
            return Ok(());
        }
    }
    Err("after escape ×3 the screen shows no stable shape and typing changes nothing (dead end)"
        .to_owned())
}

/// The documented exit chords, in order: ctrl+c (clear/exit), ctrl+c
/// again (two-press exit), then <leader>q.
fn exit_protocol(pty: &mut FuzzPty) -> Result<(), String> {
    pty.type_key(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    if pty.exit_code() == Some(0) {
        return Ok(());
    }
    pty.type_key(b"\x03");
    std::thread::sleep(Duration::from_millis(500));
    if pty.exit_code() == Some(0) {
        return Ok(());
    }
    pty.type_key(b"\x03");
    std::thread::sleep(Duration::from_millis(600));
    if pty.exit_code() == Some(0) {
        return Ok(());
    }
    pty.type_key(b"\x18q");
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        match pty.exit_code() {
            Some(0) => return Ok(()),
            Some(code) => return Err(format!("exit chord exited with code {code}")),
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    Err("the process ignored escape + ctrl+c ×2 + <leader>q (dead end)".to_owned())
}

/// Write the failure dump: invariant, reproducer (the key sequence so
/// far, hex-exact), the reconstructed screen, and the raw output tail.
fn dump_failure(
    seed: u64,
    invariant: &str,
    detail: &str,
    actions: &[Action],
    screen_rows: &[String],
    capture: &[u8],
) -> String {
    use std::fmt::Write as _;
    let path = std::path::Path::new("/tmp/opencode").join(format!("pty-fuzz-fail-s{seed}.txt"));
    let mut out = String::new();
    let _ = write!(
        out,
        "opencode PTY fuzz failure\n========================\ninvariant: {invariant}\nseed:      {seed}\ndetail:    {detail}\nkeys:      {}\n\nREPRODUCER (label + hex bytes):\n",
        actions.len()
    );
    for (i, action) in actions.iter().enumerate() {
        let hex: String =
            action.bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        let _ = writeln!(out, "#{i:03} {:24} {hex}", action.label);
    }
    out.push_str("\nSCREEN (reconstructed, 24×80):\n");
    for (i, row) in screen_rows.iter().enumerate() {
        let _ = writeln!(out, "{i:02} |{row:<80}|");
    }
    let tail = capture.len().saturating_sub(12_288);
    out.push_str("\nRAW OUTPUT TAIL (last 12 KiB, lossy):\n");
    out.push_str(&String::from_utf8_lossy(&capture[tail..]));
    let _ = std::fs::write(&path, &out);
    path.display().to_string()
}

// -- entrypoints ---------------------------------------------------------------

fn seeds_from_env(default: &[u64]) -> Vec<u64> {
    let parsed: Vec<u64> = std::env::var("OC_PTY_FUZZ_SEEDS")
        .ok()
        .and_then(|value| {
            let seeds: Vec<u64> =
                value.split(',').filter_map(|part| part.trim().parse().ok()).collect();
            (!seeds.is_empty()).then_some(seeds)
        })
        .unwrap_or_default();
    if parsed.is_empty() {
        default.to_vec()
    } else {
        parsed
    }
}

fn keys_from_env(default: usize) -> usize {
    std::env::var("OC_PTY_FUZZ_KEYS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// CI smoke: 3 fixed seeds × 120 keys (deterministic; the serialized
/// pty job). Env overrides: `OC_PTY_FUZZ_SEEDS`, `OC_PTY_FUZZ_KEYS`.
#[tokio::test(flavor = "multi_thread")]
async fn pty_fuzz_smoke() {
    let seeds = seeds_from_env(&[20_260_101, 20_260_102, 20_260_103]);
    let keys = keys_from_env(120);
    for seed in seeds {
        if let Err(message) = fuzz_one(seed, keys).await {
            panic!("pty_fuzz_smoke: {message}");
        }
    }
}

/// Nightly/local long pass: 20 seeds × 250 keys.
/// `cargo nextest run -p oc-cli --runignored only -E 'test(pty_fuzz_long)'`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "long: the nightly-style 20-seed invariant sweep"]
async fn pty_fuzz_long() {
    let seeds = seeds_from_env(&(1..=20).collect::<Vec<u64>>());
    let keys = keys_from_env(250);
    for seed in seeds {
        if let Err(message) = fuzz_one(seed, keys).await {
            panic!("pty_fuzz_long: {message}");
        }
    }
}
