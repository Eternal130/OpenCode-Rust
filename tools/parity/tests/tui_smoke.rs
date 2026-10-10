//! The OUR-TUI smoke (opt-in, the m1 pattern extended to the Rust
//! port's own TUI): boot the Rust server sandboxed with the `FakeLlm`
//! provider config, launch the `opencode` CLI's root command (the
//! oc-tui run loop) headless inside tmux against it, send one prompt,
//! and assert the fake model's deterministic reply renders in the
//! transcript.
//!
//! Opt-in: `#[ignore]`d AND requires `OC_TUI_SMOKE=1` at runtime:
//!
//! ```text
//! OC_TUI_SMOKE=1 cargo nextest run --run-ignored only -E 'test(tui_smoke)'
//! ```
//!
//! Artifacts: pane captures under `/tmp/opencode/tui-smoke/` + evidence
//! copies in `tools/parity/goldens/tui-smoke/`.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use parity::rust_server::{self, RustSpawn};
use parity::scenario::{FakeLlm, ScenarioHost};

fn tmux(args: &[&str]) -> String {
    let output = Command::new("tmux").args(args).output().expect("run tmux");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Async sleep: the `FakeLlm` HTTP server runs on THIS test's tokio
/// runtime — a blocking sleep would starve it and hang the engine's
/// provider call (the reply never arrives).
async fn sleep(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

#[tokio::test]
#[ignore = "our-TUI smoke: needs tmux; opt in with OC_TUI_SMOKE=1"]
#[allow(clippy::too_many_lines)]
async fn tui_smoke_rust_port_against_fake_llm() -> anyhow::Result<()> {
    if std::env::var("OC_TUI_SMOKE").ok().as_deref() != Some("1") {
        eprintln!("skipping: OC_TUI_SMOKE != 1 (opt-in; see the module docs)");
        return Ok(());
    }
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("skipping: tmux not installed");
        return Ok(());
    }
    let cli = rust_server::cli_binary()?;

    let fake = FakeLlm::start().await?;
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
    eprintln!("[tui-smoke] rust server ready at {}", server.url());

    let artifacts = PathBuf::from("/tmp/opencode/tui-smoke");
    let home = PathBuf::from("/tmp/opencode/tui-smoke-home");
    std::fs::create_dir_all(&artifacts)?;
    std::fs::create_dir_all(&home)?;

    // Launch OUR TUI (the CLI root command) in tmux.
    let _ = Command::new("tmux").args(["kill-session", "-t", "ocsmoke"]).output();
    let launch = format!(
        "cd {cwd} && TERM=xterm-256color HOME={home} OPENCODE_PASSWORD={password} \
         {cli} --server {url} 2>{stderr}",
        cwd = home.display(),
        home = home.display(),
        password = server.password(),
        cli = cli.display(),
        url = server.url(),
        stderr = artifacts.join("tui-stderr.txt").display(),
    );
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", "ocsmoke", "-x", "120", "-y", "34", &launch])
        .output()
        .expect("tmux new-session");
    anyhow::ensure!(status.status.success(), "tmux new-session failed: {status:?}");

    // Boot + connect (the m1 smoke's generous timing — the engine's
    // provider bootstrap needs a few seconds before first prompt).
    let mut before = String::new();
    for _ in 0..10 {
        sleep(2).await;
        before = tmux(&["capture-pane", "-p", "-t", "ocsmoke"]);
        if before.contains("Ask anything") || before.contains("opencode") {
            break;
        }
    }
    std::fs::write(artifacts.join("tui-before.txt"), &before)?;
    anyhow::ensure!(
        before.contains("opencode") || before.contains("Ask anything"),
        "the TUI home screen did not render:\n{before}"
    );
    anyhow::ensure!(
        !before.to_lowercase().contains("error:"),
        "the TUI reported an error:\n{before}"
    );

    // Dismiss any first-run state, then send a prompt + submit.
    let _ = Command::new("tmux").args(["send-keys", "-t", "ocsmoke", "Escape"]).output();
    sleep(1).await;
    let _ = Command::new("tmux").args(["send-keys", "-t", "ocsmoke", "Say hello."]).output();
    sleep(1).await;
    let _ = Command::new("tmux").args(["send-keys", "-t", "ocsmoke", "Enter"]).output();

    // Wait for the deterministic reply.
    let mut after = String::new();
    for _ in 0..8 {
        sleep(3).await;
        after = tmux(&["capture-pane", "-p", "-t", "ocsmoke"]);
        if after.contains("Parity fake says hello.") {
            break;
        }
    }
    std::fs::write(artifacts.join("tui-after.txt"), &after)?;

    // Exit cleanly (empty prompt: leader-q) so the pane closes.
    let _ = Command::new("tmux").args(["send-keys", "-t", "ocsmoke", "C-x"]).output();
    sleep(1).await;
    let _ = Command::new("tmux").args(["send-keys", "-t", "ocsmoke", "q"]).output();
    sleep(2).await;
    let closed = tmux(&["capture-pane", "-p", "-t", "ocsmoke"]);
    std::fs::write(artifacts.join("tui-exit.txt"), &closed)?;
    let _ = Command::new("tmux").args(["kill-session", "-t", "ocsmoke"]).output();

    // Evidence copies next to the goldens.
    let goldens = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-smoke");
    std::fs::create_dir_all(&goldens)?;
    for name in ["tui-before.txt", "tui-after.txt", "tui-exit.txt"] {
        let _ = std::fs::copy(artifacts.join(name), goldens.join(name));
    }

    server.stop().await?;

    anyhow::ensure!(
        after.contains("Say hello."),
        "the typed prompt is missing from the transcript:\n{after}"
    );
    anyhow::ensure!(
        after.contains("Parity fake says hello."),
        "the fake model reply is missing from the transcript:\n{after}"
    );
    eprintln!("[tui-smoke] PASS — reply rendered; captures in {}", artifacts.display());
    Ok(())
}
