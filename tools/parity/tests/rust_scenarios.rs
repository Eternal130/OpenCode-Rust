//! M1 scenario parity: the SAME five goldens scenarios
//! (`parity::scenario::scripts`) replayed against the **Rust server**
//! (`oc-server` binary) and compared to the reference goldens captured
//! against v2.0.24.
//!
//! ## Comparison policy
//!
//! The Rust capture is compared against the reference golden through
//! [`parity::sse::compare_scenario_captures`] after removing an explicit,
//! per-scenario **skip-list** of frame types from BOTH sides — the frames
//! the Rust server legitimately lacks (or the reference legitimately has
//! as service-notify noise the port does not yet emit). Everything else
//! must match exactly: group structure, per-group frame multisets, and
//! frame payload shapes (each side normalized through its own
//! [`parity::normalize::NormEnv`]).
//!
//! ## Skip-lists (documented divergences)
//!
//! - `session_lifecycle` / `permission_saved` / `location_reload`: none —
//!   exact parity (the announcer publishes the boot/reload cascades).
//! - `config_touch`: none — the poller publishes `config.updated` + the
//!   deterministic service cascade per change.
//! - `prompt_fake_llm`: `session.instructions.updated` — the reference
//!   renders per-instruction sha256 digests of markdown instructions
//!   through its instruction pipeline; the port's instruction digest
//!   stage is not yet wired to the same sources, so the frame's `delta`
//!   map differs in content (the frame itself IS emitted). Also any
//!   `mcp.*` / plugin frames would be skipped if they ever appear in a
//!   golden (documented port gap).
//!
//! Every run writes its capture as an artifact under
//! `/tmp/opencode/m1-artifacts/scenarios/` for inspection.

use parity::reference::SANDBOX_ROOT;
use parity::rust_server::{self, RustSpawn};
use parity::scenario::{self, scripts, FakeLlm, ScenarioHost};
use parity::sse;
use serde_json::Value;

/// The per-scenario documented skip-list (frame types removed from both
/// the golden and the Rust capture before comparing). Empty everywhere:
/// the built-in skills port (gate-6 F3) closed the last documented
/// divergence — `prompt_fake_llm` compares fully exact, zero skips.
fn skips_for(_name: &str) -> &'static [&'static str] {
    &[]
}

/// Boot the Rust server, run the named script, compare against the golden
/// on the intersection.
async fn rust_scenario(name: &str) -> anyhow::Result<()> {
    if rust_server::skip_if_no_cargo() {
        eprintln!("skipping {name}: cargo not found on PATH");
        return Ok(());
    }
    let fake = if name == "prompt_fake_llm" { Some(FakeLlm::start().await?) } else { None };
    let mut options = RustSpawn::default();
    if name == "prompt_fake_llm" {
        // The provider config must be visible at boot (the engine binds
        // custom providers at boot): seed it pre-spawn and point
        // OPENCODE_CONFIG at it ({sandbox} expands to the server sandbox).
        let fake = fake.as_ref().expect("fake started");
        options
            .seed_files
            .push((std::path::PathBuf::from("project/opencode.json"), fake.provider_config()));
        options.extra_env.push(("OPENCODE_CONFIG", "{sandbox}/project/opencode.json"));
    }
    let server = rust_server::spawn_rust(options).await?;
    eprintln!("[m1:{name}] rust server ready at {}", server.url());
    let result = run_and_compare(&server, name, fake.as_ref()).await;
    let shutdown = server.stop().await;
    result?;
    shutdown?;
    Ok(())
}

async fn run_and_compare(
    server: &rust_server::RustServer,
    name: &str,
    fake: Option<&FakeLlm>,
) -> anyhow::Result<()> {
    let script =
        scripts::build(name, fake).ok_or_else(|| anyhow::anyhow!("unknown scenario {name}"))?;
    let outcome = scenario::drive(server, &script.spec, &script.steps).await?;

    // Evidence artifact: the raw Rust-side capture.
    let artifact_dir = std::path::PathBuf::from(
        std::env::var("PARITY_SANDBOX_ROOT").unwrap_or_else(|_| SANDBOX_ROOT.to_owned()),
    )
    .join("../m1-artifacts/scenarios");
    std::fs::create_dir_all(&artifact_dir).ok();
    let artifact = artifact_dir.join(format!("{name}.json"));
    std::fs::write(
        &artifact,
        format!("{}\n", serde_json::to_string_pretty(&Value::Array(outcome.groups.clone()))?),
    )
    .ok();
    eprintln!("[m1:{name}] capture artifact: {}", artifact.display());

    // Golden side.
    let golden_path = scenario::golden_dir().join(format!("{name}.json"));
    let golden_text = std::fs::read_to_string(&golden_path)?;
    let golden: Value = serde_json::from_str(&golden_text)?;
    let golden_groups = golden
        .get("groups")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("golden {name} has no groups"))?;

    let skips = skips_for(name);
    let golden_filtered = sse::filter_capture_types(&golden_groups, skips);
    let actual_filtered = sse::filter_capture_types(&outcome.groups, skips);
    if let Some(diff) = sse::compare_scenario_captures(&golden_filtered, &actual_filtered) {
        anyhow::bail!(
            "rust scenario mismatch for {name} ({} golden groups vs {} captured, skips {skips:?}); {diff}",
            golden_filtered.len(),
            actual_filtered.len()
        );
    }
    Ok(())
}

#[tokio::test]
async fn m1_scenario_session_lifecycle() -> anyhow::Result<()> {
    rust_scenario("session_lifecycle").await
}

#[tokio::test]
async fn m1_scenario_config_touch() -> anyhow::Result<()> {
    rust_scenario("config_touch").await
}

#[tokio::test]
async fn m1_scenario_permission_saved() -> anyhow::Result<()> {
    rust_scenario("permission_saved").await
}

#[tokio::test]
async fn m1_scenario_location_reload() -> anyhow::Result<()> {
    rust_scenario("location_reload").await
}

#[tokio::test]
async fn m1_scenario_prompt_fake_llm() -> anyhow::Result<()> {
    rust_scenario("prompt_fake_llm").await
}
