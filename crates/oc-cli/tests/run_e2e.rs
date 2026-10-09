//! Wave-6a smoke: run the REAL `opencode` binary end-to-end.
//!
//! 1. `opencode --version` prints the pinned banner.
//! 2. `opencode run --standalone "..."` against a `FakeLlm` (the parity
//!    harness's config-seeded openai-compatible loopback): the run spawns
//!    its private `serve --stdio` lease server, prompts, streams the SSE
//!    events, and renders v2's output — step marker on stderr, assistant
//!    text on stdout.
//! 3. The same flow with `--format json` emits the JSON-lines event shape.
//!
//! Sandboxes under /tmp/opencode; never binds 49374.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use serde_json::Value;

/// Minimal OpenAI-compatible SSE responder (parity `FakeLlm` pattern).
struct FakeLlm {
    port: u16,
}

impl FakeLlm {
    async fn start(greeting: &'static str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind fake llm");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            if let Err(error) = serve(listener, greeting).await {
                eprintln!("[w6a] fake llm ended: {error}");
            }
        });
        Self { port }
    }

    fn provider_config(&self) -> String {
        format!(
            r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": {{ "providerID": "w6afake", "model": "fake-model" }},
  "providers": {{
    "w6afake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "http://127.0.0.1:{}/v1" }},
      "models": {{ "fake-model": {{ "name": "Fake Model" }} }}
    }}
  }}
}}
"#,
            self.port
        )
    }
}

async fn serve(listener: tokio::net::TcpListener, greeting: &'static str) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let response = format!(
        concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: text/event-stream\r\n",
            "Cache-Control: no-cache\r\n",
            "Connection: close\r\n",
            "\r\n",
            "data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{first} \"}}}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{second}.\"}}}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}}}\n\n",
            "data: [DONE]\n\n",
        ),
        first = greeting.split_once(' ').map_or(greeting, |(first, _)| first),
        second = greeting.split_once(' ').map_or(greeting, |(_, second)| second),
    );
    loop {
        let (mut socket, _) = listener.accept().await?;
        let response = response.clone();
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
    PathBuf::from(env!("CARGO_BIN_EXE_opencode"))
}

struct Sandbox {
    project: PathBuf,
    #[allow(dead_code)]
    root: PathBuf,
}

fn sandbox(name: &str) -> Sandbox {
    let root = std::env::temp_dir().join(format!("opencode/cli-e2e-{}-{name}", std::process::id()));
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
    Sandbox { project, root }
}

fn run_command(binary: &Path, project: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(args)
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
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[tokio::test]
async fn version_banner_is_pinned() {
    let output = run_command(&opencode_binary(), std::path::Path::new("."), &["--version"])
        .output()
        .await
        .expect("run --version");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "opencode v2.0.24");
}

#[tokio::test]
async fn run_standalone_default_format_renders_v2_output() {
    let fake = FakeLlm::start("Rust fake says hi").await;
    let sandbox = sandbox("default");
    std::fs::write(sandbox.project.join("opencode.json"), fake.provider_config()).expect("config");

    let output = tokio::time::timeout(
        Duration::from_secs(240),
        run_command(
            &opencode_binary(),
            &sandbox.project,
            &["run", "--standalone", "hello", "there"],
        )
        .output(),
    )
    .await
    .expect("run finishes within timeout")
    .expect("run spawns");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "run failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    // Assistant text lands on stdout (non-TTY shape).
    assert!(
        stdout.contains("Rust fake says hi."),
        "assistant text missing from stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Step marker on stderr: "> {agent} · {model.id}" (v2 renders the bare
    // model id, no provider prefix).
    assert!(stderr.contains("> build · fake-model"), "step marker missing from stderr:\n{stderr}");
}

#[tokio::test]
async fn run_standalone_json_format_emits_event_lines() {
    let fake = FakeLlm::start("JSON fake says hello").await;
    let sandbox = sandbox("json");
    std::fs::write(sandbox.project.join("opencode.json"), fake.provider_config()).expect("config");

    let output = tokio::time::timeout(
        Duration::from_secs(240),
        run_command(
            &opencode_binary(),
            &sandbox.project,
            &["run", "--standalone", "--format", "json", "hi"],
        )
        .output(),
    )
    .await
    .expect("run finishes within timeout")
    .expect("run spawns");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "json run failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    let mut kinds: Vec<String> = Vec::new();
    let mut text_part: Option<String> = None;
    for line in stdout.lines() {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("non-JSON line {line:?}: {error}"));
        let kind = value["type"].as_str().expect("event type").to_owned();
        if kind == "text" {
            text_part = Some(value["part"]["text"].as_str().expect("text").to_owned());
        }
        kinds.push(kind);
    }
    assert!(kinds.contains(&"step_start".to_owned()), "step_start missing: {kinds:?}");
    assert!(kinds.contains(&"text".to_owned()), "text missing: {kinds:?}");
    assert!(kinds.contains(&"step_finish".to_owned()), "step_finish missing: {kinds:?}");
    assert_eq!(text_part.as_deref(), Some("JSON fake says hello."), "{kinds:?}");
}

#[tokio::test]
async fn run_requires_a_message() {
    let sandbox = sandbox("empty");
    let mut command = run_command(&opencode_binary(), &sandbox.project, &["run", "--standalone"]);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output())
        .await
        .expect("timeout")
        .expect("spawn");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("You must provide a message"), "unexpected stderr: {stderr}");
}
