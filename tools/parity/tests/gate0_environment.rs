//! Gate 0 environment precheck (PARITY-LOOP.md §8.3, ledger B0-8): the
//! dual-end gate environment proven end to end in one test.
//!
//! Verifies, for the user actually running the test (gate runs execute
//! as `eternal130` via `tools/dev-gate.sh`, whose PATH has no bun — set
//! `PARITY_BUN` explicitly in gate commands):
//!
//! 1. bun resolves and `bun --version` executes as this user;
//! 2. the pinned read-only reference source commit is recorded
//!    (`git -C $PARITY_REF_SOURCE log -1`, default `/opt/opencode-ref`);
//! 3. the runnable reference checkout boots, answers `GET /api/info`
//!    with 200, and stops cleanly;
//! 4. the Rust server boots, answers `GET /api/info` with 200, and stops
//!    cleanly.
//!
//! Emits the §8.3 per-run record (`gate_ran`, `ref_commit`, `bun_path`,
//! `reference_boot`, `rust_boot`) to stderr and — when
//! `PARITY_GATE0_RECORD` is set — writes it to that path for the
//! evidence bundle.
//!
//! With `OC_GATE_STRICT=1` a missing prerequisite FAILS here instead of
//! self-skipping (fake-green elimination; see `parity::gate`); with the
//! env unset this skips with a note like the rest of the harness.

use std::time::Duration;

use anyhow::{Context, Result};
use parity::scenario::ScenarioHost;
use parity::{minih, reference, rust_server};

const TIMEOUT: Duration = Duration::from_secs(10);
/// The pinned read-only reference source (override: `PARITY_REF_SOURCE`).
const REF_SOURCE: &str = "/opt/opencode-ref";

/// `GET /api/info` must answer 200 with auth.
async fn api_info_ok(base: &str, password: &str) -> Result<()> {
    let auth = minih::basic_auth("opencode", password);
    let response =
        minih::request(base, Some(&auth), "GET", "/api/info", None, "application/json", TIMEOUT)
            .await
            .context("GET /api/info")?;
    anyhow::ensure!(response.status == 200, "GET /api/info => HTTP {}", response.status);
    Ok(())
}

#[tokio::test]
async fn gate0_dual_end_environment() -> Result<()> {
    if reference::skip_if_no_bun() {
        return Ok(());
    }
    if rust_server::skip_if_no_cargo() {
        return Ok(());
    }

    // 1. bun executes as this user.
    let bun = reference::resolved_bun().expect("skip_if_no_bun resolved bun");
    let version = std::process::Command::new(&bun)
        .arg("--version")
        .output()
        .with_context(|| format!("execute {} --version", bun.display()))?;
    anyhow::ensure!(
        version.status.success(),
        "{} --version failed: {}{}",
        bun.display(),
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    let bun_version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    eprintln!("[gate0] bun {} => {}", bun.display(), bun_version);

    // 2. the pinned source commit (read-only `git log -1`).
    let ref_source = std::env::var("PARITY_REF_SOURCE").unwrap_or_else(|_| REF_SOURCE.to_owned());
    let commit = std::process::Command::new("git")
        // The pinned checkout is root-owned; per-command safe.directory
        // lets the gate user read it without touching global git config.
        .arg("-c")
        .arg(format!("safe.directory={ref_source}"))
        .arg("-C")
        .arg(&ref_source)
        .args(["log", "-1", "--format=%H"])
        .output()
        .with_context(|| format!("git -C {ref_source} log -1"))?;
    anyhow::ensure!(commit.status.success(), "git log -1 in {ref_source} failed");
    let ref_commit = String::from_utf8_lossy(&commit.stdout).trim().to_owned();
    eprintln!("[gate0] ref source {ref_source} @ {ref_commit}");

    // 3. reference boot → /api/info 200 → clean stop. (`spawn` itself
    //    polls GET /api/info for readiness; the explicit probe keeps the
    //    200 in this test's own evidence.)
    let reference_server = reference::spawn().await?;
    eprintln!("[gate0] reference server ready at {}", reference_server.url());
    let reference_boot = api_info_ok(reference_server.url(), reference_server.password()).await;
    let reference_stop = reference_server.stop().await;
    reference_boot.context("reference side: GET /api/info")?;
    reference_stop?;

    // 4. rust boot → /api/info 200 → clean stop.
    let rust = rust_server::spawn_rust(rust_server::RustSpawn::default()).await?;
    eprintln!("[gate0] rust server ready at {}", rust.url());
    let rust_boot = api_info_ok(rust.url(), rust.password()).await;
    let rust_stop = rust.stop().await;
    rust_boot.context("rust side: GET /api/info")?;
    rust_stop?;

    // 5. the §8.3 per-run record.
    let record = serde_json::json!({
        "gate_ran": true,
        "ref_commit": ref_commit,
        "bun_path": bun.display().to_string(),
        "bun_version": bun_version,
        "reference_boot": "ok",
        "rust_boot": "ok",
    });
    eprintln!("[gate0] gate_ran record: {record}");
    if let Ok(path) = std::env::var("PARITY_GATE0_RECORD") {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::write(&path, format!("{record}\n"))
            .with_context(|| format!("write gate0 record {path}"))?;
    }
    Ok(())
}
