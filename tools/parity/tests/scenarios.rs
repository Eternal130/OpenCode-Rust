//! SSE event-sequence scenario goldens against the reference server.
//!
//! Each test boots a fresh reference server, subscribes to `/api/event`
//! from t0, drives a scripted no-model API flow, collects frames until
//! quiescence, and compares the normalized canonical grouping against
//! `goldens/scenarios/<name>.json` (written with `PARITY_UPDATE=1`).
//!
//! The scripts themselves live in [`parity::scenario::scripts`] — the
//! SAME scripts replay against the Rust port in `tests/rust_scenarios.rs`
//! (M1 parity gate).
//!
//! All tests skip (pass, with a note) when bun or the reference checkout
//! is missing — unless `OC_GATE_STRICT=1`, where a missing prerequisite
//! FAILS the test instead (Gate 0; see `parity::gate`).

use parity::reference::{self, ReferenceServer};
use parity::scenario::{self, scripts, FakeLlm};

/// The boot → run → shutdown dance every scenario test uses (mirrors
/// `reference_info.rs`: the scenario result and the shutdown result must
/// both be ok).
async fn scenario_test(
    name: &str,
    body: impl FnOnce(
        &ReferenceServer,
    )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + '_>>,
) -> anyhow::Result<()> {
    if reference::skip_if_no_bun() {
        return Ok(());
    }
    let server = reference::spawn().await?;
    eprintln!("[parity:{name}] server ready at {}", server.url());
    let result = body(&server).await;
    let shutdown = server.stop().await;
    result?;
    shutdown?;
    Ok(())
}

/// Runs the named canonical script against `server` and goldens it.
async fn replay(
    server: &ReferenceServer,
    name: &str,
    fake: Option<&FakeLlm>,
) -> anyhow::Result<()> {
    let script =
        scripts::build(name, fake).ok_or_else(|| anyhow::anyhow!("unknown scenario {name}"))?;
    scenario::drive_and_golden(server, &script.spec, &script.steps).await?;
    Ok(())
}

/// `session_lifecycle`: create → rename → list → delete a session in the
/// sandbox project; the capture must contain the session.created /
/// session.renamed / session.deleted durable frames (per-aggregate seq
/// order).
#[tokio::test]
async fn scenario_session_lifecycle() -> anyhow::Result<()> {
    scenario_test("session_lifecycle", |server| {
        Box::pin(async move {
            replay(server, "session_lifecycle", None).await?;
            Ok(())
        })
    })
    .await
}

/// `config_touch`: boot the sandbox project location (which starts the
/// config watcher), then create and modify `opencode.json` in the project
/// directory. Each change reloads after the watcher's 100 ms debounce and
/// publishes `config.updated`; collection uses a generous quiescence window
/// so the debounce never races the golden.
#[tokio::test]
async fn scenario_config_touch() -> anyhow::Result<()> {
    scenario_test("config_touch", |server| {
        Box::pin(async move {
            replay(server, "config_touch", None).await?;
            Ok(())
        })
    })
    .await
}

/// `permission_saved`: the v2 API has no `POST /api/permission/saved`; a
/// saved permission is produced by replying `always` to an `ask`-effect
/// permission request that carries `save` resources. Reference quirk
/// (reproduced 5/5): the first ask on a freshly booted location
/// short-circuits to `deny` (closed `Permission` service) — the warm-up
/// ask in the script makes the real ask deterministic. See the script
/// docs in `parity::scenario::scripts` for how the port's warm open
/// service reaches the same frame-less outcome.
#[tokio::test]
async fn scenario_permission_saved() -> anyhow::Result<()> {
    scenario_test("permission_saved", |server| {
        Box::pin(async move {
            replay(server, "permission_saved", None).await?;
            Ok(())
        })
    })
    .await
}

/// `location_reload`: boot the sandbox project location, then reload all
/// locations — the reference publishes `location.shutdown` per loaded
/// location and responds once the replacement services have settled.
#[tokio::test]
async fn scenario_location_reload() -> anyhow::Result<()> {
    scenario_test("location_reload", |server| {
        Box::pin(async move {
            replay(server, "location_reload", None).await?;
            Ok(())
        })
    })
    .await
}

/// `prompt_fake_llm` (bonus, deterministic fake provider — no LLM keys):
/// seed a config-defined `openai-compatible` provider pointing at a local
/// fake `OpenAI` chat server ([`FakeLlm`]), then run one full prompt →
/// assistant-message flow. The fake's SSE script is byte-identical every
/// run, so the whole session event sequence is deterministic. See the
/// `scenario` module docs for why `OPENCODE_SIMULATE` was not used.
#[tokio::test]
async fn scenario_prompt_fake_llm() -> anyhow::Result<()> {
    scenario_test("prompt_fake_llm", |server| {
        Box::pin(async move {
            let fake = FakeLlm::start().await?;
            replay(server, "prompt_fake_llm", Some(&fake)).await?;
            Ok(())
        })
    })
    .await
}
