//! PLUG-ENGINE-MERGE — plugin-declared MCP servers survive a HEADLESS
//! server boot. v2 registers plugin MCPs IN the server process (core
//! plugin.ts:46 `PluginHost.make` in-process + `ctx.mcp.transform`
//! editing the server's Mcp.Service editor, host.ts:374-403); the
//! port's first cut only carried them TUI→wire→registry (PLUG-D33),
//! which is correct interactively but drops them when no TUI is
//! attached. The engine seam merges the host's `registrations.mcps`
//! into every location's `McpRegistry` (oc-core
//! `LocationPluginSource::mcps` + `apply_location_plugins`), so the
//! ref's in-process registration parity holds headless — this test
//! boots the REAL server with the `mcpreg` fixture (a plugin whose
//! setup declares `plugindemo` through `ctx.mcp.transform`) and
//! asserts `/api/mcp` lists it with NO TUI anywhere in the picture.
//! The TUI wire seam stays (PLUG-D33): its re-push is idempotent over
//! the same registry rows.
//!
//! Hermetic: sandboxed HOME/config/cache (env redirected inside the
//! nextest per-test process), the committed `mcpreg` fixture loaded
//! from its absolute path, the server on 127.0.0.1:0, no real
//! network.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root = PathBuf::from(format!(
        "/tmp/opencode/oc-server-plugin-engine-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    Sandbox { root, home, project }
}

impl Sandbox {
    /// Stage the `mcpreg` fixture (the committed PLUG-D33 plugin whose
    /// `ctx.mcp.transform` declares the `plugindemo` stdio server).
    fn stage_mcpreg(&self) -> PathBuf {
        let fixture = self.root.join("mcpreg.ts");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../oc-plugin-host/tests/fixtures/mcpreg.ts"),
            &fixture,
        )
        .expect("fixture copy");
        fixture
    }

    /// Write the project's `opencode.json` (the `plugins[]` entry over
    /// the fixture's absolute path — a local plugin spec).
    fn write_project_config(&self, config: &Value) {
        std::fs::write(self.project.join("opencode.json"), config.to_string()).expect("config");
    }

    /// Redirect the process env into the sandbox (nextest runs each
    /// test in its own process, so the mutation is contained).
    fn redirect_env(&self) {
        std::env::set_var("HOME", &self.home);
        std::env::set_var("XDG_CONFIG_HOME", self.home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", self.home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", self.home.join(".local").join("share"));
        std::env::set_var("XDG_STATE_HOME", self.home.join(".local").join("state"));
        std::env::remove_var("OPENCODE_CONFIG");
        std::env::remove_var("OPENCODE_DISABLE_PLUGINS");
    }

    fn boot_options(&self) -> BootOptions {
        BootOptions {
            serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
            password: None,
            cors_origins: Vec::new(),
            version: "2.0.24".to_owned(),
            data_root: self.root.join("data"),
            home: self.home.clone(),
            config_location: Some(self.project.clone()),
        }
    }
}

/// List `/api/mcp` through the real client (any transient assemble
/// race retries; the deadline bounds it).
async fn mcp_rows(client: &oc_client::Client) -> Vec<Value> {
    match client.mcp().list(&oc_client::types::McpListQuery { location: None }).await {
        Ok(response) => response
            .data
            .iter()
            .map(|server| {
                json!({
                    "name": server.name,
                    "status": serde_json::to_value(&server.status)
                        .unwrap_or_else(|_| json!("pending")),
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[tokio::test]
async fn headless_boot_lists_the_plugin_declared_mcp_server() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("boot");
    let fixture = box_.stage_mcpreg();
    box_.write_project_config(&json!({ "plugins": [fixture.display().to_string()] }));
    box_.redirect_env();

    // The HEADLESS server (no TUI attached anywhere).
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // Sanity: the plugin itself loaded Active through the real boot —
    // separates "plugin never booted" from "MCP dropped by the seam"
    // when this test fails.
    let plugins = client
        .plugin()
        .list(&oc_client::types::PluginListQuery { location: None })
        .await
        .expect("plugin.list");
    let plugin_rows: Vec<Value> = plugins
        .data
        .iter()
        .map(|plugin| serde_json::to_value(plugin).expect("plugin serializes"))
        .collect();
    assert_eq!(plugin_rows.len(), 1, "exactly the fixture plugin: {plugin_rows:?}");
    assert_eq!(plugin_rows[0]["id"], "mcpreg", "{plugin_rows:?}");
    assert_eq!(plugin_rows[0]["state"]["status"], "active", "{plugin_rows:?}");

    // /api/mcp lists the plugin-declared server (host.ts:374-403 — the
    // ref registers it server-side, in-process; no TUI carries it).
    let mut rows = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        rows = mcp_rows(&client).await;
        if rows.iter().any(|row| row["name"] == "plugindemo") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        rows.iter().any(|row| row["name"] == "plugindemo"),
        "/api/mcp lists the plugin-declared server: {rows:?}"
    );

    // Idempotent at the seam: the row stays EXACTLY one (the engine's
    // per-location merge is insert-or-replace; a TUI attach re-pushing
    // the same declaration over the wire must not duplicate it either).
    tokio::time::sleep(Duration::from_millis(300)).await;
    let rows = mcp_rows(&client).await;
    let count = rows.iter().filter(|row| row["name"] == "plugindemo").count();
    assert_eq!(count, 1, "exactly one plugindemo row: {rows:?}");

    handle.shutdown_and_join().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&box_.root);
}
