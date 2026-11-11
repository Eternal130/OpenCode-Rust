//! G-379.7 — the guest rpc CLIENT call face: a guest invokes a
//! registered rpc method (`ctx.rpc.call(rpcID, method, input)` /
//! `ctx.rpc.client(definition)`), the supervisor resolves the
//! registration in the shared registry and routes the call into the
//! OWNING plugin's handler (ref core rpc.ts:108 `Rpc.Service.call` —
//! the in-process registry dispatch behind the promise adapter's
//! `ctx.rpc(definition)` client, adapter.ts:77-113). Typed failures
//! (`rpc.unavailable` / `rpc.method_not_found` /
//! `rpc.invalid_input` / declared error types) reach the caller as
//! catchable thrown errors, never transport errors.

mod common;

use std::time::Duration;

use serde_json::Value;

fn boot(tag: &str) -> Option<oc_plugin_host::PluginHost> {
    let sandbox = common::sandbox(tag);
    let server = common::copy_fixture_as(&sandbox, "rpcserver.ts", "rpcserver.ts");
    let caller = common::copy_fixture_as(&sandbox, "rpccaller.ts", "rpccaller.ts");
    let host = common::host_with(&[server, caller], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(20)));
    Some(host)
}

fn run(host: &oc_plugin_host::PluginHost, command: &str) -> Value {
    let outcome = host
        .execute_command(command, None, Duration::from_secs(10))
        .unwrap_or_else(|error| panic!("{command}: {error}"));
    // The fixture returns a JSON string; decode it.
    let text = outcome.as_str().unwrap_or_else(|| panic!("{command}: non-string {outcome}"));
    serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("{command}: bad json ({error}): {text}"))
}

/// The happy path: plugin B's call reaches plugin A's handler and the
/// typed output comes back (schema-validated host-side, G-296).
#[test]
fn guest_rpc_call_cross_plugin_returns_output() {
    let Some(host) = boot("g377ok") else { return };
    let result = run(&host, "rpcc.call.ok");
    assert_eq!(result, serde_json::json!({ "ok": true, "output": { "value": 42 } }));
    host.shutdown();
}

/// The client constructor face: `ctx.rpc.client(definition)` builds
/// one method function per declared method, all routing through the
/// same call.
#[test]
fn guest_rpc_client_face_routes_declared_methods() {
    let Some(host) = boot("g377client") else { return };
    let result = run(&host, "rpcc.client.ok");
    assert_eq!(result, serde_json::json!({ "ok": true, "output": { "value": 4 } }));
    host.shutdown();
}

/// A declared failure surfaces typed (the `boom` error with its data).
#[test]
fn guest_rpc_call_declared_error_is_typed() {
    let Some(host) = boot("g377err") else { return };
    let result = run(&host, "rpcc.call.declared-error");
    assert_eq!(
        result,
        serde_json::json!({
            "ok": false,
            "type": "boom",
            "message": "the declared failure",
            "data": { "code": 7 },
        })
    );
    host.shutdown();
}

/// A failing input never reaches the handler (`rpc.invalid_input`).
#[test]
fn guest_rpc_call_invalid_input_is_typed() {
    let Some(host) = boot("g377inval") else { return };
    let result = run(&host, "rpcc.call.invalid-input");
    assert_eq!(result["ok"], serde_json::json!(false));
    assert_eq!(result["type"], serde_json::json!("rpc.invalid_input"));
    host.shutdown();
}

/// An undeclared method is `rpc.method_not_found`; an unregistered id
/// is `rpc.unavailable` (the ref registry classification).
#[test]
fn guest_rpc_call_unknown_targets_are_typed_failures() {
    let Some(host) = boot("g377unk") else { return };
    let unknown_method = run(&host, "rpcc.call.unknown-method");
    assert_eq!(unknown_method["type"], serde_json::json!("rpc.method_not_found"));
    let unavailable = run(&host, "rpcc.call.unavailable");
    assert_eq!(unavailable["type"], serde_json::json!("rpc.unavailable"));
    host.shutdown();
}
