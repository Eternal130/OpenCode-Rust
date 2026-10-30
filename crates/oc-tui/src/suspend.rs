//! G-20(2) — `terminal.suspend` (app.tsx:1119-1129): the dead key
//! the keymap table bound (`ctrl+z`, keymap/table.rs:786) with no
//! dispatch arm. The ref's run body:
//!
//! ```ts
//! renderer.suspend()                          // leave alt-screen, restore cooked mode
//! process.once("SIGCONT", () => renderer.resume())
//! process.kill(0, "SIGTSTP")                  // stop our own process group
//! ```
//!
//! The port splits that into:
//!
//! * [`teardown`]/[`restore`] — the terminal hand-off (the SAME
//!   sequence the `$EDITOR` seam uses: alt-screen off, mouse modes
//!   popped, paste/focus reporting off — raw mode disabled/enabled
//!   around the stop).
//! * [`suspend_cycle`] — the signal discipline ONLY: catch `SIGCONT`
//!   through an async-signal-safe flag, reset `SIGTSTP` to its
//!   default (the stop must actually stop), `kill(0, SIGTSTP)` on
//!   OUR OWN process group (never a foreign pid — the safe-signal
//!   rule; pid 0 is by definition self), then wait for the continue
//!   and restore both dispositions. The ref re-arms the resume
//!   through `process.once("SIGCONT")`; the flag plays that role.
//!
//! Testing: never suspend a foreign process — the unit test forks an
//! OWN child (its own process group via `setpgid`) that runs
//! [`suspend_cycle`] while the parent continues it.

use std::sync::atomic::{AtomicBool, Ordering};

/// The `SIGCONT` latch (`process.once("SIGCONT", …)`). Only touched
/// from the handler and the wait loop.
static CONTINUED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_continue(_signal: i32) {
    CONTINUED.store(true, Ordering::SeqCst);
}

/// `renderer.suspend()`'s terminal half: leave the alternate screen,
/// pop the mouse modes, drop paste/focus reporting — everything the
/// user's shell must not see latched. Call with raw mode ALREADY
/// disabled (the caller owns that around the stop).
pub fn teardown(out: &mut dyn std::io::Write, applied_mouse: bool) {
    // `\x1b[?1049l` — LeaveAlternateScreen (crossterm's queue! needs
    // a Sized writer; the escape is fixed either way).
    let _ = out.write_all(b"\x1b[?1049l");
    let _ = crate::mouse_modes::pop(out, applied_mouse);
    let _ = out.write_all(crate::terminal_proto::PASTE_FOCUS_OFF);
    let _ = out.flush();
}

/// `renderer.resume()`'s terminal half (after the continue): re-enter
/// the alternate screen, re-arm the mouse modes under the SAME config
/// gate, re-enable paste/focus reporting.
pub fn restore(out: &mut dyn std::io::Write, applied_mouse: bool) {
    let _ = out.write_all(b"\x1b[?1049h");
    let _ = crate::mouse_modes::push(out, applied_mouse);
    let _ = out.write_all(crate::terminal_proto::PASTE_FOCUS_ON);
    let _ = out.flush();
}

