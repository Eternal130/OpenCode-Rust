//! PLUG-D11 — the engine-backed permission port answers the v2
//! `PermissionDomain` over the REAL permission machinery: a parked ask
//! lists, get validates the session, and reply resolves it (the ask's
//! future settles).
//!
//! Hermetic: plugin-less boot (no bun, no network).

use std::path::{Path, PathBuf};

use oc_plugin_host::PermissionPort as _;
use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;
use oc_server::plugins::EnginePermissionPort;

mod support;

fn sandbox(tag: &str) -> PathBuf {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-perm-port-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home")).expect("home");
    std::fs::create_dir_all(root.join("project")).expect("project");
    root
}

fn redirect_env(box_: &Path) {
    std::env::set_var("HOME", box_.join("home"));
    std::env::set_var("XDG_CONFIG_HOME", box_.join("home").join(".config"));
    std::env::set_var("XDG_CACHE_HOME", box_.join("home").join(".cache"));
    std::env::set_var("XDG_DATA_HOME", box_.join("home").join(".local").join("share"));
    std::env::set_var("XDG_STATE_HOME", box_.join("home").join(".local").join("state"));
    std::env::set_var(
        "OPENCODE_CONFIG_DIR",
        box_.join("home").join(".config").join("opencode-rust"),
    );
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::set_var("OPENCODE_CATALOG_FETCH", "0");
    std::env::set_var("OPENCODE_DISABLE_PLUGINS", "1");
}

/// list → get (wrong session rejects) → reply resolves the parked ask.
#[tokio::test]
async fn engine_permission_port_round_trip() {
    let box_ = sandbox("roundtrip");
    redirect_env(&box_);
    let options = BootOptions {
        serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
        password: None,
        cors_origins: Vec::new(),
        version: "2.0.24".to_owned(),
        data_root: box_.join("data"),
        home: box_.join("home"),
        config_location: Some(box_.join("project")),
    };
    let handle = boot_and_serve(&options).await.expect("boot and serve");
    let location = oc_core::location::LocationRef::new(box_.join("project"), None);
    let services = handle.state.engine.location(location.clone()).await.expect("services");
    let port =
        EnginePermissionPort { engine: handle.state.engine.clone(), location: location.clone() };

    // Park an ask: an unrulesetted bash over a shell resource evaluates
    // to Ask → the pending map holds it (the ask requires a live
    // session row).
    let row = handle
        .state
        .engine
        .create_session(
            &location,
            oc_core::session::store::CreateInput {
                title: Some("perm".into()),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath(
                        box_.join("project").display().to_string(),
                    ),
                    workspace_id: None,
                }),
                version: "2.0.24".into(),
                ..oc_core::session::store::CreateInput::default()
            },
        )
        .await
        .expect("session");
    let session_id = oc_schema::id::SessionId(row.id.clone());
    // `read` over a `.env` file evaluates Ask under the build agent's
    // default rules (the wildcard allow is followed by the env-file
    // ask overrides — first specific match wins).
    let ask = services
        .permission
        .ask(oc_core::permission::AssertInput::new(
            session_id.clone(),
            "read",
            vec!["/opt/prod.env".to_owned()],
        ))
        .await
        .expect("ask evaluates");
    assert_eq!(ask.effect, oc_schema::permission::Effect::Ask, "the ask parked");
    let request_id = ask.id.as_str().to_owned();

    // list: the pending request (v2 Permission.Request wire shape).
    let listed = port.list(session_id.as_str()).await.expect("list");
    assert_eq!(listed[0]["id"], json!(request_id), "pending ask listed: {listed}");
    assert_eq!(listed[0]["action"], json!("read"));

    // get: right session resolves; wrong session → v2 not-found.
    let got = port.get(session_id.as_str(), &request_id).await.expect("get");
    assert_eq!(got["id"], json!(request_id));
    let wrong = port.get("ses_someone_else", &request_id).await;
    assert!(
        wrong.clone().is_err_and(|error| error.contains("not found")),
        "wrong session: {wrong:?}"
    );

    // An invalid decision rejects before consuming the ask.
    let bad = port.reply(session_id.as_str(), &request_id, "maybe", None).await;
    assert!(bad.is_err(), "invalid decision rejects");

    // reply(once) resolves the ask (a Once effect).
    port.reply(session_id.as_str(), &request_id, "once", Some("by plugin".to_owned()))
        .await
        .expect("reply resolves");
    let drained = port.list(session_id.as_str()).await.expect("list after reply");
    assert_eq!(drained, serde_json::Value::Array(Vec::new()), "pending drained");

    handle.shutdown_and_join().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&box_);
}

use serde_json::json;
