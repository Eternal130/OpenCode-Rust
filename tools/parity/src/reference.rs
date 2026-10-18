//! Reference server control: spawn v2.0.24 (`/tmp/opencode/ref`) on a free
//! port inside a sandboxed HOME, wait until it serves, and stop the whole
//! process group.
//!
//! ## How the server is launched
//!
//! ```text
//! bun run --cwd /tmp/opencode/ref/packages/cli \
//!   --define OPENCODE_VERSION="2.0.24" \
//!   --define OPENCODE_CHANNEL="stable" \
//!   --define OPENCODE_ARTIFACT="cli" \
//!   src/index.ts serve --port <free>
//! ```
//!
//! Notes, verified against the reference source (`packages/cli/src/
//! server-process.ts`, `env.ts`, `version.ts`) and live runs:
//!
//! - `OPENCODE_VERSION` is a **build-time** define, not a runtime env var; a
//!   plain `bun run ... dev` reports `version: "local"`. Passing `--define`
//!   (and invoking `src/index.ts` directly so the define is not swallowed by
//!   the `dev` wrapper script) makes `/api/info` report `2.0.24`.
//! - Auth is HTTP Basic with username `opencode` and the server password.
//! - Password acquisition (documented simplest-reliable path): the harness
//!   passes `OPENCODE_PASSWORD` in the child environment, so the password is
//!   known a priori. As a fallback it also parses `server password <pw>` /
//!   `server listening on <url>` lines from stdout, which is what a manual
//!   foreground run prints (see `server-process.ts` lines 174-175).
//! - Readiness: poll `GET /api/info` with auth until HTTP 200.
//! - Hermeticity: the child runs with a scrubbed environment (explicit PATH /
//!   HOME / XDG_* / TMPDIR only), so it can never reach the live managed
//!   service state under `~/.local/state/opencode`. `bun run` re-execs a
//!   nested bun process, so shutdown signals the whole process group; the
//!   default managed-service port 49374 is never bound (explicit guard in the
//!   port picker).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::minih;
use crate::normalize::NormEnv;

/// Reference checkout root (override with `PARITY_REF_ROOT`).
pub const REF_ROOT: &str = "/tmp/opencode/ref";
/// Default sandbox parent (override with `PARITY_SANDBOX_ROOT`).
pub const SANDBOX_ROOT: &str = "/tmp/opencode/parity-sandbox";
/// The reference's default managed-service port; the harness never binds it.
const FORBIDDEN_PORT: u16 = 49374;
/// Deterministic password injected via `OPENCODE_PASSWORD` (loopback test
/// server inside a private sandbox).
const REF_PASSWORD: &str = "parity-ref-password";

/// Handle to a running reference server.
pub struct ReferenceServer {
    child: Child,
    url: String,
    password: String,
    sandbox: PathBuf,
}

impl ReferenceServer {
    /// Base URL, e.g. `http://127.0.0.1:54321`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The Basic-auth password for API requests (username is `opencode`).
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Sandbox directory backing this server (HOME, TMPDIR, logs).
    #[must_use]
    pub fn sandbox(&self) -> &Path {
        &self.sandbox
    }

    /// A [`NormEnv`] preloaded with this server's sandbox roots, ready for
    /// normalizing payloads produced by it.
    #[must_use]
    pub fn norm_env(&self) -> NormEnv {
        NormEnv::new().with_root(self.sandbox.to_string_lossy().to_string())
    }

    /// Shut the server down (SIGTERM to the process group, escalate to
    /// SIGKILL) and wait for it to exit.
    pub async fn stop(mut self) -> Result<()> {
        let pid = self.child.id();
        if let Some(pid) = pid {
            signal_group(pid, libc::SIGTERM).ok();
        }
        if let Ok(status) = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            return status.context("wait reference server").map(|_| ());
        }
        if let Some(pid) = pid {
            signal_group(pid, libc::SIGKILL).ok();
        }
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .map_err(|_| anyhow!("reference server ignored SIGKILL"))?
            .context("wait reference server after SIGKILL")?;
        let _ = status;
        Ok(())
    }
}

