//! G-295 — the plugin RPC events.emit host path (ref core/rpc.ts:87-105
//! + eventDefinition :188-201): `rpc.register` returns
//! `{dispose, events: {emit}}`; a declared-event emit publishes the
//! ephemeral `rpc.{id}.{event}` bus event through the embedder's port;
//! an undeclared name rejects the guest promise and publishes nothing.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_plugin_host::{PluginHost, RpcEventPort};
use serde_json::{json, Value};

/// The recording publisher (the host-side seam the engine bus binds).
#[derive(Default)]
struct RecordingRpcEventPort {
    published: Mutex<Vec<(String, String, Value)>>,
}

impl RpcEventPort for RecordingRpcEventPort {
    fn publish(&self, rpc_id: &str, event: &str, data: Value) -> Result<(), String> {
        self.published
            .lock()
            .unwrap()
            .push((rpc_id.to_owned(), event.to_owned(), data));
        Ok(())
    }
}

#[test]
fn declared_emit_publishes_rpc_event() {
    let sandbox = common::sandbox("g295emit");
    let fixture = common::copy_fixture(&sandbox, "rpcevents.ts");
    let port = Arc::new(RecordingRpcEventPort::default());
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        rpc_event_port: Some(port.clone()),
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, std::slice::from_ref(&fixture));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let outcome =
        host.execute_command("rpcev.poke", None, Duration::from_secs(5)).expect("poke executes");
    assert_eq!(outcome, json!("emitted"));
    let published = port.published.lock().unwrap().clone();
    assert_eq!(
        published,
        vec![("evt-rpc".to_owned(), "changed".to_owned(), json!({ "answer": 42 }))],
        "the host published the ephemeral rpc.evt-rpc.changed event with the payload"
    );
    host.shutdown();
}

#[test]
fn undeclared_emit_rejects_and_publishes_nothing() {
    let sandbox = common::sandbox("g295bad");
    let fixture = common::copy_fixture(&sandbox, "rpcevents.ts");
    let port = Arc::new(RecordingRpcEventPort::default());
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        rpc_event_port: Some(port.clone()),
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, std::slice::from_ref(&fixture));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let outcome = host
        .execute_command("rpcev.bad", None, Duration::from_secs(5))
        .expect("bad poke executes");
    assert_eq!(
        outcome,
        json!("rejected:Unknown RPC event: evt-rpc.nope"),
        "the guest promise rejects with the ref's undeclared-event error"
    );
    assert!(
        port.published.lock().unwrap().is_empty(),
        "an undeclared event name publishes nothing"
    );
    host.shutdown();
}

/// A host with NO port bound (the default) surfaces a clear error to
/// the guest instead of silently dropping the emit (the three-state
/// discipline of every optional engine seam).
#[test]
fn unbound_port_declines_the_emit() {
    let sandbox = common::sandbox("g295none");
    let fixture = common::copy_fixture(&sandbox, "rpcevents.ts");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let outcome = host.execute_command("rpcev.poke", None, Duration::from_secs(5));
    let error = outcome.expect_err("the emit rejects when no bus is attached");
    assert!(
        error.contains("no event bus attached"),
        "a clear seam error surfaces, got: {error}"
    );
    host.shutdown();
}
