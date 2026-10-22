//! Regression guard (incident 2026-10-22): the bun guest must NEVER
//! outlive its supervisor. Two teardown layers are pinned here:
//!
//! 1. [`shutdown_leaves_no_live_guest`] — the Rust teardown path:
//!    `PluginHost::shutdown()` kills the owned child through its
//!    handle and confirms the death (the supervise loop's
//!    kill-and-reap + the facade join). No live host child remains.
//! 2. [`parent_sigkill_leaves_no_live_guest`] — the no-Drop path that
//!    burned ~5 cores on the shared box: the supervisor's PROCESS is
//!    `SIGKILL`ed mid-flight (the adversarial kill -9 family), so no
//!    Rust teardown can run at all. The guest must still die — through
//!    its own IPC self-exit (host.js exits on stdin EOF / stdout
//!    EPIPE) plus the kernel's `PR_SET_PDEATHSIG` belt. The "parent" is
//!    a re-exec of this test binary in helper mode
//!    ([`orphan_helper_mode`]) that boots a host with a
//!    timer-holding plugin (the exact shape that kept an EOF'd guest
//!    alive forever pre-fix), publishes the guest pid through a
//!    marker file, and parks. (A marker FILE, not stdout: the libtest
//!    harness interleaves its progress line with test output — a
//!    `READY` println glues onto `test orphan_helper_mode ... ` and a
//!    stdout-scraping parent never sees the marker line.)

mod common;

use std::time::Duration;

use oc_plugin_host::PluginHost;

/// The helper-mode marker file env (parent sets it; helper writes the
/// guest pid into that path once the host is ready).
const HELPER_ENV: &str = "OC_PLUGIN_ORPHAN_MARKER";

/// `/proc/<pid>` gone entirely (not even a zombie).
fn process_gone(pid: i32) -> bool {
    !std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Boot a host over the timer-holding fixture (two `setInterval`s —
/// the event-loop keep-alive behind the orphan leak).
fn timer_host() -> (PluginHost, std::path::PathBuf) {
    let sandbox = common::sandbox("orphan-guard");
    let entry = common::copy_fixture(&sandbox, "timercrasher.ts");
    let host = common::host_with(&[entry], &sandbox);
    (host, sandbox)
}

#[test]
fn shutdown_leaves_no_live_guest() {
    let (host, _sandbox) = timer_host();
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host came up");
    let pid = host.guest_pid().expect("guest pid published while running");
    host.shutdown();
    assert!(
        common::wait_for(Duration::from_secs(5), || process_gone(pid)),
        "guest bun (pid {pid}) still alive after host.shutdown()"
    );
}

/// Helper half of [`parent_sigkill_leaves_no_live_guest`]: re-executed
/// copy of this test binary. Boots the timer-holding host, writes the
/// guest pid to the marker file named by [`HELPER_ENV`], then parks
/// until killed. Self-skips (instant return) when the env var is
/// absent.
#[test]
fn orphan_helper_mode() {
    let Ok(marker) = std::env::var(HELPER_ENV) else {
        return; // normal suite invocation — the parent test drives us
    };
    let (host, _sandbox) = timer_host();
    assert!(host.wait_ready(Duration::from_secs(15)), "helper host came up");
    let pid = host.guest_pid().expect("guest pid published while running");
    std::fs::write(&marker, pid.to_string()).expect("write marker file");
    // Park: the parent SIGKILLs this process mid-flight (no Drop runs
    // — exactly the incident's precondition).
    std::thread::sleep(Duration::from_secs(120));
}

#[test]
fn parent_sigkill_leaves_no_live_guest() {
    if !PluginHost::detect().enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let marker =
        std::path::PathBuf::from(format!("/tmp/opencode/orphan-marker-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let mut helper = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .arg("orphan_helper_mode")
        .arg("--exact")
        .arg("--test-threads=1")
        .env(HELPER_ENV, &marker)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("spawn helper");
    let helper_pid = helper.id();
    let ready = common::wait_for(Duration::from_secs(20), || {
        std::fs::read_to_string(&marker).is_ok_and(|text| text.trim().parse::<i32>().is_ok())
    });
    if !ready {
        let _ = helper.kill();
        let _ = helper.wait();
        panic!("helper never reported a ready guest (helper pid {helper_pid})");
    }
    let guest_pid: i32 =
        std::fs::read_to_string(&marker).expect("marker").trim().parse().expect("pid in marker");
    let _ = std::fs::remove_file(&marker);

    // The incident's kill: the supervisor's whole process, SIGKILLed —
    // no Drop, no teardown task, nothing but the guest's own IPC
    // self-exit and PDEATHSIG between it and eternity.
    let killed = std::process::Command::new("kill").arg("-9").arg(helper_pid.to_string()).status();
    assert!(killed.is_ok_and(|status| status.success()), "kill -9 delivered to helper");
    let _ = helper.wait();

    assert!(
        common::wait_for(Duration::from_secs(10), || process_gone(guest_pid)),
        "guest bun (pid {guest_pid}) survived its parent's SIGKILL — orphan leak is back"
    );
}
