//! M1 official-TUI smoke (opt-in): boot the `Rust server` in a sandbox
//! with the `FakeLlm` provider config, launch the official v2.0.24 TUI
//! headless inside tmux against it, send one prompt, and assert the fake
//! model's deterministic reply lands in the transcript.
//!
//! Opt-in: the test is `#[ignore]`d AND requires `OC_M1_TUI=1` at
//! runtime, so CI-style runs never need tmux:
//!
//! ```text
//! OC_M1_TUI=1 cargo nextest run --run-ignored only -E 'test(m1_official_tui)'
//! ```
//!
//! Artifacts: both pane captures are written to
//! `/tmp/opencode/m1-artifacts/` and copied into
//! `tools/parity/goldens/m1/` as the visible-proof evidence.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use parity::rust_server::{self, RustSpawn};
use parity::scenario::{FakeLlm, ScenarioHost};

/// The reference checkout root (override with `PARITY_REF_ROOT`).
fn tui_root() -> PathBuf {
    std::env::var("PARITY_REF_ROOT")
        .map_or_else(|_| PathBuf::from("/tmp/opencode/ref"), PathBuf::from)
        .join("packages/tui")
}

fn tmux(args: &[&str]) -> String {
    let output = Command::new("tmux").args(args).output().expect("run tmux");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn sleep(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

#[tokio::test]
#[ignore = "official-TUI smoke: needs tmux + the reference checkout; opt in with OC_M1_TUI=1"]
async fn m1_official_tui_smoke_against_rust_server() -> anyhow::Result<()> {
    if std::env::var("OC_M1_TUI").ok().as_deref() != Some("1") {
        eprintln!("skipping: OC_M1_TUI != 1 (opt-in; see the module docs)");
        return Ok(());
    }
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("skipping: tmux not installed");
        return Ok(());
    }
    if !tui_root().is_dir() {
        eprintln!("skipping: reference TUI checkout missing at {}", tui_root().display());
        return Ok(());
    }

    let fake = FakeLlm::start().await?;
    // The provider config ALSO pins the default model, so the TUI's first
    // session runs on the fake without a model-picker detour.
    let config = format!(
        r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "parityfake/fake-model",
  "providers": {{
    "parityfake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "{}/v1" }},
      "models": {{ "fake-model": {{ "name": "Fake Model" }} }}
    }}
  }}
}}
"#,
        fake.base()
    );
    let mut options = RustSpawn::default();
    options.seed_files.push((PathBuf::from("project/opencode.json"), config));
    options.extra_env.push(("OPENCODE_CONFIG", "{sandbox}/project/opencode.json"));
    let server = rust_server::spawn_rust(options).await?;
    eprintln!("[m1-tui] rust server ready at {}", server.url());

    // Scratch dirs.
    let artifacts = PathBuf::from("/tmp/opencode/m1-artifacts");
    let tui_home = PathBuf::from("/tmp/opencode/m1-tui");
    std::fs::create_dir_all(&artifacts)?;
    std::fs::create_dir_all(&tui_home)?;

    // Boot the official v2.0.24 TUI headless against the Rust server.
    // The TUI ships inside the CLI package (no standalone entry): the CLI
    // with no subcommand runs it; --server pins our endpoint.
    let _ = Command::new("tmux").args(["kill-session", "-t", "m1"]).output();
    let launch = format!(
        "cd {cwd} && TERM=xterm-256color HOME={home} OPENCODE_VERSION=2.0.24 \
         OPENCODE_PASSWORD={password} OPENCODE_CONFIG={config} \
         /root/.bun/bin/bun run --cwd {cli} --define OPENCODE_VERSION=\"2.0.24\" \
         --define OPENCODE_CHANNEL=\"stable\" --define OPENCODE_ARTIFACT=\"cli\" \
         src/index.ts --server {url}",
        cwd = tui_home.display(),
        home = tui_home.display(),
        password = server.password(),
        config = server.sandbox().join("project/opencode.json").display(),
        cli = tui_root().join("../cli").display(),
        url = server.url(),
    );
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", "m1", "-x", "160", "-y", "40", &launch])
        .output()
        .expect("tmux new-session");
    anyhow::ensure!(status.status.success(), "tmux new-session failed: {status:?}");

    // Let the TUI boot + connect.
    sleep(14);
    let before = tmux(&["capture-pane", "-p", "-t", "m1"]);
    std::fs::write(artifacts.join("tui-before.txt"), &before)?;
    anyhow::ensure!(
        before.contains("Server"),
        "TUI did not reach the connected home screen:\n{before}"
    );

    // Dismiss any first-run modal, then type the prompt + submit.
    Command::new("tmux").args(["send-keys", "-t", "m1", "Escape"]).output().expect("escape");
    sleep(1);
    Command::new("tmux").args(["send-keys", "-t", "m1", "Say hello."]).output().expect("send");
    sleep(1);
    Command::new("tmux").args(["send-keys", "-t", "m1", "Enter"]).output().expect("enter");

    // Wait for the deterministic reply.
    sleep(12);
    let after = tmux(&["capture-pane", "-p", "-t", "m1"]);
    std::fs::write(artifacts.join("tui-after.txt"), &after)?;
    let _ = Command::new("tmux").args(["kill-session", "-t", "m1"]).output();

    // Evidence copies next to the goldens.
    let goldens = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens/m1");
    std::fs::create_dir_all(&goldens)?;
    std::fs::copy(artifacts.join("tui-before.txt"), goldens.join("tui-before.txt"))?;
    std::fs::copy(artifacts.join("tui-after.txt"), goldens.join("tui-after.txt"))?;

    server.stop().await?;

    // Assertions: connected (no reconnect banner), the prompt text in the
    // input/transcript, the fake model's reply in the transcript.
    let combined = format!("{before}{after}");
    anyhow::ensure!(
        !combined.to_lowercase().contains("reconnect"),
        "TUI shows a reconnect banner (not connected):\n{combined}"
    );
    anyhow::ensure!(
        combined.contains("Say hello."),
        "the typed prompt text is missing from the pane:\n{combined}"
    );
    anyhow::ensure!(
        after.contains("Parity fake says hello."),
        "the fake model reply is missing from the transcript:\n{after}"
    );
    eprintln!(
        "[m1-tui] PASS — reply visible; captures in {} and {}",
        artifacts.display(),
        goldens.display()
    );
    Ok(())
}
