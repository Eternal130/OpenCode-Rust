//! beta.13 bug 1 — the REAL plugin's sidebar sections render through
//! the v2 TUI-plugin surface:
//!
//! * `load` of oh-my-opencode-slim's server entry now ALSO loads the
//!   package's `exports["./tui"]` companion (`dist/tui2.js`) — the
//!   entry that owns the sidebar — through the guest's
//!   `load.tui` + the host-materialized @opentui shims;
//! * the entry's `tui(api)` registers its `sidebar_content` render
//!   slot (`api.slots.register({slots: {sidebar_content: render}})`,
//!   tui2.js:31129-31131 — the REAL registration surface; the old
//!   `ctx.ui.sidebar.setSection` seam is OUR invention and the real
//!   plugin never calls it);
//! * each materialize (`slot.materialize`) runs the plugin's render
//!   function over its own `OpenTUI` element records and flattens the
//!   tree into the native `{title, rows}` section — the supervisor
//!   lands it as a `UiRequest::SidebarSection` on the UI-request
//!   queue (registered + rerender + the 1 s steady refresh).
//!
//! Hermetic: pre-staged copy of the read-only live install (skips
//! without it or bun); no network (the session port stub answers).

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use oc_plugin_host::{PluginHost, PluginHostConfig, SessionPort, UiRequest};

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

/// The real install's package + the hoisted deps the bundled entries
/// import at module scope (jsonc-parser, zod, solid-js).
fn stage(name: &str) -> Option<(PathBuf, PathBuf)> {
    let source = flagship_source()?;
    let sandbox = common::sandbox(name);
    let node_modules = sandbox.join("plugins").join("node_modules");
    let package = node_modules.join("oh-my-opencode-slim");
    copy_dir(&source.join("dist"), &package.join("dist"));
    std::fs::copy(source.join("package.json"), package.join("package.json")).ok();
    // The hoisted deps the bundled entries import at module scope —
    // the live install's node_modules, or OC_OMOS_DEPS when the test
    // runs under an account that cannot read /root (the gate's user).
    let deps_root = std::env::var("OC_OMOS_DEPS")
        .map(PathBuf::from)
        .ok()
        .or_else(|| source.parent().map(std::path::Path::to_path_buf));
    if let Some(parent) = deps_root {
        for dep in ["jsonc-parser", "zod", "solid-js", "lru-cache", "semver"] {
            let dep_dir = parent.join(dep);
            if dep_dir.is_dir() {
                copy_dir(&dep_dir, &node_modules.join(dep));
            }
        }
    }
    std::fs::create_dir_all(sandbox.join("config")).expect("config dir");
    std::fs::create_dir_all(sandbox.join("home")).expect("home dir");
    std::fs::create_dir_all(sandbox.join("project")).expect("project dir");
    let entry = package.join("dist").join("server").join("index.js");
    Some((sandbox, entry))
}

fn boot(name: &str) -> Option<PluginHost> {
    let (sandbox, entry) = stage(name)?;
    let config = PluginHostConfig {
        env: vec![
            ("OPENCODE_CONFIG_DIR".to_owned(), sandbox.join("config").display().to_string()),
            ("XDG_CONFIG_HOME".to_owned(), sandbox.join("config").display().to_string()),
            ("HOME".to_owned(), sandbox.join("home").display().to_string()),
            ("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string()),
        ],
        registration_timeout: Duration::from_secs(60),
        dispatch_timeout: Duration::from_secs(15),
        host_state: oc_plugin_host::HostState {
            app_version: "2.0.24-rust".into(),
            project_id: "proj_flagship".into(),
            directory: sandbox.join("project").display().to_string(),
            agents: Vec::new(),
            mcps: std::collections::BTreeMap::default(),
        },
        session_port: Some(Arc::new(NullSessionPort)),
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, std::slice::from_ref(&entry));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(120)), "host ready: {:?}", host.start_error());
    Some(host)
}

/// A minimal session port (every method answers empty — the sidebar
/// does not need session data; the CONFIG preset drives the rows).
#[derive(Default)]
struct NullSessionPort;

