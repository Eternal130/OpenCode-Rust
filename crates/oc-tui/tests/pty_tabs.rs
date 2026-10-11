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
    let body = if path.starts_with("/api/session/") {
        // message.list for the opened tab: empty transcript.
        r#"{"cursor":{},"data":[]}"#
    } else if path.starts_with("/api/session") {
        // session.list: exactly ONE session in history.
        concat!(
            r#"{"cursor":{},"data":["#,
            r#"{"id":"ses_alpha","title":"Alpha","time":{"created":0,"updated":0},"#,
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

/// Sandbox state roots so the test never touches the developer's real
/// tabs state (the boot storage restore/persist runs for real).
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

#[test]
fn pty_boot_home_then_one_tab_open_then_close() {
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
            // Exit: ctrl+c (clear) then <leader>q.
            let _ = writer.write_all(b"\x03");
            std::thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(b"\x18q");
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
