//! PLUG-D21 / PLUG-D18 / PLUG-D14 — the guest wiring battery (Batch 1A
//! sub-lane C2a): plugin-registered hooks reach the production
//! `HookRegistry`/engine dispatch chain, plugin KV lands in the database
//! (v2 `plugin:<hex(id)>:` namespace, surviving process restarts), and
//! `/api/rpc` dispatches real guest RPC.
//!
//! Hermetic: sandboxed HOME/XDG (env redirected inside the nextest
//! per-test process), a local fixture plugin (`wiring.ts`), no network,
//! ports on `127.0.0.1:0`, no 49374.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

mod support;

/// The wiring fixture (staged from oc-plugin-host's fixture dir).
fn wiring_fixture() -> PathBuf {
    let path = oc_plugin_host::fixture_dir().join("wiring.ts");
    assert!(path.is_file(), "fixture missing: {}", path.display());
    path
}

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The sandbox: redirected HOME/XDG, a project dir whose `opencode.json`
/// loads the wiring fixture as a local plugin.
struct Sandbox {
    home: PathBuf,
    project: PathBuf,
    data_root: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-wiring-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    let data_root = root.join("data");
    Sandbox { home, project, data_root }
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
        // Hermetic boot: no models-dev refresh loop.
        std::env::set_var("OPENCODE_CATALOG_FETCH", "0");
        // The fixture's marker dir (INSIDE the project so the read-tool
        // hook probe stays within the location's file boundary — no
        // permission ask).
        let markers = self.project.join("markers");
        std::fs::create_dir_all(&markers).expect("markers");
        std::env::set_var("OC_WIRING_SANDBOX", &markers);
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
        }
    }
}

