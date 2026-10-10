//! The V1 plugin-test corpus (wave 4, deliverable 5): 2-3 of v2's own
//! plugin tests, ported against the bun host —
//!
//! * `tool registration/rejection` — core/test/tool-registry.test.ts's
//!   invalid-name/invalid-namespace/reserved-`execute` arms through a
//!   REAL plugin's `ctx.tool.transform` (plus the option-parsing unit
//!   table in `registrations.rs`);
//! * `hook mutation ordering` — v2 hooks run sequentially in
//!   registration order over the SAME mutable event (the essence of
//!   every session-hook test): two plugins' `session.context` edits
//!   compose A-then-B, driven e2e through the session port;
//! * `config-sourced plugin options` — core/test/plugin.test.ts "loads
//!   a local plugin with its configured options": the plugin entry's
//!   `options` reach `ctx.options`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use oc_plugin_host::{PluginHost, PluginHostConfig};
use serde_json::json;

#[test]
fn corpus_tool_registration_rejection() {
    let sandbox = common::sandbox("corpus-toolreg");
    let entry = common::copy_fixture(&sandbox, "toolreg.ts");
    let host = common::host_with(&[entry], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let tools = host.registrations().tools;
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    // The invalid name, the invalid dotted namespace, and the reserved
    // direct `execute` are skipped (v2 `registrationError` + the
    // "Skipping invalid tool registration" notify); the healthy
    // neighbor registers.
    assert_eq!(names, ["healthy"], "only the healthy tool registers: {names:?}");
    assert!(tools[0].codemode, "code-mode-eligible by default");
    host.shutdown();
}

#[test]
fn corpus_hook_mutation_ordering() {
    let sandbox = common::sandbox("corpus-hookmut");
    // Entry order = registration order: A then B.
    let a = common::copy_fixture_as(&sandbox, "hookmut.ts", "hookmut-a.ts");
    let b = common::copy_fixture_as(&sandbox, "hookmut.ts", "hookmut-b.ts");
    let recording = Arc::new(common::RecordingSessionPort::default());
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        session_port: Some(recording.clone()),
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, &[a, b]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let subscribers = {
        let registrations = host.registrations();
        let mut plugins: Vec<String> = registrations
            .hooks
            .iter()
            .filter(|hook| hook.domain == "session" && hook.name == "context")
            .map(|hook| hook.plugin_id.clone())
            .collect();
        plugins.dedup();
        plugins
    };
    assert_eq!(subscribers.len(), 2, "both hookmut plugins subscribe: {subscribers:?}");

    // The e2e drive: plugin A's command prompts through the session
    // port; the port dispatches the context hook chain (every
    // subscriber, registration order, the SAME event object) and
    // captures the result — exactly the loop the engine runs.
    let asked = host.execute_command("hookmut-a.ask", None, Duration::from_secs(5));
    assert!(asked.is_ok(), "the prompt survived the hook chain: {asked:?}");
    let contexts = recording.contexts.lock().unwrap();
    let event = contexts.last().expect("the context event captured");
    let text = event["messages"][0]["content"][0]["text"].as_str().unwrap_or_default();
    assert_eq!(
        text, "original text [mutated-by-A] [mutated-by-B]",
        "sequential same-object mutation in registration order"
    );
    host.shutdown();
}

#[test]
fn corpus_config_sourced_plugin_options() {
    let sandbox = common::sandbox("corpus-options");
    let entry = common::copy_fixture(&sandbox, "greeting.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        entry_options: [(
            entry.display().to_string(),
            json!({ "description": "Configured greeting" }),
        )]
        .into_iter()
        .collect(),
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let snapshot = host.registrations();
    let greet =
        snapshot.commands.iter().find(|command| command.name == "greet").expect("greet registered");
    assert_eq!(greet.description, "Configured greeting");

    // Without options the same fixture degrades like v2's would
    // (`ctx.options.description` is undefined).
    let sandbox2 = common::sandbox("corpus-options-none");
    let entry2 = common::copy_fixture(&sandbox2, "greeting.ts");
    let config2 = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox2.display().to_string())],
        ..PluginHostConfig::default()
    };
    let host2 = PluginHost::detect().start_detached_with(config2, &[entry2]);
    assert!(host2.wait_ready(Duration::from_secs(15)));
    let snapshot2 = host2.registrations();
    let greet2 = snapshot2
        .commands
        .iter()
        .find(|command| command.name == "greet")
        .expect("greet registered without options too");
    assert_eq!(
        greet2.description, "",
        "ctx.options.description is undefined without config options"
    );
    host.shutdown();
    host2.shutdown();
}
