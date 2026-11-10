//! G-296 — RPC schema validation + declared-error encode/decode (ref
//! core/rpc.ts:115-141, :200-267): input validated before dispatch
//! (`rpc.invalid_input`, handler never runs), output validated after
//! (`rpc.invalid_output`), `ctx.error` gated against the DECLARED
//! error types (undeclared ⇒ the ref's die path ⇒ `rpc.internal` +
//! log), declared error data validated (invalid ⇒ `rpc.internal`).

mod common;

use std::time::Duration;

use oc_plugin_host::{PluginHost, RpcOutcome};
use serde_json::json;

fn call(host: &PluginHost, input: serde_json::Value) -> RpcOutcome {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(host.rpc_call_async("schema-rpc", "compute", input, Duration::from_secs(5)))
        .expect("rpc call dispatches")
}

fn handler_ran(sandbox: &std::path::Path) -> String {
    common::read(&sandbox.join("rpc-schema-handler-ran.txt"))
}

#[test]
fn rpc_schema_validation_legs() {
    let sandbox = common::sandbox("g296schema");
    let fixture = common::copy_fixture(&sandbox, "rpcschema.ts");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // Invalid input (wrong type): rpc.invalid_input, handler NOT run.
    assert_eq!(
        call(&host, json!({ "n": "not a number" })),
        RpcOutcome::Failure {
            type_: "rpc.invalid_input".into(),
            message: "Invalid RPC input: n Expected number, received string".into(),
            data: None,
        }
    );
    // Invalid input (undeclared extra property): same leg.
    assert!(matches!(
        call(&host, json!({ "n": 1, "extra": true })),
        RpcOutcome::Failure { type_, .. } if type_ == "rpc.invalid_input"
    ));
    assert_eq!(handler_ran(&sandbox), "0", "an invalid input never reaches the handler");

    // Happy path unchanged.
    assert_eq!(call(&host, json!({ "n": 5 })), RpcOutcome::Output(Some(json!({ "doubled": 10 }))));
    assert_eq!(handler_ran(&sandbox), "1");

    // Output violating the schema ⇒ rpc.invalid_output (the internal
    // 500 leg, like a transport failure — internals never echo).
    assert!(matches!(call(&host, json!({ "n": 2 })), RpcOutcome::Internal(message) if message.contains("Invalid RPC output")));

    // ctx.error with a DECLARED type + valid data ⇒ the typed failure
    // carries the data.
    assert_eq!(
        call(&host, json!({ "n": 0 })),
        RpcOutcome::Failure {
            type_: "boom".into(),
            message: "it broke".into(),
            data: Some(json!({ "why": "because" })),
        }
    );

    // ctx.error with an UNDECLARED type ⇒ the ref-equivalent outcome:
    // encodeError dies ⇒ catchDefect ⇒ rpc.internal (500 leg), logged
    // guest-side — never an undeclared typed failure.
    assert_eq!(
        call(&host, json!({ "n": 1 })),
        RpcOutcome::Internal("RPC call failed".into())
    );
    host.shutdown();
}

/// Declared-error DATA validation (the encode-side schema check,
/// enforced at the host boundary like the ref's `encodeError`): data
/// violating the declared error's schema degrades to `rpc.internal`.
#[test]
fn declared_error_data_is_validated() {
    let sandbox = common::sandbox("g296errdata");
    let fixture = common::copy_fixture(&sandbox, "rpcschema.ts");
    // Make the declared-error payload violate the schema: `why` must
    // be a string; the edit sends a number.
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    std::fs::write(
        &fixture,
        source.replace(
            "call.error(\"boom\", \"it broke\", { why: \"because\" })",
            "call.error(\"boom\", \"it broke\", { why: 7 })",
        ),
    )
    .expect("edit fixture");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    assert!(
        matches!(call(&host, json!({ "n": 0 })), RpcOutcome::Internal(message) if message.contains("Invalid RPC error data")),
        "invalid declared-error data degrades to the internal leg"
    );
    host.shutdown();
}
