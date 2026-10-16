//! PTY live check for the open-tabs model (the beta.7 tab bugs): a
//! scripted FakeLlm-style server answers the boot endpoints while the
//! REAL [`oc_tui::app::run_with_size`] loop runs over a pty, and the
//! test drives actual keystrokes:
//!
//! * default boot lands on HOME (wordmark + centered composer) with
//!   history present but ZERO tabs — no restored "Untitled session"
//!   tab, no all-history strip;
//! * `<leader>l` (session list) → Enter opens the history row: the
//!   strip shows exactly ONE tab plus the ` + ` affordance;
//! * `<leader>w` (tab close) empties the strip and returns home.
//!
//! The server: `/api/event` is a held-open SSE stream; `/api/session`
//! serves one session ("Alpha"); per-session `message.list` is empty.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        let winsize = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
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

/// The scripted server (FakeLlm-style): one thread accepts
/// connections; JSON endpoints close after their response, the SSE
/// stream is held open until the peer disconnects.
fn spawn_scripted_server() -> (String, Arc<AtomicBool>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking accept");
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let _ = socket.set_nodelay(true);
                    std::thread::spawn(move || handle(socket));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}"), shutdown)
}

fn handle(mut socket: std::net::TcpStream) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = [0u8; 8192];
    let mut head = String::new();
    // Block until the full request head arrives — a pooled connection
    // may be accepted BEFORE the client sends its request bytes (an
    // early would-block here would answer the wrong endpoint).
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
    socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let path = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
    if path.starts_with("/api/event") {
        // Held-open SSE stream: headers, then park until the peer
        // closes (read returns 0/Err).
        let response = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n";
        if socket.write_all(response).is_err() {
            return;
        }
        let mut sink = [0u8; 256];
        loop {
            match socket.read(&mut sink) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    }
    let body: String = if path.starts_with("/api/session/") {
        // message.list for the opened tabs: an empty transcript, or the
        // dogfood2 real-shape per-session rows when that mode is armed.
        if REAL_SHAPE.load(Ordering::SeqCst) {
            let id = path
                .trim_start_matches("/api/session/")
                .split(['/', '?'])
                .next()
                .unwrap_or_default();
            real_shape_messages(id)
        } else {
            r#"{"cursor":{},"data":[]}"#.to_owned()
        }
    } else if path.starts_with("/api/session") {
        // session.list: exactly ONE session in history — or the dogfood2
        // real shape (many sessions, all in OTHER directories).
        if REAL_SHAPE.load(Ordering::SeqCst) {
            real_shape_session_list(&real_shape_fixture())
        } else {
            concat!(
                r#"{"cursor":{},"data":["#,
                r#"{"id":"ses_alpha","title":"Alpha","time":{"created":0,"updated":0},"#,
                r#""location":{"directory":"/tmp"},"projectID":"p","cost":0,"#,
                r#""tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}"#,
                r#"]}"#
            )
            .to_owned()
        }
    } else {
        "{}".to_owned()
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes());
}

