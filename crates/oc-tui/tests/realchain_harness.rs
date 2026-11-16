//! CHANNEL 9 (real-chain) W1 harness — the machine half of the six
//! beta.15 symptom reproductions.
//!
//! Topology (the approved W1 design — the ONLY stub is the replay
//! gateway; everything else is the production assembly):
//!
//! ```text
//! real PTY keystrokes → real TUI process (the `opencode-rust`
//!   binary, production boot: oc-cli root handler → managed
//!   background oc-server via lifecycle::ensure — real config
//!   discovery, real storage, sandboxed HOME/XDG roots)
//!                          ↓ HTTP (loopback)
//!               replay gateway (tools/diff/realchain/replay_gateway.py)
//!               — speaks the real anthropic-messages wire protocol,
//!                 matches by normalized request hash, replays
//!                 recorded SSE bodies byte-for-byte; strict 404 on a
//!                 miss.
//! ```
//!
//! Boot topology conclusion (read from the code, not guessed): the
//! production TUI NEVER embeds the server — `oc-cli`'s root handler
//! resolves a server through `services::server_connection` (explicit
//! `--server`, `--standalone` stdio lease, or the MANAGED background
//! daemon via `oc_server::lifecycle::ensure`, registration under
//! `$XDG_STATE_HOME/opencode/service.json`). This harness uses the
//! managed-daemon path (the default a real user gets): `opencode-rust
//! service start` under the sandbox env, then the TUI attaches to the
//! same registration.
//!
//! Sandbox discipline: every root lives under
//! `/tmp/opencode/t9w1-<pid>-<tag>` (HOME, XDG_CONFIG/DATA/STATE/
//! CACHE); the child env is scrubbed of real-user OPENCODE_* vars; a
//! cross-process flock serializes the tests (nextest runs each test
//! in its own process — the in-process `pty_lock` pattern cannot
//! reach across). Teardown is graceful: exit chord → wait → the
//! binary's own `service stop` under the same sandbox → gateway child
//! handle teardown. No naked PIDs, no kill/pkill.

#![allow(unsafe_code)] // forkpty/ioctl/pre_exec are libc FFI by nature
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

// ---------------------------------------------------------------------------
// Workspace roots / logs
// ---------------------------------------------------------------------------

/// The worktree root (`crates/oc-tui/tests` → up three: tests, oc-tui, crates).
fn worktree_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

/// The production binary built from this worktree.
fn opencode_bin() -> PathBuf {
    worktree_root().join("target/debug/opencode-rust")
}

/// One `.test-logs` artifact file per harness instance.
fn log_file(tag: &str) -> PathBuf {
    let dir = worktree_root().join(".test-logs");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    dir.join(format!("t9-{tag}-{stamp}-t9w1.log"))
}

/// The cross-process lane lock (nextest = one process per test; a
/// std Mutex cannot serialize those). Held for the whole test.
pub struct LaneGuard {
    _file: File,
}

pub fn lane_lock() -> LaneGuard {
    let path = Path::new("/tmp/opencode").join("t9w1-realchain.lock");
    let file = File::create(&path).unwrap_or_else(|_| panic!("open {path:?}"));
    unsafe {
        let rc = libc::flock(file.as_raw_fd(), libc::LOCK_EX);
        assert_eq!(rc, 0, "flock {path:?}");
    }
    LaneGuard { _file: file }
}

// ---------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------

pub struct Sandbox {
    pub root: PathBuf,
    pub project: PathBuf,
    pub gateway_port: u16,
    /// The `--redact` sandbox-root substitution handed to the gateway
    /// (PID-unique paths hash equal across runs).
    pub service_port: u16,
}

impl Sandbox {
    pub fn config_path(&self) -> PathBuf {
        // The server's global config dir is the SHARED v2 one,
        // `$HOME/.config/opencode` (boot.rs dogfood2 bug 3 — it
        // deliberately ignores XDG_CONFIG_HOME for config).
        self.root.join("home/.config/opencode/opencode.json")
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let s = &self.root;
        vec![
            ("HOME".into(), s.join("home").to_string_lossy().into_owned()),
            ("XDG_CONFIG_HOME".into(), s.join("xdg-config").to_string_lossy().into_owned()),
            ("XDG_DATA_HOME".into(), s.join("xdg-data").to_string_lossy().into_owned()),
            ("XDG_STATE_HOME".into(), s.join("xdg-state").to_string_lossy().into_owned()),
            ("XDG_CACHE_HOME".into(), s.join("xdg-cache").to_string_lossy().into_owned()),
            ("TERM".into(), "xterm-256color".into()),
            // A unique daemon port per sandbox: the dev-channel default
            // (0xc0de) would collide if two sandboxes ever overlapped.
            ("COLUMNS".into(), "120".into()),
            ("LINES".into(), "36".into()),
        ]
    }