/// Send `signal` to the process group of `pid` (the child was spawned with
/// `process_group(0)`, so its pgid equals its pid).
#[allow(unsafe_code)]
fn signal_group(pid: u32, signal: i32) -> Result<()> {
    // Incident 2026-10 rule: filtered group signal — members must look like
    // our tooling (bun/opencode/node family) via /proc cmdline allowlist;
    // never a blind `kill(-pgid)` on a possibly recycled id. The leader's
    // live-or-zombie /proc entry pins the group during the scan.
    let delivered = oc_util::procutil::signal_group_filtered(
        pid,
        signal,
        &["bun", "opencode", "oc-server", "node", "cargo"],
    );
    if delivered > 0 {
        Ok(())
    } else {
        Err(anyhow!("signal_group_filtered({pid}) matched no processes"))
    }
}

#[cfg(not(unix))]
compile_error!("the parity harness currently supports unix only (process groups, sandboxed HOME)");

fn ref_root() -> PathBuf {
    std::env::var("PARITY_REF_ROOT").map_or_else(|_| PathBuf::from(REF_ROOT), PathBuf::from)
}

fn sandbox_root() -> PathBuf {
    std::env::var("PARITY_SANDBOX_ROOT").map_or_else(|_| PathBuf::from(SANDBOX_ROOT), PathBuf::from)
}

/// Well-known bun install paths probed when `PARITY_BUN` is unset. The
/// lane home is probed right after root's: gate runs execute as
/// `eternal130` (`tools/dev-gate.sh`), whose `$HOME` bun install is the
/// reachable one on this host — `/root` is mode 700, so
/// `/root/.bun/bin/bun` only ever resolves for root, and root must not
/// run gates (PARITY-LOOP.md §2.1 incident B).
const WELL_KNOWN_BUN: &[&str] = &[
    "/root/.bun/bin/bun",
    "/home/eternal130/.bun/bin/bun",
    "/usr/local/bin/bun",
    "/usr/bin/bun",
    "/opt/homebrew/bin/bun",
];

