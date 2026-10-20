//! Safe process-group signalling for test helpers and spawners.
#![allow(unsafe_code)] // contained raw syscalls (pidfd open/send/close), reviewed per-site
//!
//! Incident 2026-10 rule: never signal a process group id (`kill -TERM
//! -PGID`) blindly — after a reboot (or once a group fully empties) the id
//! can be recycled onto innocent processes. This helper scans `/proc` for
//! the group's members while the group LEADER still has a `/proc` entry
//! (a live-or-zombie leader pins the id against recycling) and signals
//! only members whose command line matches an explicit allowlist, via
//! pidfds.

/// Signals every member of the process group led by `leader` whose
/// `/proc/<pid>/cmdline` contains one of `allowlist` substrings. The scan
/// runs only while the leader has a `/proc` entry (zombies included — the
/// caller is expected to still hold the unreaped `Child`).
#[cfg(target_os = "linux")]
pub fn signal_group_filtered(leader: u32, signal: i32, allowlist: &[&str]) -> usize {
    if std::fs::read_to_string(format!("/proc/{leader}/stat")).is_err() {
        return 0;
    }
    let mut delivered = 0;
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(member) = name.parse::<u32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{member}/stat")) else {
            continue;
        };
        let Some(tail) = stat.rsplit_once(')') else { continue };
        let mut fields = tail.1.split_whitespace();
        let _state = fields.next();
        let _ppid = fields.next();
        let Some(pgrp) = fields.next().and_then(|f| f.parse::<u32>().ok()) else {
            continue;
        };
        if pgrp != leader {
            continue;
        }
        let Ok(cmdline) = std::fs::read(format!("/proc/{member}/cmdline")) else {
            continue;
        };
        // Zombies (and kernel threads that slipped the pgrp filter)
        // report an EMPTY cmdline; `windows(0)` would panic below.
        if cmdline.is_empty() {
            continue;
        }
        if !allowlist.iter().any(|needle| {
            needle.is_empty()
                || cmdline.windows(needle.len().min(cmdline.len())).any(|w| w == needle.as_bytes())
        }) {
            continue;
        }
        if let Some(pidfd) = pidfd_open(member) {
            // SAFETY: plain syscall, integer args.
            let rc =
                unsafe { libc::syscall(libc::SYS_pidfd_send_signal, pidfd, signal, 0_usize, 0) };
            // SAFETY: closing our own fd.
            unsafe { libc::close(pidfd) };
            if rc == 0 {
                delivered += 1;
            }
        }
    }
    delivered
}

#[cfg(target_os = "linux")]
fn pidfd_open(pid: u32) -> Option<i32> {
    let pid = i32::try_from(pid).unwrap_or(-1);
    // SAFETY: plain syscall, integer args.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    i32::try_from(fd).ok().filter(|f| *f >= 0)
}

/// Non-Linux fallback for [`signal_group_filtered`]: does NOTHING and
/// reports zero delivered signals.
///
/// The real implementation is a `/proc` scan that pins the group via the
/// leader's live-or-zombie `/proc` entry and signals allowlisted members
/// through `pidfd`s — all of that is Linux-only (`SYS_pidfd_open` /
/// `SYS_pidfd_send_signal` do not exist in libc for other targets, and
/// there is no `/proc` to scan). Per the incident-2026-10 rule this
/// helper must never degrade into a blind group kill on a platform where
/// its safety mechanism cannot run, so non-Linux callers get a
/// deliberate no-op instead: no process is ever signalled.
#[cfg(not(target_os = "linux"))]
pub fn signal_group_filtered(_leader: u32, _signal: i32, _allowlist: &[&str]) -> usize {
    0
}
