//! The wave-12 acceptance battery: the production plugin wiring —
//! oc-server's boot constructs the `PluginHost` from the config's
//! `plugins[]`, the engine applies its registrations (agents into the
//! location registry via the `plugin_apply` seam, tools behind the
//! synthesized `execute` surface), and the REAL TUI app path (the
//! oc-tui plugin boot) sees the plugin's commands in the palette.
//!
//! * **(a) flagship** — `oh-my-opencode-slim` (the sandbox-staged copy
//!   of the live install, per the w9w2 precedent) booted through the
//!   REAL `boot_and_serve`: its agents appear in `agent.list`, its
//!   palette commands render in the oc-tui command palette, and
//!   `/api/plugin` lists it Active with its package source;
//! * **(b) Code Mode is core, not plugin-gated** — a config with NO
//!   plugins and ONE local MCP fixture server: the host still boots
//!   (the built-in runtime), the MCP tool registers codemode-true, and
//!   the request snapshot hides it behind `execute` (v2 mcp.ts:45 +
//!   tool.ts:225-241 — the exact `select` shaping the session runner
//!   applies).
//!
//! Hermetic: sandboxed HOME/config/cache (env redirected inside the
//! nextest per-test process), pre-staged install copies, no network,
//! no port 49374.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

/// The live flagship install (read-only; staged into the sandbox).
fn flagship_source() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("OC_OMOS_ROOT") {
        let root = PathBuf::from(root);
        return root.join("dist/server/index.js").is_file().then_some(root);
    }
    let base = Path::new("/root/.cache/opencode/npm/oh-my-opencode-slim@latest");
    let mut stamps: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect();
    stamps.sort();
    stamps
        .into_iter()
        .rev()
        .map(|stamp| stamp.join("node_modules").join("oh-my-opencode-slim"))
        .find(|root| root.join("dist/server/index.js").is_file())
}

