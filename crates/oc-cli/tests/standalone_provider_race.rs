//! Regression: the standalone-boot provider race (gate-7 race-hunt task).
//!
//! The observed flake (w6a lane): a standalone (`serve --stdio`) child
//! OCCASIONALLY booted without the project config — the first run failed
//! with "No model is available" or silently fell back to a catalog model,
//! and a retry self-healed. Deterministic reproduction per the gate's
//! strategy: spawn the standalone pair ×50 in a loop with an unmistakable
//! custom model id (`race-probe-model-7f3a`), child environment pinned
//! (XDG roots + `OPENCODE_TEST_HOME` inside the sandbox — also covers the
//! env-redirect mechanism), and assert EVERY completed iteration resolves
//! and runs the config-seeded provider end-to-end (step marker carries
//! the custom model id; the `FakeLlm` greeting lands on stdout).
//!
//! The fixes under test:
//!
//! 1. **Per-location provider overlay** (oc-core): the session resolver
//!    consults the location's config-seeded custom providers FIRST, so
//!    the child serves the project's provider regardless of boot-order
//!    details;
//! 2. **Ready-after-boot ordering** (oc-server `boot_and_serve`): the
//!    boot location's assembly (config discovery → provider overlay →
//!    graph) completes before the listener binds and the `{"url"}` ready
//!    line prints — the ready signal can never race the provider/catalog
//!    assembly.
//!
//! Slow-but-ON: bounded at ~55s wall (waves stop at the deadline; a
//! minimum of 12 completed iterations is required for the run to count).
//! Never binds a fixed port (`FakeLlm` + `--port 0`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

/// The unmistakable probe ids (a catalog provider can never collide).
const PROBE_PROVIDER: &str = "raceprobe7f3a";
const PROBE_MODEL: &str = "race-probe-model-7f3a";
const GREETING: &str = "Race probe says ok";

/// Total standalone spawns across all waves.
const ITERATIONS: usize = 50;
/// Concurrent children per wave (5 waves of 10 in the common case).
const CONCURRENCY: usize = 10;
/// Overall wall bound (the test stays "slow but ON" — bounded slow).
const DEADLINE: Duration = Duration::from_secs(55);
/// Minimum completed iterations for the run to count as a hunt.
const MIN_COMPLETED: usize = 12;

/// Minimal OpenAI-compatible SSE responder (the `FakeLlm` pattern from
/// `run_e2e`; every completed request gets the same greeting stream).
struct FakeLlm {
    port: u16,
}

impl FakeLlm {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind fake llm");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            if let Err(error) = serve(listener).await {
                eprintln!("[race-probe] fake llm ended: {error}");
            }
        });
        Self { port }
    }

    fn provider_config(&self) -> String {
        format!(
            r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": {{ "providerID": "{PROBE_PROVIDER}", "model": "{PROBE_MODEL}" }},
  "providers": {{
    "{PROBE_PROVIDER}": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "http://127.0.0.1:{}/v1" }},
      "models": {{ "{PROBE_MODEL}": {{ "name": "Race Probe" }} }}
    }}
  }}
}}
"#,
            self.port
        )
    }
}

async fn serve(listener: tokio::net::TcpListener) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[rustfmt::skip]
    let response: &'static str = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: close\r\n",
        "\r\n",
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Race \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"probe says ok.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}\n\n",
        "data: [DONE]\n\n",
    );
    loop {
        let (mut socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut buffer = [0u8; 8192];
            let mut seen = 0;
            while seen < buffer.len() {
                match tokio::time::timeout(
                    Duration::from_secs(10),
                    socket.read(&mut buffer[seen..]),
                )
                .await
                {
                    Ok(Ok(0) | Err(_)) | Err(_) => break,
                    Ok(Ok(read)) => {
                        seen += read;
                        if buffer[..seen].windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
    }
}

fn opencode_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_opencode-rust"))
}

struct Sandbox {
    project: PathBuf,
}

fn sandbox() -> Sandbox {
    let root = std::env::temp_dir().join(format!("opencode/cli-race-{}", std::process::id()));
    let project = root.join("project");
    let home = root.join("home");
    for dir in [
        project.join(".opencode"),
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        root.join("tmp"),
    ] {
        std::fs::create_dir_all(&dir).expect("sandbox dir");
    }
    Sandbox { project }
}

fn run_command(binary: &Path, project: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .args(["run", "--standalone", "hello", "there"])
        .current_dir(project)
        .env("OPENCODE_TEST_HOME", project.parent().unwrap().join("home"))
        .env("XDG_DATA_HOME", project.parent().unwrap().join("home/.local/share"))
        .env("XDG_STATE_HOME", project.parent().unwrap().join("home/.local/state"))
        .env("XDG_CONFIG_HOME", project.parent().unwrap().join("home/.config"))
        .env("XDG_CACHE_HOME", project.parent().unwrap().join("home/.cache"))
        .env("TMPDIR", project.parent().unwrap().join("tmp"))
        .env_remove("OPENCODE_PASSWORD")
        .env_remove("OPENCODE_SERVER_PASSWORD")
        .env_remove("OPENCODE_CHANNEL")
        .env_remove("OPENCODE_CONFIG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// One standalone spawn: the `opencode run --standalone` process spawns
/// its private `serve --stdio` child, waits for the `{"url"}` ready line,
/// then immediately drives the session — the exact ready-line-then-request
/// shape the intermittent boot race lived in.
async fn probe(binary: &Path, project: &Path, index: usize) -> Result<(), String> {
    let output =
        tokio::time::timeout(Duration::from_secs(45), run_command(binary, project).output())
            .await
            .map_err(|_| format!("iteration {index}: timed out"))?
            .map_err(|error| format!("iteration {index}: spawn failed: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = format!("iteration {index}:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}");
    if !output.status.success() {
        return Err(format!(
            "iteration {index}: exit {:?}\n{stdout}\n{stderr}",
            output.status.code()
        ));
    }
    if !stdout.contains(&format!("{GREETING}.")) {
        return Err(format!("assistant text missing (provider did not serve)\n{report}"));
    }
    if !stderr.contains(PROBE_MODEL) {
        return Err(format!(
            "step marker does not carry the custom model id — the run fell back to a non-config model\n{report}"
        ));
    }
    Ok(())
}

#[tokio::test]
async fn standalone_boot_spawn_loop_custom_provider_stable() {
    let fake = FakeLlm::start().await;
    let sandbox = sandbox();
    std::fs::write(sandbox.project.join("opencode.json"), fake.provider_config()).expect("config");
    let binary = opencode_binary();

    let start = Instant::now();
    let mut completed = 0usize;
    let mut next_index = 0usize;
    let mut failures: Vec<String> = Vec::new();

    while next_index < ITERATIONS && start.elapsed() < DEADLINE {
        let wave = (next_index..(next_index + CONCURRENCY).min(ITERATIONS)).collect::<Vec<_>>();
        next_index = wave.last().expect("non-empty wave") + 1;
        let mut tasks = tokio::task::JoinSet::new();
        for &index in &wave {
            let binary = binary.clone();
            let project = sandbox.project.clone();
            tasks.spawn(async move { probe(&binary, &project, index).await });
        }
        while let Some(joined) = tasks.join_next().await {
            match joined.expect("probe task panics nowhere") {
                Ok(()) => completed += 1,
                Err(failure) => failures.push(failure),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "standalone boot race reproduced ({} failures after {completed} green iterations):\n{}",
        failures.len(),
        failures.join("\n=====\n")
    );
    assert!(
        completed >= MIN_COMPLETED,
        "only {completed} iterations completed inside the deadline — the hunt did not run"
    );
}
