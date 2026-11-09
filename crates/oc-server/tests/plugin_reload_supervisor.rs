//! G-222 — the plugin reload supervisor (plugin/supervisor.ts:196-237):
//! editing a plugin file reloads WITHOUT any config entry change (the
//! plugin-source fs-watch leg, source.ts:60-68), drop-in add/delete
//! drives the add/remove path, the `sdk.plugin.updated` bus event
//! triggers a pass (supervisor.ts:213), config-only changes keep the
//! ConfigReloadBridge's refresh working (no regression), and the 100ms
//! debounce coalesces a burst of quick edits into ONE pass
//! (supervisor.ts:236).
//!
//! Hermetic: sandboxed HOME/config/cache (env redirected inside the
//! nextest per-test process), the committed `helloworld.ts` fixture
//! copied into the sandbox (never the repo file), no network. Bun-gated
//! like the oc-plugin-host guest tests: a host without bun never comes
//! ready and there is no supervisor to observe.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;
use oc_server::plugins::PluginBoot;

struct Sandbox {
    #[allow(dead_code)]
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root = PathBuf::from(format!("/tmp/opencode/oc-server-g222-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    // The drop-in root is the `.opencode` project config dir
    // (config/discovery.ts CANDIDATES) — its `plugin/` child is what
    // `discover_directory_ops` scans.
    std::fs::create_dir_all(project.join(".opencode").join("plugin")).expect("plugin dir");
    Sandbox { root, home, project }
}

impl Sandbox {
    /// Redirect the process env into the sandbox (nextest runs each
    /// test in its own process, so the mutation is contained).
    fn redirect_env(&self) {
        std::env::set_var("HOME", &self.home);
        std::env::set_var("XDG_CONFIG_HOME", self.home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", self.home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", self.home.join(".local").join("share"));
        std::env::set_var("XDG_STATE_HOME", self.home.join(".local").join("state"));
        std::env::set_var("OPENCODE_CONFIG_DIR", self.home.join(".config").join("opencode-rust"));
        std::env::remove_var("OPENCODE_CONFIG");
    }

    fn write_project_config(&self, config: &Value) {
        std::fs::write(self.project.join("opencode.json"), config.to_string()).expect("config");
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

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// The committed helloworld fixture copied into the sandbox (tests
/// mutate their copies — reload edits must never touch the repo).
fn copy_helloworld(destination: &Path) -> PathBuf {
    let source = oc_plugin_host::fixture_dir().join("helloworld.ts");
    std::fs::copy(&source, destination).unwrap_or_else(|error| panic!("copy helloworld: {error}"));
    destination.to_path_buf()
}

/// The `hw.hello` command's description (the reload observable: the
/// fixture embeds `VERSION` — "v1" pristine, "v2" after the edit).
fn hello_description(boot: &Arc<PluginBoot>) -> Option<String> {
    boot.host
        .registrations()
        .commands
        .iter()
        .find(|command| command.name == "hw.hello")
        .map(|command| command.description.clone())
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

/// Boots the real server and waits for a READY plugin host with its
/// supervisor (the G-222 spawn gate).
async fn boot_with_plugins(box_: &Sandbox) -> (oc_server::lifecycle::ServeHandle, Arc<PluginBoot>) {
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let boot = wait_for(Duration::from_secs(30), || {
        handle.state.plugins.as_ref().and_then(|boot| {
            if boot.ready {
                Some(Arc::clone(boot))
            } else {
                None
            }
        })
    })
    .await
    .expect("plugin host ready");
    (handle, boot)
}

/// The fs-watch leg: editing a plugin drop-in reloads it with NO config
/// entry change (config entries deep-equal — the old bridge-only shape
/// never reloaded; supervisor.ts:198 + source.ts:60-68).
#[tokio::test]
async fn plugin_file_edit_reloads_without_config_change() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("edit");
    let fixture = copy_helloworld(&box_.project.join(".opencode").join("plugin").join("hello.ts"));
    box_.write_project_config(&json!({}));
    box_.redirect_env();
    let (handle, boot) = boot_with_plugins(&box_).await;
    let description =
        wait_for(Duration::from_secs(20), || hello_description(&boot)).await.expect("hw.hello v1");
    assert!(description.contains("v1"), "{description}");
    let supervisor = boot.supervisor().expect("supervisor spawned");

    let before = supervisor.passes();
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    std::fs::write(&fixture, source.replace("v1", "v2")).expect("edit fixture");

    let reloaded = wait_for(Duration::from_secs(20), || {
        let description = hello_description(&boot)?;
        (supervisor.passes() > before && description.contains("v2")).then_some(description)
    })
    .await
    .expect("reload after fs edit");
    assert!(reloaded.contains("v2"), "{reloaded}");
    handle.begin_shutdown();
}

/// The add/remove legs: a new drop-in file loads its generation, a
/// deleted drop-in unloads it (supervisor.ts:23-94 through the pass).
#[tokio::test]
async fn dropin_add_and_delete_drive_the_add_remove_path() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("addrm");
    box_.write_project_config(&json!({}));
    box_.redirect_env();
    let (handle, boot) = boot_with_plugins(&box_).await;
    let supervisor =
        wait_for(Duration::from_secs(20), || boot.supervisor()).await.expect("supervisor");
    assert_eq!(hello_description(&boot), None, "no drop-in at boot");

    // Add: the directory watch fires, the pass re-scans, add_entry loads.
    let added = box_.project.join(".opencode").join("plugin").join("hello.ts");
    copy_helloworld(&added);
    wait_for(Duration::from_secs(20), || hello_description(&boot))
        .await
        .expect("drop-in added and loaded");

    // Delete: remove_entry tears the generation down.
    std::fs::remove_file(&added).expect("delete drop-in");
    let removed =
        wait_for(Duration::from_secs(20), || hello_description(&boot).is_none().then_some(()))
            .await;
    assert!(removed.is_some(), "drop-in removed and unloaded (passes={})", supervisor.passes());
    handle.begin_shutdown();
}

/// The bus leg: `sdk.plugin.updated` (sdk.ts:9) triggers a debounced
/// pass (supervisor.ts:213).
#[tokio::test]
async fn sdk_plugin_updated_bus_event_triggers_a_pass() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("sdkbus");
    copy_helloworld(&box_.project.join(".opencode").join("plugin").join("hello.ts"));
    box_.write_project_config(&json!({}));
    box_.redirect_env();
    let (handle, boot) = boot_with_plugins(&box_).await;
    wait_for(Duration::from_secs(20), || hello_description(&boot)).await.expect("hw.hello v1");
    let supervisor = boot.supervisor().expect("supervisor spawned");

    let before = supervisor.passes();
    let definition = oc_core::bus::registry::by_type(oc_core::bus::registry::SDK_PLUGIN_UPDATED)
        .expect("sdk.plugin.updated registered");
    handle
        .state
        .engine
        .bus
        .publish(definition, json!({}), oc_core::bus::PublishOptions::default())
        .await
        .expect("publish sdk.plugin.updated");

    wait_for(Duration::from_secs(10), || (supervisor.passes() > before).then_some(()))
        .await
        .expect("bus event drove a supervisor pass");
    handle.begin_shutdown();
}

/// No regression: a config-ONLY change (no plugin source touched) still
/// goes through ConfigReloadBridge's refresh — the plugin survives the
/// reload (the bridge leg kept its immediate pass, boot.rs).
#[tokio::test]
async fn config_only_change_keeps_the_plugin_alive() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("cfgonly");
    copy_helloworld(&box_.project.join(".opencode").join("plugin").join("hello.ts"));
    box_.write_project_config(&json!({ "theme": "opencode-dark" }));
    box_.redirect_env();
    let (handle, boot) = boot_with_plugins(&box_).await;
    wait_for(Duration::from_secs(20), || hello_description(&boot)).await.expect("hw.hello v1");

    // A config-entry change (deep-equal differs) — the bridge's leg.
    box_.write_project_config(&json!({ "theme": "opencode-light" }));
    let survived = wait_for(Duration::from_secs(10), || {
        let description = hello_description(&boot)?;
        description.contains("v1").then_some(description)
    })
    .await;
    assert!(survived.is_some(), "plugin survived the config-only reload");
    handle.begin_shutdown();
}

/// supervisor.ts:236 — the 100ms debounce: a burst of quick edits
/// coalesces into ONE pass (the sliding(1) queue retains only the
/// latest request; the window extends while triggers keep landing).
#[tokio::test]
async fn debounce_coalesces_a_burst_of_edits_into_one_pass() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("burst");
    let fixture = copy_helloworld(&box_.project.join(".opencode").join("plugin").join("hello.ts"));
    box_.write_project_config(&json!({}));
    box_.redirect_env();
    let (handle, boot) = boot_with_plugins(&box_).await;
    wait_for(Duration::from_secs(20), || hello_description(&boot)).await.expect("hw.hello v1");
    let supervisor = boot.supervisor().expect("supervisor spawned");
    // Let any boot-time pass settle before counting.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let before = supervisor.passes();

    // Three rapid writes of the same edited content — one physical
    // outcome, a burst of fs events, exactly one debounced pass.
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    let edited = source.replace("v1", "v2");
    for _ in 0..3 {
        std::fs::write(&fixture, &edited).expect("burst write");
    }

    let reloaded = wait_for(Duration::from_secs(20), || {
        let description = hello_description(&boot)?;
        description.contains("v2").then_some(description)
    })
    .await
    .expect("burst produced a reload");
    assert!(reloaded.contains("v2"), "{reloaded}");
    // Quiet window well past the debounce: no second pass may arrive.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        supervisor.passes(),
        before + 1,
        "G-222: the burst coalesced into exactly one reload pass"
    );
    handle.begin_shutdown();
}
