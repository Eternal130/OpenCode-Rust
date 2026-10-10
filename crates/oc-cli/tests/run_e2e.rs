//! Wave-6a smoke: run the REAL `opencode` binary end-to-end.
//!
//! 1. `opencode --version` prints the pinned banner.
//! 2. `opencode run --standalone "..."` against a `FakeLlm` (the parity
//!    harness's config-seeded openai-compatible loopback): the run spawns
//!    its private `serve --stdio` lease server, prompts, streams the SSE
//!    events, and renders v2's output — step marker on stderr, assistant
//!    text on stdout.
//! 3. The same flow with `--format json` emits the JSON-lines event shape.
//! 4. `--format json` on a failing tool emits the FULL v2 tool part
//!    (gate-7 F2); `run -c` keeps the resumed session's model (F3); the
//!    hidden SSH askpass entry forwards the compiled-binary argv shape
//!    (F6).
//!
//! Sandboxes under /tmp/opencode; never binds 49374.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::process::Command;

use serde_json::Value;

/// One OpenAI-compatible SSE script (the body served for a request).
fn text_script(greeting: &str) -> String {
    format!(
        concat!(
            "data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{first} \"}}}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{second}.\"}}}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}}}\n\n",
            "data: [DONE]\n\n",
        ),
        first = greeting.split_once(' ').map_or(greeting, |(first, _)| first),
        second = greeting.split_once(' ').map_or(greeting, |(_, second)| second),
    )
}

/// A tool-call script invoking a tool the server does not have — the run
/// settles it as a failed tool and continues with the next request.
fn unknown_tool_script() -> String {
    concat!(
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_e2e_fail\",\"type\":\"function\",\"function\":{\"name\":\"no_such_tool\",\"arguments\":\"{\\\"x\\\":1}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3,\"total_tokens\":12}}\n\n",
        "data: [DONE]\n\n",
    )
    .to_owned()
}

/// Minimal OpenAI-compatible SSE responder (parity `FakeLlm` pattern).
struct FakeLlm {
    port: u16,
}

impl FakeLlm {
    async fn start(greeting: &'static str) -> Self {
        Self::start_scripted(vec![text_script(greeting)]).await
    }

    /// Scripts are served in order, one per MAIN request (a request whose
    /// body carries a `tools` array — the engine's title generation fires
    /// tool-less requests alongside); the last script repeats. Title and
    /// other tool-less requests always get the last script.
    async fn start_scripted(scripts: Vec<String>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind fake llm");
        let port = listener.local_addr().expect("addr").port();
        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        tokio::spawn(async move {
            if let Err(error) = serve(listener, scripts, counter).await {
                eprintln!("[w6a] fake llm ended: {error}");
            }
        });
        Self { port }
    }

    fn provider_config_with_models(&self, default_model: &str) -> String {
        format!(
            r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": {{ "providerID": "w6afake", "model": "{default_model}" }},
  "providers": {{
    "w6afake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "http://127.0.0.1:{}/v1" }},
      "models": {{
        "fake-model": {{ "name": "Fake Model" }},
        "fake-model-a": {{ "name": "Fake Model A" }},
        "fake-model-b": {{ "name": "Fake Model B" }}
      }}
    }}
  }}
}}
"#,
            self.port
        )
    }

    fn provider_config(&self) -> String {
        self.provider_config_with_models("fake-model")
    }
}

