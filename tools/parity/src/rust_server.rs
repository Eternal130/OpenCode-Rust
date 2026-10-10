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

/// The built `opencode` CLI binary path (the TUI smoke drives it),
/// building it first when missing.
///
/// # Errors
/// When cargo cannot build the binary.
pub fn cli_binary() -> Result<PathBuf> {
    let binary = target_dir().join("debug").join("opencode");
    build_binary("oc-cli", "opencode", &binary)
}

fn build_binary(package: &str, bin: &str, binary: &std::path::Path) -> Result<PathBuf> {
    if binary.is_file() {
        return Ok(binary.to_path_buf());
    }
    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg(package)
        .arg("--bin")
        .arg(bin)
        .current_dir(workspace_root())
        .status()
        .with_context(|| format!("cargo build {package}"))?;
    if !status.success() || !binary.is_file() {
        return Err(anyhow!("cargo build {package} failed (status {status})"));
    }
    Ok(binary.to_path_buf())
}

/// Test helper: skip when the harness cannot build the Rust server (no
/// cargo on PATH).
#[must_use]
pub fn skip_if_no_cargo() -> bool {
    !std::env::var("PATH")
        .is_ok_and(|paths| paths.split(':').any(|dir| Path::new(dir).join("cargo").is_file()))
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
