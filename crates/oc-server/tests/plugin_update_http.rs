//! RF-02 — `POST /api/plugin/update` now performs the update: the
//! handler validates targets against the booted inventory (400 on
//! unknown), runs the oc-plugin-host update flow (registry check via a
//! PATH-shimmed `npm view`, new-generation install), 503s with the
//! rolled-up failures when an update fails, and reloads the updated
//! plugin (`plugin.updated` notify leg).
//!
//! Hermetic: sandboxed HOME/config/cache (the plugins_production
//! pattern), pre-staged install generations under the v2 cache layout,
//! an `npm view` shim ahead of the real npm on PATH, no network.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

fn sandbox(tag: &str) -> PathBuf {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-plugin-update-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    root
}

/// Stage `demo-plugin@<version>` under `<home>/.cache/opencode-rust/npm`
/// (the layout `plugin add` writes and `resolve` scans).
fn stage_generation(home: &Path, version: &str, stamp: &str) {
    let pkg = home
        .join(".cache")
        .join("opencode-rust")
        .join("npm")
        .join(format!("demo-plugin@{version}"))
        .join(stamp)
        .join("node_modules")
        .join("demo-plugin");
    std::fs::create_dir_all(&pkg).expect("generation dir");
    std::fs::write(
        pkg.join("package.json"),
        json!({ "name": "demo-plugin", "version": version, "main": "plugin.js" }).to_string(),
    )
    .expect("manifest");
    std::fs::write(
        pkg.join("plugin.js"),
        "export default { id: 'demo-plugin', async setup() {} }",
    )
    .expect("entry");
}

/// The `npm view demo-plugin version` shim: answers with the current
/// `registry-version.txt` (the test flips the registry between calls)
/// and logs every invocation (the update-path proof).
fn npm_shim(root: &Path, registry_version: &str) -> PathBuf {
    let dir = root.join("shims");
    std::fs::create_dir_all(&dir).expect("shim dir");
    std::fs::write(dir.join("registry-version.txt"), registry_version).expect("registry seed");
    let shim = dir.join("npm");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho \"view $*\" >> \"{}/invocations.log\"\n[ \"$1\" = \"view\" ] && cat \"{}/registry-version.txt\" && exit 0\nexit 1\n",
            dir.display(),
            dir.display()
        ),
    )
    .expect("shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    dir
}

fn boot_options(root: &Path, home: &Path, project: &Path) -> BootOptions {
    BootOptions {
        serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
        password: None,
        cors_origins: Vec::new(),
        version: "2.0.24".to_owned(),
        data_root: root.join("data"),
        home: home.to_owned(),
        config_location: Some(project.to_owned()),
    }
}

fn redirect_env(home: &Path, shims: &Path) {
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
    std::env::set_var("XDG_STATE_HOME", home.join(".local").join("state"));
    std::env::remove_var("OPENCODE_CONFIG");
    // The registry shim wins; everything after it still resolves.
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", shims.display(), path.to_string_lossy()),
    );
}

#[tokio::test]
async fn plugin_update_performs_the_update() {
    let root = sandbox("lane");
    let home = root.join("home");
    let project = root.join("project");
    stage_generation(&home, "1.0.0", "1000");
    // The update target generation (pre-staged — the install reuse
    // path, exactly what `plugin add` leaves behind after a pin).
    stage_generation(&home, "2.0.0", "2000");
    std::fs::write(project.join("opencode.json"), json!({ "plugins": ["demo-plugin"] }).to_string())
        .expect("config");
    let shims = npm_shim(&root, "2.0.0");
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // Unknown target → 400 InvalidRequestError naming the target.
    let error = client
        .plugin()
        .update(
            &oc_client::types::PluginUpdateQuery { location: None },
            &oc_client::types::PluginUpdateBody {
                targets: vec!["not-a-plugin".to_owned()],
            },
        )
        .await
        .expect_err("unknown target is a 400");
    match &error {
        oc_client::ClientError::Api { status, name, message, fields } => {
            assert_eq!(*status, 400, "{error}");
            assert_eq!(name, "InvalidRequestError", "{error}");
            assert!(
                message.contains("not in the current server inventory"),
                "message names the offender: {error}"
            );
            assert_eq!(fields.get("field"), Some(&json!("targets")), "{error}");
        }
        other => panic!("unexpected error shape: {other}"),
    }

    // The real update: the registry (shim) says 2.0.0, the install
    // lands on the pre-staged 2.0.0 generation → 204 no content.
    client
        .plugin()
        .update(
            &oc_client::types::PluginUpdateQuery { location: None },
            &oc_client::types::PluginUpdateBody { targets: vec!["demo-plugin".to_owned()] },
        )
        .await
        .expect("update returns 204");

    // The update path actually executed: the registry was probed for
    // the target (the new-generation install itself is covered by the
    // oc-plugin-host lane test through a shimmed installer).
    let invocations =
        std::fs::read_to_string(root.join("shims").join("invocations.log")).unwrap_or_default();
    assert!(
        invocations.contains("view demo-plugin"),
        "the update probed the registry: {invocations:?}"
    );

    // The inventory still lists the plugin (the reload is contained —
    // the previous generation keeps its place on any failure).
    let plugins = client
        .plugin()
        .list(&oc_client::types::PluginListQuery { location: None })
        .await
        .expect("plugin.list");
    let entries: Vec<Value> =
        plugins.data.iter().map(|p| serde_json::to_value(p).unwrap()).collect();
    assert!(
        entries.iter().any(|entry| entry["source"]["target"] == "demo-plugin"),
        "demo-plugin still listed: {entries:?}"
    );

    handle.shutdown_and_join().await.expect("shutdown");
}

#[tokio::test]
async fn plugin_update_rolls_failures_into_a_503() {
    let root = sandbox("fail");
    let home = root.join("home");
    let project = root.join("project");
    stage_generation(&home, "1.0.0", "1000");
    std::fs::write(project.join("opencode.json"), json!({ "plugins": ["demo-plugin"] }).to_string())
        .expect("config");
    // An empty registry answer → the version pin fails → the update
    // fails → one 503 carrying the rolled-up failure.
    let shims = npm_shim(&root, "");
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    let error = client
        .plugin()
        .update(
            &oc_client::types::PluginUpdateQuery { location: None },
            &oc_client::types::PluginUpdateBody { targets: vec!["demo-plugin".to_owned()] },
        )
        .await
        .expect_err("failed update is a 503");
    match &error {
        oc_client::ClientError::Api { status, name, message, .. } => {
            assert_eq!(*status, 503, "{error}");
            assert_eq!(name, "ServiceUnavailableError", "{error}");
            assert!(
                message.contains("Failed to update plugin packages"),
                "rolled-up message: {error}"
            );
        }
        other => panic!("unexpected error shape: {other}"),
    }

    handle.shutdown_and_join().await.expect("shutdown");
}