    /// Scrubbed child env: sandbox vars in, real-user OPENCODE_* out.
    pub fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for (key, value) in std::env::vars() {
            // Keep PATH/locale so the binary can exec python-free helpers;
            // drop everything OPENCODE_* (never inherit real creds/config).
            if key.starts_with("OPENCODE") || key == "HOME" || key.starts_with("XDG_") {
                continue;
            }
            cmd.env(key, value);
        }
        for (key, value) in self.env() {
            cmd.env(key, value);
        }
        cmd.current_dir(&self.project);
        cmd
    }

    /// Write the global config: the custom `replay` provider
    /// (anthropic wire family via `package: "@ai-sdk/anthropic"`)
    /// pointing at the gateway, plus its model entries.
    pub fn write_config(&self, extra_models: &[(&str, &str)], default_model: &str) {
        let mut models = serde_json::Map::new();
        models.insert("claude-opus-4-7".into(), serde_json::json!({ "name": "claude-opus-4-7" }));
        for (id, name) in extra_models {
            models.insert((*id).into(), serde_json::json!({ "name": name }));
        }
        let config = serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "model": default_model,
            "autoupdate": false,
            "providers": {
                "replay": {
                    "name": "Replay Anthropic",
                    "package": "@ai-sdk/anthropic",
                    "settings": {
                        "baseURL": format!("http://127.0.0.1:{}/v1", self.gateway_port),
                        "apiKey": "replay-sandbox-key"
                    },
                    "models": models
                }
            }
        });
        let path = self.config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("config dir");
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
            .unwrap_or_else(|e| panic!("write config: {e}"));
        // Unique daemon port per sandbox (service.json — `ServiceConfig.Info.port`).
        let service = serde_json::json!({ "port": self.service_port });
        let service_path = self.root.join("xdg-config/opencode-rust/service.json");
        std::fs::create_dir_all(service_path.parent().unwrap()).expect("service config dir");
        std::fs::write(&service_path, serde_json::to_string_pretty(&service).unwrap())
            .unwrap_or_else(|e| panic!("write service config: {e}"));
    }

    /// The daemon registration written by `lifecycle::register` —
    /// `service start` prints only the URL; the password lives here.
    pub fn registration(&self) -> serde_json::Value {
        for base in ["opencode-rust", "opencode"] {
            for name in ["service.json", "service-dev.json", "service-beta.json"] {
                let path = self.root.join("xdg-state").join(base).join(name);
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        if value.get("url").is_some() {
                            return value;
                        }
                    }
                }
            }
        }
        serde_json::Value::Null
    }
}

pub fn sandbox(tag: &str, gateway_port: u16) -> Sandbox {
    let root = Path::new("/tmp/opencode").join(format!("t9w1-{}-{tag}", std::process::id()));
    for dir in ["home", "xdg-config", "xdg-data", "xdg-state", "xdg-cache", "project"] {
        std::fs::create_dir_all(root.join(dir)).expect("sandbox dir");
    }
    std::fs::write(root.join("project/README.md"), "# t9 sandbox project\n").expect("project file");
    let service_port = 20_000 + (std::process::id() % 20_000) as u16;
    Sandbox { root: root.clone(), project: root.join("project"), gateway_port, service_port }
}

// ---------------------------------------------------------------------------
// Replay gateway child
// ---------------------------------------------------------------------------

pub struct Gateway {
    pub port: u16,
    child: Child,
    log_path: PathBuf,
}

impl Gateway {
    /// Spawn the gateway; `record` = bootstrap seeding mode (writes
    /// generated cassettes into the committed corpus dir — sandbox
    /// traffic only, no user data).
    pub fn spawn(tag: &str, record: bool) -> Gateway {
        let root = worktree_root();
        let log_path = log_file(&format!("gateway-{tag}"));
        let mut cmd = Command::new("python3");
        cmd.arg(root.join("tools/diff/realchain/replay_gateway.py"));
        cmd.arg("--port").arg("0");
        for corpus in [
            root.join("crates/oc-ai/test-fixtures/recordings"),
            root.join("tools/diff/realchain/corpus"),
        ] {
            cmd.arg("--corpus").arg(&corpus);
        }
        // PID-unique sandbox paths must hash equal across runs.
        cmd.arg("--redact").arg(format!(
            "/tmp/opencode/t9w1-{}-{}:__SANDBOX__",
            std::process::id(),
            tag
        ));
        cmd.arg("--log").arg(&log_path);
        if record {
            cmd.arg("--seed-reply").arg(root.join("tools/diff/realchain/seeds/anthropic-text.txt"));
            cmd.arg("--record-dir").arg(root.join("tools/diff/realchain/corpus"));
        }
        // stderr rides the same log file (spawn/import errors land there).
        let err_sink = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("open gateway log");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::from(err_sink));
        let mut child = cmd.spawn().expect("spawn replay gateway");

