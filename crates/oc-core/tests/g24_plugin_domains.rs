//! G-24 — the plugin host's formerly-missing domains on the oc-core
//! side: plugin-registered websearch providers (guest-dispatched
//! execute), plugin-registered integration methods (the host.ts:631
//! `methodImplementation` channel into `IntegrationHost`), and
//! plugin-registered worktree strategies (the G-4 residual — the
//! `StrategyRegistry`'s `add`).

use std::sync::Arc;

use oc_core::plugin_apply::{
    apply_plugin_integration_methods, apply_plugin_websearch_providers,
    apply_plugin_worktree_strategies, GuestHookDispatcher, PluginIntegrationMethodDef,
    PluginWebsearchProviderDef, PluginWorktreeStrategyDef,
};
use serde_json::{json, Value};

/// The mock guest: canned replies per (domain, name) + recorded calls.
struct MockGuest {
    calls: std::sync::Mutex<Vec<(String, String, Value)>>,
}

#[async_trait::async_trait]
impl GuestHookDispatcher for MockGuest {
    async fn dispatch(
        &self,
        plugin_id: &str,
        domain: &str,
        name: &str,
        event: Value,
    ) -> Result<Value, String> {
        self.calls.lock().unwrap().push((
            format!("{domain}.{name}"),
            plugin_id.to_owned(),
            event.clone(),
        ));
        Ok(match (domain, name) {
            ("websearch", "execute") => json!({
                "results": [{ "url": "https://example.invalid/guest", "time": {} }]
            }),
            ("worktree", "create") => json!({ "directory": "/tmp/opencode/wt-guest" }),
            ("worktree", "list") => json!({ "entries": [
                { "directory": "/tmp/opencode/root", "kind": "root" },
                { "directory": "/tmp/opencode/wt-guest", "kind": "worktree" },
            ]}),
            _ => event,
        })
    }
}

fn dispatcher() -> Arc<MockGuest> {
    Arc::new(MockGuest { calls: std::sync::Mutex::new(Vec::new()) })
}

fn shim(guest: &Arc<MockGuest>) -> Arc<dyn GuestHookDispatcher> {
    struct Shim(Arc<MockGuest>);
    #[async_trait::async_trait]
    impl GuestHookDispatcher for Shim {
        async fn dispatch(
            &self,
            plugin_id: &str,
            domain: &str,
            name: &str,
            event: Value,
        ) -> Result<Value, String> {
            self.0.dispatch(plugin_id, domain, name, event).await
        }
    }
    Arc::new(Shim(Arc::clone(guest)))
}

#[tokio::test]
async fn guest_websearch_provider_registers_and_queries() {
    let db = oc_core::database::Database::open_in_memory().await.expect("database");
    let search = oc_core::tool::websearch::WebSearch::new(db, Vec::new());
    let guest = dispatcher();
    let defs = vec![PluginWebsearchProviderDef {
        plugin_id: "gdomains".to_owned(),
        id: "guestsearch".to_owned(),
        name: "Guest Search".to_owned(),
    }];
    assert_eq!(apply_plugin_websearch_providers(&search, &shim(&guest), &defs), 1);
    let providers = search.providers();
    assert!(
        providers.iter().any(|(id, name)| id == "guestsearch" && name == "Guest Search"),
        "guest provider listed: {providers:?}"
    );
    let response = search.query("rust opencode", None, Some("guestsearch")).await.expect("query");
    assert_eq!(response.provider_id.0, "guestsearch");
    assert_eq!(response.results.len(), 1, "one guest result");
    assert_eq!(response.results[0].url, "https://example.invalid/guest");
    let calls = guest.calls.lock().unwrap();
    assert_eq!(calls[0].0, "websearch.execute", "execute crossed into the guest");
    assert_eq!(calls[0].2["query"], json!("rust opencode"));
}

#[tokio::test]
async fn guest_integration_method_registers_into_the_host() {
    let db = Arc::new(oc_core::database::Database::open_in_memory().await.expect("database"));
    let bus = Arc::new(oc_core::bus::Bus::new(
        (*db).clone(),
        oc_core::bus::BusOptions { persist: false, ..oc_core::bus::BusOptions::deterministic() },
    ));
    let host = oc_core::integration::IntegrationHost::empty(db, bus, "opencode-test");
    let guest = dispatcher();
    let defs = vec![PluginIntegrationMethodDef {
        plugin_id: "gdomains".to_owned(),
        registration: json!({
            "integrationID": "acme",
            "method": { "type": "oauth", "id": "acme-oauth", "label": "Acme OAuth" },
            "authorize": true,
            "refresh": false,
        }),
    }];
    assert_eq!(apply_plugin_integration_methods(&host, &shim(&guest), &defs), 1);
    assert!(host.has_integration("acme"), "the method's integration row landed");
    // A malformed registration is skipped containedly, never fatal.
    let broken = vec![PluginIntegrationMethodDef {
        plugin_id: "gdomains".to_owned(),
        registration: json!({ "integrationID": "broken", "method": { "type": "?" } }),
    }];
    assert_eq!(apply_plugin_integration_methods(&host, &shim(&guest), &broken), 0);
    assert!(!host.has_integration("broken"));
}

#[tokio::test]
async fn guest_worktree_strategy_registers_and_creates() {
    let guest = dispatcher();
    let mut registry = oc_core::project::worktree_strategy::StrategyRegistry::new();
    let defs = vec![PluginWorktreeStrategyDef {
        plugin_id: "gdomains".to_owned(),
        id: "pluginwt".to_owned(),
    }];
    assert_eq!(apply_plugin_worktree_strategies(&mut registry, &shim(&guest), &defs), 1);
    assert_eq!(registry.selected(), "pluginwt", "the last registration is selected");
    let strategy = registry.get("pluginwt").expect("guest strategy registered");
    let created = strategy
        .create(&oc_core::project::worktree_strategy::CreateInput {
            source_directory: "/tmp/opencode".into(),
            directory: "/tmp/opencode/wt".into(),
            branch: None,
        })
        .await
        .expect("guest create");
    assert_eq!(created, std::path::PathBuf::from("/tmp/opencode/wt-guest"));
    let entries = strategy.list(std::path::Path::new("/tmp/opencode")).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].kind, "root");
    // The default git strategy survives alongside the plugin's.
    assert!(registry.get("git").is_some(), "git stays registered");
}
