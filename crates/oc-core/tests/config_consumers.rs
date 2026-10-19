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
use oc_core::permission::{AssertInput, PermissionError};
use oc_core::session::store::CreateInput as SessionCreateInput;
use oc_schema::id::SessionId;
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

// ---------------------------------------------------------------------------
// CONFIG-C7 — managed-policy runtime: both execution surfaces
// ---------------------------------------------------------------------------

/// A config-seeded custom provider entry (the shape `providers.<id>`
/// takes in the project config).
fn custom_provider(id: &str, model: &str) -> Value {
    json!({
        "name": id,
        "package": "@opencode/ai/providers/openai-compatible",
        "settings": { "baseURL": "https://synth.invalid/v1" },
        "models": {
            model: {
                "name": model,
                "cost": [ { "input": 0, "output": 0 } ],
                "limit": { "context": 60000, "output": 8000 }
            }
        }
    })
}

/// A session pre-approved for every action (allow-all session rules) —
/// the managed-policy deny must override even that (the evaluate hook
/// runs on the non-short-circuited path, policy.ts:54-67).
async fn allow_all_session(engine: &Engine, location: &LocationRef, id: &str) -> SessionId {
    let session_id =
        SessionId::parse(id.to_string()).unwrap_or_else(|error| SessionId(error.value));
    engine
        .create_session(
            location,
            SessionCreateInput {
                id: Some(session_id.clone()),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath::parse(
                        location.directory.to_string_lossy().into_owned(),
                    ),
                    workspace_id: None,
                }),
                title: Some("Config consumers test".to_string()),
                agent: None,
                model: None,
                metadata: None,
                permissions: Some(vec![oc_schema::permission::Rule {
                    action: "*".into(),
                    resource: "*".into(),
                    effect: oc_schema::permission::Effect::Allow,
                }]),
                parent_id: None,
                project: None,
                version: "test".to_string(),
            },
        )
        .await
        .expect("create session");
    session_id
}

/// The `permission` deny surface: an `experimental.policies`
/// `permission` statement denies matching `{action}:{resource}`
/// requests through the real permission pipeline — with the
/// configuration-policy message — while non-matching requests pass.
#[tokio::test]
async fn managed_policy_deny() {
    let (engine, _dir, location) = engine_with_config(Some(json!({
        "experimental": {
            "policies": [
                { "action": "permission", "resource": "edit:secret/*", "effect": "deny" }
            ]
        }
    })))
    .await;
    let services = engine.location(location.clone()).await.expect("location assembles");
    let session = allow_all_session(&engine, &location, "ses_policy_1").await;

    // The deny verdict (even the allow-all session rules cannot save it).
    let error = services
        .permission
        .assert(AssertInput::new(session.clone(), "edit", vec!["secret/keys.rs".to_owned()]))
        .await
        .expect_err("policy denies");
    match error {
        PermissionError::Blocked(blocked) => {
            assert_eq!(blocked.reason.as_deref(), Some("Blocked by configuration policy"));
        }
        other => panic!("expected blocked, got {other:?}"),
    }

    // A non-matching resource never hits the policy: allow-all stands.
    services
        .permission
        .assert(AssertInput::new(session, "edit", vec!["src/main.rs".to_owned()]))
        .await
        .expect("non-matching resource is untouched by the policy");
}

/// The `provider.use` deny surface: a denied provider never binds —
/// the config-seeded custom provider is pruned from the location's
/// resolver, so resolving one of its models fails with the policy
/// message while a sibling provider still resolves.
#[tokio::test]
async fn managed_policy_provider_use_prunes_bindings() {
    let (engine, _dir, location) = engine_with_config(Some(json!({
        "experimental": {
            "policies": [
                { "action": "provider.use", "resource": "banned-*", "effect": "deny" }
            ]
        },
        "providers": {
            "banned-own": custom_provider("banned-own", "b-model"),
            "allowed-own": custom_provider("allowed-own", "a-model")
        }
    })))
    .await;
    let services = engine.location(location.clone()).await.expect("location assembles");

    let row = session_row(&location);
    let banned = model_ref("banned-own", "b-model");
    let error = services
        .resolver
        .resolve_selected(&row, &banned)
        .expect_err("denied provider never resolves");
    assert!(
        error.to_string().contains("provider.use deny"),
        "the policy message surfaces, got: {error}"
    );

    let allowed = model_ref("allowed-own", "a-model");
    let resolved = services
        .resolver
        .resolve_selected(&row, &allowed)
        .expect("the sibling provider still binds");
    assert_eq!(resolved.reference.provider_id.as_str(), "allowed-own");
}

/// The v2 wire literal (`provider.use`) decodes — and the historical
/// kebab-case spelling still does (CONFIG-C7's "literal provider.use"
/// clause: authored documents with the v2 literal are the source, the
/// v1 migration products keep the alias).
#[test]
fn managed_policy_action_literal_is_provider_use() {
    let v2 = serde_json::from_value::<oc_schema::config::policy::Policy>(json!({
        "action": "provider.use", "resource": "anthropic", "effect": "deny"
    }))
    .expect("the v2 literal decodes");
    assert_eq!(v2.action, oc_schema::config::policy::Action::ProviderUse);
    let legacy = serde_json::from_value::<oc_schema::config::policy::Policy>(json!({
        "action": "provider-use", "resource": "anthropic", "effect": "deny"
    }))
    .expect("the historical spelling still decodes");
    assert_eq!(legacy.action, oc_schema::config::policy::Action::ProviderUse);
    // And serializes back as the v2 literal.
    let wire = serde_json::to_value(&v2).expect("serialize");
    assert_eq!(wire["action"], "provider.use");
}

/// A minimal session row for resolver calls.
fn session_row(location: &LocationRef) -> oc_core::database::schema::SessionRow {
    oc_core::database::schema::SessionRow::new(
        "ses_resolver_1",
        "global",
        &location.directory.to_string_lossy(),
        "config-consumers",
        "test",
    )
    .expect("session row")
}

/// A model reference for resolver calls.
fn model_ref(provider: &str, model: &str) -> oc_schema::model::Ref {
    oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse(provider),
        id: oc_schema::id::ModelId::parse(model),
        variant: None,
    }
}
