//! G-350 — `/api/vcs/init` plugin provider dispatch over a REAL
//! plugin host (bun-gated, sandboxed like the G-222 reload tests):
//! a plugin-registered vcs provider's `init` closure runs through the
//! "vcs"/"init" hook seam when the endpoint names it, a registered
//! provider WITHOUT `init` is the 501 `VcsInitNotSupportedError`
//! (ref vcs.ts:182), and an unknown provider keeps the G-270④ 400
//! `field:"provider"`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::{ServeHandle, ServeOptions};
use oc_server::plugins::PluginBoot;

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root = PathBuf::from(format!("/tmp/opencode/oc-server-g350-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(home.join(".config")).expect("home");
    std::fs::create_dir_all(project.join(".opencode").join("plugin")).expect("plugin dir");
    Sandbox { root, home, project }
}

impl Sandbox {
    fn redirect_env(&self) {
        std::env::set_var("HOME", &self.home);
        std::env::set_var("XDG_CONFIG_HOME", self.home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", self.home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", self.home.join(".local").join("share"));
        std::env::set_var("XDG_STATE_HOME", self.home.join(".local").join("state"));
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

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn copy_fixture(destination: &Path) -> PathBuf {
    let source = oc_plugin_host::fixture_dir().join("fakevcs.ts");
    std::fs::copy(&source, destination).unwrap_or_else(|e| panic!("copy fakevcs: {e}"));
    destination.to_path_buf()
}

async fn wait_for_boot(handle: &ServeHandle) -> Option<Arc<PluginBoot>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(boot) = handle.state.plugins.as_ref().filter(|boot| boot.ready) {
            return Some(Arc::clone(boot));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

/// Raw POST returning `(status, body)`.
async fn post(addr: std::net::SocketAddr, target: &str) -> (u16, Value) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!(
        "POST {target} HTTP/1.1\r\nhost: {addr}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status =
        text.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn vcs_init_dispatches_to_registered_plugin_provider() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("http");
    copy_fixture(&box_.project.join(".opencode").join("plugin").join("fakevcs.ts"));
    std::fs::write(box_.project.join("opencode.json"), "{}").expect("config");
    box_.redirect_env();
    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let boot = wait_for_boot(&handle).await.expect("plugin host ready");

    // The provider registered WITH its init closure.
    assert!(
        boot.host
            .registrations()
            .vcs_providers
            .iter()
            .any(|row| row.id == "fakevcs" && row.has_init),
        "fakevcs registered: {:?}",
        boot.host.registrations().vcs_providers
    );

    let addr = *handle.state.local_addr.read();
    let addr = addr.expect("bound addr");
    // Unknown provider → the G-270④ 400 field:"provider".
    let (status, body) = post(addr, "/api/vcs/init?provider=darcs").await;
    assert_eq!(status, 400, "unknown provider: {body}");
    assert_eq!(body["field"], "provider", "{body}");

    // Registered WITHOUT init → 501 VcsInitNotSupportedError.
    let (status, body) = post(addr, "/api/vcs/init?provider=novcs").await;
    assert_eq!(status, 501, "no-init provider: {body}");
    assert_eq!(body["_tag"], "VcsInitNotSupportedError", "{body}");

    // Registered WITH init → the guest closure ran (the minimal
    // repository it materializes makes the post-init re-resolve see a
    // vcs root → 204).
    assert!(!box_.project.join(".git").exists(), "no vcs before init");
    let (status, body) = post(addr, "/api/vcs/init?provider=fakevcs").await;
    assert_eq!(status, 204, "plugin provider init: {body}");
    assert!(
        box_.project.join(".git").join("HEAD").exists(),
        "the guest init closure materialized the repo"
    );

    // Re-init is the 409 conflict (the marker walk sees the new .git).
    let (status, body) = post(addr, "/api/vcs/init?provider=fakevcs").await;
    assert_eq!(status, 409, "re-init conflicts: {body}");
}
