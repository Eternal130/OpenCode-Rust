//! PTY boot regression (wave-6): the TUI booted inside a
//! pseudo-terminal whose emulator ANSWERS the OSC 11 background query
//! — the class of terminal the golden harness never emulated (the
//! reported beta.2 tmux regression). Drives the real
//! [`oc_tui::app::run_with_size`] loop over a pty (fd 0/1 swapped to
//! the slave) and asserts:
//!
//! * the query reply NEVER surfaces as user input (`rgb:` must not
//!   appear anywhere in the app's output — the beta.2 leak showed
//!   `11;rgb:1717/1717/1717` typed into the composer);
//! * the answered background resolves the theme mode (the dark
//!   #171717 reply boots the dark theme; a #ffffff reply boots the
//!   light theme — proving the probe actually PARSES the answer, not
//!   just the dark fallback);
//! * the app still composes input and exits cleanly on ctrl+c + the
//!   leader exit chord.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

/// Open a pty pair and swap fd 0/1 onto the slave. The guard restores
/// the real fds on drop.
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
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&slave_path)
        .expect("open slave (read+write — a read-only slave fd makes fd 1 unwritable)");
    // A real terminal has a size: a 0x0 winsize would collapse
    // ratatui's autoresize into an empty buffer.
    unsafe {
        let winsize = libc::winsize { ws_row: 30, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
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

/// One boot session: run the TUI over the pty while a reader thread
/// emulates the answering terminal.
struct Boot {
    output: Arc<Mutex<Vec<u8>>>,
    replied: Arc<AtomicBool>,
    master_writer: File,
}

fn boot_tui(reply: &'static [u8]) -> (PtyGuard, Boot) {
    let guard = attach_pty();
    let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let replied = Arc::new(AtomicBool::new(false));
    let reader_master = guard.master.try_clone().expect("clone master read");
    let writer_master = guard.master.try_clone().expect("clone master write");

    // The terminal emulator: drain the app's output, answer the OSC 11
    // query as soon as it appears, capture everything for assertions.
    {
        let output = Arc::clone(&output);
        let replied = Arc::clone(&replied);
        let mut reply_writer = guard.master.try_clone().expect("clone master reply");
        std::thread::spawn(move || {
            let mut reader = reader_master;
            let mut chunk = [0u8; 4096];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break, // master closed / teardown EIO
                    Ok(count) => {
                        output.lock().expect("output lock").extend_from_slice(&chunk[..count]);
                        pending.extend_from_slice(&chunk[..count]);
                        if !replied.load(Ordering::SeqCst)
                            && pending.windows(6).any(|w| w == b"\x1b]11;?")
                        {
                            replied.store(true, Ordering::SeqCst);
                            let _ = reply_writer.write_all(reply);
                        }
                        if pending.len() > 8192 {
                            pending.drain(..4096);
                        }
                    }
                }
            }
        });
    }
    let boot = Boot { output, replied, master_writer: writer_master };
    (guard, boot)
}

/// Wait until the captured output contains `needle` (or time out).
fn wait_for_output(boot: &Boot, needle: &[u8], timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let data = boot.output.lock().expect("lock").clone();
        if data.windows(needle.len()).any(|w| w == needle) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "oc-tui-pty-{tag}-{}-{}",
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

/// The full boot session: swap in the pty, run the TUI against an
/// unreachable server, answer OSC 11 with `reply`, type, exit.
fn run_session(reply: &'static [u8], tag: &str) -> (Boot, std::path::PathBuf) {
    let sandbox = sandbox_env(tag);
    let (_guard, boot) = boot_tui(reply);

    // The dead server: connection failures retry silently (the SSE
    // backoff); nothing the boot depends on comes from it.
    let client = oc_client::Client::new("http://127.0.0.1:9").expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");

    // The keystroke choreography runs beside the loop (the store's Rc
    // sinks make the run future !Send — it is driven directly below,
    // never spawned).
    {
        let mut boot = Boot {
            output: Arc::clone(&boot.output),
            replied: Arc::clone(&boot.replied),
            master_writer: boot.master_writer.try_clone().expect("writer"),
        };
        std::thread::spawn(move || {
            assert!(
                wait_for_output(&boot, b"\x1b]11;?", std::time::Duration::from_secs(5)),
                "the TUI emitted the OSC 11 query"
            );
            assert!(
                wait_for_output(&boot, b"\x1b[?1049h", std::time::Duration::from_secs(5)),
                "the alternate screen was entered"
            );
            // Let the boot settle (config apply + first draws), then type.
            std::thread::sleep(std::time::Duration::from_millis(400));
            let _ = boot.master_writer.write_all(b"hi");
            std::thread::sleep(std::time::Duration::from_millis(600));
            // ctrl+c (clear the draft) then <leader>q (exit).
            let _ = boot.master_writer.write_all(b"\x03");
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = boot.master_writer.write_all(b"\x18q");
        });
    }

    let finished = runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), async move {
            oc_tui::app::run_with_size(client, Some((80, 30))).await
        })
        .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => panic!("the TUI did not exit within 10s of the exit chord: {elapsed}"),
    }
    (boot, sandbox)
}

/// tmux's own answer (`rgb:1717/1717/1717` = #171717): dark mode, and
/// the reply bytes NEVER appear as input.
#[test]
fn pty_boot_with_answering_terminal_dark() {
    let (boot, sandbox) = run_session(b"\x1b]11;rgb:1717/1717/1717\x1b\\", "dark");
    let output = boot.output.lock().expect("lock").clone();
    // (a) No response bytes ever appear as user input: the query reply
    // carries `rgb:`; a leaked echo would paint it into the frame.
    assert!(
        !String::from_utf8_lossy(&output).contains("rgb:"),
        "the OSC reply leaked into the TUI output"
    );
    // (b) The theme resolved DARK for #171717 (luma 23 ≪ 127.5): the
    // frame background is the dark theme's base #0a0a0a.
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("48;2;10;10;10"),
        "the dark base background was painted (dark mode resolved)"
    );
    let _ = std::fs::remove_dir_all(sandbox);
}

/// A light answer (#ffffff) boots the LIGHT theme — proving the probe
/// parses the reply (the dark fallback would paint 10;10;10).
#[test]
fn pty_boot_with_answering_terminal_light() {
    let (boot, sandbox) = run_session(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\", "light");
    let output = boot.output.lock().expect("lock").clone();
    assert!(
        !String::from_utf8_lossy(&output).contains("rgb:"),
        "the OSC reply leaked into the TUI output"
    );
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("48;2;255;255;255"),
        "the light base background was painted (light mode resolved)"
    );
    let _ = std::fs::remove_dir_all(sandbox);
}
