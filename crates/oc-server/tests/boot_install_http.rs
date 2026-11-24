//! G-313 (overturned 2026-11-24, ref parity) — the SERVER boot
//! auto-install leg: a config `plugins[]` entry whose generation is
//! absent from the cache installs at boot (the ref's `install: true`
//! reconcile — context.tsx:298-302 → default.ts:140 → npm.ts:344-349)
//! and comes up activated in `plugin.list` with NO resolve failure.
//!
//! Hermetic (the `plugin_check_install_http` pattern): sandboxed
//! HOME/config/cache, a `bun` shim ahead of the real bun that
//! materializes `node_modules/<pkg>` for `bun install` and DELEGATES
//! every other subcommand to the real bun (the guest host runtime
//! keeps working), no network.

use std::path::{Path, PathBuf};

use serde_json::json;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

fn sandbox(tag: &str) -> PathBuf {
    let root =
        PathBuf::from(format!("/tmp/opencode/oc-server-boot-install-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home")).expect("home");
    std::fs::create_dir_all(root.join("project")).expect("project");
    root
}

fn redirect_env(home: &Path, shims: &Path) {
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
    std::env::set_var("XDG_STATE_HOME", home.join(".local").join("state"));
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::remove_var("OPENCODE_CONFIG_DIR");
    // The install leg resolves bun through bunman — the env override
    // pins it to the shim (the plugin_update_lane shim pattern).
    std::env::set_var("OC_PLUGIN_BUN", shims.join("bun"));
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

/// A `bun` shim: `bun install` materializes `node_modules/<pkg>` from
/// the `<pkg>@<version>` cache key two levels up (logging the
/// invocation); every other subcommand delegates to the REAL bun so
/// the plugin host guest runtime behaves unchanged.
fn bun_shim(root: &Path) -> PathBuf {
    let dir = root.join("shims");
    std::fs::create_dir_all(&dir).expect("shim dir");
    let real_bun = which_bun();
    let shim = dir.join("bun");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho \"$*\" >> \"{log}/install-invocations.log\"\n\
             [ \"$1\" = \"install\" ] || exec \"{real}\" \"$@\"\n\
             key=$(basename \"$(dirname \"$PWD\")\")\nname=${{key%@*}}\nver=${{key##*@}}\n\
             pkgdir=\"$PWD/node_modules/$name\"\nmkdir -p \"$pkgdir\"\n\
             printf '{{\"name\":\"%s\",\"version\":\"%s\",\"main\":\"plugin.js\"}}\\n' \"$name\" \
             \"$ver\" > \"$pkgdir/package.json\"\n\
             printf 'export default {{ id: \"%s\", async setup() {{}} }}\\n' \"$name\" > \
             \"$pkgdir/plugin.js\"\nexit 0\n",
            log = dir.display(),
            real = real_bun.display()
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

fn which_bun() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join("bun"))
        .find(|candidate| candidate.is_file())
        .expect("real bun on PATH (skip-grade box: this test requires bun)")
}

#[tokio::test]
async fn server_boot_auto_installs_missing_config_plugin() {
    let root = sandbox("boot-install");
    let home = root.join("home");
    let project = root.join("project");
    // The config names a plugin with NO pre-staged generation.
    std::fs::write(
        project.join("opencode.json"),
        json!({ "plugins": ["demo-plugin@1.0.0"] }).to_string(),
    )
    .expect("config");
    let shims = bun_shim(&root);
    redirect_env(&home, &shims);

    let handle = boot_and_serve(&boot_options(&root, &home, &project)).await.expect("boot");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // The boot leg installed the missing spec...
    let invocations = std::fs::read_to_string(root.join("shims").join("install-invocations.log"))
        .expect("the installer ran");
    assert!(
        invocations.contains("install"),
        "boot auto-install ran the installer: {invocations:?}"
    );

    // ...and the plugin came up in the inventory (activated this
    // boot, not left as a resolve failure).
    let listed = client
        .plugin()
        .list(&oc_client::types::PluginListQuery { location: None })
        .await
        .expect("plugin list");
    let entries: Vec<serde_json::Value> =
        listed.data.iter().map(|p| serde_json::to_value(p).unwrap()).collect();
    let demo = entries
        .iter()
        .find(|entry| entry["source"]["target"] == "demo-plugin@1.0.0")
        .expect("the auto-installed plugin is listed");
    assert_ne!(demo["state"]["type"], "failed", "plugin activated: {demo}");

    handle.shutdown_and_join().await.expect("shutdown");
}
