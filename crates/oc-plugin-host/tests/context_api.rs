//! PLUG-D11 — `ctx.permission` carries the v2 `PermissionDomain`
//! (list/get/reply) and the calls reach a REAL port through the host
//! bridge (the Rust side stubs the port with canned values; the guest
//! asserts shapes and reports through KV).
//!
//! Hermetic: bun-gated (skip when bun is absent), sandboxed env, no
//! network.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use oc_plugin_host::{PermissionPort, PluginHost, PluginHostConfig};

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn sandbox() -> PathBuf {
    let root =
        PathBuf::from(format!("/tmp/opencode/lane-plugin2-permprobe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("markers")).expect("markers");
    std::env::set_var("OC_WIRING_SANDBOX", root.join("markers"));
    root
}

/// The canned port: one pending request, v2 semantics (wrong session →
/// not found; reply resolves).
struct ProbePort;

#[async_trait]
impl PermissionPort for ProbePort {
    async fn list(&self, session_id: &str) -> Result<Value, String> {
        if session_id != "ses_probe" {
            return Ok(Value::Array(Vec::new()));
        }
        Ok(json!([{
            "id": "per_probe1",
            "sessionID": "ses_probe",
            "action": "bash",
            "resources": ["sh -c true"],
        }]))
    }

    async fn get(&self, session_id: &str, request_id: &str) -> Result<Value, String> {
        if session_id != "ses_probe" || request_id != "per_probe1" {
            return Err(format!("Permission request not found: {request_id}"));
        }
        Ok(json!({
            "id": "per_probe1",
            "sessionID": "ses_probe",
            "action": "bash",
            "resources": ["sh -c true"],
        }))
    }

    async fn reply(
        &self,
        session_id: &str,
        request_id: &str,
        _decision: &str,
        _message: Option<String>,
    ) -> Result<(), String> {
        if session_id != "ses_probe" || request_id != "per_probe1" {
            return Err(format!("Permission request not found: {request_id}"));
        }
        Ok(())
    }
}

/// PLUG-D11: `ctx.permission.list/get/reply` exist, round-trip through
/// the port, and wrong-session gets the v2 not-found error.
#[tokio::test]
async fn permission_methods() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let root = sandbox();
    let fixture = oc_plugin_host::fixture_dir().join("permprobe.ts");
    assert!(fixture.is_file(), "fixture missing: {}", fixture.display());

    let host = PluginHost::detect();
    let config = PluginHostConfig {
        permission_port: Some(Arc::new(ProbePort)),
        registration_timeout: Duration::from_secs(10),
        ..PluginHostConfig::default()
    };
    let host = host.start_detached_with(config, &[fixture]);
    assert!(host.wait_ready(Duration::from_secs(20)), "host came up: {:?}", host.start_error());

    let read = |key: &str| -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(
                root.join("markers").join(format!("{}.json", key.replace(':', "-"))),
            )
            .expect("marker file"),
        )
        .expect("marker json")
    };
    let has = read("perm:has");
    assert_eq!(
        has,
        json!({ "list": true, "get": true, "reply": true }),
        "ctx.permission exposes the v2 PermissionDomain: {has:?}"
    );
    let listed = read("perm:list");
    assert_eq!(listed[0]["id"], json!("per_probe1"), "list returns the pending requests");
    let got = read("perm:get");
    assert_eq!(got["sessionID"], json!("ses_probe"), "get returns the request");
    assert_eq!(read("perm:wrong-session"), json!("not-found"), "wrong session → v2 not-found");
    // The reply marker exists only if the promise resolved (a rejected
    // reply fails setup, so the reads above would have panicked).
    // The bridge encodes a null body as {ok: true} (protocol::response).
    assert_eq!(read("perm:reply"), json!({ "ok": true }), "reply resolves with the null body");

    let _ = std::fs::remove_dir_all(&root);
}
