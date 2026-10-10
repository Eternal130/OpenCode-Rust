//! Shared helpers for the bun-spawning integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oc_plugin_host::PluginHost;

/// A PID-unique sandbox under `/tmp/opencode` (gate constraint).
pub fn sandbox(name: &str) -> PathBuf {
    let dir =
        PathBuf::from(format!("/tmp/opencode/oc-plugin-host-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("sandbox dir");
    dir
}

/// Copy one committed fixture under a new name (per-instance fixtures
/// from one template — e.g. hookmut-a/hookmut-b).
pub fn copy_fixture_as(sandbox: &Path, name: &str, dest: &str) -> PathBuf {
    let source = oc_plugin_host::fixture_dir().join(name);
    let destination = sandbox.join(dest);
    std::fs::copy(&source, &destination).unwrap_or_else(|error| panic!("copy {name}: {error}"));
    destination
}

/// Copy one committed fixture into the sandbox (tests mutate their
/// copies — hot reload edits must never touch the repo).
pub fn copy_fixture(sandbox: &Path, name: &str) -> PathBuf {
    let source = oc_plugin_host::fixture_dir().join(name);
    let destination = sandbox.join(name);
    std::fs::copy(&source, &destination).unwrap_or_else(|error| panic!("copy {name}: {error}"));
    destination
}

/// Start a host over `entries` with the sandbox env injected.
pub fn host_with(entries: &[PathBuf], sandbox: &Path) -> PluginHost {
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    PluginHost::detect().start_detached_with(config, entries)
}

/// Poll `probe` every 25 ms until it holds or `timeout` elapses.
pub fn wait_for(timeout: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if probe() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    probe()
}

/// Read a small text file (markers).
pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The recording session port (wave 2): the text-level session-engine seam
// ---------------------------------------------------------------------------

use async_trait::async_trait;
use oc_plugin_host::{Bridge, SessionPort};
use serde_json::{json, Value};

/// A [`SessionPort`] that records `session.prompt` calls and — the
/// text-level engine seam — runs the prompt through the plugin
/// session-context hook chain (`Bridge::dispatch_hook`), capturing the
/// rewritten context event. This is exactly the loop the real engine
/// will drive: command → marker prompt → context hooks → model.
///
/// The wave-4 `generate.text` arm answers with a canned summary (the
/// engine seam owns one-off generation — `GenerateApi`).
#[derive(Default)]
pub struct RecordingSessionPort {
    /// (sessionID, text) per admitted prompt.
    pub prompts: std::sync::Mutex<Vec<(String, String)>>,
    /// The post-hook context events (one per prompt).
    pub contexts: std::sync::Mutex<Vec<Value>>,
    /// (prompt, model) per one-off generation.
    pub generates: std::sync::Mutex<Vec<(String, Value)>>,
}

#[async_trait]
impl SessionPort for RecordingSessionPort {
    async fn call(&self, bridge: &Bridge, method: &str, args: &Value) -> Result<Value, String> {
        match method {
            "prompt" => {
                let session_id =
                    args.get("sessionID").and_then(Value::as_str).unwrap_or_default().to_owned();
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default().to_owned();
                self.prompts.lock().unwrap().push((session_id.clone(), text.clone()));
                let mut event = json!({
                    "sessionID": session_id,
                    "system": [{ "type": "text", "text": "" }],
                    "messages": [{
                        "id": "msg_user",
                        "role": "user",
                        "content": [{ "type": "text", "text": text }],
                    }],
                });
                for plugin in bridge.hook_subscribers("session", "context") {
                    event = bridge
                        .dispatch_hook(&plugin, "session", "context", event)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                self.contexts.lock().unwrap().push(event);
                Ok(Value::Null)
            }
            "list" | "context" => Ok(json!([])),
            "messages" => Ok(json!({ "data": [] })),
            "generate.text" => {
                let prompt =
                    args.get("prompt").and_then(Value::as_str).unwrap_or_default().to_owned();
                let model = args.get("model").cloned().unwrap_or(Value::Null);
                self.generates.lock().unwrap().push((prompt.clone(), model));
                Ok(json!(format!("summary of: {prompt}")))
            }
            "get" => Err("no such session in the recording port".into()),
            other => Err(format!("session.{other}: not wired in the recording port")),
        }
    }
}
