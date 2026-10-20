//! PLUG-D01 — `plugin`/`plugins` directory auto-discovery.
//!
//! v2 `PluginSourceDirectory.discover` (core/plugin/source-directory.ts) plus
//! `ConfigPluginSource.scan` (core/config/plugin/source.ts): every config
//! layer DIRECTORY's `plugin`/`plugins` children auto-add (`.ts`/`.js`
//! files, subdirectories, symlinks resolving to either), sorted by target
//! path, and the discovered adds come BEFORE the document `plugins[]`
//! operations so an explicit remove can drop a discovered plugin.
//!
//! Hermetic: a tmp sandbox under /tmp/opencode, no bun run, no network.

use oc_plugin_host::{discover_directory_ops, parse_ops, resolve, PluginOp};
use serde_json::json;
use std::path::PathBuf;

fn sandbox(tag: &str) -> PathBuf {
    let root =
        PathBuf::from(format!("/tmp/opencode/lane-plugin2-discovery-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".opencode").join("plugins").join("alpha")).expect("alpha");
    std::fs::create_dir_all(root.join(".opencode").join("plugin")).expect("plugin");
    std::fs::write(
        root.join(".opencode").join("plugins").join("alpha").join("index.ts"),
        "export default { id: 'alpha', async setup() {} }",
    )
    .expect("alpha entry");
    std::fs::write(
        root.join(".opencode").join("plugin").join("beta.ts"),
        "export default { id: 'beta', async setup() {} }",
    )
    .expect("beta entry");
    // Not a source file: ignored.
    std::fs::write(root.join(".opencode").join("plugins").join("notes.txt"), "x").expect("txt");
    root
}

/// The core regression: drop-in local plugins under a config dir's
/// `plugin`/`plugins` are discovered, load as local plugins, and an
/// explicit config remove drops a discovered entry.
#[test]
fn local_plugin_dir() {
    let root = sandbox("core");
    let ops = discover_directory_ops(&[root.join(".opencode")]);
    let targets: Vec<&str> = ops
        .iter()
        .map(|op| match op {
            PluginOp::Add { target, .. } | PluginOp::Remove { target } => target.as_str(),
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            root.join(".opencode").join("plugin").join("beta.ts").display().to_string().as_str(),
            root.join(".opencode").join("plugins").join("alpha").display().to_string().as_str(),
        ],
        "both names discovered, sorted by target, non-source files skipped: {targets:?}"
    );

    // The discovered entries resolve as LOCAL plugins (entries loadable
    // by the host).
    let resolution = resolve(&ops, None);
    assert_eq!(
        resolution.plugins.len(),
        2,
        "both discovered plugins resolve: {:?}",
        resolution.failures
    );
    assert_eq!(
        resolution.plugins[0].source,
        oc_plugin_host::PluginSourceKind::Local {
            path: root.join(".opencode").join("plugin").join("beta.ts").display().to_string(),
        }
    );

    // Discovered ops compose with document ops: an explicit remove (by
    // path stem) drops a discovered plugin — the v2 "explicit config is
    // applied last" rule.
    let mut combined = ops;
    combined.extend(parse_ops(&[json!("-alpha")]).expect("remove op"));
    let resolution = resolve(&combined, None);
    let ids: Vec<&str> = resolution.plugins.iter().map(|plugin| plugin.target.as_str()).collect();
    assert!(
        ids.iter().all(|target| !target.ends_with("plugins/alpha")),
        "explicit remove drops the discovered plugin: {ids:?}"
    );
    assert_eq!(resolution.plugins.len(), 1);
}

/// Missing directories contribute nothing (a config layer without
/// plugin dirs is the common case).
#[test]
fn absent_directories_discover_nothing() {
    let ops = discover_directory_ops(&[PathBuf::from(
        "/tmp/opencode/lane-plugin2-discovery-absent-does-not-exist",
    )]);
    assert!(ops.is_empty(), "no plugin dirs, no ops: {ops:?}");
}