/// Locate bun: `$PARITY_BUN`, then well-known install paths, then `PATH`.
fn bun_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PARITY_BUN") {
        return Some(PathBuf::from(path));
    }
    for candidate in WELL_KNOWN_BUN {
        if Path::new(candidate).is_file() {
            return Some(PathBuf::from(candidate));
        }
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            let candidate = Path::new(dir).join("bun");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The bun executable the harness would launch, for Gate 0 evidence
/// records (see `tests/gate0_environment.rs`).
#[must_use]
pub fn resolved_bun() -> Option<PathBuf> {
    bun_path()
}

/// Test helper: returns `true` when the reference cannot be run and tests
/// should skip: bun missing, or the checkout missing with no rebuild
/// source. Under `OC_GATE_STRICT=1` (Gate 0, B0-8) a missing prerequisite
/// fails the test instead of skipping — see [`crate::gate`].
#[must_use]
pub fn skip_if_no_bun() -> bool {
    if let Some(reason) = reference_unavailable_reason() {
        return crate::gate::skip_or_fail(&reason);
    }
    false
}

/// Exactly why the reference side cannot run (`None` = runnable): bun
/// unresolved, a (strict-mode-only) `PARITY_BUN` pointing at a
/// nonexistent file, or a missing checkout with no rebuild source.
fn reference_unavailable_reason() -> Option<String> {
    match bun_path() {
        None => {
            return Some(format!(
                "bun not found (set PARITY_BUN; probed $PARITY_BUN, well-known paths [{}], then $PATH)",
                WELL_KNOWN_BUN.join(", ")
            ));
        }
        // Strict only: Gate 0 must fail fast with the path named when the
        // explicitly configured bun does not exist. Non-strict keeps
        // today's behavior (the spawn itself errors out loudly).
        Some(path) if crate::gate::strict() && !path.is_file() => {
            return Some(format!(
                "bun not found at {} (PARITY_BUN points at a nonexistent file)",
                path.display()
            ));
        }
        Some(_) => {}
    }
    if reference_checkout_present() {
        return None;
    }
    let source = std::env::var("PARITY_REF_SOURCE").unwrap_or_else(|_| REF_SOURCE.to_owned());
    if Path::new(&source).join("packages/cli").is_dir() {
        return None; // missing, but rebuildable on demand
    }
    Some(format!(
        "reference checkout {} missing and not rebuildable (source {} has no packages/cli; \
         set PARITY_REF_ROOT or PARITY_REF_SOURCE)",
        ref_root().display(),
        source
    ))
}

/// Whether the reference checkout (the part the spawner needs) exists.
fn reference_checkout_present() -> bool {
    ref_root().join("packages/cli").is_dir()
}

/// The canonical source checkout the working copy is rebuilt from.
const REF_SOURCE: &str = "/opt/opencode-ref";

/// Ref-dir self-healing: before spawning, rebuild `/tmp/opencode/ref` from
/// `/opt/opencode-ref` when `packages/cli` is missing (tmpfs cleanup, fresh
/// sandbox). The copy excludes `.git`; dependencies come from
/// `bun install --frozen-lockfile`. Every step is logged so a failing
/// rebuild is diagnosable from test output.
pub fn ensure_reference_checkout() -> Result<()> {
    if reference_checkout_present() {
        return Ok(());
    }
    let root = ref_root();
    let bun = bun_path().ok_or_else(|| {
        anyhow!(
            "bun not found (set PARITY_BUN); cannot rebuild the reference checkout at {}",
            root.display()
        )
    })?;
    let source =
        PathBuf::from(std::env::var("PARITY_REF_SOURCE").unwrap_or_else(|_| REF_SOURCE.to_owned()));
    if !source.join("packages/cli").is_dir() {
        return Err(anyhow!(
            "reference source checkout missing {} (set PARITY_REF_SOURCE); cannot rebuild {}",
            source.display(),
            root.display()
        ));
    }
    eprintln!(
        "[parity] reference checkout missing; rebuilding {} from {} (excluding .git)",
        root.display(),
        source.display()
    );
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent).context("create reference parent dir")?;
    }
    std::fs::create_dir_all(&root).context("create reference dir")?;
    let copy = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "tar -C {} --exclude=./.git -cf - . | tar -C {} -xf -",
            source.display(),
            root.display()
        ))
        .status()
        .context("run tar copy")?;
    if !copy.success() {
        return Err(anyhow!("tar copy from {} failed", source.display()));
    }
    eprintln!("[parity] reference copy complete; running bun install --frozen-lockfile");
    let install = std::process::Command::new(&bun)
        .arg("install")
        .arg("--frozen-lockfile")
        .current_dir(&root)
        .status()
        .with_context(|| format!("run bun install in {}", root.display()))?;
    if !install.success() {
        return Err(anyhow!("bun install failed in {}", root.display()));
    }
    eprintln!("[parity] reference checkout rebuilt");
    Ok(())
}

/// Pick a free loopback port (never the managed-service default 49374).
fn free_port() -> Result<u16> {
    for _ in 0..16 {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").context("bind ephemeral listener")?;
        let port = listener.local_addr().context("local addr")?.port();
        drop(listener);
        if port != FORBIDDEN_PORT {
            return Ok(port);
        }
    }
    Err(anyhow!("could not find a free port"))
}

#[derive(Default)]
struct StdoutLines {
    listening_url: Option<String>,
    printed_password: Option<String>,
}