/// The machine's flagship preset (read-only; copied into the sandbox).
fn flagship_preset() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OC_OMOS_CONFIG") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path = PathBuf::from("/root/.config/opencode/oh-my-opencode-slim.json");
    path.is_file().then_some(path)
}

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn copy_dir(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create dir");
    for entry in std::fs::read_dir(source).expect("read dir") {
        let entry = entry.expect("dir entry");
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

/// The sandbox: redirected HOME/XDG (config + cache), the staged
/// flagship install under the v2 cache layout (`plugin add`'s shape —
/// what `oc_plugin_host::resolve` scans), the plugin preset, and the
/// project config carrying the `plugins[]` entry.
struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-plugins-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    Sandbox { root, home, project }
}

impl Sandbox {
    /// Stage the flagship install into `<home>/.cache/opencode-rust/npm`
    /// (the layout `plugin add` writes and `resolve` scans) + the
    /// preset into the redirected config dir.
    fn stage_flagship(&self, source: &Path) {
        let node_modules = self
            .home
            .join(".cache")
            .join("opencode-rust")
            .join("npm")
            .join("oh-my-opencode-slim@latest")
            .join("1791474452027")
            .join("node_modules");
        let package = node_modules.join("oh-my-opencode-slim");
        for dir in ["dist", "src"] {
            copy_dir(&source.join(dir), &package.join(dir));
        }
        for file in ["package.json", "oh-my-opencode-slim.schema.json"] {
            std::fs::copy(source.join(file), package.join(file)).ok();
        }
        // jsonc-parser: externalized by the plugin's build.
        if let Some(dep) = source.parent().map(|parent| parent.join("jsonc-parser")) {
            if dep.is_dir() {
                copy_dir(&dep, &node_modules.join("jsonc-parser"));
            }
        }
        let config = self.home.join(".config").join("opencode-rust");
        std::fs::create_dir_all(&config).expect("config dir");
        if let Some(preset) = flagship_preset() {
            std::fs::copy(preset, config.join("oh-my-opencode-slim.json")).expect("preset");
        }
    }

    /// Write the project's `opencode.json`.
    fn write_project_config(&self, config: &Value) {
        std::fs::write(self.project.join("opencode.json"), config.to_string()).expect("config");
    }

    /// Redirect the process env into the sandbox (nextest runs each
    /// test in its own process, so the mutation is contained). The
    /// plugin cache resolution, the flagship's own config discovery,
    /// and oc-util's global roots all land inside.
    fn redirect_env(&self) {
        std::env::set_var("HOME", &self.home);
        std::env::set_var("XDG_CONFIG_HOME", self.home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", self.home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", self.home.join(".local").join("share"));
        std::env::set_var("XDG_STATE_HOME", self.home.join(".local").join("state"));
        // The flagship's primary config lookup.
        std::env::set_var("OPENCODE_CONFIG_DIR", self.home.join(".config").join("opencode-rust"));
        std::env::remove_var("OPENCODE_CONFIG");
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

/// Poll every 50 ms until `probe` holds or `timeout` elapses.
async fn wait_for<T>(timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(value) = probe() {
            return Some(value);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    probe()
}

// ---------------------------------------------------------------------------
// (a) The flagship through the REAL oc-server + REAL TUI app path
// ---------------------------------------------------------------------------

#[tokio::test]
#[allow(clippy::too_many_lines)] // one sequential end-to-end acceptance scenario
async fn flagship_boots_through_the_real_server_and_tui_app_path() {
    let Some(source) = flagship_source() else {
        eprintln!("flagship install absent; skipping");
        return;
    };
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("flagship");
    box_.stage_flagship(&source);
    box_.write_project_config(&json!({ "plugins": ["oh-my-opencode-slim"] }));
    box_.redirect_env();

    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // -- /api/plugin: the flagship listed Active with its source ------
    let plugins = client
        .plugin()
        .list(&oc_client::types::PluginListQuery { location: None })
        .await
        .expect("plugin.list");
    let entries: Vec<Value> = plugins
        .data
        .iter()
        .map(|plugin| serde_json::to_value(plugin).expect("plugin serializes"))
        .collect();
    assert_eq!(entries.len(), 1, "exactly the flagship: {entries:?}");
    assert_eq!(entries[0]["id"], "oh-my-opencode-slim");
    assert_eq!(entries[0]["source"]["type"], "package");
    assert_eq!(entries[0]["source"]["target"], "oh-my-opencode-slim");
    assert_eq!(entries[0]["state"]["status"], "active", "{entries:?}");

    // -- agent.list: the flagship's agents in a session's agent list --
    let agents = client
        .agent()
        .list(&oc_client::types::AgentListQuery { location: None })
        .await
        .expect("agent.list");
    let rows: Vec<Value> = agents
        .data
        .iter()
        .map(|agent| serde_json::to_value(agent).expect("agent serializes"))
        .collect();
    let ids: Vec<&str> =
        rows.iter().filter_map(|row| row.get("id").and_then(Value::as_str)).collect();
    assert!(
        ids.contains(&"orchestrator"),
        "the flagship's orchestrator agent landed in the registry: {ids:?}"
    );
    // The preset applied (w9w2's proof: zhipuai preset → own/glm on
    // the orchestrator).
    let orchestrator = rows
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some("orchestrator"))
        .expect("orchestrator row");
    assert!(
        orchestrator.pointer("/model/providerID").is_some()
            || orchestrator.get("model").is_some_and(|model| !model.is_null()),
        "the preset's model landed on the orchestrator: {orchestrator:?}"
    );

    // -- the REAL TUI app path: the oc-tui plugin boot against the same
    //    config (the discovery walks the project's opencode.json), the
    //    seam attach, and the flagship's commands in the palette ------
    let agents_seed = oc_tui::plugin_boot::fetch_agent_seed(&client).await;
    let booted = oc_tui::plugin_boot::TuiPluginRuntime::boot(
        &client,
        agents_seed,
        &box_.home.join(".config").join("opencode-rust"),
        &box_.project,
        "2.0.24",
    )
    .expect("the TUI boots the flagship");
    assert!(booted.ready, "host ready: {:?}", booted.host.start_error());

    let store = oc_tui::store::Store::new(oc_tui::reactive::Runtime::new());
    let runtime = oc_tui::plugin_boot::TuiPluginRuntime::new(booted);
    let projection = runtime.attach(&store);
    assert!(
        projection.owned.is_empty(),
        "the flagship registers no keybinds: {:?}",
        projection.owned
    );

    // The palette lists its commands (group "Plugin" — the wave-3
    // projection over the REAL registration snapshot).
    let props = oc_tui::components::command_palette::palette_props(&store);
    let plugin_rows: Vec<&str> = props
        .options
        .iter()
        .filter(|option| option.category.as_deref() == Some("Plugin"))
        .map(|option| option.value.as_str())
        .collect();
    for expected in ["reflect", "deepwork", "loop", "interview"] {
        assert!(plugin_rows.contains(&expected), "palette contains {expected}: {plugin_rows:?}");
    }

    // The palette dispatch reaches the guest (execute_command_in_session
    // with the ACTIVE session): create a real session, dispatch the
    // flagship's reflect command through the store's sink, and assert
    // the guest-side execution round-tripped (the command's
    // ctx.session.prompt crosses the HTTP session port into the REAL
    // server).
    let session = client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: box_.project.display().to_string(),
            }),
            ..oc_client::types::SessionCreateBody::default()
        })
        .await
        .expect("session create");
    runtime.set_current_session(Some(session.id.clone()));
    let dispatch = store.execute_plugin_command("reflect", Some("--last 7 production wiring"));
    assert!(dispatch, "the palette command dispatched to the guest");

    // The dispatch is fire-and-forget (v2's unawaited execute promise);
    // the ROUND TRIP lands when the command's `ctx.session.prompt`
    // crosses the HTTP session port into the REAL server — the session's
    // message list gains the marker prompt (guest-side context hooks
    // are the documented engine-side residual, so the marker text is
    // the observable).
    let mut marker_landed = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(messages) = client
            .message()
            .list(&session.id, &oc_client::types::SessionMessageListQuery::default())
            .await
        {
            let text = serde_json::to_string(&messages).unwrap_or_default();
            if text.contains("omos-cmd-command") || text.contains("reflect skill") {
                marker_landed = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(marker_landed, "the marker prompt reached the real session");

    runtime.shutdown();
    handle.shutdown_and_join().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&box_.root);
}

