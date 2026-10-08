//! SSE event-sequence scenario goldens against the reference server.
//!
//! Each test boots a fresh reference server, subscribes to `/api/event`
//! from t0, drives a scripted no-model API flow, collects frames until
//! quiescence, and compares the normalized canonical grouping against
//! `goldens/scenarios/<name>.json` (written with `PARITY_UPDATE=1`).
//!
//! All tests skip (pass, with a note) when bun or the reference checkout
//! is missing.

use parity::reference::{self, ReferenceServer};
use parity::scenario::{self, FakeLlm, ScenarioSpec, Step};
use serde_json::json;

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
        eprintln!(
            "skipping {name}: bun or reference checkout not available (see tools/parity/README.md)"
        );
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

/// `session_lifecycle`: create → rename → list → delete a session in the
/// sandbox project; the capture must contain the session.created /
/// session.renamed / session.deleted durable frames (per-aggregate seq
/// order).
#[tokio::test]
async fn scenario_session_lifecycle() -> anyhow::Result<()> {
    scenario_test("session_lifecycle", |server| {
        Box::pin(async move {
            let spec = ScenarioSpec::new(
                "session_lifecycle",
                "POST /api/session (create) -> PATCH rename -> GET list -> DELETE; session lifecycle frames",
            );
            let steps = vec![
                Step::request(
                    "POST",
                    "/api/session",
                    Some(json!({
                        "title": "parity lifecycle",
                        "location": { "directory": "{project_dir}" }
                    })),
                )
                .capture("session_id", "/data/id"),
                Step::request(
                    "PATCH",
                    "/api/session/{session_id}",
                    Some(json!({ "title": "parity renamed" })),
                ),
                Step::request_nobody("GET", "/api/session"),
                Step::request_nobody("DELETE", "/api/session/{session_id}").expect(204),
                Step::expect_frames("session.created", 1),
                Step::expect_frames("session.renamed", 1),
                Step::expect_frames("session.deleted", 1),
            ];
            scenario::drive_and_golden(server, &spec, &steps).await?;
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
            let spec = ScenarioSpec::new(
                "config_touch",
                "boot project location -> create opencode.json -> modify it; config.updated frames",
            );
            let steps = vec![
                Step::request_nobody("GET", "/api/location?location[directory]={project_dir_q}"),
                Step::write_file(
                    "{project_dir}/opencode.json",
                    "{\n  \"username\": \"parity-one\"\n}\n",
                ),
                // Let the watcher's 100 ms reload debounce fire (and the
                // reload's config.updated land) before the second write, so
                // each modification gets its own reload.
                Step::sleep_ms(800),
                Step::write_file(
                    "{project_dir}/opencode.json",
                    "{\n  \"username\": \"parity-two\"\n}\n",
                ),
                Step::expect_frames("config.updated", 2),
            ];
            scenario::drive_and_golden(server, &spec, &steps).await?;
            Ok(())
        })
    })
    .await
}

/// `permission_saved`: the v2 API has no `POST /api/permission/saved`; a
/// saved permission is produced by replying `always` to an `ask`-effect
/// permission request that carries `save` resources (shapes adapted from
/// `packages/protocol/openapi.json`): create session → ask → reply always
/// → list saved → delete the saved row.
#[tokio::test]
async fn scenario_permission_saved() -> anyhow::Result<()> {
    scenario_test("permission_saved", |server| {
        Box::pin(async move {
            let spec = ScenarioSpec::new(
                "permission_saved",
                "ask permission (save) -> reply always -> GET /api/permission/saved -> DELETE row",
            );
            let steps = vec![
                Step::request(
                    "POST",
                    "/api/session",
                    Some(json!({
                        "title": "parity permission",
                        "location": { "directory": "{project_dir}" }
                    })),
                )
                .capture("session_id", "/data/id"),
                // Warm-up ask. Reference quirk (reproduced 5/5): the first
                // permission ask on a freshly booted location is served by
                // a closed Permission service and short-circuits to
                // `deny` — no pending request, no events. A warm-up ask
                // (rules say `allow` on a warm service, `deny` on the cold
                // one; either way frame-less) makes the real ask below
                // deterministic.
                Step::request(
                    "POST",
                    "/api/session/{session_id}/permission",
                    Some(json!({ "action": "read", "resources": ["/etc/parity-warmup"] })),
                ),
                Step::request(
                    "POST",
                    "/api/session/{session_id}/permission",
                    // The default agent permissions allow everything except
                    // `external_directory` outside the sandbox globs (agent
                    // schema defaults + location allows), so this asks.
                    Some(json!({
                        "action": "external_directory",
                        "resources": ["/etc/parity-readonly"],
                        "save": ["/etc/parity-readonly"]
                    })),
                )
                .capture("request_id", "/data/id"),
                Step::request(
                    "POST",
                    "/api/session/{session_id}/permission/{request_id}/reply",
                    Some(json!({ "decision": "always" })),
                )
                .expect(204),
                Step::request_nobody(
                    "GET",
                    "/api/permission/saved?location[directory]={project_dir_q}",
                )
                .capture("saved_id", "/data/0/id"),
                Step::request_nobody("DELETE", "/api/permission/saved/{saved_id}").expect(204),
                Step::expect_frames("permission.asked", 1),
                Step::expect_frames("permission.replied", 1),
            ];
            scenario::drive_and_golden(server, &spec, &steps).await?;
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
            let spec = ScenarioSpec::new(
                "location_reload",
                "boot project location -> POST /api/location/reload; location.shutdown frames",
            );
            let steps = vec![
                Step::request_nobody("GET", "/api/location?location[directory]={project_dir_q}"),
                Step::request_nobody("POST", "/api/location/reload").expect(204),
                Step::expect_frames("location.shutdown", 1),
            ];
            scenario::drive_and_golden(server, &spec, &steps).await?;
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
            let spec = ScenarioSpec::new(
                "prompt_fake_llm",
                "config-seeded openai-compatible provider -> fake local LLM -> prompt/message flow frames",
            );
            let steps = vec![
                Step::write_file("{project_dir}/opencode.json", fake.provider_config()),
                Step::request(
                    "POST",
                    "/api/session",
                    Some(json!({
                        "title": "parity prompt",
                        "location": { "directory": "{project_dir}" },
                        "model": { "providerID": "parityfake", "id": "fake-model" }
                    })),
                )
                .capture("session_id", "/data/id"),
                Step::request(
                    "POST",
                    "/api/session/{session_id}/prompt",
                    Some(json!({ "text": "Say hello." })),
                ),
                Step::expect_frames("session.text.delta", 1),
            ];
            scenario::drive_and_golden(server, &spec, &steps).await?;
            Ok(())
        })
    })
    .await
}