#[async_trait::async_trait]
impl SessionPort for NullSessionPort {
    async fn call(
        &self,
        _bridge: &oc_plugin_host::Bridge,
        method: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Ok(match method {
            "list" => serde_json::json!({ "data": [] }),
            _ => serde_json::Value::Null,
        })
    }
}
/// The acceptance: the REAL plugin's sidebar section lands on the
/// UI-request queue — title carrying the "OMO-Slim" header, rows
/// carrying its agent list (the fallback roster when no config
/// preset is present: explorer/librarian/…/fixer).
#[test]
fn flagship_sidebar_section_materializes_from_the_real_plugin() {
    let Some(host) = boot("flagship-sidebar") else { return };

    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let mut section: Option<(String, Vec<String>)> = None;
    while std::time::Instant::now() < deadline {
        for request in host.drain_ui_requests() {
            if let UiRequest::SidebarSection { plugin_id, title, rows, .. } = request {
                if plugin_id.ends_with(":tui") {
                    section = Some((title, rows));
                }
            }
        }
        if section.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let (title, rows) = section.expect("the real plugin's sidebar section landed");
    assert!(title.contains("OMO-Slim"), "the header row renders: {title:?}");
    assert!(rows.iter().any(|row| row.contains("explorer")), "the agent roster renders: {rows:?}");
    // Every row is a sane single-line label (the tree walk flattened,
    // not interleaved).
    for row in &rows {
        assert!(!row.contains('\n'), "one line per row: {row:?}");
    }
    host.shutdown();
}

/// SA-F5 (SCAN-AB): the materialized section carries the PLUGIN's own
/// render contract — the version badge shows the plugin's package
/// version (the ref's `v3.0.4`, never our app version), and the
/// compact agent rows keep their padded columns (the name column and
/// the right-aligned model column, ref capture ref-session-00: `•
/// orchestrator               default` — all rows one width, the
/// header's `v…` badge right-aligned on the same edge).
#[test]
fn flagship_sidebar_renders_plugin_version_and_padded_columns() {
    let Some(source) = flagship_source() else {
        eprintln!("flagship source unavailable; skipping");
        return;
    };
    let package = std::fs::read_to_string(source.join("package.json")).expect("package.json");
    let version = package
        .split("\"version\"")
        .nth(1)
        .and_then(|rest| rest.split('"').nth(1))
        .unwrap_or_default()
        .to_owned();
    assert!(!version.is_empty(), "the staged package carries a version");
    let Some(host) = boot("flagship-fidelity") else { return };

    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let mut section: Option<(String, Vec<String>)> = None;
    while std::time::Instant::now() < deadline {
        for request in host.drain_ui_requests() {
            if let UiRequest::SidebarSection { plugin_id, title, rows, .. } = request {
                if plugin_id.ends_with(":tui") {
                    section = Some((title, rows));
                }
            }
        }
        if section.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let (title, rows) = section.expect("the real plugin's sidebar section landed");
    // The badge is the PLUGIN's version (meta.version — the plugin's
    // package.json), never the app version the host runs under.
    assert!(
        title.contains(&format!("v{version}")),
        "the badge shows the plugin's own version {version}: {title:?}"
    );
    assert!(!title.contains("2.0.24-rust"), "the app version must not leak: {title:?}");
    // The compact agent rows: every row pads to the SAME width with
    // the model column right-aligned (the ref's materialized shape).
    let agent_rows: Vec<&String> =
        rows.iter().filter(|row| row.starts_with('•') || row.contains("• ")).collect();
    assert!(!agent_rows.is_empty(), "the compact roster renders: {rows:?}");
    let widths: Vec<usize> = agent_rows.iter().map(|row| row.chars().count()).collect();
    for (index, width) in widths.iter().enumerate() {
        assert_eq!(
            *width, widths[0],
            "row {index} pads to the section width (the right-aligned column): {:?}",
            agent_rows[index]
        );
    }
    // The header badge right-aligns on the SAME edge (the
    // space-between header): the title's width equals the rows'.
    assert_eq!(
        title.chars().count(),
        widths[0],
        "the header's badge right-aligns on the rows' edge: {title:?} vs {:?}",
        agent_rows[0]
    );
    // The name/model gap is PADDED (multi-space), not the legacy
    // single-space join.
    assert!(
        agent_rows.iter().all(|row| row.contains("  ")),
        "the padded column shape: {agent_rows:?}"
    );
    // The model column starts at the same cell on every row.
    let model_cols: Vec<usize> =
        agent_rows.iter().map(|row| row.rfind(' ').map(|at| at + 1).unwrap_or(0)).collect();
    assert!(
        model_cols.windows(2).all(|pair| pair[0] == pair[1]),
        "the model column aligns: {agent_rows:?}"
    );
    host.shutdown();
}
