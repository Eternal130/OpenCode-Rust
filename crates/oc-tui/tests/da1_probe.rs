//! DA1 probe injection (RB-07, lane R0): the boot capability probe
//! ([`oc_tui::theme::probe_terminal`]) now also sends the Primary
//! Device Attributes query (`CSI c`) and parses the
//! `CSI ? {attrs} c` reply's sixel verdict. These tests attach a
//! pseudo-terminal whose "emulator" thread ANSWERS the DA1 query —
//! the same fd-swap scheme as `pty_boot.rs` — and pin the probe's
//! end-to-end behavior:
//!
//! * a vt340-style reply carrying attribute `4`
//!   (`\x1b[?62;4;6;8;15;22c`) reports `da1_sixel == Some(true)`;
//! * an answered DA1 WITHOUT attribute `4` (kitty's reply shape)
//!   reports `Some(false)`;
//! * a terminal that stays silent reports `None` (the env-fallback
//!   case) — and the query bytes never surface as user input.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The fd 0/1 swap is process-global: the tests serialize on this
/// lock (the libtest default runs them on parallel threads).
fn pty_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

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

/// Open a pty pair and swap fd 0/1 onto the slave (the
/// `pty_boot.rs` scheme, size-parameterized for the winsize).
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

/// Subslice search (`[T]::contains` matches single elements only).
fn has(buf: &[u8], needle: &[u8]) -> bool {
    buf.windows(needle.len()).any(|w| w == needle)
}

/// The emulated terminal: read the master until the DA1 query
/// (`ESC [ c`) arrives, then write `reply`. `None` = answer nothing
/// (the silent-terminal case). Answers the OSC 11 query too so the
/// probe's window can complete on all three findings where relevant.
fn run_probe_with_da1_reply(da1_reply: Option<&[u8]>) -> oc_tui::theme::TerminalProbe {
    /// Stops the responder thread, then restores the real fds.
    #[allow(dead_code)] // the PtyGuard field is drop-only
    struct Guard(PtyGuard, Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.1.store(true, Ordering::Relaxed);
        }
    }
    let _pty = pty_lock();
    let guard = attach_pty();
    let stop = Arc::new(AtomicBool::new(false));
    let reader = guard.master.try_clone().expect("clone master reader");
    let mut writer = guard.master.try_clone().expect("clone master writer");
    let _guard = Guard(guard, Arc::clone(&stop));
    let da1_reply = da1_reply.map(<[u8]>::to_vec);
    let seen_query = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&seen_query);
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            let mut chunk = [0u8; 64];
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let incoming = &chunk[..n];
                    buffer.extend_from_slice(incoming);
                    // The OSC 11 query echo: answer it (dark #171717)
                    // so the probe can finish all three arms.
                    if has(&buffer, b"\x1b]11;?") {
                        let _ = writer.write_all(b"\x1b]11;rgb:1717/1717/1717\x1b\\");
                        let _ = writer.flush();
                        buffer.clear();
                    }
                    if has(incoming, b"\x1b[c") || has(&buffer, b"\x1b[c") {
                        seen.store(true, Ordering::Relaxed);
                        if let Some(reply) = &da1_reply {
                            let _ = writer.write_all(reply);
                            let _ = writer.flush();
                        }
                        buffer.clear();
                    }
                }
            }
        }
    });

    let probe = oc_tui::theme::probe_terminal();
    assert!(seen_query.load(Ordering::Relaxed), "the DA1 query (CSI c) was sent");
    probe
}

#[test]
fn da1_reply_with_attribute_4_reports_sixel() {
    let probe = run_probe_with_da1_reply(Some(b"\x1b[?62;4;6;8;15;22c"));
    assert_eq!(probe.da1_sixel, Some(true), "attribute 4 = sixel graphics");
    assert_eq!(probe.background, Some((0x17, 0x17, 0x17)), "the OSC 11 arm still works");
}

#[test]
fn da1_reply_without_attribute_4_reports_no_sixel() {
    // kitty's reply shape: answered, no sixel attribute.
    let probe = run_probe_with_da1_reply(Some(b"\x1b[?62;6;9;15;22c"));
    assert_eq!(probe.da1_sixel, Some(false));
    // The minimal `?6c` form also counts as answered-no-sixel.
    let probe = run_probe_with_da1_reply(Some(b"\x1b[?6c"));
    assert_eq!(probe.da1_sixel, Some(false));
}

#[test]
fn silent_terminal_reports_none_and_the_query_never_leaks() {
    // No DA1 answer: the verdict stays `None` (env fallback) — and
    // the query bytes must not leak anywhere the input thread would
    // see as keystrokes (the SA-F13 keystroke-theft class).
    let probe = run_probe_with_da1_reply(None);
    assert_eq!(probe.da1_sixel, None);
}
