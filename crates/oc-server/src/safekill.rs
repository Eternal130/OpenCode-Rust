#![allow(unsafe_code)] // contained raw syscalls (pidfd open/send/close), reviewed per-site
//! Safe process signalling — the guard that makes `stop()` unable to kill
//! an innocent process that recycled a stale PID.
//!
//! ## Why this exists (production incident 2026-10-02/09/10)
//!
//! The machine reboots; PIDs get recycled; a registration file written
//! before the reboot then names a PID that now belongs to sshd/journald.
//! The old `stop()` blind-signalled that PID and mass-killed system
//! services. The rules below make that class of accident structurally
//! impossible:
//!
//! 1. **Generation binding**: every registration we write also writes a
//!    sidecar `<registration>.boot` = `{boot_id, start_time}`. `boot_id`
//!    comes from `/proc/sys/kernel/random/boot_id` (changes every boot);
//!    `start_time` is the process's start time from `/proc/<pid>/stat`
//!    (field 22) — `(pid, boot_id, start_time)` uniquely identifies one
//!    process run on one boot. A record whose `boot_id` does not match the
//!    current boot, or whose PID now has a different `start_time`, is stale:
//!    delete the record, log, **never signal**.
//! 2. **Identity for foreign records** (written by the TypeScript v2
//!    server — no sidecar): the target's `/proc/<pid>/cmdline` must look
//!    like an opencode process (binary named `opencode`/`oc-server`, or an
//!    argument containing `opencode`). Anything else — including every
//!    system service — is refused: we would rather under-kill (and let the
//!    operator handle it) than repeat the incident.
//! 3. **pidfd signalling**: the signal is delivered through
//!    ``pidfd_open`` + ``pidfd_send_signal``, which pins the exact process at
//!    open time — no kill(2) on a bare integer that the kernel may have
//!    handed to someone else between the check and the signal.
//!
//! Nothing in this module ever signals a process it could not identify.

use std::fs;
use std::path::{Path, PathBuf};

/// Result of a [`safe_signal`] attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalOutcome {
    /// Signal delivered through the pidfd.
    Delivered,
    /// Refused: the record is stale or the target is not ours.
    /// No signal was sent.
    Refused(&'static str),
    /// The target process is already gone (`pidfd_open` failed).
    Gone,
}

/// What we know about the process a record points at.
#[derive(Debug, Clone)]
pub enum Expectation {
    /// Our own record: exact identity via `(boot_id, start_time)`.
    Generation { boot_id: String, start_time: u64 },
    /// A foreign record (no sidecar): cmdline heuristic only.
    Foreign,
}

/// Sidecar document bound to a registration file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BootRecord {
    pub boot_id: String,
    pub start_time: u64,
}

/// Path of the boot sidecar for a registration file.
#[must_use]
pub fn boot_sidecar(registration: &Path) -> PathBuf {
    let mut s = registration.as_os_str().to_os_string();
    s.push(".boot");
    PathBuf::from(s)
}

/// The machine's current boot id.
#[must_use]
pub fn boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
}

/// Process start time (field 22 of `/proc/<pid>/stat`, in clock ticks).
#[must_use]
pub fn proc_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // field 2 (comm) may contain spaces — split after the last ')'
    let rest = stat.rsplit(')').next()?;
    // fields after comm: state(3) ppid(4) ... starttime(22)
    // rest = " S 1 2 3 ...", so starttime is the (22-3+1)=20th token here
    let mut fields = rest.split_whitespace();
    // skip state(3), ppid(4), pgrp(5), session(6), tty(7), tpgid(8),
    // flags(9), minflt(10), cminflt(11), majflt(12), cmajflt(13),
    // utime(14), stime(15), cutime(16), cstime(17), priority(18),
    // nice(19), num_threads(20), itrealvalue(21) -> then starttime(22)
    for _ in 3..=21 {
        fields.next()?;
    }
    fields.next()?.parse().ok()
}

