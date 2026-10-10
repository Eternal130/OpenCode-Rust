//! Wave 3 — the flagship's TUI export probed (deliverable: what the
//! `./tui` entry of oh-my-opencode-slim v3.0.3 is, and precisely what
//! it needs — the finding feeds the lane report):
//!
//! * `exports["./tui"]` → `dist/tui2.js` — resolvable through the
//!   same probing the server entry uses ([`resolve_subpath_entry`]).
//! * The file STATICALLY imports `@opentui/core`, `@opentui/solid`
//!   and `solid-js`: the first two are HOST-provided in v2 (the
//!   desktop opencode install injects them; they are absent from the
//!   package's own `node_modules`), so a load attempt in this host
//!   fails at module resolution — CONTAINED (the entry fails, the
//!   host keeps running).
//! * Its v2 `setup(ctx)` drives a `SolidJS` sidebar runtime
//!   (`createSidebarRuntime`) that renders `JSX` into the host's
//!   `CliRenderer` (`ctx.renderer.requestRender`), navigates via
//!   `ctx.ui.router`, reads `ctx.data.session.list` through the SDK
//!   client, and claims slots via `ctx.ui.slot({append`:
//!   `"sidebar.content"|"app", render}` — a desktop/opentui surface,
//!   not mappable to the Rust text-grid slots without a Solid host.
//!   The Rust bridge instead offers the native `ctx.ui.*` slot seams
//!   (toast/sidebar/footer/dialog), which is what this wave proves.
//!
//! Hermetic: pre-staged copy of the read-only live install (skips
//! without it or bun); no network.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use oc_plugin_host::context::install::resolve_subpath_entry;

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

/// Stage the package copy (dist only — the probe reads the entry and
/// its import graph shape, no config preset needed).
fn stage(name: &str) -> Option<(PathBuf, PathBuf)> {
    let source = flagship_source()?;
    let sandbox = common::sandbox(name);
    let package = sandbox.join("plugins").join("node_modules").join("oh-my-opencode-slim");
    copy_dir(&source.join("dist"), &package.join("dist"));
    std::fs::copy(source.join("package.json"), package.join("package.json")).ok();
    // The sibling deps the bundled entry may resolve (the live
    // install's hoisted node_modules).
    if let Some(parent) = source.parent() {
        let node_modules = package.parent().expect("node_modules parent").to_path_buf();
        for dep in ["solid-js", "jsonc-parser"] {
            if parent.join(dep).is_dir() {
                copy_dir(&parent.join(dep), &node_modules.join(dep));
            }
        }
    }
    Some((sandbox, package))
}

#[test]
fn flagship_tui_export_resolves_and_is_opentui_bound() {
    let Some((_sandbox, package)) = stage("flagship-tui") else {
        eprintln!("flagship install absent; skipping");
        return;
    };

    // (a) The ./tui export resolves through the same probing as the
    // server entry.
    let tui_entry = resolve_subpath_entry(&package, "./tui").expect("the ./tui export");
    assert!(tui_entry.ends_with("dist/tui2.js"), "{}", tui_entry.display());

    // (b) The entry statically binds the desktop host renderer stack.
    let text = std::fs::read_to_string(&tui_entry).expect("read tui2.js");
    assert!(text.contains("from \"@opentui/core\""), "parseColor/RGBA imported");
    assert!(text.contains("from \"@opentui/solid\""), "createElement/insert imported");
    assert!(text.contains("from \"solid-js\""), "createSignal imported");
    // …and @opentui is HOST-provided in v2 (absent from the package's
    // own dependencies — only solid-js is an (optional) package dep).
    let node_modules =
        package.parent().map_or_else(|| package.clone(), std::path::Path::to_path_buf);
    assert!(
        !node_modules.join("@opentui").exists(),
        "@opentui is host-provided in the v2 desktop install"
    );

    // (c) A load attempt in THIS host fails containedly at module
    // resolution (Cannot find module '@opentui/core'); the entry
    // fails, the host stays up.
    let host = oc_plugin_host::PluginHost::detect();
    if !host.enabled() {
        eprintln!("bun unavailable; skipping the load probe");
        return;
    }
    let host = host.start_detached(std::slice::from_ref(&tui_entry));
    assert!(host.wait_ready(Duration::from_secs(60)), "host ready: {:?}", host.start_error());
    let snapshot = host.registrations();
    let status = snapshot
        .plugins
        .iter()
        .find(|status| status.entry == tui_entry)
        .expect("the tui entry tracked");
    assert!(
        matches!(&status.state, oc_plugin_host::PluginState::Failed(reason) if reason.contains("@opentui")),
        "the load fails on the missing host renderer dep: {:?}",
        status.state
    );
    host.shutdown();
}