fn wait_for(boot: &Boot, needle: &[u8], timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = boot.output.lock().expect("lock").clone();
        if data.windows(needle.len()).any(|w| w == needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn snapshot(boot: &Boot) -> String {
    String::from_utf8_lossy(&boot.output.lock().expect("lock").clone()).into_owned()
}

struct Boot {
    output: Arc<Mutex<Vec<u8>>>,
}

fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-tabs-{tag}-{}-{}",
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

/// The shared boot/open/close choreography. The strip renders through
/// the opentui-solid tree — the only renderer since the M1 retirement
/// (the `OC_TABS_OPENTUI` flag and its hand-rolled fallback are
/// gone).
fn pty_boot_open_close_body() {
    let _sandbox = sandbox_env("live");
    let (url, shutdown) = spawn_scripted_server();
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
    let boot = Boot { output: Arc::clone(&output) };

    // The keystroke choreography beside the loop.
    {
        let mut writer = guard.master.try_clone().expect("clone keys");
        std::thread::spawn(move || {
            let probe = || Boot { output: Arc::clone(&output) };
            // (a) Default boot: the home wordmark appears.
            assert!(
                wait_for(&probe(), "█▀▀█".as_bytes(), Duration::from_secs(5)),
                "the home wordmark rendered"
            );
            std::thread::sleep(Duration::from_millis(600));
            // (b) <leader>l: the session-list dialog shows the history
            // row (never a strip of every session).
            let _ = writer.write_all(b"\x18l");
            assert!(
                wait_for(&probe(), b"Alpha", Duration::from_secs(5)),
                "the dialog lists the history session"
            );
            // (c) Enter: open it — ONE tab.
            let _ = writer.write_all(b"\r");
            std::thread::sleep(Duration::from_millis(700));
            // (d) <leader>w: close the tab — back home, strip empty.
            let _ = writer.write_all(b"\x18w");
            std::thread::sleep(Duration::from_millis(700));
            // Exit: ctrl+c (clear), ctrl+c again (exit — <leader>q
            // is the queue manager's chord since Wave A).
            let _ = writer.write_all(b"\x03");
            std::thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(b"\x03");
        });
    }

    let client = oc_client::Client::new(&url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let finished = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(20),
            oc_tui::app::run_with_size(client, Some((80, 24)), oc_tui::app::BootArgs::default()),
        )
        .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => panic!("the TUI did not exit in time: {elapsed}"),
    }
    shutdown.store(true, Ordering::SeqCst);
    drop(guard);

    // Frame assertions over the captured stream: after the boot the
    // wordmark painted (home), the tab opened (title + add affordance
    // in ONE strip row), and the close returned home.
    let captured = snapshot(&boot);
    assert!(captured.contains("█▀▀█"), "the home wordmark painted");
    assert!(captured.contains("Alpha"), "the opened tab's title painted");
    assert!(captured.contains(" + "), "the add affordance beside the one tab");
    // The strip carried exactly one tab: no overflow markers (`‹n` /
    // `n›`) and no second title ever painted.
    assert!(!captured.contains("›"), "no overflow marker for a single tab");
    assert!(!captured.contains("Untitled session"), "no untitled restored tab");
}

#[test]
fn pty_boot_home_then_one_tab_open_then_close() {
    pty_boot_open_close_body();
}

/// Lane M1 (换血 pilot): the opentui-solid strip path's POINTER cycle —
/// hover reveals the `✕`, the ` + ` add navigates home, the `✕` click
/// closes the tab — over the real app loop on a pty, driven with SGR
/// mouse sequences (`session-tabs.tsx:1652-1754` + `1768-1796`; the
/// routing bridge stays `app.rs` → `strip_hit` → the store handlers).
///
/// Geometry at 80 cols with ONE tab ("Alpha"): the adaptive layout
/// gives the tab `min(77, 32) = 32` columns (x0..32), the ` + `
/// affordance follows directly (x32..35, `+` glyph at 33 — the zone is
/// glyph-tight, corpus2 #28), and the hovered tab's `✕` sits one cell
/// in from its right edge (x30, `session-tabs.tsx:264`).
#[test]
fn pty_solid_strip_hover_add_close_cycle() {
    let _sandbox = sandbox_env("solid-mouse");
    let (url, shutdown) = spawn_scripted_server();
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
    let boot = Boot { output: Arc::clone(&output) };

    // SGR mouse helpers (1006 encoding: button 0 press/release,
    // 35 = button-motion with no button).
    let motion = |col: u8| format!("\x1b[<35;{col};1M");
    let press = |col: u8| format!("\x1b[<0;{col};1M");
    let release = |col: u8| format!("\x1b[<0;{col};1m");
    let count_wordmark = |boot: &Boot| -> usize { snapshot(boot).matches("█▀▀█").count() };
    let wait_wordmark_count = move |boot: &Boot, want: usize| -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if count_wordmark(boot) >= want {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };

    {
        let mut writer = guard.master.try_clone().expect("clone keys");
        std::thread::spawn(move || {
            let probe = || Boot { output: Arc::clone(&output) };
            // (a) Boot: home wordmark (occurrence 1).
            assert!(
                wait_for(&probe(), "█▀▀█".as_bytes(), Duration::from_secs(5)),
                "the home wordmark rendered"
            );
            std::thread::sleep(Duration::from_millis(600));
            // (b) Open the history session: ONE tab.
            let _ = writer.write_all(b"\x18l");
            assert!(wait_for(&probe(), b"Alpha", Duration::from_secs(5)), "the dialog lists Alpha");
            let _ = writer.write_all(b"\r");
            std::thread::sleep(Duration::from_millis(800));
            // (c) HOVER the tab body (row 0, col 5): the ✕ reveals.
            let _ = writer.write_all(motion(5).as_bytes());
            assert!(
                wait_for(&probe(), "✕".as_bytes(), Duration::from_secs(3)),
                "hover reveals the close mark"
            );
            // (d) Click the ` + ` affordance (glyph cell 33): add →
            // the home route repaints (wordmark occurrence 2).
            let _ = writer.write_all(press(33).as_bytes());
            let _ = writer.write_all(release(33).as_bytes());
            assert!(
                wait_wordmark_count(&probe(), 2),
                "the + affordance navigated home (wordmark repainted)"
            );
            // (e) Reopen the session (the tab persists), hover again.
            let _ = writer.write_all(b"\x18l");
            assert!(wait_for(&probe(), b"Alpha", Duration::from_secs(5)), "the dialog again");
            let _ = writer.write_all(b"\r");
            std::thread::sleep(Duration::from_millis(800));
            let _ = writer.write_all(motion(5).as_bytes());
            assert!(
                wait_for(&probe(), "✕".as_bytes(), Duration::from_secs(3)),
                "hover reveals the close mark again"
            );
            // (f) Click the ✕ (col 30): the tab closes → home paints
            // once more (wordmark occurrence 3).
            let _ = writer.write_all(press(30).as_bytes());
            let _ = writer.write_all(release(30).as_bytes());
            assert!(
                wait_wordmark_count(&probe(), 3),
                "the ✕ click closed the tab and returned home"
            );
            // Exit.
            let _ = writer.write_all(b"\x03");
            std::thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(b"\x03");
        });
    }

    let client = oc_client::Client::new(&url).expect("client");
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
        Err(elapsed) => panic!("the TUI did not exit in time: {elapsed}"),
    }
    shutdown.store(true, Ordering::SeqCst);
    drop(guard);

    let captured = snapshot(&boot);
    assert!(captured.contains("✕"), "the hovered tab revealed its close mark");
    assert!(captured.contains("Alpha"), "the tab painted before the close");
}