async fn serve(
    listener: tokio::net::TcpListener,
    scripts: Vec<String>,
    counter: std::sync::Arc<AtomicUsize>,
) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    loop {
        let (mut socket, _) = listener.accept().await?;
        let scripts = scripts.clone();
        let counter = std::sync::Arc::clone(&counter);
        tokio::spawn(async move {
            // Generous buffer: main requests carry the full tools array.
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut seen = 0;
            // Read headers + body (main requests carry a large tools array).
            while seen < buffer.len() {
                match tokio::time::timeout(
                    Duration::from_secs(10),
                    socket.read(buffer.as_mut_slice()),
                )
                .await
                {
                    Ok(Ok(0) | Err(_)) | Err(_) => break,
                    Ok(Ok(read)) => {
                        seen += read;
                        if let Some(header_end) =
                            find_subslice(&buffer[..seen], b"\r\n\r\n").map(|index| index + 4)
                        {
                            let text = String::from_utf8_lossy(&buffer[..header_end]);
                            let length = text
                                .lines()
                                .find_map(|line| line.strip_prefix("Content-Length:"))
                                .and_then(|value| value.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            if seen >= header_end + length {
                                break;
                            }
                        }
                    }
                }
            }
            // Only MAIN requests carry a `tools` array (title generation
            // fires tool-less requests alongside the prompt).
            let wants_tools = find_subslice(&buffer, b"\"tools\"").is_some();
            let index =
                if wants_tools { counter.fetch_add(1, Ordering::SeqCst) } else { usize::MAX };
            let script =
                scripts.get(index).or_else(|| scripts.last()).expect("at least one script").clone();
            let response = format!(
                concat!(
                    "HTTP/1.1 200 OK\r\n",
                    "Content-Type: text/event-stream\r\n",
                    "Cache-Control: no-cache\r\n",
                    "Connection: close\r\n",
                    "\r\n",
                    "{script}",
                ),
                script = script,
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn opencode_binary() -> PathBuf {
    // `OPENCODE_SMOKE_BIN` points the whole battery at an arbitrary
    // build (tools/release-smoke.sh drives it with the RELEASE
    // binary); unset, the cargo-built binary is the subject.
    if let Some(path) = std::env::var_os("OPENCODE_SMOKE_BIN") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_opencode-rust"))
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
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "opencode-rust v2.0.24");
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

/// Gate-7 F2: `--format json` on a failing tool must emit the FULL v2 part
/// shape (tool identity, input, error text, providerCall envelope) — not an
/// empty payload.
#[tokio::test]
async fn run_json_format_failed_tool_carries_full_part() {
    let fake = FakeLlm::start_scripted(vec![
        unknown_tool_script(),
        text_script("Recovered after failure"),
    ])
    .await;
    let sandbox = sandbox("json-tool-fail");
    std::fs::write(sandbox.project.join("opencode.json"), fake.provider_config()).expect("config");

    let output = tokio::time::timeout(
        Duration::from_secs(240),
        run_command(
            &opencode_binary(),
            &sandbox.project,
            &["run", "--standalone", "--format", "json", "use", "the", "tool"],
        )
        .output(),
    )
    .await
    .expect("run finishes within timeout")
    .expect("run spawns");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "json tool-failure run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let mut failed_part: Option<Value> = None;
    let mut recovered = false;
    for line in stdout.lines() {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("non-JSON line {line:?}: {error}"));
        if value["type"] == "tool_use" && value["part"]["state"]["status"] == "error" {
            failed_part = Some(value["part"].clone());
        }
        if value["type"] == "text"
            && value["part"]["text"].as_str() == Some("Recovered after failure.")
        {
            recovered = true;
        }
    }
    let part = failed_part.expect("a failed tool_use event in the json stream");
    assert_eq!(part["tool"], "no_such_tool", "tool identity: {part}");
    assert_eq!(part["id"], "call_e2e_fail", "tool call id: {part}");
    assert_eq!(part["type"], "tool");
    assert_eq!(part["state"]["input"], serde_json::json!({"x": 1}), "input payload: {part}");
    let error = part["state"]["error"].as_str().unwrap_or_else(|| panic!("error text: {part}"));
    assert!(error.contains("No tool named"), "error message: {error}");
    assert!(part["state"]["metadata"]["providerCall"].is_object(), "providerCall envelope: {part}");
    assert!(
        part["state"]["metadata"]["providerResult"].is_object(),
        "providerResult envelope: {part}"
    );
    assert!(part["state"]["time"]["start"].is_u64() && part["state"]["time"]["end"].is_u64());
    assert!(part["messageID"].as_str().is_some_and(|id| id.starts_with("msg_")));
    assert!(recovered, "the run continues after the tool failure: {stdout}");
}

/// Gate-7 F3: a resumed (`-c`) session keeps its own model — the
/// `model.default` fallback only applies to NEW sessions. Run 1 creates the
/// session on `fake-model-a`; the location default is then changed to
/// `fake-model-b`; run 2 (`-c`) must still step on `fake-model-a`.
#[tokio::test]
async fn run_continue_keeps_session_model() {
    let fake = FakeLlm::start("Session model kept").await;
    let sandbox = sandbox("continue-model");
    std::fs::write(
        sandbox.project.join("opencode.json"),
        fake.provider_config_with_models("fake-model-a"),
    )
    .expect("config");

    let first = tokio::time::timeout(
        Duration::from_secs(240),
        run_command(&opencode_binary(), &sandbox.project, &["run", "--standalone", "hello"])
            .output(),
    )
    .await
    .expect("first run finishes")
    .expect("first run spawns");
    let first_err = String::from_utf8_lossy(&first.stderr);
    assert!(
        first.status.success() && first_err.contains("> build · fake-model-a"),
        "first run must create the session on fake-model-a\nstderr:\n{first_err}"
    );

    // Flip the location default; the resumed session must not follow it.
    std::fs::write(
        sandbox.project.join("opencode.json"),
        fake.provider_config_with_models("fake-model-b"),
    )
    .expect("config rewrite");

    let second = tokio::time::timeout(
        Duration::from_secs(240),
        run_command(&opencode_binary(), &sandbox.project, &["run", "--standalone", "-c", "again"])
            .output(),
    )
    .await
    .expect("second run finishes")
    .expect("second run spawns");
    let second_err = String::from_utf8_lossy(&second.stderr);
    assert!(
        second.status.success() && second_err.contains("> build · fake-model-a"),
        "resumed session must keep its own model (fake-model-a)\nstderr:\n{second_err}"
    );
    assert!(
        !second_err.contains("fake-model-b"),
        "the session's model must not be reset to the location default\nstderr:\n{second_err}"
    );
}

/// Gate-7 F6: the hidden askpass entry is exec'd by OpenSSH as
/// `[bin, prompt]` (the compiled-binary contract) — the broker must receive
/// the prompt text, the confirm flag, and the token.
#[tokio::test]
async fn ssh_askpass_forwards_compiled_binary_argv_shape() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("broker bind");
    let port = listener.local_addr().expect("addr").port();
    let broker = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("broker accept");
        let (reader, mut writer) = socket.into_split();
        let mut reader = BufReader::new(reader);
        let mut request = String::new();
        reader.read_line(&mut request).await.expect("broker request line");
        writer
            .write_all(b"{\"value\":\"passphrase with spaces\"}\n")
            .await
            .expect("broker response");
        let _ = writer.shutdown().await;
        let mut sink = String::new();
        let _ = reader.read_to_string(&mut sink).await;
        request
    });

    let mut command = run_command(&opencode_binary(), std::path::Path::new("."), &["Prompt text"]);
    command
        .env("OPENCODE_SSH_ASKPASS_PORT", port.to_string())
        .env("OPENCODE_SSH_ASKPASS_TOKEN", "fixture")
        .env("SSH_ASKPASS_PROMPT", "confirm");
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .expect("askpass finishes")
        .expect("askpass spawns");

    assert_eq!(output.status.code(), Some(0), "askpass exit");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "passphrase with spaces\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "", "no CLI noise on stderr");

    let request: Value =
        serde_json::from_str(broker.await.expect("broker join").trim()).expect("broker JSON");
    assert_eq!(
        request,
        serde_json::json!({
            "token": "fixture",
            "text": "Prompt text",
            "confirm": true,
        }),
        "the compiled-binary argv [bin, prompt] forwards the prompt text"
    );
}

// ---------------------------------------------------------------------------
// Phase 10 packaging: real upgrade/uninstall against the compiled binary.
// ---------------------------------------------------------------------------

/// A fake "release tarball" carrying a package/opencode-rust payload
/// (the lane B naming; the legacy `opencode` member stays supported as
/// a fallback, covered by unit tests).
fn release_tarball(sandbox: &Sandbox, payload: &str) -> PathBuf {
    let staging = sandbox.root.join("release-src").join("package");
    std::fs::create_dir_all(&staging).expect("tarball staging dir");
    std::fs::write(staging.join("opencode-rust"), payload).expect("payload");
    let tarball = sandbox.root.join("release.tar.gz");
    let status = std::process::Command::new("tar")
        .args(["-czf"])
        .arg(&tarball)
        .arg("-C")
        .arg(sandbox.root.join("release-src"))
        .arg("package")
        .status()
        .expect("tar exists (the upgrade path requires it)");
    assert!(status.success(), "tarball packs");
    tarball
}

#[tokio::test]
async fn upgrade_without_target_skips_source_builds() {
    let sandbox = sandbox("upgrade-noop");
    // No target on a dev build (the test binary carries no release
    // tag): the release channel is skipped cleanly, exit 0, no network.
    let output = run_command(&opencode_binary(), &sandbox.project, &["upgrade"])
        .output()
        .await
        .expect("run upgrade");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("source build"), "{stdout}");
    assert!(stdout.contains("2.0.24"), "{stdout}");
    // local-file without a path stays a clear error.
    let output =
        run_command(&opencode_binary(), &sandbox.project, &["upgrade", "-m", "local-file"])
            .output()
            .await
            .expect("run upgrade");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("need a path"), "{stderr}");
}