/// The signal discipline (`process.kill(0, "SIGTSTP")` + the
/// `SIGCONT` resume): stop our OWN process group, return once the
/// job-control continue landed. Only valid on unix.
///
/// # Safety discipline
/// `kill(0, …)` addresses the caller's own process group — never a
/// foreign pid. `SIGTSTP`/`SIGCONT` dispositions are restored before
/// returning.
#[cfg(unix)]
pub fn suspend_cycle() {
    unsafe {
        // The resume latch first (`process.once("SIGCONT")`).
        CONTINUED.store(false, Ordering::SeqCst);
        libc::signal(libc::SIGCONT, on_continue as usize);
        // The stop must actually stop: default `SIGTSTP`.
        libc::signal(libc::SIGTSTP, libc::SIG_DFL);
        // Stop OUR OWN process group (pid 0 = self group — the ref's
        // `process.kill(0, "SIGTSTP")`).
        libc::kill(0, libc::SIGTSTP);
        // Stopped here. Once continued, the latch fires; poll it with
        // a short sleep until it does (nanosleep is
        // async-signal-safe and the stop already happened).
        while !CONTINUED.load(Ordering::SeqCst) {
            let request = libc::timespec { tv_sec: 0, tv_nsec: 10_000_000 };
            let mut remaining = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            libc::nanosleep(&request, &mut remaining);
        }
        // Restore both dispositions (the continue handler is
        // one-shot in the ref; `SIGTSTP` is normally not delivered —
        // raw mode turns ctrl+z into a key).
        libc::signal(libc::SIGCONT, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
pub fn suspend_cycle() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// The terminal hand-off emits the exact byte sequences (mirror
    /// of the `$EDITOR` seam's writer output).
    #[test]
    fn teardown_and_restore_round_trip() {
        let mut down = Vec::new();
        teardown(&mut down, true);
        // The `$EDITOR` seam's order: alt-screen off, mouse pops, paste
        // /focus off.
        assert_eq!(
            down,
            b"\x1b[?1049l\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?1004l"
        );
        let mut up = Vec::new();
        restore(&mut up, true);
        assert_eq!(
            up,
            b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?1004h"
        );
        // The no-mouse config gate pops/pushes nothing for the modes.
        let mut bare_down = Vec::new();
        teardown(&mut bare_down, false);
        assert_eq!(bare_down, b"\x1b[?1049l\x1b[?2004l\x1b[?1004l");
    }

    /// The signal cycle against an OWN child process (its own process
    /// group — the parent's group is never signaled): the child
    /// stops itself, the parent continues it, the child reports the
    /// resume and a clean exit. Marker FILES carry the handshake (the
    /// test harness owns stdout, a pipe would race its banner).
    #[test]
    #[cfg(unix)]
    fn child_suspends_and_resumes() {
        use std::io::Write as _;
        let markers =
            std::env::var("OC_SUSPEND_MARKERS").map(std::path::PathBuf::from).unwrap_or_else(
                |_| std::env::temp_dir().join(format!("oc-suspend-{}", std::process::id())),
            );
        let _ = std::fs::remove_dir_all(&markers);
        std::fs::create_dir_all(&markers).expect("markers dir");
        if std::env::var("OC_SUSPEND_CHILD").is_ok() {
            // CHILD: own process group, then the real cycle. Marker
            // files speak for the stdout the harness owns.
            unsafe {
                libc::setpgid(0, 0);
            }
            std::fs::write(markers.join("stopped"), b"1").expect("stopped marker");
            suspend_cycle();
            std::fs::write(markers.join("resumed"), b"1").expect("resumed marker");
            std::process::exit(0);
        }
        // PARENT: fork the test binary as the child persona (the env
        // var routes the SAME test into its child branch).
        let exe = std::env::current_exe().expect("exe");
        let mut child = std::process::Command::new(exe)
            .args(["suspend::tests::child_suspends_and_resumes", "--exact", "--nocapture"])
            .env("OC_SUSPEND_CHILD", "1")
            .env("OC_SUSPEND_MARKERS", &markers)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn child");
        let pid = child.id() as i32;
        // "stopped" lands before the self-stop (bounded wait).
        let mut announced = false;
        for _ in 0..200 {
            if markers.join("stopped").is_file() {
                announced = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(announced, "child must announce the stop first");
        unsafe {
            // Wait until the child is actually stopped (WUNTRACED poll
            // on our OWN child — allowed), then continue it.
            let mut status = 0;
            loop {
                let waited = libc::waitpid(pid, &mut status, libc::WUNTRACED);
                if waited == pid && libc::WIFSTOPPED(status) {
                    break;
                }
                if waited == pid {
                    panic!("child settled before continue: status {status}");
                }
            }
            libc::kill(pid, libc::SIGCONT);
        }
        let output = child.wait_with_output().expect("wait child");
        assert!(output.status.success(), "child must exit 0 after resume");
        assert!(markers.join("resumed").is_file(), "the resume latch fired");
        let _ = std::fs::remove_dir_all(&markers);
    }
}
