//! The TUI's stderr guard: from the moment the TUI owns the screen
//! (alternate screen entered), **every** stderr write lands in
//! `<state>/tui.log` instead of the terminal — subsystem errors
//! (`opencode tui: session create: …`), plugin-host boot lines
//! (`opencode plugin-host: …`, including the piped bun child's
//! stderr, which the supervisor forwards through this process's fd 2)
//! and any other post-boot diagnostics must never splat raw text over
//! the frame (v2's TUI never does).
//!
//! Mechanism: `dup(2)` fd 2 (the real terminal stderr stays reachable
//! through the saved descriptor), then `dup2(log_fd, 2)` so every
//! inherited `eprintln!` — ours and any library's — appends to the
//! log. Dropping the guard restores the real stderr.
//!
//! Panics stay visible: a panic hook writes the panic report to the
//! SAVED descriptor (the real stderr), so a crash still surfaces after
//! the terminal teardown — the redirect only silences diagnostics, not
//! fatal errors.

#![allow(unsafe_code)] // dup/dup2/poll are only reachable through libc FFI

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Serialized install/restore (tests redirect the process-wide fd 2;
/// concurrent installs would interleave dup2s).
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

/// The saved real stderr descriptor while a redirect is active
/// (the panic hook writes here).
static REAL_STDERR: OnceLock<std::fs::File> = OnceLock::new();

/// The active redirect (taken down by [`StderrRedirect::restore`] /
/// the run loop's teardown).
static ACTIVE: Mutex<Option<RawFd>> = Mutex::new(None);

/// Install the redirect for the rest of the TUI process lifetime:
/// append stderr to `log_path`, keep the real stderr on a dup'd fd.
/// Returns `None` when the log cannot be opened (stderr stays real —
/// boot diagnostics are then visible, which is the pre-fix behavior)
/// or when a redirect is already active.
#[must_use]
pub fn install(log_path: &Path) -> Option<StderrRedirect> {
    let _guard = INSTALL_LOCK.lock().ok()?;
    if ACTIVE.lock().ok()?.is_some() {
        return None; // already redirected
    }
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(log_path).ok()?;
    let saved = unsafe { libc::dup(libc::STDERR_FILENO) };
    if saved < 0 {
        return None;
    }
    if unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
        unsafe { libc::close(saved) };
        return None;
    }
    let real = unsafe { std::fs::File::from_raw_fd(saved) };
    // The panic hook keeps its OWN dup; `real` (which owns `saved`)
    // must not close it on scope exit — the guard closes `saved` at
    // restore time.
    let _ = REAL_STDERR.set(real.try_clone().ok()?);
    std::mem::forget(real);
    *ACTIVE.lock().ok()? = Some(saved);
    install_panic_hook();
    // A session header line (fd 2 already points at the log; `file`
    // closes at the end of install, fd 2 keeps the description open).
    let _ = writeln!(
        &file,
        "--- tui session {} ---",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis())
    );
    let _ = file.flush();
    Some(StderrRedirect { saved })
}

/// The installed guard; dropping it restores the real stderr.
pub struct StderrRedirect {
    saved: RawFd,
}

impl StderrRedirect {
    /// Explicit restore (the run loop tears down before leaving the
    /// alternate screen, so anything the teardown prints goes to the
    /// real terminal again).
    pub fn restore(self) {
        drop(self);
    }
}

impl Drop for StderrRedirect {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            if active.is_some_and(|fd| fd == self.saved) {
                unsafe { libc::dup2(self.saved, libc::STDERR_FILENO) };
                unsafe { libc::close(self.saved) };
                *active = None;
            }
        }
    }
}

/// Write panics to the REAL stderr (the saved fd): a crash must stay
/// visible even while diagnostics are redirected to the log. Both the
/// default hook's report and a log copy are emitted.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(real) = REAL_STDERR.get() {
            let mut handle = real
                .try_clone()
                .unwrap_or_else(|_| unsafe { std::fs::File::from_raw_fd(libc::STDERR_FILENO) });
            let _ = writeln!(handle, "opencode tui panicked: {info}");
        }
        previous(info);
    }));
}

/// The `<state>/tui.log` path for the resolved roots
/// (`XDG_STATE_HOME/opencode/tui.log`).
#[must_use]
pub fn log_path() -> std::path::PathBuf {
    oc_util::global_roots::resolve().state.join("tui.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Install → eprintln lands in the log → restore → stderr is real
    /// again. The fd shuffle is serialized against the other tests in
    /// this module through `INSTALL_LOCK` (nextest's process-per-test
    /// keeps the whole affair contained to this process).
    #[test]
    fn redirect_captures_eprintln_and_restores() {
        let dir =
            std::env::temp_dir().join(format!("oc-tui-stderr-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("tui.log");
        let guard = install(&log).expect("install");
        eprintln!("captured line {}", 0x2A);
        let _ = std::io::stderr().flush();
        // The guard must be the only active one.
        assert!(install(&log).is_none(), "second install is rejected");
        guard.restore();
        let contents = std::fs::read_to_string(&log).unwrap();
        assert!(contents.contains("captured line 42"), "log: {contents}");
        // After restore a write goes to the real stderr again (not the
        // log): the log must not grow.
        eprintln!("not captured");
        let _ = std::io::stderr().flush();
        let after = std::fs::read_to_string(&log).unwrap();
        assert!(!after.contains("not captured"), "log: {after}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
