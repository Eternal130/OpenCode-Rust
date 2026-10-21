#![allow(unsafe_code)] // contained libc FFI (open/fcntl/close/kill/waitpid), reviewed per-site

//! RUNTIME cluster regression (ledger RUNTIME-FDLEAK): a managed-service
//! daemon spawned by the lifecycle `ensure()` path (the CLI's
//! `ensure_managed`/`ensure_with` reach it through
//! `oc_server::lifecycle::ensure`) must not inherit the parent's file
//! descriptors. A gate command that spawns a daemon and then dies would
//! otherwise leave the daemon holding the parent's locks (e.g.
//! `/tmp/opencode/dev-gate.lock`) forever — deadlocking every later gate.
//!
//! Process safety (the discipline this defect is about): the test only
//! spawns processes it owns (`sleep` fixtures), holds a std `Child`
//! handle for the control child, and terminates the daemon by the exact
//! pid it published — after re-verifying parentage via `/proc/<pid>/stat`
//! — then reaps it with `waitpid`. Nothing outside this test's own
//! process tree is signalled.
//!
//! Hermetic: everything under /tmp/opencode, no ports, no network.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use oc_server::lifecycle::{self, EnsureOptions, EnsureTiming};

/// Own tempdir per test under /tmp/opencode.
struct Sandbox {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    root: PathBuf,
}

fn sandbox(prefix: &str) -> Sandbox {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    let root = dir.path().to_path_buf();
    Sandbox { dir, root }
}

/// An OWNED raw fd opened WITHOUT `O_CLOEXEC` — the shape of an inherited
/// lock fd (the gate's `flock` handle): a plain `File::open` would set
/// close-on-exec and hide the defect.
struct InheritedFd(libc::c_int);

impl InheritedFd {
    fn open(path: &Path) -> Self {
        let cpath =
            std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("cstring lock path");
        let fd = unsafe {
            libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC, 0o600)
        };
        assert!(fd >= 0, "open lock file {}", path.display());
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert_eq!(flags & libc::FD_CLOEXEC, 0, "fixture fd must be inheritable (no FD_CLOEXEC)");
        Self(fd)
    }
}

impl Drop for InheritedFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

/// Whether `/proc/<pid>/fd` holds a descriptor resolving to `target`.
fn holds_fd_for(pid: u32, target: &Path) -> bool {
    let canonical = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let dir = format!("/proc/{pid}/fd");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("cannot read {dir} for pid {pid}");
    };
    for entry in entries.flatten() {
        if let Ok(link) = std::fs::read_link(entry.path()) {
            let linked = link.canonicalize().unwrap_or(link);
            if linked == canonical {
                return true;
            }
        }
    }
    false
}

/// RUNTIME-FDLEAK: the daemon spawn closes inherited fds.
///
/// Two children, both spawned by THIS test:
/// 1. a control `sleep` through a plain `std::process::Command` (default
///    fd inheritance) — proves the fixture fd is genuinely inheritable,
///    so the daemon-side absence below can only come from spawn hygiene;
/// 2. the "daemon": `lifecycle::ensure` with a fixture command that
///    publishes its pid and sleeps — the exact spawn path the CLI's
///    managed-service connection uses (`ensure_managed` → `ensure_with`
///    → `lifecycle::ensure` → `Contender::spawn`).
#[tokio::test]
async fn daemon_closes_inherited_fds() {
    let sandbox = sandbox("oc-cli-daemon-spawn-");
    let lock_path = sandbox.root.join("gate.lock");
    let lock = InheritedFd::open(&lock_path);
    let me = std::process::id();

    // -- Control: default std spawn DOES inherit the fd -------------------
    let mut control = std::process::Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("control sleep spawns");
    let control_pid = control.id();
    assert!(
        holds_fd_for(control_pid, &lock_path),
        "control child (plain std spawn) inherits the non-CLOEXEC fd — the fixture is real"
    );

    // -- Daemon: the managed-service spawn seam ---------------------------
    let pidfile = sandbox.root.join("daemon.pid");
    let registration = sandbox.root.join("service.json");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        format!("echo $$ > {}; exec sleep 60", pidfile.to_string_lossy()),
    ];
    let started = Instant::now();
    let ensured = lifecycle::ensure(&EnsureOptions {
        file: Some(registration),
        version: None,
        build: None,
        command: Some(command),
        env: Vec::new(),
        on_start: None,
        timing: EnsureTiming {
            poll_interval: Duration::from_millis(100),
            request_timeout: Duration::from_millis(500),
            // One contender only: the wall clock expires long before a
            // second spawn delay passes.
            spawn_delay: Duration::from_secs(30),
            max_spawn_delay: Duration::from_secs(30),
            promise_timeout: Duration::from_millis(1500),
            ..EnsureTiming::default()
        },
    })
    .await;
    // No HTTP server ever answers — ensure times out after spawning the
    // contender; the contender itself is RELEASED (detached), which is
    // exactly the daemon-survives-the-client shape being pinned.
    assert!(
        matches!(ensured, Err(lifecycle::EnsureError::Timeout)),
        "fixture daemon never answers; ensure must time out (got: {ensured:?})"
    );
    assert!(started.elapsed() >= Duration::from_millis(1400), "ensure respected its wall clock");

    // Read the daemon's self-published pid.
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid_text = loop {
        if let Ok(text) = std::fs::read_to_string(&pidfile) {
            break text;
        }
        assert!(Instant::now() < deadline, "daemon never wrote its pidfile");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let daemon_pid: u32 = pid_text.trim().parse().expect("pidfile holds the pid");

    // The assertion: the daemon must NOT hold the parent's lock fd.
    let leaked = holds_fd_for(daemon_pid, &lock_path);

    // -- Cleanup FIRST-class: reap both owned children ---------------------
    // Daemon: verify parentage via /proc/<pid>/stat (field 4) before any
    // signal, then TERM + waitpid as the direct parent.
    let stat = std::fs::read_to_string(format!("/proc/{daemon_pid}/stat")).unwrap_or_default();
    let ppid = stat
        .rsplit(')')
        .next()
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|field| field.parse::<u32>().ok())
        .unwrap_or(0);
    assert_eq!(ppid, me, "daemon must be this test's direct child before cleanup");
    let daemon_pid_t = daemon_pid as libc::pid_t;
    unsafe {
        libc::kill(daemon_pid_t, libc::SIGTERM);
    }
    let mut status: libc::c_int = 0;
    let waited = loop {
        let code = unsafe { libc::waitpid(daemon_pid_t, &raw mut status, 0) };
        if code == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        break code;
    };
    assert_eq!(waited, daemon_pid_t, "reaped the daemon we spawned");

    // Control child: std Child handle.
    control.kill().expect("stop control child");
    let _ = control.wait();
    drop(lock);

    assert!(
        !leaked,
        "RUNTIME-FDLEAK: the managed-service daemon (pid {daemon_pid}) must not inherit the \
         parent's lock fd — a killed gate command would leave the daemon holding \
         {} forever; ensure Contender::spawn marks inherited fds close-on-exec",
        lock_path.display()
    );
}
