//! G-29 — `POST /api/plugin/check` now performs the outdated check and
//! `POST /api/plugin/install` performs the pinned install: the check
//! handler validates a named target against the booted inventory (400
//! on unknown), probes the registry with `refresh: true` (the
//! PATH-shimmed `npm view`) and returns the inventory with fresh
//! `outdated` flags; the install handler rejects directive-ish
//! targets (400), installs (the reuse-first path over a pre-staged
//! generation), appends the target to the global configuration, and
//! activates it so `plugin.list` shows it immediately.
//!
//! Hermetic: sandboxed HOME/config/cache (the `plugins_production`
//! pattern), pre-staged install generations under the v2 cache layout,
//! an `npm view` shim ahead of the real npm on PATH, no network.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

fn sandbox(tag: &str) -> PathBuf {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-plugin-ci-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    root
}

/// Stage `<package>@<version>` under `<home>/.cache/opencode-rust/npm`
/// (the layout `plugin add` writes and `resolve` scans).
fn stage_generation(home: &Path, package: &str, version: &str, stamp: &str) {
    let pkg = home
        .join(".cache")
        .join("opencode-rust")
        .join("npm")
        .join(format!("{package}@{version}"))
        .join(stamp)
        .join("node_modules")
        .join(package);
    std::fs::create_dir_all(&pkg).expect("generation dir");
    std::fs::write(
        pkg.join("package.json"),
        json!({ "name": package, "version": version, "main": "plugin.js" }).to_string(),
    )
    .expect("manifest");
    std::fs::write(
        pkg.join("plugin.js"),
        format!("export default {{ id: '{package}', async setup() {{}} }}"),
    )
    .expect("entry");
}

/// The `npm view <pkg> version` shim (the `plugin_update_http` pattern):
/// answers with the current `registry-version.txt` and logs every
/// invocation.
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
        events_persist: None,
    }
}

fn redirect_env(home: &Path, shims: &Path) {
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
    std::env::set_var("XDG_STATE_HOME", home.join(".local").join("state"));
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::remove_var("OPENCODE_CONFIG_DIR");
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", shims.display(), path.to_string_lossy()));
}

/// The check response as raw JSON values (the `outdated` flag rides
/// `source`).
async fn check_all(
    client: &oc_client::Client,
    target: Option<&str>,
) -> Result<oc_client::types::PluginCheckResponse, oc_client::ClientError> {
    client
        .plugin()
        .check(
            &oc_client::types::PluginCheckQuery { location: None },
            &oc_client::types::PluginCheckBody { target: target.map(str::to_owned) },
        )
        .await
}

#[tokio::test]
async fn plugin_check_flags_the_outdated_package() {
    let root = sandbox("outdated");
    let home = root.join("home");
    let project = root.join("project");
    stage_generation(&home, "demo-plugin", "1.0.0", "1000");
    std::fs::write(
        project.join("opencode.json"),
        json!({ "plugins": ["demo-plugin"] }).to_string(),
    )
    .expect("config");
    // The registry moved to 2.0.0 → the installed 1.0.0 is outdated.
    let shims = npm_shim(&root, "2.0.0");
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // Unknown named target → 400 InvalidRequestError naming the target.
    let error =
        check_all(&client, Some("not-a-plugin")).await.expect_err("unknown target is a 400");
    match &error {
        oc_client::ClientError::Api { status, name, message, fields } => {
            assert_eq!(*status, 400, "{error}");
            assert_eq!(name, "InvalidRequestError", "{error}");
            assert!(
                message.contains("not in the current server inventory"),
                "message names the offender: {error}"
            );
            assert_eq!(fields.get("field"), Some(&json!("target")), "{error}");
        }
        other => panic!("unexpected error shape: {other}"),
    }

    // The full-inventory check: fresh `outdated: true` on the package
    // source, applied directly in the response (the ref's mutate).
    let response = check_all(&client, None).await.expect("check");
    let entries: Vec<Value> =
        response.data.iter().map(|p| serde_json::to_value(p).unwrap()).collect();
    let demo = entries
        .iter()
        .find(|entry| entry["source"]["target"] == "demo-plugin")
        .expect("demo-plugin listed");
    assert_eq!(demo["source"]["outdated"], json!(true), "flagged outdated: {demo}");

    // The probe actually hit the registry with refresh semantics.
    let invocations =
        std::fs::read_to_string(root.join("shims").join("invocations.log")).unwrap_or_default();
    assert!(
        invocations.contains("view demo-plugin"),
        "the check probed the registry: {invocations:?}"
    );

    handle.shutdown_and_join().await.expect("shutdown");
}

