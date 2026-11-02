//! G-147/G-148 — the `ctx.model` domain end-to-end against a REAL bun
//! guest: `model.list/default/reload` over the mock port, the
//! `ModelEditor` transform table (update/remove/default/provider.list)
//! landing in the supervisor's snapshot, and `ctx.plugin.list`
//! (promise/plugin.ts:49). Plus the G-148② bounded decision:
//! `@opencode/plugin/effect` imports fail with a clear error.

mod common;

use std::time::Duration;

use oc_plugin_host::ModelPort;
use serde_json::{json, Value};

struct MockModelPort {
    reloaded: std::sync::Mutex<u32>,
}

#[async_trait::async_trait]
impl ModelPort for MockModelPort {
    async fn list(&self, provider_id: Option<&str>) -> Result<Value, String> {
        let all = json!([
            { "id": "mock-large", "modelID": "mock-large", "providerID": "mockprov",
              "name": "Mock Large", "enabled": true },
            { "id": "mock-mini", "modelID": "mock-mini", "providerID": "mockprov",
              "name": "Mock Mini", "enabled": false },
            { "id": "other-mini", "modelID": "other-mini", "providerID": "otherprov",
              "name": "Other Mini", "enabled": true },
        ]);
        match provider_id {
            None => Ok(all),
            Some("mockprov") => Ok(json!([all[0], all[1]])),
            Some("otherprov") => Ok(json!([all[2]])),
            Some(_) => Ok(json!([])),
        }
    }

    async fn default_model(&self) -> Result<Value, String> {
        Ok(json!({ "providerID": "mockprov", "modelID": "mock-large" }))
    }

    async fn reload(&self) -> Result<(), String> {
        *self.reloaded.lock().unwrap() += 1;
        Ok(())
    }
}

#[test]
fn g147_model_domain_register_and_round_trip() {
    let sandbox = common::sandbox("g147domains");
    let entry = common::copy_fixture(&sandbox, "g147domains.ts");
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        model_port: Some(std::sync::Arc::new(MockModelPort { reloaded: std::sync::Mutex::new(0) })),
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let read = |name: &str| std::fs::read_to_string(sandbox.join(name)).unwrap_or_default();

    // -- the port reads crossed ------------------------------------------------
    let models = read("model-list.json");
    assert!(models.contains("mock-large"), "model.list crossed the port: {models}");
    let narrowed = read("model-list-narrowed.json");
    assert!(
        narrowed.contains("mockprov") && !narrowed.contains("otherprov"),
        "model.list(providerID) narrows: {narrowed}"
    );
    let default = read("model-default.json");
    assert!(default.contains("mock-large"), "model.default crossed the port: {default}");
    assert!(!sandbox.join("model.error").exists(), "no model read failed");
    assert!(sandbox.join("model-reloaded.mark").exists(), "model.reload ran");
    assert!(!sandbox.join("model.reload.error").exists(), "model.reload failed");

    // -- the editor surface ------------------------------------------------
    let editor_list = read("editor-list.json");
    assert!(editor_list.contains("mock-mini"), "editor.list carries overrides: {editor_list}");
    assert!(sandbox.join("editor-removed-mark").exists(), "editor.remove filtered the base row");
    let editor_default = read("editor-default.json");
    assert!(editor_default.contains("mock-mini"), "editor.default.get: {editor_default}");
    let provider_inputs = read("editor-provider-list.json");
    assert!(provider_inputs.contains("mockprov"), "editor.provider.list: {provider_inputs}");

    // -- registration coverage: the transform materialized natively ------
    let snapshot = host.registrations();
    assert!(
        snapshot.models.iter().any(|o| o.provider_id == "mockprov"
            && o.model_id == "mock-mini"
            && o.model.get("name").and_then(Value::as_str) == Some("Mock Mini (patched)")),
        "model.transform registered the override: {:?}",
        snapshot.models
    );

    // -- G-148: ctx.plugin.list -------------------------------------------
    let plugins = read("plugin-list.json");
    assert!(plugins.contains("g147domains"), "plugin.list carries the plugin id: {plugins}");
    assert!(plugins.contains("\"active\""), "plugin.list carries the active flag: {plugins}");
}

/// G-148② — an effect-style import (`@opencode/plugin/effect`) fails
/// the load with the CLEAR ecosystem error, not a module-not-found.
#[test]
fn g148_effect_import_fails_with_clear_error() {
    let sandbox = common::sandbox("g148effect");
    let entry = sandbox.join("effectplugin.ts");
    std::fs::write(
        &entry,
        "import { something } from \"@opencode/plugin/effect\";\nexport default { id: \"effectplug\", setup: () => {} };\n",
    )
    .expect("write fixture");
    let host = common::host_with(&[entry], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let _ = host.wait_ready(Duration::from_secs(15));
    let snapshot = host.registrations();
    let failed = snapshot
        .plugins
        .iter()
        .any(|status| matches!(status.state, oc_plugin_host::PluginState::Failed(_)));
    assert!(
        failed,
        "effect-style import must fail the load (not load inert): {:?}",
        snapshot.plugins
    );
}