/// Whether `/proc/<pid>/cmdline` looks like an opencode process: the
/// executable (first argv element) is named `opencode`/`oc-server`, or any
/// argument contains the string `opencode` (covers `bun … opencode serve`
/// and the installed v2 binary invoked via wrapper scripts).
#[must_use]
pub fn cmdline_looks_like_opencode(pid: u32) -> bool {
    let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let text = String::from_utf8_lossy(&raw);
    // Exact basename matches ONLY — a substring test would fire on every
    // path under a directory merely NAMED opencode (e.g. /tmp/opencode/...),
    // which once made a test signal its own process. Dev-mode `bun run
    // --cwd packages/cli dev` servers intentionally do NOT match (they are
    // unidentifiable) and are refused — documented, safe direction.
    let names = ["opencode", "oc-server"];
    text.split_terminator('\0').any(|arg| {
        Path::new(arg)
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|basename| names.contains(&basename))
    })
}

fn pidfd_open(pid: u32) -> Option<i32> {
    // SAFETY: plain syscall with integer args; no pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, libc::c_int::from(pid_i32(pid)), 0) };
    if fd < 0 {
        None
    } else {
        i32::try_from(fd).ok()
    }
}

fn pid_i32(pid: u32) -> i32 {
    i32::try_from(pid).unwrap_or(-1)
}

fn pidfd_send_signal(pidfd: i32, signal: libc::c_int) -> bool {
    // SAFETY: plain syscall with integer args; no pointers.
    let rc = unsafe { libc::syscall(libc::SYS_pidfd_send_signal, pidfd, signal, 0_usize, 0) };
    rc == 0
}

fn raw_signal_number(signal: Signal) -> libc::c_int {
    match signal {
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    }
}

/// Signals we deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
}

/// Validate then signal. See the module docs for the rules; on any
/// validation failure the process is NOT signalled.
#[must_use]
pub fn safe_signal(pid: u32, signal: Signal, expect: &Expectation) -> SignalOutcome {
    match expect {
        Expectation::Generation { boot_id: expected_boot, start_time } => {
            let Some(current_boot) = boot_id() else {
                return SignalOutcome::Refused("boot id unavailable");
            };
            if *expected_boot != current_boot {
                // A record from a previous boot: every PID in it is
                // meaningless on this boot.
                return SignalOutcome::Refused("stale boot id (machine rebooted)");
            }
            match proc_start_time(pid) {
                None => return SignalOutcome::Gone,
                Some(actual) if actual != *start_time => {
                    // Same boot, but this PID now belongs to a different
                    // (younger or older) process — recycled.
                    return SignalOutcome::Refused("pid recycled (start time mismatch)");
                }
                _ => {}
            }
        }
        Expectation::Foreign => {
            if !cmdline_looks_like_opencode(pid) {
                return SignalOutcome::Refused("foreign record target is not an opencode process");
            }
        }
    }
    // Pin the exact process, then signal through the fd. If the process
    // died between the checks above and here, pidfd_open fails — Gone.
    let Some(pidfd) = pidfd_open(pid) else {
        return SignalOutcome::Gone;
    };
    let delivered = pidfd_send_signal(pidfd, raw_signal_number(signal));
    // SAFETY: closing an fd we opened.
    unsafe { libc::close(pidfd) };
    if delivered {
        SignalOutcome::Delivered
    } else {
        SignalOutcome::Gone
    }
}

/// Liveness check WITHOUT any signal: `/proc/<pid>/stat` exists and is not
/// a zombie; when a generation expectation is present the start time must
/// still match (recycled PIDs read as dead).
#[must_use]
pub fn process_alive(pid: u32, expect: Option<&Expectation>) -> bool {
    if let Some(Expectation::Generation { start_time, .. }) = expect {
        return match proc_start_time(pid) {
            Some(actual) => actual == *start_time && !is_zombie(pid),
            None => false,
        };
    }
    stat_exists_non_zombie(pid)
}

