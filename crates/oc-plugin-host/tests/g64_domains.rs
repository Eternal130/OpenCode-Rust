//! G-64 — the newly-bridged Context API domains end-to-end against a
//! REAL bun guest: `ctx.provider.list/get/transform`,
//! `ctx.reference.list/transform`, `ctx.experimental.terminal.read`,
//! the `ctx.integration` method surface (`list` /
//! `connection.active`), and the G-24 residual surfaces
//! (`ctx.vcs.transform`/`reload`, the websearch `editor.default.set`
//! persistence). The mock ports answer the reads; the transforms land
//! in the supervisor's snapshot.

mod common;

use std::path::Path;
use std::time::Duration;

use oc_plugin_host::{
    IntegrationPort, PluginHostConfig, ProviderPort, ReferencePort, TerminalPort,
};
use serde_json::{json, Value};

struct MockProviderPort;

#[async_trait::async_trait]
impl ProviderPort for MockProviderPort {
    async fn list(&self) -> Result<Value, String> {
        Ok(json!([{ "id": "mockprov", "name": "Mock Provider" }]))
    }

    async fn get(&self, id: &str) -> Result<Value, String> {
        if id == "mockprov" {
            Ok(json!({ "id": "mockprov", "name": "Mock Provider" }))
        } else {
            Ok(Value::Null)
        }
    }

    async fn reload(&self) -> Result<(), String> {
        Ok(())
    }
}

struct MockReferencePort;

#[async_trait::async_trait]
impl ReferencePort for MockReferencePort {
    async fn list(&self) -> Result<Value, String> {
        Ok(json!([{ "name": "mockdocs", "path": "/tmp/opencode/mockdocs" }]))
    }

    async fn reload(&self) -> Result<(), String> {
        Ok(())
    }
}

struct MockTerminalPort;

#[async_trait::async_trait]
impl TerminalPort for MockTerminalPort {
    async fn read(&self, session_id: &str, lines: Option<u32>) -> Result<Value, String> {
        Ok(json!({
            "id": "pty_mock",
            "text": format!("terminal of {session_id} ({} lines)", lines.unwrap_or(0)),
        }))
    }
}

struct MockIntegrationPort;

#[async_trait::async_trait]
impl IntegrationPort for MockIntegrationPort {
    async fn call(&self, method: &str, args: &Value) -> Result<Value, String> {
        match method {
            "list" => Ok(json!([{ "id": "mockint", "name": "Mock Integration" }])),
            "get" => Ok(json!({ "id": "mockint", "name": "Mock Integration" })),
            "connection.active" => Ok(json!({
                "type": "credential",
                "id": args.get("id").and_then(Value::as_str).unwrap_or(""),
                "label": "Mock",
                "method": "oauth",
            })),
            other => Err(format!("mock integration port: unsupported method {other:?}")),
        }
    }
}

#[test]
fn g64_domains_register_and_round_trip() {
    let sandbox = common::sandbox("g64domains");
    let entry = common::copy_fixture(&sandbox, "g64domains.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        provider_port: Some(std::sync::Arc::new(MockProviderPort)),
        reference_port: Some(std::sync::Arc::new(MockReferencePort)),
        terminal_port: Some(std::sync::Arc::new(MockTerminalPort)),
        integration_port: Some(std::sync::Arc::new(MockIntegrationPort)),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let read = |name: &str| std::fs::read_to_string(sandbox.join(name)).unwrap_or_default();

    // -- setup-time port reads answered (the sandbox files) ------------
    let providers = read("provider-list.json");
    assert!(providers.contains("mockprov"), "provider.list crossed the port: {providers}");
    assert!(
        read("provider-get.json").contains("Mock Provider"),
        "provider.get crossed the port"
    );
    let references = read("reference-list.json");
    assert!(references.contains("mockdocs"), "reference.list crossed the port: {references}");
    let terminal = read("terminal-read.json");
    assert!(
        terminal.contains("ses_terminal_probe"),
        "experimental.terminal.read crossed the port: {terminal}"
    );
    let integrations = read("integration-list.json");
    assert!(
        integrations.contains("mockint"),
        "integration.list crossed the port: {integrations}"
    );
    let active = read("connection-active.json");
    assert!(
        active.contains("credential"),
        "integration.connection.active crossed the port: {active}"
    );
    assert!(
        !Path::new(&sandbox).join("provider.error").exists()
            && !Path::new(&sandbox).join("integration.error").exists()
            && !Path::new(&sandbox).join("terminal.error").exists(),
        "no domain read failed"
    );

    // -- registration coverage: the transforms materialized natively --
    let snapshot = host.registrations();
    assert!(
        snapshot.providers.iter().any(|def| def.id == "plugprov"),
        "provider.transform registered: {:?}",
        snapshot.providers
    );
    assert!(
        snapshot.references.iter().any(|def| def.name == "plugdocs"),
        "reference.transform registered: {:?}",
        snapshot.references
    );
    assert!(
        snapshot.vcs_providers.iter().any(|provider| provider.id == "plugvcs"),
        "vcs.transform registered: {:?}",
        snapshot.vcs_providers
    );

    // -- the provider definition crossed with its models ---------------
    let plugprov = snapshot
        .providers
        .iter()
        .find(|def| def.id == "plugprov")
        .expect("plugprov registered");
    assert_eq!(
        plugprov.definition["baseURL"], json!("https://plug.invalid/v1"),
        "provider definition rode whole: {}",
        plugprov.definition
    );
    assert_eq!(
        plugprov.definition["models"]["plug-mini"]["name"], json!("Plug Mini"),
        "the models sub-editor landed: {}",
        plugprov.definition
    );
}

/// Without ports bound the reads decline with a clear error (the
/// declining-default contract) instead of wedging the guest.
#[test]
fn g64_domains_decline_without_ports() {
    let sandbox = common::sandbox("g64decline");
    let entry = common::copy_fixture(&sandbox, "g64decline.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let errors = std::fs::read_to_string(sandbox.join("decline.json")).unwrap_or_default();
    assert!(
        errors.contains("no provider registry attached"),
        "provider.list declined clearly: {errors}"
    );
    assert!(
        errors.contains("no integration service attached"),
        "integration.list declined clearly: {errors}"
    );
}
