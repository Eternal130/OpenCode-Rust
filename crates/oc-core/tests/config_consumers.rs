//! CONFIG cluster regressions (ledger CONFIG-C1, CONFIG-C7): the
//! config `compaction` key reaches the runner's compaction service
//! (per-key latest-wins across the location's documents), and the
//! managed-policy runtime executes on both surfaces (provider.use
//! pruning of the config provider bindings + permission denial with
//! the configuration-policy message).
//!
//! Hermetic: sandboxed engine roots under /tmp/opencode, config written
//! into the sandbox project, no network (`catalog_fetch` stays off).

use std::sync::Arc;

use oc_core::engine::{Engine, EngineOptions};
use oc_core::location::LocationRef;
use serde_json::{json, Value};

/// Boots an engine over a sandboxed project, optionally writing a
/// project config document first (`<project>/.opencode/opencode.json`).
async fn engine_with_config(
    config: Option<Value>,
) -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-config-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    if let Some(config) = config {
        std::fs::create_dir_all(dir.path().join(".opencode")).expect("config dir");
        std::fs::write(
            dir.path().join(".opencode").join("opencode.json"),
            serde_json::to_string(&config).expect("config json"),
        )
        .expect("write config");
    }
    let engine = Engine::boot(EngineOptions {
        activity_sweeper: false,
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = LocationRef::new(dir.path().to_path_buf(), None);
    (Arc::new(engine), dir, location)
}

// ---------------------------------------------------------------------------
// CONFIG-C1 — compaction.auto / keep.tokens / buffer reach the runner
// ---------------------------------------------------------------------------

/// `compaction: {auto: false}` (plus keep/buffer) configures the
/// location's compaction service: only the user compacts when auto is
/// off (v2 `ConfigCompactionPlugin`, config/plugin/compaction.ts).
#[tokio::test]
async fn compaction_auto_false() {
    let (engine, _dir, location) = engine_with_config(Some(json!({
        "compaction": {
            "auto": false,
            "keep": { "tokens": 5_000 },
            "buffer": 32_000
        }
    })))
    .await;
    let services = engine.location(location).await.expect("location assembles");

    let settings = services.runner.compaction().settings();
    assert!(!settings.auto, "auto:false disables automatic compaction");
    assert_eq!(settings.keep, 5_000, "keep.tokens is the verbatim window");
    assert_eq!(settings.buffer, Some(32_000), "buffer reserves below the limit");

    // Absent config: the service keeps the defaults (auto on, 10% free,
    // 15k verbatim).
    let (engine, _dir, location) = engine_with_config(None).await;
    let services = engine.location(location).await.expect("location assembles");
    let settings = services.runner.compaction().settings();
    assert!(settings.auto, "default keeps automatic compaction on");
    assert_eq!(settings.buffer, None);
    assert_eq!(settings.keep, 15_000);
}

/// The per-key spread semantics (`editor.configure({...})` per
/// document): a document that only mentions `auto` never resets a
/// previous document's `keep`/`buffer` — latest document owning a key
/// wins that key alone (`Config.latest` per key, not per section).
#[tokio::test]
async fn compaction_settings_merge_per_key_across_documents() {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-config-merge-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    // Global document owns keep + buffer; the project document owns auto.
    let global = dir.path().join("home").join(".config").join("opencode");
    std::fs::create_dir_all(&global).expect("global config dir");
    std::fs::write(
        global.join("opencode.json"),
        serde_json::to_string(&json!({
            "compaction": { "keep": { "tokens": 9_000 }, "buffer": 12_000 }
        }))
        .expect("json"),
    )
    .expect("global config");
    std::fs::create_dir_all(dir.path().join(".opencode")).expect("project config dir");
    std::fs::write(
        dir.path().join(".opencode").join("opencode.json"),
        serde_json::to_string(&json!({ "compaction": { "auto": false } })).expect("json"),
    )
    .expect("project config");

    let engine = Engine::boot(EngineOptions {
        activity_sweeper: false,
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = LocationRef::new(dir.path().to_path_buf(), None);
    let services = engine.location(location).await.expect("location assembles");
    let settings = services.runner.compaction().settings();
    assert!(!settings.auto, "project doc wins auto");
    assert_eq!(settings.keep, 9_000, "global doc keeps keep.tokens");
    assert_eq!(settings.buffer, Some(12_000), "global doc keeps buffer");
}