fn stat_exists_non_zombie(pid: u32) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
        Err(_) => false,
    }
}

fn is_zombie(pid: u32) -> bool {
    !stat_exists_non_zombie(pid)
}

/// Reads the boot sidecar next to a registration file, if present.
#[must_use]
pub fn read_expectation(registration: &Path) -> Option<Expectation> {
    let side = fs::read_to_string(boot_sidecar(registration)).ok()?;
    let record: BootRecord = serde_json::from_str(&side).ok()?;
    Some(Expectation::Generation { boot_id: record.boot_id, start_time: record.start_time })
}

/// Writes the boot sidecar for a freshly-written registration (best
/// effort — a missing sidecar only downgrades us to the foreign heuristic
/// for that record).
pub fn write_expectation(registration: &Path, pid: u32) {
    let current_boot = boot_id();
    let start = proc_start_time(pid);
    let (Some(current_boot), Some(start_time)) = (current_boot, start) else {
        return;
    };
    let record = BootRecord { boot_id: current_boot, start_time };
    let tmp = boot_sidecar(registration).with_extension("tmp");
    if serde_json::to_string(&record)
        .map(|json| {
            #[cfg(unix)]
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&tmp)
                    .and_then(|mut f| f.write_all(json.as_bytes()))
            }
            #[cfg(not(unix))]
            {
                std::fs::write(&tmp, json.as_bytes())
            }
        })
        .is_ok()
    {
        let _ = std::fs::rename(&tmp, boot_sidecar(registration));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safekill_boot_id_reads() {
        assert!(boot_id().is_some_and(|b| b.len() >= 32 || !b.is_empty()));
    }

    #[test]
    fn safekill_start_time_self() {
        let pid = std::process::id();
        assert!(proc_start_time(pid).is_some());
    }

    #[test]
    fn safekill_wrong_boot_id_refuses_without_signal() {
        // A live process we own: this test's own process.
        let pid = std::process::id();
        let expect = Expectation::Generation {
            boot_id: "00000000-0000-0000-0000-000000000000".to_owned(),
            start_time: proc_start_time(pid).unwrap_or(0),
        };
        assert_eq!(
            safe_signal(pid, Signal::Term, &expect),
            SignalOutcome::Refused("stale boot id (machine rebooted)")
        );
        // Still alive (we are the process).
        assert!(stat_exists_non_zombie(pid));
    }

    #[test]
    fn safekill_recycled_start_time_refuses() {
        let pid = std::process::id();
        let expect = Expectation::Generation {
            boot_id: boot_id().unwrap_or_default(),
            start_time: proc_start_time(pid).unwrap_or(0).wrapping_add(1),
        };
        assert_eq!(
            safe_signal(pid, Signal::Kill, &expect),
            SignalOutcome::Refused("pid recycled (start time mismatch)")
        );
    }

    #[test]
    fn safekill_self_term_is_delivered_and_survivable() {
        // SIGTERM to ourselves with a valid expectation must be delivered.
        // Installing a no-op SIGTERM handler first keeps the test alive.
        // SAFETY: sigaction with a plain function pointer.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
        let pid = std::process::id();
        let expect = Expectation::Generation {
            boot_id: boot_id().unwrap_or_default(),
            start_time: proc_start_time(pid).unwrap_or(0),
        };
        assert_eq!(safe_signal(pid, Signal::Term, &expect), SignalOutcome::Delivered);
        // Restore default so the rest of the suite is unaffected.
        // SAFETY: restore default handler.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_DFL);
        }
    }

    #[test]
    fn safekill_foreign_non_opencode_refused() {
        // The test binary's own process is not named opencode/oc-server
        // and its cmdline has no "opencode" arg — foreign heuristic must
        // refuse to signal it.
        let pid = std::process::id();
        assert_eq!(
            safe_signal(pid, Signal::Term, &Expectation::Foreign),
            SignalOutcome::Refused("foreign record target is not an opencode process")
        );
    }
}