// ---------------------------------------------------------------------------
// dogfood2 — the real-DB restored-tabs shape (home boot with tabs the
// store has NOT loaded, sessions in OTHER directories, none for the cwd)
// ---------------------------------------------------------------------------

/// The dogfood2 fixture builder — SYNTHETIC data shaped like the user's
/// real DB (the copied DB itself is never committed): MANY sessions
/// across OTHER directories (none for the TUI's cwd), plus the port's
/// persisted tabs document (`<state>/opencode-rust/stable/tui/tabs.json`,
/// the `stable` channel, version-stamped) whose cwd scope restores two
/// tabs referencing not-yet-loaded sessions — the exact boot shape of
/// the user's ~ run (v2 restores through `useStorage` the same way).
struct RealShapeFixture {
    /// (session id, directory, title) — every directory != the cwd.
    sessions: Vec<(String, String, String)>,
    /// (sessionID, title) restored for the RUN cwd's scope.
    restored: Vec<(String, String)>,
}

fn real_shape_fixture() -> RealShapeFixture {
    let mut sessions: Vec<(String, String, String)> = (0..24)
        .map(|index| {
            (
                format!("ses_shape_{index:02}"),
                format!("/opt/synth-{index}"),
                format!("Synth Session {index}"),
            )
        })
        .collect();
    // The restored tabs reference two of them (like the real tabs.json:
    // sessions of /opt/lobechat + /opt/new-api restored into ~).
    sessions[7].2 = "Synth Restored Seven".to_owned();
    sessions[11].2 = "Synth Restored Eleven".to_owned();
    RealShapeFixture {
        restored: vec![
            ("ses_shape_07".to_owned(), "Synth Restored Seven".to_owned()),
            ("ses_shape_11".to_owned(), "Synth Restored Eleven".to_owned()),
        ],
        sessions,
    }
}