        // Read stdout lines until the READY line (with a deadline).
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut stdout = child.stdout.take().expect("gateway stdout");
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            let mut pending = String::new();
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                        while let Some(pos) = pending.find('\n') {
                            let line = pending[..pos].to_owned();
                            pending.drain(..=pos);
                            if tx.send(line).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        let mut port = None;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(line) => {
                    eprintln!("[gateway] {line}");
                    if let Some(rest) = line.strip_prefix("READY") {
                        for part in rest.split_whitespace() {
                            if let Some(value) = part.strip_prefix("port=") {
                                port = value.parse::<u16>().ok();
                            }
                        }
                        if port.is_some() {
                            break;
                        }
                    }
                }
                Err(_) => continue,
            }
        }
        let port = port.unwrap_or_else(|| panic!("the replay gateway never reported READY"));
        Gateway { port, child, log_path }
    }

    pub fn stats(&self) -> Value {
        let (status, body) = http_get(&format!("http://127.0.0.1:{}/__stats", self.port), None)
            .unwrap_or((0, String::new()));
        serde_json::from_str(&body).unwrap_or(json_stats_fallback(status))
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        // Graceful: close stdin/stdout (the server exits on EOF-free
        // SIGTERM through its own handle); then reap.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn json_stats_fallback(status: u16) -> Value {
    Value::String(format!("gateway stats unavailable (http {status})"))
}

// ---------------------------------------------------------------------------
// Tiny HTTP GET/POST (loopback server-API asserts)
// ---------------------------------------------------------------------------

/// Minimal standard base64 (no crate dep in the test env).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let triple = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(triple >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[triple as usize & 63] as char } else { '=' });
    }
    out
}

pub fn http_get(url: &str, password: Option<&str>) -> Option<(u16, String)> {
    http_request("GET", url, None, password)
}

pub fn http_post(url: &str, body: &str, password: Option<&str>) -> Option<(u16, String)> {
    http_request("POST", url, Some(body), password)
}

