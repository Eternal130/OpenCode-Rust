//! PTY live checks for the beta.8 interaction bugs (round 2):
//!
//! * **`/help` dispatches, not submits** — typing the full command +
//!   Enter in the composer OPENS the help dialog (v2 `onSelect:
//!   command.run`, `autocomplete.tsx:481` → `dialog.replace(<DialogHelp/>)`,
//!   `app.tsx:1076-1083`); the scripted server records ZERO
//!   `/api/session/{id}/prompt` POSTs.
//! * **The tab strip's mouse surface** — two tabs open, a hover +
//!   release on the hovered tab's `✕` (one cell in from its right
//!   edge, `session-tabs.tsx:264`) closes it, and the ` + `
//!   affordance (`tabs.add()` → home route, `session-tabs.tsx:
//!   406-416`) navigates home. SGR mouse sequences drive the real
//!   loop (`EnableMouseCapture` enables 1003/1006).

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// The scripted server: two sessions in history, held-open SSE, and a
/// counter over prompt POSTs (what "/help must NOT send" asserts on).
struct Shared {
    shutdown: Arc<AtomicBool>,
    prompt_posts: Arc<AtomicUsize>,
}

fn spawn_scripted_server() -> (String, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let prompt_posts = Arc::new(AtomicUsize::new(0));
    let shared = Arc::new(Shared {
        shutdown: Arc::clone(&shutdown),
        prompt_posts: Arc::clone(&prompt_posts),
    });
    std::thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking accept");
        while !shared.shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let shared = Arc::clone(&shared);
                    std::thread::spawn(move || handle(socket, &shared));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}"), shutdown, prompt_posts)
}

fn handle(mut socket: std::net::TcpStream, shared: &Shared) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = [0u8; 8192];
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
    socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let path = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
    let _ = &shared;
    if path.starts_with("/api/event") {
        let response =
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n";
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
    if path.ends_with("/prompt") {
        shared.prompt_posts.fetch_add(1, Ordering::SeqCst);
    }
    let body = if path.starts_with("/api/session/") {
        r#"{"cursor":{},"data":[]}"#
    } else if path.starts_with("/api/session") {
        concat!(
            r#"{"cursor":{},"data":["#,
            r#"{"id":"ses_alpha","title":"Alpha","time":{"created":0,"updated":0},"#,
            r#""location":{"directory":"/tmp"},"projectID":"p","cost":0,"#,
            r#""tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}},"#,
            r#"{"id":"ses_beta","title":"Beta","time":{"created":0,"updated":0},"#,
            r#""location":{"directory":"/tmp"},"projectID":"p","cost":0,"#,
            r#""tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}"#,
            r#"]}"#
        )
    } else {
        "{}"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes());
}

struct Boot {
    output: Arc<Mutex<Vec<u8>>>,
}