/// The session-list body the scripted server serves (the "many sessions,
/// none for the cwd" half of the shape).
fn real_shape_session_list(fixture: &RealShapeFixture) -> String {
    let rows: Vec<String> = fixture
        .sessions
        .iter()
        .map(|(id, directory, title)| {
            format!(
                r#"{{"id":"{id}","title":"{title}","time":{{"created":0,"updated":0}},
                     "location":{{"directory":"{directory}"}},"projectID":"p","cost":0,
                     "tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}}"#
            )
        })
        .collect();
    format!(r#"{{"cursor":{{}},"data":[{}]}}"#, rows.join(","))
}

/// One distinctive user row per session, so the switched route paints an
/// unambiguous transcript.
fn real_shape_messages(session_id: &str) -> String {
    format!(
        r#"{{"cursor":{{}},"data":[{{"type":"user","id":"msg_{sid}","text":"SYNTH TRANSCRIPT {sid}","time":{{"created":1}}}}]}}"#,
        sid = session_id
    )
}

/// Persist the restored tabs document into the sandbox state root (the
/// port's own channel + version stamp; the cwd scope keys the RUN cwd).
fn seed_restored_tabs(restored: &[(String, String)], state_root: &std::path::Path) {
    let cwd = std::env::current_dir().expect("cwd").display().to_string();
    let tabs: Vec<String> = restored
        .iter()
        .map(|(id, title)| format!(r#"{{"sessionID":"{id}","title":"{title}"}}"#))
        .collect();
    let document = format!(
        r#"{{"version":2,"global":{{"tabs":[],"unread":{{}}}},"cwd":{{"{cwd}":{{"tabs":[{}],"unread":{{}}}}}}}}"#,
        tabs.join(",")
    );
    let dir = state_root.join("opencode-rust").join("stable").join("tui");
    std::fs::create_dir_all(&dir).expect("state channel dir");
    std::fs::write(dir.join("tabs.json"), document).expect("seed tabs.json");
}

/// The scripted server in real-shape mode (the shared `handle` reads
/// this): the many-session list + per-session transcripts.
static REAL_SHAPE: AtomicBool = AtomicBool::new(false);

fn spawn_real_shape_server() -> (String, Arc<AtomicBool>) {
    REAL_SHAPE.store(true, Ordering::SeqCst);
    spawn_scripted_server()
}

/// The last explicit cursor-position escape (`CSI row;col H`) in the
/// captured stream — the hardware cursor's cell (the app positions it
/// exactly once per frame, at the caret).
fn last_cursor_cell(bytes: &[u8]) -> Option<(usize, usize)> {
    let text = bytes.as_ref();
    let mut best: Option<(usize, usize)> = None;
    let mut index = 0;
    while let Some(escape) = text[index..].iter().position(|&b| b == 0x1b) {
        let at = index + escape;
        let tail = &text[at + 1..];
        if tail.first() == Some(&b'[') {
            if let Some(end) = tail.iter().position(|&b| b == b'H') {
                let params = &tail[1..end];
                if let Ok(text) = std::str::from_utf8(params) {
                    if let Some((row, col)) = text.split_once(';') {
                        if let (Ok(row), Ok(col)) = (row.parse::<usize>(), col.parse::<usize>()) {
                            best = Some((row, col));
                        }
                    }
                }
            }
        }
        index = at + 1;
    }
    best
}

/// dogfood2 bugs 1 + 2 over the real app loop: a home boot with restored
/// tabs referencing not-yet-loaded sessions (the sanitized user shape) —
/// the strip is a POINTER surface on home (v2 app.tsx:1353-1355 + 576-579
/// `tabsAvailable`, never session-gated; `select()` = route.navigate,
/// context/session-tabs.tsx:393-396), and the hardware cursor sits ON the
/// composer's input row under the strip (home lays out in height-1, the
/// caret rides the shifted subtree).
#[test]
fn pty_restored_tabs_home_click_switches_and_cursor_on_composer_row() {
    let state_root = sandbox_env("realshape");
    let fixture = real_shape_fixture();
    seed_restored_tabs(&fixture.restored, &state_root.join(".local/state"));
    let (url, shutdown) = spawn_real_shape_server();
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
    let boot = Boot { output: Arc::clone(&output) };
    let click_cursor: Arc<Mutex<Option<(usize, usize)>>> = Arc::new(Mutex::new(None));

    // The choreography: boot (home + restored strip), parse the cursor,
    // SGR-click tab 1's body, expect the session switch, exit.
    {
        let mut writer = guard.master.try_clone().expect("clone keys");
        let output = Arc::clone(&output);
        let click_cursor = Arc::clone(&click_cursor);
        std::thread::spawn(move || {
            let probe = move || Boot { output: Arc::clone(&output) };
            // (a) Home painted WITH the restored strip: the wordmark and
            // the restored title share the boot frame.
            assert!(
                wait_for(&probe(), "█▀▀█".as_bytes(), Duration::from_secs(8)),
                "the home wordmark rendered"
            );
            assert!(
                wait_for(&probe(), b"Synth Restored Seven", Duration::from_secs(5)),
                "the restored tab's title leads the home strip"
            );
            assert!(
                wait_for(&probe(), b"Synth Restored Eleven", Duration::from_secs(3)),
                "the second restored tab paints beside it"
            );
            std::thread::sleep(Duration::from_millis(700));
            // (b) The hardware cursor BEFORE any click: the composer
            // input row under the strip (row 15 in 1-based SGR terms at
            // 80x24 — strip row 1, input row 15; the home unit test
            // pins the row == input-line invariant).
            let captured = snapshot(&probe());
            *click_cursor.lock().expect("lock") = last_cursor_cell(captured.as_bytes());
            // (c) The click: SGR press+release at col 5 (1-based; tab 1
            // spans cols 1..32), row 1 (the strip) — selects tab 1.
            let _ = writer.write_all(b"\x1b[<0;5;1M");
            std::thread::sleep(Duration::from_millis(150));
            let _ = writer.write_all(b"\x1b[<0;5;1m");
            // (d) The route switched: the restored session's transcript
            // paints (the boot fetch hydrated it; only the session route
            // renders it).
            assert!(
                wait_for(&probe(), b"SYNTH TRANSCRIPT ses_shape_07", Duration::from_secs(5)),
                "the click switched into the restored session"
            );
            std::thread::sleep(Duration::from_millis(300));
            // Exit: ctrl+c (clear), ctrl+c again.
            let _ = writer.write_all(b"\x03");
            std::thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(b"\x03");
        });
    }

    let client = oc_client::Client::new(&url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let finished = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(30),
            oc_tui::app::run_with_size(client, Some((80, 24)), oc_tui::app::BootArgs::default()),
        )
        .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => panic!("the TUI did not exit in time: {elapsed}"),
    }
    shutdown.store(true, Ordering::SeqCst);
    drop(guard);

    // The cursor row at the home boot: ON the composer input row (the
    // strip occupies row 1; the input row is 15), never the strip row
    // and never a row above the composer (dogfood2 bug 2).
    let cursor = click_cursor.lock().expect("lock").expect("a cursor position painted");
    assert_eq!(cursor.0, 15, "the hardware cursor sits on the composer input row (1-based)");
    assert_ne!(cursor.0, 1, "never on the strip row");
    // And the switch evidence survived to the end of the stream.
    let captured = snapshot(&boot);
    assert!(captured.contains("SYNTH TRANSCRIPT ses_shape_07"), "the session route painted");
}