// ---------------------------------------------------------------------------
// (b) Code Mode is core, not plugin-gated (MCP tool behind execute)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_tool_appears_only_behind_execute_with_the_builtin_runtime() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("mcp-codemode");
    // A fixture MCP server over stdio + NO plugins in the config.
    let fixture = box_.root.join("mcp_probe_server.js");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_probe_server.js"),
        &fixture,
    )
    .expect("fixture copy");
    let bun = which_bun().expect("bun resolvable");
    box_.write_project_config(&json!({
        "mcp": {
            "servers": {
                "fixture": {
                    "type": "local",
                    "command": [bun, fixture.display().to_string()],
                }
            }
        }
    }));
    box_.redirect_env();

    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");

    // Assemble the location (the first request path — here directly).
    let location = oc_core::location::LocationRef::new(box_.project.clone(), None);
    let services = handle.state.engine.location(location.clone()).await.expect("assemble");

    // The MCP tool registers once the bridge's initial discovery
    // settles (the fixture answers tools/list with `probe`).
    let registered = wait_for(Duration::from_secs(20), || {
        services.graph.tools.snapshot(None).iter().find(|tool| tool.name == "probe").cloned()
    })
    .await
    .expect("the fixture MCP tool registered");
    assert!(registered.codemode, "mcp.ts:45 — MCP tools are codemode-default-true");
    assert_eq!(registered.namespace.as_deref(), Some("fixture"));

    // The session runner's EXACT shaping (context.rs select): with the
    // built-in runtime attached (zero plugins!), the fixture tool is
    // invisible-direct — only `execute` carries it.
    let code_mode = services.graph.tools.executor(oc_core::tool::codemode::EXECUTE_NAME).is_some();
    assert!(code_mode, "the built-in Code Mode runtime attached plugin-less");
    let shaped = oc_core::tool::codemode::shape_request_tools(
        services.graph.tools.snapshot(None),
        None,
        code_mode,
    );
    let names: Vec<&str> = shaped.iter().map(|tool| tool.name.as_str()).collect();
    assert!(!names.contains(&"probe"), "the MCP tool is hidden behind execute: {names:?}");
    assert!(names.contains(&"execute"), "execute is advertised: {names:?}");

    // The catalog lists the fixture tool under its namespace (what a
    // program's `tools.fixture.probe(...)` resolves against).
    let catalog = oc_core::tool::codemode::code_mode_entries(&services.graph.tools);
    let paths: Vec<&str> = catalog.iter().map(|entry| entry.path.as_str()).collect();
    assert!(
        paths.iter().any(|path| path.ends_with("fixture.probe")),
        "the catalog carries the fixture tool: {paths:?}"
    );

    handle.shutdown_and_join().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&box_.root);
}

fn which_bun() -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("bun"))
        .find(|candidate| candidate.is_file())
        .map(|bun| bun.display().to_string())
}