/// Create the sandbox layout and return `(home, tmp)`.
fn prepare_sandbox(sandbox: &Path) -> Result<(PathBuf, PathBuf)> {
    let home = sandbox.join("home");
    let tmp = sandbox.join("tmp");
    for dir in [
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        tmp.clone(),
        sandbox.join("logs"),
    ] {
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    Ok((home, tmp))
}

/// Build the hermetic bun command line (see module docs).
fn build_command(
    bun: &Path,
    root: &Path,
    port: u16,
    home: &Path,
    tmp: &Path,
    extra_env: &[(&str, &str)],
) -> Command {
    let mut command = Command::new(bun);
    command
        .current_dir(root)
        .arg("run")
        .arg("--cwd")
        .arg(root.join("packages/cli"))
        .arg("--define")
        .arg("OPENCODE_VERSION=\"2.0.24\"")
        .arg("--define")
        .arg("OPENCODE_CHANNEL=\"stable\"")
        .arg("--define")
        .arg("OPENCODE_ARTIFACT=\"cli\"")
        .arg("src/index.ts")
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        // Hermetic environment: nothing from the parent leaks in (most
        // importantly no live-service credentials or state paths).
        .env_clear()
        .env("PATH", "/root/.bun/bin:/usr/local/bin:/usr/bin:/bin")
        .env("HOME", home)
        .env("OPENCODE_TEST_HOME", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMPDIR", tmp)
        .env("OPENCODE_PASSWORD", REF_PASSWORD)
        .env("OPENCODE_VERSION", "2.0.24")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    // New process group so shutdown can signal bun's nested child too.
    command.process_group(0);
    command
}

/// Tee a child pipe into `<sandbox>/logs/<name>.log`.
fn tee_to_log(pipe: impl tokio::io::AsyncRead + Send + Unpin + 'static, path: PathBuf) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        let mut sink = std::fs::File::create(&path).ok();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(sink) = sink.as_mut() {
                use std::io::Write;
                let _ = writeln!(sink, "{line}");
            }
        }
    });
}

/// Spawn the reference server and wait until `GET /api/info` answers 200.
///
/// Fails (rather than hangs) if the process exits during boot; stdout/stderr
/// are teed into `<sandbox>/logs/` for diagnosis.
pub async fn spawn() -> Result<ReferenceServer> {
    spawn_with_env(&[]).await
}

/// [`spawn`] with additional child environment entries (applied after the
/// hermetic base, so they can override it). Used by scenario experiments —
/// e.g. `OPENCODE_SIMULATE=1` (see `tools/parity/src/scenario.rs` docs for
/// what simulation mode does and does not provide).
pub async fn spawn_with_env(extra_env: &[(&str, &str)]) -> Result<ReferenceServer> {
    spawn_reference(ReferenceSpawn { extra_env: extra_env.to_vec(), seed_files: Vec::new() }).await
}

/// Options for [`spawn_reference`] (mirrors [`crate::rust_server::RustSpawn`]).
#[derive(Default)]
pub struct ReferenceSpawn<'a> {
    /// Extra child environment entries, applied after the hermetic base
    /// (so they can override `HOME`/`XDG_*` — used by the Phase-10 DB
    /// interop battery to point two servers at one shared data home).
    /// `{sandbox}` in a value expands to this server's sandbox root.
    pub extra_env: Vec<(&'a str, &'a str)>,
    /// Files to write inside the sandbox before spawning
    /// (relative path, contents) — e.g. a pre-seeded project
    /// `opencode.json` provider config.
    pub seed_files: Vec<(PathBuf, String)>,
}

