//! G-384/G-385 — the `ctx.integration.transform` editor CRUD face and
//! `ctx.integration.reload` end-to-end against a REAL bun guest: the
//! editor's reads (list/get/method.list) and reload cross the mock
//! integration port, the mutations land in the supervisor's
//! registration tables (rows with plugin attribution, removal markers,
//! method removals — plus the shared `integration.method.update`
//! path), and the transform's dispose retracts only this plugin's rows.

mod common;

use std::sync::Arc;
use std::time::Duration;

use oc_plugin_host::{IntegrationPort, PluginHostConfig};
use serde_json::{json, Value};

/// The mock integration port: a static registry (the engine side of
/// `ctx.integration.list/get`) that records every call.
struct MockIntegrationPort {
    calls: std::sync::Mutex<Vec<String>>,
}

impl MockIntegrationPort {
    fn new() -> Arc<Self> {
        Arc::new(Self { calls: std::sync::Mutex::new(Vec::new()) })
    }

    fn called(&self, method: &str) -> bool {
        self.calls.lock().expect("calls").iter().any(|call| call == method)
    }
}

#[async_trait::async_trait]
impl IntegrationPort for MockIntegrationPort {
    async fn call(&self, method: &str, args: &Value) -> Result<Value, String> {
        self.calls.lock().expect("calls").push(method.to_owned());
        match method {
            "list" => Ok(json!([
                { "id": "base", "name": "Base", "methods": [{ "type": "key" }], "connections": [] },
                { "id": "gone", "name": "Gone", "methods": [], "connections": [] },
            ])),
            "get" => {
                let id = args.get("integrationID").and_then(Value::as_str).unwrap_or_default();
                match id {
                    "base" => Ok(json!({
                        "id": "base", "name": "Base",
                        "metadata": { "orig": "1" },
                        "methods": [{ "type": "key" }], "connections": [],
                    })),
                    "gone" => Ok(
                        json!({ "id": "gone", "name": "Gone", "methods": [], "connections": [] }),
                    ),
                    _ => Ok(Value::Null),
                }
            }
            // G-384 — the editor's method read face.
            "method.list" => Ok(json!([{ "type": "key" }])),
            // G-385 — invalidate + publish (the engine side); the mock
            // resolves.
            "reload" => Ok(Value::Null),
            other => Err(format!("unexpected integration method {other:?}")),
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)] // single end-to-end domain fixture test
fn g384_integration_transform_editor_and_reload() {
    let sandbox = common::sandbox("g384int");
    let entry = common::copy_fixture(&sandbox, "g384integration.ts");
    let port = MockIntegrationPort::new();
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        integration_port: Some(port.clone() as Arc<dyn IntegrationPort>),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // -- G-385: ctx.integration.reload resolved over the port ----------
    let reload = common::read(&sandbox.join("reload.json"));
    assert_eq!(
        reload,
        "ok",
        "ctx.integration.reload resolved (error: {:?})",
        common::read(&sandbox.join("reload.error"))
    );
    assert!(port.called("reload"), "reload crossed the port");

    // -- the editor reads crossed the port --------------------------------
    let list = common::read(&sandbox.join("list.json"));
    assert!(list.contains("base") && list.contains("gone"), "editor.list: {list}");
    let methods = common::read(&sandbox.join("methods.json"));
    assert!(methods.contains("key"), "editor.method.list: {methods}");
    assert!(port.called("method.list"), "method.list crossed the port");
    let got = common::read(&sandbox.join("got.json"));
    assert!(got.contains("Base"), "editor.get round-trip: {got}");

    // -- the fixture ran to completion (all phases) ----------------------
    assert!(
        common::wait_for(Duration::from_secs(15), || {
            common::read(&sandbox.join("done.json")) == "1"
        }),
        "fixture phases completed"
    );
    let phase1 = common::read(&sandbox.join("phase1.json"));
    assert_eq!(phase1, "1", "phase 1 marker present");

    // -- the mutations landed, then transform #1's dispose retracted
    //    the plugin's rows and transform #2 re-registered one (the
    //    final snapshot proves the retraction path end-to-end) -------
    let snapshot = host.registrations();
    let row_ids: Vec<&str> = snapshot.integrations.iter().map(|def| def.id.as_str()).collect();
    assert_eq!(row_ids, vec!["survivor"], "post-dispose rows: {row_ids:?}");
    assert_eq!(snapshot.integrations[0].name, "Survivor");
    assert!(snapshot.integration_removals.is_empty(), "dispose retracted the removal markers");
    assert!(
        snapshot.integration_method_removals.is_empty(),
        "dispose retracted the method-removal markers"
    );
    // The shared method.update registration (G-24 surface) survives the
    // transform dispose — it is its own registration, not a table row.
    assert!(
        snapshot
            .integration_methods
            .iter()
            .any(|method| method.registration["integrationID"] == json!("acme")),
        "editor.method.update rode the shared integration_methods table"
    );
    let hook_keys: Vec<String> =
        snapshot.hooks.iter().map(|hook| format!("{}.{}", hook.domain, hook.name)).collect();
    for expected in ["integration.authorize", "integration.callback"] {
        assert!(
            hook_keys.iter().any(|key| key == expected),
            "{expected} registered through the shared path: {hook_keys:?}"
        );
    }

    host.shutdown();
}
