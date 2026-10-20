//! Rust-port server control: spawn the `oc-server` binary on a free port
//! inside a sandboxed HOME, wait until it serves, and stop the process
//! group. Mirrors [`crate::reference`] so the same scenario scripts drive
//! both servers ([`crate::scenario::ScenarioHost`]).
//!
//! ## How the server is launched
//!
//! ```text
//! <cargo-target>/debug/oc-server serve --port <free>
//! ```
//!
//! The binary is built on demand (`cargo build -p oc-server --bin
//! oc-server`, honoring `CARGO_TARGET_DIR`), then executed **directly**
//! with a hermetic environment — no cargo/rustup in the child, so the
//! sandboxed HOME cannot leak build-tool state.
//!
//! Parity-relevant spawn details:
//!
//! - **cwd** mirrors the reference spawn (`packages/cli` of the reference
//!   checkout when present): the server's own working directory is the
//!   location that shows up as the second boot group in the
//!   `permission_saved` golden, and the reference runs it from there.
//! - **`OPENCODE_CONFIG`** points at a pre-seeded project config for
//!   scenarios whose provider config is written before boot (the Rust
//!   engine binds custom providers at boot — see
//!   `oc_server::boot::providers_from_config`).
//! - Readiness: poll `GET /api/info` with auth until HTTP 200.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::minih;
use crate::normalize::NormEnv;
use crate::scenario::ScenarioHost;

/// The managed-service default port; the harness never binds it.
const FORBIDDEN_PORT: u16 = 49374;
/// Deterministic password injected through the child environment.
const RUST_PASSWORD: &str = "parity-rust-password";

/// Handle to a running Rust server.
pub struct RustServer {
    child: Child,
    url: String,
    password: String,
    sandbox: PathBuf,
}

impl ScenarioHost for RustServer {
    fn url(&self) -> &str {
        &self.url
    }
    fn password(&self) -> &str {
        &self.password
    }
    fn sandbox(&self) -> &Path {
        &self.sandbox
    }
}

impl RustServer {
    /// A [`NormEnv`] with the sandbox + the checkout-relative cwd roots.
    #[must_use]
    pub fn norm_env(&self) -> NormEnv {
        ScenarioHost::norm_env(self)
    }

    /// Shut the server down (SIGTERM to the process group, escalate to
    /// SIGKILL) and wait for it to exit.
    pub async fn stop(mut self) -> Result<()> {
        let pid = self.child.id();
        if let Some(pid) = pid {
            signal_group(pid, libc::SIGTERM).ok();
        }
        if let Ok(status) = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            return status.context("wait rust server").map(|_| ());
        }
        if let Some(pid) = pid {
            signal_group(pid, libc::SIGKILL).ok();
        }
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .map_err(|_| anyhow!("rust server ignored SIGKILL"))?
            .context("wait rust server after SIGKILL")?;
        let _ = status;
        Ok(())
    }
}

/// Send `signal` to the process group of `pid` (the child was spawned with
/// `process_group(0)`).
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

/// The workspace root (tests compile with `CARGO_MANIFEST_DIR` pointing at
/// `tools/parity`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

/// The cargo target directory (honors `CARGO_TARGET_DIR`).
fn target_dir() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map_or_else(|_| workspace_root().join("target"), PathBuf::from)
}

/// The built server binary path, building it first when missing.
fn server_binary() -> Result<PathBuf> {
    let binary = target_dir().join("debug").join("oc-server");
    build_binary("oc-server", "oc-server", &binary)
}

/// The built `opencode-rust` CLI binary path (the TUI drivers use it),
/// building it first when missing.
///
/// # Errors
/// When cargo cannot build the binary.
pub fn cli_binary() -> Result<PathBuf> {
    let binary = target_dir().join("debug").join("opencode-rust");
    build_binary("oc-cli", "opencode-rust", &binary)
}

/// Whether `path` is a regular file carrying at least one executable
/// mode bit (unix: any of the `0o111` x-bits).
///
/// Regression (CI run 39468671027, legs 2+3): `actions/upload-artifact@v4`
/// does NOT preserve the executable bit, so an artifact-shipped
/// `target/debug/oc-server` is a plain 0644 file — present, but every
/// spawn dies with `Permission denied (os error 13)`. "The file exists"
/// is therefore NOT a usable-binary predicate.
#[must_use]
pub fn is_executable_file(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        // The parity harness is unix-first; on other platforms fall back
        // to plain presence (there is no exec bit to inspect).
        path.is_file()
    }
}

/// Ensure `binary` is a usable executable, invoking `build` when it is
/// not — and NEVER returning a path that would fail at spawn time.
///
/// Contract (the CI-RUNTIME-GATES-RED regression):
///
/// - an already-executable file is returned as-is, without building;
/// - anything else (missing, or present WITHOUT the exec bit) is
///   REMOVED first, then built. The removal is load-bearing: cargo does
///   not relink while its fingerprint is fresh and the output file is
///   present, so the stale 0644 artifact must go or `cargo build` would
///   "succeed" without ever producing an executable file;
/// - the post-build result must itself pass [`is_executable_file`] — a
///   build that leaves a non-executable file is an error, not a path.
///
/// The build step is injected so the regression test can pin this
/// contract hermetically (no cargo, no server rebuild).
///
/// # Errors
/// When the stale file cannot be removed, when `build` fails to run or
/// exits non-zero, or when its result is not an executable file.
pub fn ensure_binary(
    binary: &std::path::Path,
    build: impl FnOnce() -> std::io::Result<std::process::ExitStatus>,
) -> Result<PathBuf> {
    if is_executable_file(binary) {
        return Ok(binary.to_path_buf());
    }
    // Force a real relink instead of trusting cargo's fresh fingerprint
    // next to the broken output (see the contract above).
    match std::fs::remove_file(binary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(anyhow!(
                "cannot remove stale non-executable {}: {error}",
                binary.display()
            ));
        }
    }
    let status = build().context("run cargo build")?;
    if !status.success() {
        return Err(anyhow!("cargo build failed (status {status})"));
    }
    if !is_executable_file(binary) {
        return Err(anyhow!(
            "cargo build succeeded but {} is not an executable file",
            binary.display()
        ));
    }
    Ok(binary.to_path_buf())
}