/// [`spawn`] with full options: pre-seeded sandbox files + extra child env.
pub async fn spawn_reference(options: ReferenceSpawn<'_>) -> Result<ReferenceServer> {
    let bun = bun_path().ok_or_else(|| anyhow!("bun not found (set PARITY_BUN)"))?;
    ensure_reference_checkout()?;
    let root = ref_root();
    let port = free_port()?;
    let sandbox = sandbox_root().join(format!("ref-{}-{}", std::process::id(), port));
    let (home, tmp) = prepare_sandbox(&sandbox)?;
    for (path, contents) in &options.seed_files {
        let path = sandbox.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
    }
    let expanded: Vec<(String, String)> = options
        .extra_env
        .iter()
        .map(|(key, value)| {
            ((*key).to_string(), value.replace("{sandbox}", sandbox.to_string_lossy().as_ref()))
        })
        .collect();
    let expanded_refs: Vec<(&str, &str)> =
        expanded.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();

    let url = format!("http://127.0.0.1:{port}");
    let auth = minih::basic_auth("opencode", REF_PASSWORD);

    let mut child = build_command(&bun, &root, port, &home, &tmp, &expanded_refs)
        .spawn()
        .with_context(|| format!("spawn {}", bun.display()))?;

    let stdout = child.stdout.take().context("child stdout not piped")?;
    let stderr = child.stderr.take().context("child stderr not piped")?;
    tee_to_log(stderr, sandbox.join("logs/stderr.log"));
    let lines = Arc::new(Mutex::new(StdoutLines::default()));
    let lines_for_task = Arc::clone(&lines);
    let stdout_path = sandbox.join("logs/stdout.log");
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut sink = std::fs::File::create(&stdout_path).ok();
        // Lock per line only: the readiness loop contends on this mutex.
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(sink) = sink.as_mut() {
                use std::io::Write;
                let _ = writeln!(sink, "{line}");
            }
            let mut guard = lines_for_task.lock().await;
            if let Some(rest) = line.strip_prefix("server listening on ") {
                guard.listening_url = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("server password ") {
                guard.printed_password = Some(rest.trim().to_string());
            }
        }
    });

    // Readiness: poll GET /api/info until 200, bounded by an overall budget.
    let deadline = Duration::from_secs(90);
    let started = std::time::Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow!(
                "reference server exited during boot (status {status}); see {}",
                sandbox.join("logs").display()
            ));
        }
        if started.elapsed() > deadline {
            return Err(anyhow!(
                "reference server not ready after {}s; see {}",
                deadline.as_secs(),
                sandbox.join("logs").display()
            ));
        }
        match minih::request(
            &url,
            Some(&auth),
            "GET",
            "/api/info",
            None,
            "application/json",
            Duration::from_secs(2),
        )
        .await
        {
            Ok(response) if response.status == 200 => break,
            _ => {}
        }
        {
            let guard = lines.lock().await;
            if let Some(listening) = guard.listening_url.as_ref() {
                if !listening.contains(&format!(":{port}")) {
                    return Err(anyhow!("reference server listening on unexpected url {listening} (wanted port {port})"));
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // Resolve the password: env-provided first, stdout-printed fallback.
    let password = {
        let guard = lines.lock().await;
        guard.printed_password.clone().unwrap_or_else(|| REF_PASSWORD.to_string())
    };

    Ok(ReferenceServer { child, url, password, sandbox })
}

#[cfg(test)]
mod tests {
    #[test]
    fn checkout_present_fast_path_never_rebuilds() {
        // An existing checkout (even a stand-in tree) short-circuits the
        // rebuild, so a present checkout never needs bun.
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::fs::create_dir_all(dir.path().join("packages/cli")).expect("checkout stand-in");
        std::env::set_var("PARITY_REF_ROOT", dir.path());
        assert!(super::reference_checkout_present());
        super::ensure_reference_checkout().expect("fast path succeeds without bun");
        std::env::remove_var("PARITY_REF_ROOT");
    }

    #[test]
    fn missing_checkout_reports_source_absence() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::env::set_var("PARITY_REF_ROOT", dir.path());
        std::env::set_var("PARITY_REF_SOURCE", dir.path().join("nope").display().to_string());
        let error = super::ensure_reference_checkout().expect_err("source missing");
        assert!(error.to_string().contains("cannot rebuild"), "{error}");
        std::env::remove_var("PARITY_REF_ROOT");
        std::env::remove_var("PARITY_REF_SOURCE");
    }
}
