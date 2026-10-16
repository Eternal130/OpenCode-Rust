//! L1 PTY cycle — the models panel open→select→apply through the
//! REAL loop: `/models` + Enter mounts the panel titled "Select
//! model" (dialog-model.tsx:122 — NOT the palette row's "Switch
//! model"), the unconnected body renders flat rows + the "View all
//! integrations ctrl+a" footer action (:168-176), Enter applies the
//! cursor row (row 0 — `focusCurrent={false}`, :171) staging the
//! selection, and the panel closes.
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

/// One catalog model (free, active) at release `released`.
fn model_json(id: &str, name: &str, provider: &str, released: f64) -> String {
    format!(
        concat!(
            r#"{{"id":"{id}","modelID":"{id}","name":"{name}","providerID":"{provider}","#,
            r#""enabled":true,"status":"active","#,
            r#""cost":[{{"cache":{{"read":0,"write":0}},"input":0,"output":0}}],"#,
            r#""capabilities":{{"input":["text"],"output":["text"],"tools":true}},"#,
            r#""limit":{{"context":200000,"output":8192}},"#,
            r#""time":{{"released":{released}}},"variants":[]}}"#
        ),
        id = id,
        name = name,
        provider = provider,
        released = released
    )
}

/// The scripted server: held-open SSE, a two-model catalog (the
/// free-tier first by release DESC), providers, no integrations.
fn spawn_scripted_server() -> (String, Arc<AtomicBool>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking accept");
        while !server_shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let server_shutdown = Arc::clone(&server_shutdown);
                    std::thread::spawn(move || handle(socket, &server_shutdown));
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

fn handle(mut socket: std::net::TcpStream, shutdown: &Arc<AtomicBool>) {
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok();
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
    let body = if path.starts_with("/api/config") {
        "[]".to_owned()
    } else if path.starts_with("/api/model") {
        format!(
            r#"{{"data":[{},{}],"location":{{"directory":"/tmp"}}}}"#,
            model_json("fledge", "Fledge Alpha", "opencode", 10.0),
            model_json("muse", "Muse Spark", "opencode", 5.0),
        )
    } else if path.starts_with("/api/provider") {
        r#"{"data":[{"activation":"auto","id":"opencode","name":"opencode","package":"opencode"}],"location":{"directory":"/tmp"}}"#
            .to_owned()
    } else if path.starts_with("/api/agent") || path.starts_with("/api/integration") {
        r#"{"data":[],"location":{"directory":"/tmp"}}"#.to_owned()
    } else if path.starts_with("/api/session") {
        r#"{"cursor":{},"data":[]}"#.to_owned()
    } else {
        "{}".to_owned()
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes());
    let _ = shutdown;
}

fn wait_for_after(
    output: &Arc<Mutex<Vec<u8>>>,
    needle: &[u8],
    from: usize,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = output.lock().expect("lock").clone();
        if data.len() > from && data[from..].windows(needle.len()).any(|window| window == needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn len_of(output: &Arc<Mutex<Vec<u8>>>) -> usize {
    output.lock().expect("lock").len()
}

#[test]
fn pty_models_panel_open_select_apply_cycle() {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-models-{}-{}",
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
                    Ok(count) => output.lock().expect("lock").extend_from_slice(&chunk[..count]),
                }
            }
        });
    }

    {
        let mut writer = guard.master.try_clone().expect("clone keys");
        let output = Arc::clone(&output);
        std::thread::spawn(move || {
            let probe = || Arc::clone(&output);
            assert!(
                wait_for_after(&probe(), "█▀▀█".as_bytes(), 0, Duration::from_secs(5)),
                "the home wordmark rendered"
            );
            std::thread::sleep(Duration::from_millis(700));
            // OPEN: /models + Enter mounts the models panel. The
            // frame lands as ONE chunk — every needle waits from the
            // offset snapshotted BEFORE the keys.
            let before = len_of(&probe());
            let _ = writer.write_all(b"/models\r");
            assert!(
                wait_for_after(&probe(), b"Select model", before, Duration::from_secs(5)),
                "the panel mounts titled 'Select model'"
            );
            assert!(
                wait_for_after(&probe(), b"View all integrations", before, Duration::from_secs(3)),
                "the unconnected footer action row"
            );
            assert!(
                wait_for_after(&probe(), b"Fledge Alpha", before, Duration::from_secs(3)),
                "the catalog rows render"
            );
            std::thread::sleep(Duration::from_millis(400));
            // APPLY: Enter commits the cursor row (row 0 = Fledge
            // Alpha, release DESC) and the panel closes.
            let _ = writer.write_all(b"\r");
            std::thread::sleep(Duration::from_millis(900));
            // Exit (ctrl+c on the empty prompt = app.exit).
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

    let captured = String::from_utf8_lossy(&output.lock().expect("lock").clone()).into_owned();
    // The applied selection lands in the composer statusline (the
    // footer's model hint — what a submit WOULD commit).
    let applied = captured.matches("Fledge Alpha").count();
    assert!(applied >= 2, "the selection applied (row + statusline): {applied} occurrences");
}