fn build_binary(package: &str, bin: &str, binary: &std::path::Path) -> Result<PathBuf> {
    ensure_binary(binary, || {
        std::process::Command::new("cargo")
            .arg("build")
            .arg("-p")
            .arg(package)
            .arg("--bin")
            .arg(bin)
            .current_dir(workspace_root())
            .status()
    })
    .with_context(|| format!("cargo build {package}"))
}

/// Test helper: skip when the harness cannot build the Rust server (no
/// cargo on PATH). Under `OC_GATE_STRICT=1` (Gate 0, B0-8) this fails
/// the test instead of skipping — see [`crate::gate`].
#[must_use]
pub fn skip_if_no_cargo() -> bool {
    let cargo_on_path = std::env::var("PATH")
        .is_ok_and(|paths| paths.split(':').any(|dir| Path::new(dir).join("cargo").is_file()));
    if cargo_on_path {
        return false;
    }
    crate::gate::skip_or_fail(
        "cargo not found on PATH (the Rust gate side cannot be built; run tests \
         through tools/dev-gate.sh)",
    )
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

/// Options for [`spawn_rust`].
#[derive(Default)]
pub struct RustSpawn<'a> {
    /// Extra child environment entries (e.g. `OPENCODE_CONFIG`).
    pub extra_env: Vec<(&'a str, &'a str)>,
    /// Files to write inside the sandbox before spawning
    /// (relative path, contents) — used to pre-seed the project config.
    pub seed_files: Vec<(PathBuf, String)>,
}

/// Spawn the Rust server and wait until `GET /api/info` answers 200.
pub async fn spawn_rust(options: RustSpawn<'_>) -> Result<RustServer> {
    // Ref-dir self-healing (shared with the reference spawner): the Rust
    // server mirrors the reference cwd when the checkout exists, so keep it
    // alive too. A failed rebuild is logged and skipped — the Rust server
    // itself never needs the checkout.
    if let Err(error) = crate::reference::ensure_reference_checkout() {
        eprintln!("[parity] reference checkout rebuild skipped: {error}");
    }
    let binary = server_binary()?;
    let port = free_port()?;
    let sandbox = std::env::var("PARITY_SANDBOX_ROOT")
        .map_or_else(|_| PathBuf::from(crate::reference::SANDBOX_ROOT), PathBuf::from)
        .join(format!("rust-{}-{}", std::process::id(), port));
    let home = sandbox.join("home");
    let tmp = sandbox.join("tmp");
    for dir in [
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        tmp.clone(),
        sandbox.join("project"),
        sandbox.join("logs"),
    ] {
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    for (path, contents) in &options.seed_files {
        let path = sandbox.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
    }

    // Mirror the reference cwd (its own working directory is the location
    // that pins the second boot group of permission_saved).
    let cwd = PathBuf::from("/tmp/opencode/ref/packages/cli");
    let cwd = if cwd.is_dir() { cwd } else { sandbox.join("project") };

    let url = format!("http://127.0.0.1:{port}");
    let mut command = Command::new(&binary);
    command
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(&cwd)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", &home)
        .env("OPENCODE_TEST_HOME", &home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMPDIR", &tmp)
        .env("OPENCODE_PASSWORD", RUST_PASSWORD)
        // Hermetic boots (S1/GATE4-A.3): kill the boot-spawned models-dev
        // refresh loop in every parity-spawned server — an online box
        // would otherwise publish `models-dev.refreshed` at a
        // wall-clock-dependent moment (the goldens contain none of it).
        // Set BEFORE extra_env so a test can still override if it ever
        // needs the loop.
        .env("OPENCODE_CATALOG_FETCH", "0")
        .env("RUST_LOG", "warn");
    for (key, value) in &options.extra_env {
        // `{sandbox}` expands to this server's sandbox root (harness-side
        // path templating for OPENCODE_CONFIG-style pointers).
        let value = value.replace("{sandbox}", sandbox.to_string_lossy().as_ref());
        command.env(key, value);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    command.process_group(0);

    let mut child = command.spawn().with_context(|| format!("spawn {}", binary.display()))?;
    let stdout = child.stdout.take().context("child stdout not piped")?;
    let stderr = child.stderr.take().context("child stderr not piped")?;
    tee_to_log(stdout, sandbox.join("logs/stdout.log"));
    tee_to_log(stderr, sandbox.join("logs/stderr.log"));

    let auth = minih::basic_auth("opencode", RUST_PASSWORD);
    let deadline = Duration::from_secs(60);
    let started = std::time::Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow!(
                "rust server exited during boot (status {status}); see {}",
                sandbox.join("logs").display()
            ));
        }
        if started.elapsed() > deadline {
            return Err(anyhow!(
                "rust server not ready after {}s; see {}",
                deadline.as_secs(),
                sandbox.join("logs").display()
            ));
        }
        if let Ok(response) = minih::request(
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
            if response.status == 200 {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    Ok(RustServer { child, url, password: RUST_PASSWORD.to_owned(), sandbox })
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