#[tokio::test]
async fn upgrade_local_file_replaces_the_binary() {
    use std::os::unix::fs::PermissionsExt as _;
    let sandbox = sandbox("upgrade-local");
    // The "installed" binary is a COPY of the real one; self-replacement
    // targets the copy's current_exe, never the cargo artifact.
    let installed = sandbox.root.join("opencode-installed");
    std::fs::copy(opencode_binary(), &installed).expect("copy binary");
    let payload = "#!/bin/sh\necho replaced-opencode\n";
    let tarball = release_tarball(&sandbox, payload);

    let output = tokio::time::timeout(
        Duration::from_secs(30),
        run_command(
            &installed,
            &sandbox.project,
            &["upgrade", tarball.to_str().expect("path"), "--method", "local-file"],
        )
        .output(),
    )
    .await
    .expect("upgrade finishes")
    .expect("upgrade spawns");
    assert!(output.status.success(), "exit: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Upgrade complete"), "{stdout}");
    assert_eq!(std::fs::read_to_string(&installed).expect("read replaced"), payload);
    let mode = std::fs::metadata(&installed).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "replacement is executable");
}

#[tokio::test]
async fn uninstall_dry_run_lists_and_force_removes() {
    let sandbox = sandbox("uninstall");
    let home = sandbox.root.join("home");
    // Sentinel state: data/db, cache/bin/x, config/opencode.json, and a
    // (stale) service registration in state/.
    std::fs::create_dir_all(home.join(".local/share/opencode-rust")).expect("data dir");
    std::fs::write(home.join(".local/share/opencode-rust/db"), "db").expect("data sentinel");
    std::fs::create_dir_all(home.join(".cache/opencode-rust/bin")).expect("cache dir");
    std::fs::write(home.join(".cache/opencode-rust/bin/x"), "x").expect("cache sentinel");
    std::fs::create_dir_all(home.join(".config/opencode-rust")).expect("config dir");
    std::fs::write(home.join(".config/opencode-rust/opencode.json"), "{}")
        .expect("config sentinel");
    std::fs::create_dir_all(home.join(".local/state/opencode-rust")).expect("state dir");
    std::fs::write(
        home.join(".local/state/opencode-rust/service.json"),
        r#"{"id":"x","version":"2.0.24","url":"http://127.0.0.1:1","pid":99999999,"password":"x"}"#,
    )
    .expect("registration sentinel");
    let installed = sandbox.root.join("opencode-installed");
    std::fs::copy(opencode_binary(), &installed).expect("copy binary");

    // Dry run: lists the plan, changes nothing.
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        run_command(&installed, &sandbox.project, &["uninstall", "--dry-run"]).output(),
    )
    .await
    .expect("dry run finishes")
    .expect("dry run spawns");
    assert!(output.status.success(), "exit: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry run - no changes made"), "{stdout}");
    assert!(stdout.contains("Stop background service"), "{stdout}");
    assert!(home.join(".local/share/opencode-rust/db").exists(), "dry run removes nothing");

    // Force (non-TTY): removes data/cache/state + the binary, keeps
    // config via --keep-config. The stale registration is deleted by the
    // safekill path (foreign pid), never signalled.
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        run_command(&installed, &sandbox.project, &["uninstall", "--force", "--keep-config"])
            .output(),
    )
    .await
    .expect("uninstall finishes")
    .expect("uninstall spawns");
    assert!(output.status.success(), "exit: {}", String::from_utf8_lossy(&output.stderr));
    assert!(!home.join(".local/share/opencode-rust").exists(), "data removed");
    assert!(!home.join(".cache/opencode-rust").exists(), "cache removed");
    assert!(!home.join(".local/state/opencode-rust").exists(), "state removed");
    assert!(home.join(".config/opencode-rust/opencode.json").exists(), "config kept");
    assert!(!installed.exists(), "binary self-unlinked");
}

#[tokio::test]
async fn uninstall_without_force_in_non_tty_errors() {
    let sandbox = sandbox("uninstall-tty");
    let output = run_command(&opencode_binary(), &sandbox.project, &["uninstall"])
        .output()
        .await
        .expect("run uninstall");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--force"), "{stderr}");
    assert!(stderr.contains("--dry-run"), "{stderr}");
}
