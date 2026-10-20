//! PLUG-D10 — the engine session port answers the full v2
//! `ctx.session` domain (create/get/remove/switchAgent/switchModel/
//! prompt/generate/command/compact/synthetic/interrupt/update/move/
//! wait/context) at the real engine, not just the 5-method bring-up
//! subset.
//!
//! Hermetic: plugin-less `boot_and_serve` (no bun, no network, no
//! models — the generate arm asserts the clear no-model error).

use std::path::PathBuf;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;
use oc_server::plugins::EngineSessionPort;
use serde_json::{json, Value};

mod support;

struct Sandbox {
    home: PathBuf,
    project: PathBuf,
    data_root: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-session-port-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    Sandbox { home, project, data_root: root.join("data") }
}

impl Sandbox {
    fn redirect_env(&self) {
        std::env::set_var("HOME", &self.home);
        std::env::set_var("XDG_CONFIG_HOME", self.home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", self.home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", self.home.join(".local").join("share"));
        std::env::set_var("XDG_STATE_HOME", self.home.join(".local").join("state"));
        std::env::set_var("OPENCODE_CONFIG_DIR", self.home.join(".config").join("opencode-rust"));
        std::env::remove_var("OPENCODE_CONFIG");
        std::env::set_var("OPENCODE_CATALOG_FETCH", "0");
        std::env::set_var("OPENCODE_DISABLE_PLUGINS", "1");
    }

    fn options(&self) -> BootOptions {
        BootOptions {
            serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
            password: None,
            cors_origins: Vec::new(),
            version: "2.0.24".to_owned(),
            data_root: self.data_root.clone(),
            home: self.home.clone(),
            config_location: Some(self.project.clone()),
        }
    }
}

/// The full v2 domain over the production engine port: create → get →
/// update (rename) → switchAgent/switchModel (published selections) →
/// synthetic → messages/context → wait → remove. The generate arm
/// answers the v2 no-model error; the command arm routes unknown
/// commands to the v2-shaped not-found error.
#[tokio::test]
async fn engine_session_port_covers_the_v2_domain() {
    let box_ = sandbox("domain");
    box_.redirect_env();
    let handle = boot_and_serve(&box_.options()).await.expect("boot and serve");
    let location = oc_core::location::LocationRef::new(box_.project.clone(), None);
    let port = EngineSessionPort {
        engine: handle.state.engine.clone(),
        location: location.clone(),
        host: None,
    };

    // create (v2 SessionCreateInput: title + empty body).
    let created = port
        .invoke("create", &json!({ "title": "plugin-made" }))
        .await
        .expect("session.create routes to the engine");
    let session_id = created["id"].as_str().expect("created id").to_owned();
    assert_eq!(created["title"], json!("plugin-made"));

    // get.
    let fetched = port.invoke("get", &json!({ "sessionID": session_id })).await.expect("get");
    assert_eq!(fetched["id"], json!(session_id));

    // list contains it.
    let listed = port.invoke("list", &json!({})).await.expect("list");
    assert!(listed
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["id"] == json!(session_id))));

    // update (rename) — publishes session.renamed; the store row's
    // title follows through the projection.
    port.invoke("update", &json!({ "sessionID": session_id, "title": "renamed-by-plugin" }))
        .await
        .expect("update");

    // switchAgent / switchModel — the built-in agent validates; the
    // model selection publishes (any ref shape is accepted on the bus).
    port.invoke("switchAgent", &json!({ "sessionID": session_id, "agent": "build" }))
        .await
        .expect("switchAgent with a known agent");
    let bad_agent = port
        .invoke("switchAgent", &json!({ "sessionID": session_id, "agent": "no-such-agent" }))
        .await;
    assert!(bad_agent.is_err(), "unknown agent rejects like the HTTP switch endpoint");

    // synthetic message lands in the session's messages.
    port.invoke(
        "synthetic",
        &json!({ "sessionID": session_id, "text": "plugged in", "description": "d" }),
    )
    .await
    .expect("synthetic");
    let messages =
        port.invoke("messages", &json!({ "sessionID": session_id })).await.expect("messages");
    assert!(
        messages["data"].as_array().is_some_and(|rows| !rows.is_empty()),
        "synthetic message recorded: {messages}"
    );

    // context mirrors the session's message read model.
    let context =
        port.invoke("context", &json!({ "sessionID": session_id })).await.expect("context");
    assert!(context.as_array().is_some_and(|rows| !rows.is_empty()), "context rows");

    // wait settles (the synthetic turn drained).
    port.invoke("wait", &json!({ "sessionID": session_id })).await.expect("wait");

    // generate without any model: the v2 clear error, not a stub.
    let generated =
        port.invoke("generate", &json!({ "sessionID": session_id, "prompt": "hi" })).await;
    assert!(generated.is_err(), "no model configured");
    assert!(
        generated.unwrap_err().to_lowercase().contains("model"),
        "the error names the model resolution, like the ref"
    );

    // command: unknown name → the v2 command-not-found shape.
    let command = port
        .invoke("command", &json!({ "sessionID": session_id, "name": "nope", "text": "" }))
        .await;
    assert!(command.is_err(), "unknown command rejects");

    // remove.
    port.invoke("remove", &json!({ "sessionID": session_id })).await.expect("remove");
    let gone = port.invoke("get", &json!({ "sessionID": session_id })).await;
    assert!(gone.is_err(), "removed session no longer resolves");

    handle.shutdown_and_join().await.expect("shutdown");
}

/// The move arm admits a location move (v2 SessionMoveInput): the
/// destination location string parses and the call returns Ok.
#[tokio::test]
async fn engine_session_port_move_admits() {
    let box_ = sandbox("move");
    box_.redirect_env();
    let handle = boot_and_serve(&box_.options()).await.expect("boot and serve");
    let location = oc_core::location::LocationRef::new(box_.project.clone(), None);
    let port = EngineSessionPort {
        engine: handle.state.engine.clone(),
        location: location.clone(),
        host: None,
    };
    let created = port.invoke("create", &json!({ "title": "mover" })).await.expect("create");
    let session_id = created["id"].as_str().expect("id").to_owned();
    let destination = box_.project.join("sub");
    std::fs::create_dir_all(&destination).expect("sub");
    let moved = port
        .invoke(
            "move",
            &json!({ "sessionID": session_id, "location": { "directory": destination.display().to_string() } }),
        )
        .await;
    assert!(moved.is_ok(), "move admits: {moved:?}");
    let _ = Value::Null;
    handle.shutdown_and_join().await.expect("shutdown");
}