fn http_request(
    method: &str,
    url: &str,
    body: Option<&str>,
    password: Option<&str>,
) -> Option<(u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host_port, path) = rest.split_once('/')?;
    let mut stream = TcpStream::connect(host_port).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut request =
        format!("{method} /{path} HTTP/1.1\r\nhost: {host_port}\r\nconnection: close\r\n");
    if let Some(password) = password {
        let encoded = base64_encode(format!("opencode:{password}").as_bytes());
        request.push_str(&format!("authorization: Basic {encoded}\r\n"));
    }
    if let Some(body) = body {
        request.push_str(&format!(
            "content-type: application/json\r\ncontent-length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).ok()?;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let status = response.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok())?;
    let payload = response.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
    Some((status, payload))
}

// ---------------------------------------------------------------------------
// The TUI child on its own PTY
// ---------------------------------------------------------------------------

pub struct TuiProcess {
    pub child: Child,
    master: File,
    pub raw: Arc<Mutex<Vec<u8>>>,
    vt: Arc<Mutex<vt100::Parser>>,
}

impl TuiProcess {
    /// fork a PTY, exec the production binary on its slave (setsid +
    /// TIOCSCTTY — the TUI demands a real controlling terminal).
    pub fn spawn(sandbox: &Sandbox, cols: u16, rows: u16) -> TuiProcess {
        let bin = opencode_bin();
        assert!(bin.exists(), "production binary missing: {bin:?} (build it first)");

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
        let slave_c = CString::new(slave_path).expect("slave path nul");
        let slave_fd = unsafe { libc::open(slave_c.as_ptr(), libc::O_RDWR) };
        assert!(slave_fd >= 0, "open pty slave");
        unsafe {
            let winsize = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
            libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
        }

        let mut command = sandbox.command(&bin);
        command.stdin(unsafe { Stdio::from_raw_fd(libc::dup(slave_fd)) });
        command.stdout(unsafe { Stdio::from_raw_fd(libc::dup(slave_fd)) });
        command.stderr(unsafe { Stdio::from_raw_fd(libc::dup(slave_fd)) });
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
                Ok(())
            });
        }
        let child = command.spawn().expect("spawn opencode-rust");
        unsafe {
            libc::close(slave_fd);
        }

        let raw: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let vt: Arc<Mutex<vt100::Parser>> = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let reader = master.try_clone().expect("clone master");
        let feed_raw = Arc::clone(&raw);
        let feed_vt = Arc::clone(&vt);
        let start = Instant::now();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let _ = start;
                        feed_vt.lock().expect("vt").process(&chunk[..count]);
                        feed_raw.lock().expect("raw").extend_from_slice(&chunk[..count]);
                    }
                }
            }
        });
        TuiProcess { child, master, raw, vt }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).expect("write keys");
        self.master.flush().expect("flush keys");
    }

    pub fn raw_len(&self) -> usize {
        self.raw.lock().expect("raw").len()
    }

    /// Wait for `needle` in the raw stream at/after byte `from`.
    pub fn wait_for(&self, needle: &[u8], from: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let raw = self.raw.lock().expect("raw");
            if raw[from.min(raw.len())..].windows(needle.len()).any(|w| w == needle) {
                return true;
            }
            drop(raw);
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// The current screen text (rows joined by \n) from the VT parser.
    pub fn screen_text(&self) -> String {
        let parser = self.vt.lock().expect("vt");
        let screen = parser.screen();
        let mut out = String::new();
        for row in screen.rows(0, u16::MAX) {
            out.push_str(&row);
            out.push('\n');
        }
        out
    }

    /// The app's exit footprint in the raw stream.
    pub fn exited_alt_screen(&self, from: usize) -> bool {
        const LEAVE: &[u8] = b"\x1b[?1049l";
        self.wait_for(LEAVE, from, Duration::from_secs(1))
    }

    /// The graceful exit chord (adapted from the vt lane's CI-hardened
    /// chord: escapes clear dialogs, ctrl+u clears a dirty draft, then
    /// the double ctrl+c).
    pub fn exit_chord(&mut self) {
        self.write(b"\x1b\x1b\x15\x03\x03");
    }

    /// Graceful teardown: chord, wait, escalate only through the OWN
    /// child handle if the app is wedged.
    pub fn shutdown(&mut self, grace: Duration) {
        self.exit_chord();
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => return,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// The managed server (production boot step 1)
// ---------------------------------------------------------------------------

pub struct ManagedServer {
    pub url: String,
    pub password: Option<String>,
}

/// `opencode-rust service start` under the sandbox env — the exact
/// production managed-daemon path (`lifecycle::ensure`); the TUI then
/// attaches to the same registration.
pub fn service_start(sandbox: &Sandbox) -> ManagedServer {
    // Defensive pre-clean: a previous run's daemon on this sandbox's
    // fixed port would wedge `ensure` (its own graceful stop path).
    service_stop(sandbox);
    let mut child = sandbox
        .command(&opencode_bin())
        .args(["service", "start"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn service start");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut url = None;
    let mut early_exit: Option<String> = None;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A clean fast exit prints the endpoint URL on stdout.
                let out = child.stdout.take().map(|mut s| {
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf);
                    buf
                });
                let err = child.stderr.take().map(|mut s| {
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf);
                    buf
                });
                if status.success() {
                    if let Some(found) = out
                        .as_deref()
                        .and_then(|text| text.lines().find(|l| l.starts_with("http://")))
                    {
                        url = Some(found.trim().to_owned());
                    }
                }
                early_exit =
                    Some(format!("service start exited ({status}): out={out:?} err={err:?}"));
                break;
            }
            Ok(None) => {
                // Registration file already tells us the endpoint once the
                // daemon registered (before the probe even finishes).
                let reg = sandbox.registration();
                if reg.is_object() {
                    if let Some(found) = reg.get("url").and_then(|v| v.as_str()) {
                        if !found.is_empty() {
                            url = Some(found.to_owned());
                            break;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    // Let `service start` finish its probe (prints the URL, exits 0).
    let _ = child.wait();
    let url = url.unwrap_or_else(|| {
        panic!(
            "the managed server never registered; {}",
            early_exit.as_deref().unwrap_or("no early exit observed")
        )
    });
    let password =
        sandbox.registration().get("password").and_then(|v| v.as_str()).map(str::to_owned);
    ManagedServer { url: url.trim_end_matches('/').to_owned(), password }
}

/// `opencode-rust service stop` under the sandbox env — the daemon's
/// own graceful stop path.
pub fn service_stop(sandbox: &Sandbox) {
    let _ = sandbox.command(&opencode_bin()).args(["service", "stop"]).output();
}

/// The home wordmark's opening block (boot-settled marker, same needle
/// family as the vt lane).
pub const WORDMARK: &[u8] = "█▀▀█".as_bytes();
