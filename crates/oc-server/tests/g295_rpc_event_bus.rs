//! G-295 — the end-to-end `rpc.events.emit` path: a bun guest plugin
//! registers an RPC definition with a declared event and emits it; the
//! oc-server `RpcEventPort` binding must publish the ephemeral
//! `rpc.{id}.{event}` event on the ENGINE bus with the payload, scoped
//! to the boot location, and the type must stay SSE-public (`rpc.*`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

mod support;

/// The events fixture (staged from oc-plugin-host's fixture dir).
fn events_fixture() -> PathBuf {
    let path = oc_plugin_host::fixture_dir().join("rpcevents.ts");
    assert!(path.is_file(), "fixture missing: {}", path.display());
    path
}

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

struct Sandbox {
    home: PathBuf,
    project: PathBuf,
    data_root: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root = PathBuf::from(format!("/tmp/opencode/oc-server-g295-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    Sandbox { home, project, data_root: root.join("data") }
}

impl Sandbox {
    fn write_project_config(&self, plugin_path: &Path) {
        std::fs::write(
            self.project.join("opencode.json"),
            json!({ "plugins": [plugin_path.display().to_string()] }).to_string(),
        )
        .expect("config");
    }

    fn redirect_env(&self) {
        std::env::set_var("HOME", &self.home);
        std::env::set_var("XDG_CONFIG_HOME", self.home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", self.home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", self.home.join(".local").join("share"));
        std::env::set_var("XDG_STATE_HOME", self.home.join(".local").join("state"));
        std::env::set_var("OPENCODE_CONFIG_DIR", self.home.join(".config").join("opencode-rust"));
        std::env::remove_var("OPENCODE_CONFIG");
        std::env::set_var("OPENCODE_CATALOG_FETCH", "0");
    }

    fn boot_options(&self) -> BootOptions {
        BootOptions {
            serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
            password: None,
            cors_origins: Vec::new(),
            version: "2.0.24".to_owned(),
            data_root: self.data_root.clone(),
            home: self.home.clone(),
            config_location: Some(self.project.clone()),
            events_persist: None,
        }
    }
}

/// The SSE visibility precondition (`event_manifest::RPC_PREFIX`): every
/// `rpc.*` type is public — the emitted event must reach subscribers
/// exactly like the ref's.
#[test]
fn rpc_types_are_sse_public() {
    assert!(oc_server::sse::is_public_event("rpc.evt-rpc.changed"));
    assert!(!oc_server::sse::is_public_event("plugin.internal"));
}

#[tokio::test]
async fn rpc_event_emit_reaches_the_engine_bus() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("emit");
    box_.write_project_config(&events_fixture());
    box_.redirect_env();
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");

    // A raw bus listener captures every `rpc.*` frame with its location.
    let captured: Arc<Mutex<Vec<oc_core::bus::Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let listener: oc_core::bus::Listener = Arc::new(move |event: &oc_core::bus::Event| {
        if event.type_.starts_with("rpc.") {
            sink.lock().unwrap().push(event.clone());
        }
        Ok(())
    });
    let _guard = handle.state.engine.bus.listen(listener);

    // The plugin's command emits the DECLARED event (the fixture also
    // proves the undeclared name rejects guest-side — pinned in the
    // oc-plugin-host g295 tests; here the engine-bus leg is the point).
    let boot = handle.state.plugins.as_ref().expect("plugin boot attached");
    let outcome = boot
        .host
        .execute_command_in_session_async("rpcev.poke", None, "", Duration::from_secs(10))
        .await
        .expect("rpcev.poke dispatches");
    assert_eq!(outcome, json!("emitted"));

    // The engine bus received `rpc.evt-rpc.changed` with the payload,
    // scoped to the boot location (the ref's per-registration location
    // scope, approximated by the boot location — PLUG-D07 precedent).
    let frames = captured.lock().unwrap().clone();
    assert_eq!(frames.len(), 1, "exactly one rpc event published: {frames:?}");
    let event = &frames[0];
    assert_eq!(event.type_, "rpc.evt-rpc.changed");
    assert_eq!(event.data, json!({ "answer": 42 }));
    assert!(event.durable.is_none(), "the rpc event is ephemeral");
    let location = event.location.as_ref().expect("the event is location-scoped");
    assert!(
        Path::new(location.directory.0.as_str()).canonicalize().ok()
            == box_.project.canonicalize().ok(),
        "scoped to the boot location ({:?})",
        location.directory
    );
    handle.shutdown_and_join().await.expect("shutdown");
}