#[tokio::test]
async fn plugin_check_fresh_when_current() {
    let root = sandbox("current");
    let home = root.join("home");
    let project = root.join("project");
    stage_generation(&home, "demo-plugin", "1.0.0", "1000");
    std::fs::write(
        project.join("opencode.json"),
        json!({ "plugins": ["demo-plugin"] }).to_string(),
    )
    .expect("config");
    // The registry still says 1.0.0 → not outdated.
    let shims = npm_shim(&root, "1.0.0");
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    let response = check_all(&client, None).await.expect("check");
    let entries: Vec<Value> =
        response.data.iter().map(|p| serde_json::to_value(p).unwrap()).collect();
    let demo = entries
        .iter()
        .find(|entry| entry["source"]["target"] == "demo-plugin")
        .expect("demo-plugin listed");
    assert_ne!(demo["source"]["outdated"], json!(true), "current version: {demo}");

    handle.shutdown_and_join().await.expect("shutdown");
}

#[tokio::test]
async fn plugin_install_persists_and_activates() {
    let root = sandbox("install");
    let home = root.join("home");
    let project = root.join("project");
    // The install target's pre-staged generation (the reuse path — the
    // registry pin resolves to 1.0.0 and the tree already exists).
    stage_generation(&home, "fresh-plugin", "1.0.0", "1000");
    let shims = npm_shim(&root, "1.0.0");
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // Directive-ish targets are a 400 (the CLI `plugin add` guard).
    for bad in ["*", "opencode.core", "-removed"] {
        let error = client
            .plugin()
            .install(
                &oc_client::types::PluginInstallQuery { location: None },
                &oc_client::types::PluginInstallBody { target: bad.to_owned() },
            )
            .await
            .expect_err("directive target is a 400");
        match &error {
            oc_client::ClientError::Api { status, name, .. } => {
                assert_eq!(*status, 400, "{error}");
                assert_eq!(name, "InvalidRequestError", "{error}");
            }
            other => panic!("unexpected error shape: {other}"),
        }
    }

    // The real install: registry pin + reuse → 204 no content.
    client
        .plugin()
        .install(
            &oc_client::types::PluginInstallQuery { location: None },
            &oc_client::types::PluginInstallBody { target: "fresh-plugin".to_owned() },
        )
        .await
        .expect("install returns 204");

    // The user-scope config names the target (the `plugin add` leg).
    let config_path = home.join(".config").join("opencode-rust").join("opencode.json");
    let config = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|_| panic!("config at {config_path:?}"));
    let parsed: Value =
        serde_json::from_str(&config).unwrap_or_else(|_| panic!("config JSON: {config}"));
    assert!(
        parsed["plugins"].as_array().is_some_and(|plugins| {
            plugins.iter().any(|entry| entry.as_str() == Some("fresh-plugin"))
        }),
        "config plugins[] names the target: {config}"
    );

    // The activation leg: `/api/plugin` lists it immediately.
    let plugins = client
        .plugin()
        .list(&oc_client::types::PluginListQuery { location: None })
        .await
        .expect("plugin.list");
    let entries: Vec<Value> =
        plugins.data.iter().map(|p| serde_json::to_value(p).unwrap()).collect();
    assert!(
        entries.iter().any(|entry| entry["source"]["target"] == "fresh-plugin"),
        "fresh-plugin listed after install: {entries:?}"
    );

    // The install publishes the typed public `plugin.updated` event
    // (RF-02's bus row): the wire name round-trips through the
    // client's event classification (the event_schema guard's
    // coverage leg).
    let frame = oc_client::bus_event::BusEventFrame::decode(&json!({
        "id": "e1",
        "type": "plugin.updated",
        "data": {},
    }))
    .expect("plugin.updated decodes");
    assert_eq!(frame.kind, oc_client::bus_event::EventKind::PluginUpdated);

    handle.shutdown_and_join().await.expect("shutdown");
}

#[tokio::test]
async fn plugin_install_failure_is_a_503() {
    let root = sandbox("fail");
    let home = root.join("home");
    let project = root.join("project");
    // Nothing staged + an empty registry answer → the version pin
    // fails → one 503 naming the target.
    let shims = npm_shim(&root, "");
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    let error = client
        .plugin()
        .install(
            &oc_client::types::PluginInstallQuery { location: None },
            &oc_client::types::PluginInstallBody { target: "ghost-plugin".to_owned() },
        )
        .await
        .expect_err("failed install is a 503");
    match &error {
        oc_client::ClientError::Api { status, name, message, .. } => {
            assert_eq!(*status, 503, "{error}");
            assert_eq!(name, "ServiceUnavailableError", "{error}");
            assert!(
                message.contains("Failed to install plugin package"),
                "rolled-up message: {error}"
            );
        }
        other => panic!("unexpected error shape: {other}"),
    }

    handle.shutdown_and_join().await.expect("shutdown");
}