// ---------------------------------------------------------------------------
// PLUG-D21: guest hooks enter the production HookRegistry / engine chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn guest_hook_dispatches() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("hooks");
    box_.write_project_config(&wiring_fixture());
    box_.redirect_env();
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");

    // The production engine location (lazy assembly — this is the same
    // path the first HTTP request takes).
    let location = oc_core::location::LocationRef::new(box_.project.clone(), None);
    let services = handle.state.engine.location(location.clone()).await.expect("location services");

    // The guest's hook registrations landed in THIS location's production
    // HookRegistry (the engine's dispatch sites consult exactly this
    // registry).
    assert!(
        services.graph.hooks.has_tool_execute_before(None),
        "tool.execute.before guest registration must land in the production HookRegistry"
    );
    assert!(
        services.graph.hooks.has_session_prompt(None),
        "session.prompt guest registration must land in the production HookRegistry"
    );
    assert!(
        services.graph.hooks.has_shell_create_before(None),
        "shell.create.before guest registration must land in the production HookRegistry"
    );

    // The PRODUCTION dispatch path: the registered `read` executor is the
    // engine's pipeline executor (the same one the session runner calls);
    // its `tool.execute.before` dispatch must reach the guest, whose
    // mutation rewrites the read target to the marker file.
    std::fs::write(box_.project.join("markers").join("hook-target.txt"), "hook-rewrote-the-path")
        .expect("marker");
    let session = handle
        .state
        .engine
        .create_session(
            &location,
            oc_core::session::store::CreateInput {
                title: Some("wiring".into()),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath(box_.project.display().to_string()),
                    workspace_id: None,
                }),
                version: "2.0.24".into(),
                ..oc_core::session::store::CreateInput::default()
            },
        )
        .await
        .expect("session for the tool context");
    let executor = services.graph.tools.executor("read").expect("read executor registered");
    let context = oc_core::location::ToolCallContext {
        session_id: oc_schema::id::SessionId(session.id.clone()),
        agent: oc_schema::id::AgentId::parse("build"),
        message_id: "msg_wiring".into(),
        call_id: "cal_wiring".into(),
    };
    let input = json!({ "path": box_.project.join("decoy.txt").display().to_string() });
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        executor.execute_with_context(&input, &context),
    )
    .await
    .expect("the read pipeline settles (no permission hang)")
    .expect("read executes");
    let text = match &outcome.content[0] {
        oc_schema::tool::Content::Text { text } => text.clone(),
        oc_schema::tool::Content::File { .. } => {
            panic!("expected text content: a file part")
        }
    };
    assert!(
        text.contains("hook-rewrote-the-path") && !text.contains("decoy"),
        "the guest hook's input mutation flowed through the production pipeline: {text}"
    );

    handle.shutdown_and_join().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// PLUG-D18: plugin KV lands in the DB (v2 namespace) and survives restart
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kv_survives_restart() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("kv");
    box_.write_project_config(&wiring_fixture());
    box_.redirect_env();

    // Boot 1: the plugin's setup writes `wiring:note` through ctx.storage.
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let boot = handle.state.plugins.as_ref().expect("plugin boot attached");
    let note = boot
        .host
        .execute_command_in_session_async("wiring.kv", None, "", Duration::from_secs(10))
        .await
        .expect("wiring.kv command dispatches");
    assert_eq!(note, json!("hello-from-setup"), "setup-time KV write is readable in-process");

    // The v2 namespace: `plugin:<hex(id)>:` rows in the engine's database
    // (host.ts:602-629 — every char as 4-hex UTF-16 units; "wiring" ->
    // 0077 0069 0072 0069 006e 0067).
    handle.shutdown_and_join().await.expect("shutdown");
    let db = oc_core::database::Database::open_location(
        oc_core::database::engine_location(&box_.data_root),
        oc_core::database::MigrationOptions::default(),
    )
    .await
    .expect("open engine db");
    let namespace = "plugin:0077006900720069006e0067:";
    let stored = db
        .transaction(move |tx| tx.kv_get(&format!("{namespace}wiring:note")))
        .await
        .expect("kv read");
    assert_eq!(
        stored,
        Some(json!("hello-from-setup")),
        "KV landed in the engine database under the v2 namespace"
    );
    drop(db);

    // Boot 2 over the SAME data root: a fresh guest process (fresh tmp
    // sandbox) must read the note back from the database.
    let handle = boot_and_serve(&box_.boot_options()).await.expect("re-boot and serve");
    let boot = handle.state.plugins.as_ref().expect("plugin boot attached");
    let note = boot
        .host
        .execute_command_in_session_async("wiring.kv", None, "", Duration::from_secs(10))
        .await
        .expect("wiring.kv command dispatches after restart");
    assert_eq!(note, json!("hello-from-setup"), "KV survives the process restart");
    handle.shutdown_and_join().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// PLUG-D14: /api/rpc dispatches real guest RPC
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rpc_dispatch() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("rpc");
    box_.write_project_config(&wiring_fixture());
    box_.redirect_env();
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let addr = handle.addr;

    // The happy path: the request reaches the guest, the handler's output
    // returns (no 500 stub).
    let response = support::request(
        addr,
        "POST",
        "/api/rpc/wiring-rpc/echo",
        &[("content-type", "application/json")],
        br#"{"input":{"text":"hi"}}"#,
    )
    .await
    .expect("rpc call");
    let (status, body) = (response.status, response);
    assert_eq!(status, 200, "rpc.call dispatches: {}", body.text());
    assert_eq!(body.json(), json!({ "output": { "text": "echo:hi" } }), "guest output round-trips");

    // Unknown rpc id: v2's rpc.unavailable (RpcError 400).
    let response = support::request(
        addr,
        "POST",
        "/api/rpc/nope/echo",
        &[("content-type", "application/json")],
        br#"{"input":{}}"#,
    )
    .await
    .expect("unknown rpc");
    let (status, body) = (response.status, response);
    assert_eq!(status, 400, "unknown rpc id maps to RpcError: {}", body.text());
    assert_eq!(body.json()["_tag"], "RpcError");
    assert_eq!(body.json()["type"], "rpc.unavailable");

    handle.shutdown_and_join().await.expect("shutdown");
}