/// Wait for `needle` in bytes APPENDED after `from` — the stream is
/// cumulative, so "appeared later" needs an offset snapshot.
fn wait_for_after(boot: &Boot, needle: &[u8], from: usize, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = boot.output.lock().expect("lock").clone();
        if data.len() > from && data[from..].windows(needle.len()).any(|window| window == needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn len(boot: &Boot) -> usize {
    boot.output.lock().expect("lock").len()
}

fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-int-{tag}-{}-{}",
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

/// SGR mouse motion (button 3 = no button) and left release. Wire
/// coordinates are 1-BASED and crossterm normalizes to 0-based, so
/// both axes get the +1 here.
fn sgr_motion(col: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<35;{};{}M", col + 1, row + 1).into_bytes()
}

fn sgr_left_up(col: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<0;{};{}m", col + 1, row + 1).into_bytes()
}

#[test]
fn pty_help_slash_opens_the_dialog_and_sends_nothing() {
    let _sandbox = sandbox_env("help");
    let (url, shutdown, prompt_posts) = spawn_scripted_server();
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

    {
        let mut writer = guard.master.try_clone().expect("clone keys");
        let output = Arc::clone(&output);
        std::thread::spawn(move || {
            let probe = || Boot { output: Arc::clone(&output) };
            assert!(
                wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
                "the home wordmark rendered"
            );
            std::thread::sleep(Duration::from_millis(600));
            // Open the history session (one tab).
            let _ = writer.write_all(b"\x18l");
            assert!(
                wait_for_after(&probe(), b"Alpha", len(&probe()), Duration::from_secs(5)),
                "the session list shows the history row"
            );
            let _ = writer.write_all(b"\r");
            std::thread::sleep(Duration::from_millis(700));
            // THE BUG: /help + Enter must open the dialog, not submit.
            let _ = writer.write_all(b"/help\r");
            std::thread::sleep(Duration::from_millis(900));
            // Exit: escape (close the dialog), ctrl+c (empty prompt
            // — app.exit; <leader>q is the queue manager since Wave A).
            let _ = writer.write_all(b"\x1b");
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

    let captured = String::from_utf8_lossy(&boot.output.lock().expect("lock").clone()).into_owned();
    // The dialog rendered: the v2 DialogHelp body copy + the ok
    // button (dialog-help.tsx:31, 41).
    assert!(
        captured.contains("Press ctrl+p to see all available actions and commands"),
        "the help dialog opened: missing hint row"
    );
    assert!(captured.contains("esc/enter"), "the dialog's close hint: dialog-help.tsx:25-27");
    // NOT a prompt send: zero prompt POSTs hit the server.
    assert_eq!(
        prompt_posts.load(Ordering::SeqCst),
        0,
        "/help must execute locally, never POST session.prompt"
    );
}

#[test]
fn pty_mouse_closes_a_tab_and_add_opens_home() {
    let _sandbox = sandbox_env("mouse");
    let (url, shutdown, _prompt_posts) = spawn_scripted_server();
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
    let results: Arc<Mutex<std::collections::HashMap<&'static str, bool>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));

    {
        let mut writer = guard.master.try_clone().expect("clone keys");
        let output = Arc::clone(&output);
        let results = Arc::clone(&results);
        std::thread::spawn(move || {
            let probe = || Boot { output: Arc::clone(&output) };
            results.lock().expect("results").insert(
                "home",
                wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
            );
            std::thread::sleep(Duration::from_millis(600));
            // Open Alpha (first history row).
            let _ = writer.write_all(b"\x18l");
            results.lock().expect("results").insert(
                "list1",
                wait_for_after(&probe(), b"Alpha", len(&probe()), Duration::from_secs(5)),
            );
            let _ = writer.write_all(b"\r");
            std::thread::sleep(Duration::from_millis(700));
            // Open Beta (second row) — TWO tabs. The offset snapshot
            // MUST precede the keystroke: the strip's "Beta" can
            // render while a sleep runs before the check starts.
            let before_dialog2 = len(&probe());
            let _ = writer.write_all(b"\x18l");
            results.lock().expect("results").insert(
                "list2",
                wait_for_after(&probe(), b"Beta", before_dialog2, Duration::from_secs(5)),
            );
            let before_open_beta = len(&probe());
            let _ = writer.write_all(b"\x1b[B\r"); // down + enter
            results.lock().expect("results").insert(
                "two_tabs",
                wait_for_after(&probe(), b"Beta", before_open_beta, Duration::from_secs(5)),
            );
            // Let the two-tab frame settle before the mouse sequence.
            std::thread::sleep(Duration::from_millis(900));
            // Snapshot the stream length: everything asserted next must
            // happen AFTER the two-tab state.
            let mark = len(&probe());
            // Hover Beta (the active, second tab: slots 0..32, 32..64
            // at 80 cols) and release on its ✕ at 32+32−2 = 62.
            let _ = writer.write_all(&sgr_motion(62, 0));
            std::thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(&sgr_left_up(62, 0));
            std::thread::sleep(Duration::from_millis(700));
            // One tab left: the ` + ` affordance moved to col 32..35 —
            // a release at 33 only navigates home in the ONE-tab
            // geometry (in the two-tab geometry col 33 is Beta's body
            // and merely selects it).
            let _ = writer.write_all(&sgr_motion(33, 0));
            std::thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(&sgr_left_up(33, 0));
            results.lock().expect("results").insert(
                "plus_home",
                wait_for_after(&probe(), "█▀▀█".as_bytes(), mark, Duration::from_secs(5)),
            );
            // Exit (unconditional — the run loop must always stop):
            // ctrl+c on the empty prompt; <leader>q is the queue
            // manager's chord since Wave A.
            std::thread::sleep(Duration::from_millis(300));
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

    let results = results.lock().expect("results");
    if results.values().any(|value| !value) {
        let captured =
            String::from_utf8_lossy(&boot.output.lock().expect("lock").clone()).into_owned();
        std::fs::write("/tmp/opencode/pty-mouse-debug.txt", &captured).expect("dump");
    }
    for stage in ["home", "list1", "list2", "two_tabs", "plus_home"] {
        assert!(
            results.get(stage).copied().unwrap_or(false),
            "stage {stage} failed — stages: {results:?}"
        );
    }
}
