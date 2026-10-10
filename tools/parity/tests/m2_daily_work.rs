//! M2 acceptance exercise: a scripted daily-work session driving the REAL
//! Rust TUI binary (`opencode`, the oc-cli root) against the REAL Rust
//! server with a deterministic scripted fake LLM, inside tmux at 160x40.
//! Every step asserts through `capture-pane` between actions and saves the
//! captures as evidence.
//!
//! The nine daily-work steps:
//!
//! 1.  Boot: Rust server sandboxed with the `FakeLlm` provider config
//!     (two models), our TUI in tmux 160x40 against it.
//! 2.  Prompt → the fake emits a `read` tool call (in-project file) then a
//!     text reply; the tool row and the reply must render in the
//!     transcript.
//! 3.  Permission: a second prompt makes the fake request an
//!     EXTERNAL-directory read → the engine asks (`permission.asked`) →
//!     the permission footer renders + the bell byte is written; Enter
//!     replies "once" through the TUI keys — the TUI POSTs the reply
//!     itself (`session.permission.reply`) and the parked tool proceeds
//!     (no driver fallback; `OC_M2_DRIVER_REPLY=1` restores the old
//!     API-side escape hatch for debugging).
//! 4.  Tabs: `<leader>n` → new session, second prompt → session 2; tab
//!     cycling (`ctrl+tab`), `<leader>2`/`<leader>1` quick slots; an
//!     API-injected background prompt exercises the unread attention
//!     path (bell) while tab 1 is active — the live tabs strip carries
//!     the unread dot on the inactive tab.
//! 5.  Model switching: `<leader>m` lists the LIVE server models, select
//!     `Fake Model A`, assert the server-side switch (the session row
//!     re-read shows the new model) + the dialog's current marker moved.
//! 6.  Copy: `<leader>y` → the OSC-52 sequence in the raw pane stream +
//!     the "Message copied to clipboard!" toast (the live toast layer).
//! 7.  Theme: `<leader>t` → select `dracula` → the ANSI captures differ.
//!
//! 7b. External editor (G11-F2): `<leader>e` spawns the stub `$EDITOR`
//!     (one-line script: append + exit 0) → the draft comes back
//!     replaced and the TUI still takes input after the resume (the
//!     run loop's raw-mode/alternate-screen surgery, exercised for
//!     real — opt-in like every M2 step, needs the tmux sandbox).
//! 8.  Kill the TUI, reboot against the same server → tabs restored from
//!     `<XDG_STATE_HOME>/opencode/local/tui/tabs.json`, transcripts
//!     hydrated on the FIRST connect (the boot resync fetches
//!     `message.list` per restored tab), both sessions switchable.
//! 9.  Clean exit (`<leader>q`) + SIGTERM-safe server shutdown < 5s.
//!
//! Opt-in: `#[ignore]`d AND requires `OC_M2=1` at runtime:
//!
//! ```text
//! OC_M2=1 cargo nextest run --run-ignored only -E 'test(m2_daily_work)'
//! ```
//!
//! Artifacts: pane captures under `/tmp/opencode/m2-artifacts/` + evidence
//! copies in `tools/parity/goldens/m2/`. Never binds 49374.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use parity::rust_server::{self, RustSpawn};
use parity::scenario::ScenarioHost;

// ---------------------------------------------------------------------------
// The scripted fake LLM (the oc-cli `run` e2e pattern: scripts served in
// order, one per MAIN request — a request whose body carries a `tools`
// array; title generation and other tool-less requests always get the last
// script).
// ---------------------------------------------------------------------------

struct ScriptedFakeLlm {
    port: u16,
}

impl ScriptedFakeLlm {
    async fn start(scripts: Vec<String>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.context("bind scripted fake llm")?;
        let port = listener.local_addr()?.port();
        let counter = Arc::new(AtomicUsize::new(0));
        tokio::spawn(async move {
            if let Err(error) = serve_fake(listener, scripts, counter).await {
                eprintln!("[m2] scripted fake llm ended: {error}");
            }
        });
        Ok(Self { port })
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

async fn serve_fake(
    listener: TcpListener,
    scripts: Vec<String>,
    counter: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let scripts = scripts.clone();
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            // Generous buffer: main requests carry the full tools array.
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut seen = 0;
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
                                .find_map(|line| {
                                    line.strip_prefix("Content-Length:")
                                        .or_else(|| line.strip_prefix("content-length:"))
                                })
                                .and_then(|value| value.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            if seen >= header_end + length {
                                break;
                            }
                        }
                    }
                }
            }
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

/// A plain text turn (role + one content delta + stop + usage).
fn text_script(text: &str) -> String {
    let role = serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]}).to_string();
    let content = serde_json::json!({"choices":[{"delta":{"content":text}}]}).to_string();
    let finish = serde_json::json!({
        "choices":[{"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}
    })
    .to_string();
    format!("data: {role}\n\ndata: {content}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

/// A `read` tool-call turn (the engine executes it and comes back with the
/// tool result, consuming the next script).
fn read_tool_script(call_id: &str, path: &str) -> String {
    let arguments = serde_json::json!({"path": path}).to_string();
    let call = serde_json::json!({
        "choices":[{"delta":{"role":"assistant","tool_calls":[{
            "index":0,"id":call_id,"type":"function",
            "function":{"name":"read","arguments":arguments}
        }]}}]
    })
    .to_string();
    let finish = serde_json::json!({
        "choices":[{"delta":{},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}
    })
    .to_string();
    format!("data: {call}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

// ---------------------------------------------------------------------------
// Minimal authed JSON client for the acceptance-driver API calls (the
// parity harness's own `minih` is private; this is the same subset).
// ---------------------------------------------------------------------------

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let triple = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(TABLE[(triple >> 18) as usize & 0x3f] as char);
        out.push(TABLE[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 { TABLE[(triple >> 6) as usize & 0x3f] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[triple as usize & 0x3f] as char } else { '=' });
    }
    out
}

/// Decode the OSC-52 payload back (standard alphabet, padding optional).
fn b64_decode(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bytes = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for ch in text.chars() {
        let Some(value) = TABLE.iter().position(|item| *item as char == ch) else {
            continue;
        };
        let value = u32::try_from(value).unwrap_or(0);
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// One buffered request against the server (Basic auth, JSON bodies,
/// Connection: close). Returns `(status, body)`.
async fn api(
    url: &str,
    password: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, String)> {
    let port: u16 = url
        .rsplit(':')
        .next()
        .and_then(|raw| raw.parse().ok())
        .ok_or_else(|| anyhow!("parse port from {url}"))?;
    let auth =
        format!("Authorization: Basic {}", base64(format!("opencode:{password}").as_bytes()));
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\n\
         Accept-Encoding: identity\r\nConnection: close\r\n{auth}\r\n"
    );
    if let Some(body) = body {
        use std::fmt::Write as _;
        let _ =
            write!(head, "Content-Type: application/json\r\nContent-Length: {}\r\n", body.len());
    }
    head.push_str("\r\n");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    stream.write_all(head.as_bytes()).await?;
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await?;
    }
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status =
        text.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
    Ok((status, body))
}

/// The id of the session whose row appears in `GET /api/session` but not in
/// `known` (driver-side session identification).
async fn new_session_id(url: &str, password: &str, known: &[String]) -> Result<String> {
    let (status, body) = api(url, password, "GET", "/api/session", None).await?;
    anyhow::ensure!(status == 200, "session list status {status}: {body}");
    let rows = envelope_data(&body)
        .as_array()
        .ok_or_else(|| anyhow!("session list data is not an array: {body}"))?
        .clone();
    for row in &rows {
        if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
            if !known.iter().any(|known| known == id) {
                return Ok(id.to_owned());
            }
        }
    }
    Err(anyhow!("no new session in list of {} rows", rows.len()))
}

/// The envelope-aware variant used where handlers wrap in `{"data": ...}`.
fn envelope_data(body: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("data").cloned())
        .unwrap_or_else(|| serde_json::json!([]))
}

// ---------------------------------------------------------------------------
// tmux plumbing
// ---------------------------------------------------------------------------

fn tmux(args: &[&str]) -> String {
    let output = Command::new("tmux").args(args).output().expect("run tmux");
    if !output.status.success() {
        eprintln!(
            "[m2] tmux {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn key(session: &str, key_name: &str) {
    tmux(&["send-keys", "-t", session, key_name]);
}

fn type_text(session: &str, text: &str) {
    tmux(&["send-keys", "-t", session, "-l", text]);
}

fn capture(session: &str) -> String {
    tmux(&["capture-pane", "-p", "-t", session])
}

fn capture_ansi(session: &str) -> String {
    tmux(&["capture-pane", "-p", "-e", "-t", session])
}

fn leader(session: &str, follow: &str) {
    key(session, "C-x");
    std::thread::sleep(Duration::from_millis(80));
    key(session, follow);
}

fn bell_flag(session: &str) -> u32 {
    tmux(&["list-windows", "-t", session, "-F", "#{window_bell_flag}"]).trim().parse().unwrap_or(0)
}

fn count_bytes(path: &Path, byte: u8) -> usize {
    match std::fs::read(path) {
        Ok(raw) => raw.iter().fold(0usize, |count, item| count + usize::from(*item == byte)),
        Err(_) => 0,
    }
}

/// Open a raw SSE subscription for `secs` and return everything the server
/// sends (unused diagnostics helper kept for local debugging of restore
/// failures; call from a scratch run).
#[allow(dead_code)]
async fn sse_probe(url: &str, password: &str, secs: u64) -> String {
    let port: u16 = url.rsplit(':').next().and_then(|raw| raw.parse().ok()).unwrap_or(0);
    let auth =
        format!("Authorization: Basic {}", base64(format!("opencode:{password}").as_bytes()));
    let request = format!(
        "GET /api/event HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\n\
         Connection: close\r\n{auth}\r\n\r\n"
    );
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await else {
        return "<connect failed>".to_owned();
    };
    if stream.write_all(request.as_bytes()).await.is_err() {
        return "<write failed>".to_owned();
    }
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(secs), stream.read_to_end(&mut raw)).await;
    String::from_utf8_lossy(&raw).into_owned()
}

/// Async sleep (the fake LLM runs on THIS test's runtime — a blocking
/// sleep would starve it).
async fn sleep(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

/// Poll captures until one contains `needle` (saving the last capture as
/// `artifact` when it matches, or the tail capture on timeout for the
/// failure message).
async fn wait_capture(session: &str, needle: &str, tries: usize, secs: u64) -> Result<String> {
    let mut last = String::new();
    for _ in 0..tries {
        sleep(secs).await;
        last = capture(session);
        if last.contains(needle) {
            return Ok(last);
        }
    }
    Err(anyhow!("timed out waiting for {needle:?} in the pane:\n{last}"))
}

/// The step ledger: what passed, what was soft-checked (finding), what was
/// skipped — printed at the end and mirrored in `m2-lane.md`.
struct Ledger {
    rows: Vec<(&'static str, bool, String)>,
}

impl Ledger {
    fn new() -> Self {
        Self { rows: Vec::new() }
    }
    fn pass(&mut self, step: &'static str, note: impl Into<String>) {
        let note = note.into();
        println!("[m2] PASS {step}: {note}");
        self.rows.push((step, true, note));
    }
    fn finding(&mut self, step: &'static str, note: impl Into<String>) {
        let note = note.into();
        println!("[m2] FINDING {step}: {note}");
        self.rows.push((step, false, note));
    }
    #[allow(dead_code)] // kept for future opt-in steps (the 7b editor step replaced its last call)
    fn skip(&mut self, step: &'static str, note: impl Into<String>) {
        let note = note.into();
        println!("[m2] SKIP {step}: {note}");
        self.rows.push((step, false, format!("SKIP — {note}")));
    }
}

// ---------------------------------------------------------------------------
// The exercise
// ---------------------------------------------------------------------------

const SESSION: &str = "ocm2";
const W: &str = "160";
const H: &str = "40";

/// Launch the TUI in tmux against `url` with the sandbox env; returns the
/// stderr log path. `raw_log` receives the pane's raw byte stream
/// (pipe-pane) — the bell bytes + OSC-52 sequences land there.
/// `editor` mounts a stub `$EDITOR`/`$VISUAL` (the external-editor step)
/// instead of stripping the variables.
#[allow(clippy::too_many_arguments)]
fn launch_tui(
    cli: &Path,
    url: &str,
    password: &str,
    home: &Path,
    project: &Path,
    stderr_log: &Path,
    raw_log: &Path,
    editor: Option<&Path>,
) -> Result<()> {
    let _ = Command::new("tmux").args(["kill-session", "-t", SESSION]).output();
    let editor_env = match editor {
        Some(path) => format!("EDITOR={p} VISUAL={p} ", p = path.display()),
        None => "env -u EDITOR -u VISUAL ".to_owned(),
    };
    let launch = format!(
        "cd {project} && {editor_env}\
         TERM=xterm-256color HOME={home} \
         XDG_DATA_HOME={data} XDG_CACHE_HOME={cache} XDG_CONFIG_HOME={config} \
         XDG_STATE_HOME={state} TMPDIR={tmp} \
         OPENCODE_PASSWORD={password} OC_TUI_DEBUG=1 \
         {cli} --server {url} 2>{stderr}",
        project = project.display(),
        editor_env = editor_env,
        home = home.display(),
        data = home.join(".local/share").display(),
        cache = home.join(".cache").display(),
        config = home.join(".config").display(),
        state = home.join(".local/state").display(),
        tmp = home.join("tmp").display(),
        password = password,
        cli = cli.display(),
        url = url,
        stderr = stderr_log.display(),
    );
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", SESSION, "-x", W, "-y", H, &launch])
        .output()
        .context("tmux new-session")?;
    anyhow::ensure!(status.status.success(), "tmux new-session failed: {status:?}");
    // Raw pane stream (bell bytes / OSC 52) + bell monitoring.
    tmux(&["pipe-pane", "-t", SESSION, "-o", &format!("cat >> {}", raw_log.display())]);
    let _ = Command::new("tmux")
        .args(["set-option", "-w", "-t", SESSION, "monitor-bell", "on"])
        .output();
    Ok(())
}

/// `tmux has-session` until the session is gone (the TUI exited and the
/// pane's command finished).
async fn wait_tui_gone(tries: usize) -> Result<()> {
    for _ in 0..tries {
        let alive = Command::new("tmux").args(["has-session", "-t", SESSION]).output();
        if alive.map_or(true, |out| !out.status.success()) {
            return Ok(());
        }
        sleep(1).await;
    }
    Err(anyhow!("the TUI tmux session is still alive after {tries} tries"))
}

#[tokio::test]
#[ignore = "M2 daily-work acceptance: needs tmux; opt in with OC_M2=1"]
#[allow(clippy::too_many_lines)]
async fn m2_daily_work_rust_tui_against_rust_server() -> Result<()> {
    if std::env::var("OC_M2").ok().as_deref() != Some("1") {
        eprintln!("skipping: OC_M2 != 1 (opt-in; see the module docs)");
        return Ok(());
    }
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("skipping: tmux not installed");
        return Ok(());
    }
    let mut ledger = Ledger::new();
    let cli = rust_server::cli_binary()?;

    // -- sandbox layout (all under /tmp/opencode) -------------------------
    let artifacts = PathBuf::from("/tmp/opencode/m2-artifacts");
    let _ = std::fs::remove_dir_all(&artifacts);
    std::fs::create_dir_all(&artifacts)?;
    let root = PathBuf::from(format!("/tmp/opencode/m2-sandbox-{}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project");
    let outside = root.join("outside");
    for dir in [
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        home.join("tmp"),
        project.clone(),
        outside.clone(),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    std::fs::write(project.join("notes.md"), "m2 daily-work notes file\n")?;
    let secret_path = outside.join("secret.txt");
    std::fs::write(&secret_path, "m2-secret\n")?;

    // -- the scripted fake + the server ------------------------------------
    let fake = ScriptedFakeLlm::start(vec![
        read_tool_script("call_m2_read_in", &project.join("notes.md").display().to_string()),
        text_script("The notes file is present."),
        read_tool_script("call_m2_read_out", &secret_path.display().to_string()),
        text_script("The outside file says m2-secret."),
        text_script("Second session reply."),
        text_script("Unread ping."),
    ])
    .await?;
    let config = format!(
        r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "parityfake/fake-model",
  "providers": {{
    "parityfake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "{}/v1" }},
      "models": {{
        "fake-model": {{ "name": "Fake Model" }},
        "fake-model-a": {{ "name": "Fake Model A" }}
      }}
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
    eprintln!("[m2] rust server ready at {}", server.url());
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    // -- step 1: boot the TUI ----------------------------------------------
    // The $EDITOR stub for step 7b: a one-line script that appends text
    // and exits 0 (oracle gate 11's named residual — the run loop's
    // suspend/resume block around $EDITOR is invisible to unit tests).
    let editor_script = artifacts.join("stub-editor.sh");
    std::fs::write(&editor_script, "#!/bin/sh\nprintf 'EDITED-BY-STUB' >> \"$1\"\n")?;
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&editor_script)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&editor_script, permissions)?;
    }
    let stderr_log = artifacts.join("tui-stderr.txt");
    let raw_log = artifacts.join("pane-raw.log");
    launch_tui(
        &cli,
        &url,
        &password,
        &home,
        &project,
        &stderr_log,
        &raw_log,
        Some(&editor_script),
    )?;
    let mut boot = String::new();
    for _ in 0..30 {
        sleep(2).await;
        boot = capture(SESSION);
        if boot.contains("Ask anything") {
            break;
        }
    }
    std::fs::write(artifacts.join("step-01-boot.txt"), &boot)?;
    anyhow::ensure!(
        boot.contains("Ask anything"),
        "step 1: the TUI home screen did not render:\n{boot}"
    );
    anyhow::ensure!(
        !boot.to_lowercase().contains("error:"),
        "step 1: the TUI reported an error:\n{boot}"
    );
    ledger.pass("1-boot", "home rendered (composer placeholder visible)");

    // Dismiss any first-run state.
    key(SESSION, "Escape");
    sleep(1).await;

    // -- step 2: prompt → read tool + reply --------------------------------
    type_text(SESSION, "What files are here?");
    sleep(1).await;
    key(SESSION, "Enter");
    let reply1 = wait_capture(SESSION, "The notes file is present.", 20, 3).await?;
    std::fs::write(artifacts.join("step-02-tool-reply.txt"), &reply1)?;
    anyhow::ensure!(
        reply1.contains("What files are here?"),
        "step 2: the prompt is missing from the transcript:\n{reply1}"
    );
    anyhow::ensure!(
        reply1.contains("plored: 1 read"),
        "step 2: the read tool row is missing from the transcript:\n{reply1}"
    );
    ledger.pass("2-tool-reply", "read tool row + fake reply rendered in transcript");

    // The session id (driver-side identity for the permission step).
    let session1 = new_session_id(&url, &password, &[]).await?;
    eprintln!("[m2] session 1 = {session1}");

    // -- step 3: permission ask (external directory read) ------------------
    type_text(SESSION, "Now read the outside file please.");
    sleep(1).await;
    key(SESSION, "Enter");
    let mut ask_row = serde_json::Value::Null;
    for _ in 0..30 {
        sleep(1).await;
        let (status, body) =
            api(&url, &password, "GET", &format!("/api/session/{session1}/permission"), None)
                .await?;
        if status == 200 {
            let rows = envelope_data(&body);
            if let Some(first) = rows.as_array().and_then(|rows| rows.first()) {
                ask_row = first.clone();
                break;
            }
        }
    }
    anyhow::ensure!(!ask_row.is_null(), "step 3: no permission ask appeared on the server");
    let request_id = ask_row
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("step 3: ask row without id: {ask_row}"))?
        .to_owned();
    let action =
        ask_row.get("action").and_then(serde_json::Value::as_str).unwrap_or("?").to_owned();
    eprintln!("[m2] permission ask {request_id} (action {action})");
    anyhow::ensure!(
        action == "external_directory",
        "step 3: expected an external_directory ask, got {action}: {ask_row}"
    );

    let ask = wait_capture(SESSION, "Permission required", 10, 1).await?;
    std::fs::write(artifacts.join("step-03-permission-ask.txt"), &ask)?;
    anyhow::ensure!(
        ask.contains("Allow once") && ask.contains("external directory"),
        "step 3: the permission footer did not render the ask:\n{ask}"
    );
    // The bell: the TUI writes \x07 on permission.asked — assert the RAW
    // pane stream (primary) + tmux's window bell flag (secondary).
    sleep(1).await;
    let bells = count_bytes(&raw_log, 0x07);
    let flag = bell_flag(SESSION);
    if bells >= 1 {
        ledger.pass(
            "3-bell",
            format!("bell byte in raw pane stream (count {bells}; tmux flag {flag})"),
        );
    } else {
        ledger.finding(
            "3-bell",
            format!("no \\x07 in raw pane stream (count {bells}; tmux flag {flag})"),
        );
    }

    // Reply "once" through the TUI keys: Enter on the default "Allow once"
    // selection resolves the ask locally AND the run loop POSTs the reply
    // (`session.permission.reply`) — the parked tool proceeds on its own
    // (no driver fallback). `OC_M2_DRIVER_REPLY=1` restores the old
    // API-side escape hatch for debugging a broken transport.
    let driver_reply = std::env::var("OC_M2_DRIVER_REPLY").ok().as_deref() == Some("1");
    key(SESSION, "Enter");
    let mut resolved = String::new();
    for _ in 0..5 {
        sleep(1).await;
        resolved = capture(SESSION);
        if !resolved.contains("Permission required") {
            break;
        }
    }
    std::fs::write(artifacts.join("step-03-permission-resolved.txt"), &resolved)?;
    anyhow::ensure!(
        !resolved.contains("Permission required"),
        "step 3: Enter did not resolve the permission footer:\n{resolved}"
    );
    if driver_reply {
        let (reply_status, reply_body) = api(
            &url,
            &password,
            "POST",
            &format!("/api/session/{session1}/permission/{request_id}/reply"),
            Some(r#"{"decision":"once"}"#),
        )
        .await?;
        anyhow::ensure!(
            reply_status == 204,
            "step 3: driver API reply failed ({reply_status}): {reply_body}"
        );
    }

    let reply2 = wait_capture(SESSION, "The outside file says m2-secret.", 20, 3).await?;
    std::fs::write(artifacts.join("step-03-tool-proceeded.txt"), &reply2)?;
    anyhow::ensure!(
        reply2.contains("The outside file says m2-secret."),
        "step 3: the parked tool never proceeded after the TUI's once reply (no driver \
         fallback; the TUI must POST the permission reply itself)"
    );
    ledger.pass(
        "3-permission",
        "ask rendered, once replied THROUGH THE TUI (permission.reply POST), external read \
         executed",
    );

    // -- step 4: tabs -------------------------------------------------------
    leader(SESSION, "n"); // session.new → home for the next session
    sleep(1).await;
    type_text(SESSION, "Say something for session two.");
    sleep(1).await;
    key(SESSION, "Enter");
    let reply3 = wait_capture(SESSION, "Second session reply.", 20, 3).await?;
    std::fs::write(artifacts.join("step-04-tab2-reply.txt"), &reply3)?;
    anyhow::ensure!(reply3.contains("Second session reply."), "step 4: session 2's reply missing");
    let session2 = new_session_id(&url, &password, std::slice::from_ref(&session1)).await?;
    eprintln!("[m2] session 2 = {session2}");

    // The tabs strip is not mounted on the live route (module docs): the
    // tab MODEL is what renders — each route shows its session's title
    // row + transcript. Cycle with the tab-next command (`ctrl+tab`'s
    // second default binding `alt+down` — tmux cannot deliver C-Tab, it
    // arrives as a plain tab the composer would eat), then the quick
    // slots.
    key(SESSION, "M-Down"); // session.tab.next: tab 2 → tab 1
    let cycled = wait_capture(SESSION, "The outside file says m2-secret.", 8, 1).await?;
    std::fs::write(artifacts.join("step-04-cycled-tab1.txt"), &cycled)?;
    leader(SESSION, "2"); // explicit tab 2
    let tab2 = wait_capture(SESSION, "Second session reply.", 8, 1).await?;
    std::fs::write(artifacts.join("step-04-tab2-again.txt"), &tab2)?;
    leader(SESSION, "1"); // back to tab 1
    let tab1 = wait_capture(SESSION, "The outside file says m2-secret.", 8, 1).await?;
    std::fs::write(artifacts.join("step-04-tab1.txt"), &tab1)?;
    ledger.pass("4-tabs", "two sessions in tabs; ctrl+tab + leader+1/2 switch transcripts");

    // The live tabs strip: both tabs' surfaces in ONE capture — the
    // strip row (the ` + ` affordance; the inactive tab's pill) mounts
    // on the session route. tmux strips trailing blank cells, so the
    // affordance asserts as the trimmed row ending in `+`. Distinct
    // titles land with the renames in step 8; here the affordance +
    // unread dot are the strip proof.
    let strip_check = capture(SESSION);
    std::fs::write(artifacts.join("step-04-tab-strip.txt"), &strip_check)?;
    let strip_row = strip_check.lines().next().unwrap_or("").trim_end();
    anyhow::ensure!(
        strip_row.ends_with('+'),
        "step 4: the live tabs strip (add affordance) did not render:\n{strip_check}"
    );
    ledger.pass("4-tab-strip", "the live tabs strip renders (tab pills + add affordance)");

    // Unread attention: with tab 1 active, inject a background prompt into
    // session 2 through the API — the reply must ring the bell (unread
    // transition) even though nothing is visible on the inactive tab.
    let bells_before = count_bytes(&raw_log, 0x07);
    let (inject_status, inject_body) = api(
        &url,
        &password,
        "POST",
        &format!("/api/session/{session2}/prompt"),
        Some(r#"{"text":"More news."}"#),
    )
    .await?;
    anyhow::ensure!(
        inject_status == 200,
        "step 4: background prompt inject failed ({inject_status}): {inject_body}"
    );
    let mut stderr_has_reply = false;
    for _ in 0..15 {
        sleep(1).await;
        let stderr = std::fs::read_to_string(&stderr_log).unwrap_or_default();
        if stderr.contains("Unread ping.") {
            stderr_has_reply = true;
            break;
        }
    }
    let bells_after = count_bytes(&raw_log, 0x07);
    anyhow::ensure!(
        stderr_has_reply,
        "step 4: the background reply never reached the TUI (OC_TUI_DEBUG stderr)"
    );
    anyhow::ensure!(
        bells_after > bells_before,
        "step 4: the unread bell did not ring (bells {bells_before} → {bells_after})"
    );
    ledger.pass(
        "4-unread",
        format!(
            "background reply ingested + unread bell rang (bells {bells_before} → \
             {bells_after})"
        ),
    );
    // The unread dot on the INACTIVE tab's pill (the live strip).
    let mut dot = String::new();
    for _ in 0..5 {
        sleep(1).await;
        dot = capture(SESSION);
        if dot.contains('\u{2022}') {
            break;
        }
    }
    std::fs::write(artifacts.join("step-04-unread-dot.txt"), &dot)?;
    anyhow::ensure!(
        dot.contains('\u{2022}'),
        "step 4: the unread dot did not appear on the inactive tab's pill:\n{dot}"
    );
    ledger.pass("4-unread-dot", "the strip carries the unread dot on the inactive tab");

    // -- step 5: model switching -------------------------------------------
    leader(SESSION, "m");
    let dialog = wait_capture(SESSION, "Switch model", 10, 1).await?;
    std::fs::write(artifacts.join("step-05-model-dialog.txt"), &dialog)?;
    // The live list carries the whole catalog; filter to the fake
    // provider's second model (the row surfacing IS the live-list proof —
    // the open dialog starts scrolled to the top provider group; the
    // config model's display name is dropped server-side, rows carry the
    // model id).
    type_text(SESSION, "fake-model-a");
    let filtered = wait_capture(SESSION, "fake-model-a", 10, 1).await?;
    std::fs::write(artifacts.join("step-05-model-dialog-filtered.txt"), &filtered)?;
    anyhow::ensure!(
        filtered.contains("fake-model-a"),
        "step 5: the live model list has no FakeLlm `fake-model-a` row:\n{filtered}"
    );
    ledger.pass("5-model-list", "leader+m lists the FakeLlm provider's models (live server)");

    key(SESSION, "Enter");
    // The selection POSTs session.switchModel through the selection sink
    // and the server's durable commit persists it (the D3 fix: the
    // projector's previous-model lookup no longer breaks on NULL
    // columns) — the session row must show the new model.
    let mut switched = false;
    let mut row_body = String::new();
    for _ in 0..8 {
        sleep(1).await;
        let (status, body) =
            api(&url, &password, "GET", &format!("/api/session/{session1}"), None).await?;
        if status == 200 && body.contains("fake-model-a") {
            switched = true;
            row_body = body;
            break;
        }
    }
    anyhow::ensure!(
        switched,
        "step 5: the model switch never reached the server's session row:\n{row_body}"
    );
    std::fs::write(artifacts.join("step-05-model-server-row.txt"), &row_body)?;
    ledger.pass("5-model-server", "switch reached the server's session row (fake-model-a)");
    // The visible hint: the dialog's current marker moved to the selected
    // model (focus-current reveals its row on reopen).
    leader(SESSION, "m");
    let dialog2 = wait_capture(SESSION, "fake-model-a", 10, 1).await?;
    std::fs::write(artifacts.join("step-05-model-dialog-current.txt"), &dialog2)?;
    anyhow::ensure!(
        dialog2.contains("\u{25cf} fake-model-a"),
        "step 5: the current marker did not move to fake-model-a:\n{dialog2}"
    );
    key(SESSION, "Escape");
    sleep(1).await;
    ledger.pass(
        "5-model-switch",
        "picker selection applied to the store (dialog current marker moved)",
    );
    // The persistent model hint: the composer's agent · model statusline
    // row (the footer-prompt-idle surface) carries the selection.
    let hint = capture(SESSION);
    std::fs::write(artifacts.join("step-05-model-hint.txt"), &hint)?;
    anyhow::ensure!(
        hint.contains("fake-model-a"),
        "step 5: no persistent model hint on the route (composer statusline):\n{hint}"
    );
    ledger.pass("5-model-hint", "the composer's agent · model hint row shows fake-model-a");

    // -- step 6: copy --------------------------------------------------------
    leader(SESSION, "y");
    sleep(2).await; // the sink writes OSC-52 to the pane stream
    let raw = std::fs::read_to_string(&raw_log).unwrap_or_default();
    // The live toast layer composites the ToastQueue's current toast
    // over the frame (V1) — the copy must be visible, not just written.
    let mut toast_visible = false;
    let mut toast = String::new();
    for _ in 0..6 {
        toast = capture(SESSION);
        if toast.contains("Message copied to clipboard!") {
            toast_visible = true;
            break;
        }
        sleep(1).await;
    }
    std::fs::write(artifacts.join("step-06-copy-toast.txt"), &toast)?;
    anyhow::ensure!(
        toast_visible,
        "step 6: the copy toast did not render in the live frame:\n{toast}"
    );
    ledger.pass("6-copy-toast", "the copy toast rendered (live toast layer)");
    anyhow::ensure!(
        raw.contains("\u{1b}]52;c;"),
        "step 6: no OSC-52 sequence in the raw pane stream after leader+y"
    );
    let payload = raw
        .split("\u{1b}]52;c;")
        .nth(1)
        .and_then(|rest| rest.split('\u{7}').next())
        .and_then(|rest| rest.split("\u{1b}\\").next())
        .unwrap_or("");
    let decoded = b64_decode(payload);
    anyhow::ensure!(
        decoded.contains("The outside file says m2-secret."),
        "step 6: the OSC-52 payload is not the last assistant reply: {decoded:?}"
    );
    ledger.pass("6-copy", "leader+y → OSC-52 with the reply text in the raw pane stream");

    // -- step 7: theme -------------------------------------------------------
    let ansi_before = capture_ansi(SESSION);
    std::fs::write(artifacts.join("step-07-theme-before.ansi"), &ansi_before)?;
    leader(SESSION, "t");
    let theme_dialog = wait_capture(SESSION, "Themes", 10, 1).await?;
    std::fs::write(artifacts.join("step-07-theme-dialog.txt"), &theme_dialog)?;
    anyhow::ensure!(
        theme_dialog.contains("Themes"),
        "step 7: the theme dialog did not open:\n{theme_dialog}"
    );
    type_text(SESSION, "dracula");
    sleep(1).await;
    key(SESSION, "Enter");
    sleep(2).await;
    let ansi_after = capture_ansi(SESSION);
    std::fs::write(artifacts.join("step-07-theme-after.ansi"), &ansi_after)?;
    let after_text = capture(SESSION);
    std::fs::write(artifacts.join("step-07-theme-after.txt"), &after_text)?;
    anyhow::ensure!(
        !after_text.contains("Themes"),
        "step 7: the theme dialog did not close:\n{after_text}"
    );
    if ansi_after == ansi_before {
        ledger.finding("7-theme", "theme selected but ANSI capture unchanged");
    } else {
        ledger.pass("7-theme", "leader+t → dracula selected; ANSI captures differ");
    }

    // -- step 7b: external editor ($EDITOR over the focused draft) ---------
    // G11-F2: the run loop's suspend/resume block (leave alternate
    // screen, disable raw mode, run $EDITOR, resume, clear) is terminal
    // state surgery unit tests stub out. Exercise it for real: leader+e
    // must run the stub EDITOR (appends + exit 0) and the draft must
    // come back replaced. Driven on the EMPTY draft — the port's stroke
    // routing feeds printables to the composer whenever a draft is
    // non-empty, so the leader continuation only completes on an empty
    // composer (v2's keymap suspends textarea handling for chord
    // resolution; that ordering difference is a NAMED residual for the
    // TUI lane, out of this sweep's scope).
    leader(SESSION, "e");
    let edited = wait_capture(SESSION, "EDITED-BY-STUB", 10, 1).await?;
    std::fs::write(artifacts.join("step-07b-editor-draft.txt"), &edited)?;
    anyhow::ensure!(
        edited.contains("EDITED-BY-STUB"),
        "step 7b: the draft was not replaced with the editor's output:\n{edited}"
    );
    ledger.pass("7b-editor-draft", "leader+e → $EDITOR ran (append + exit 0), draft replaced");
    anyhow::ensure!(
        edited.contains("The outside file says m2-secret."),
        "step 7b: the session frame did not survive the editor suspend/resume:\n{edited}"
    );
    type_text(SESSION, "!");
    let resumed = wait_capture(SESSION, "EDITED-BY-STUB!", 10, 1).await?;
    std::fs::write(artifacts.join("step-07b-editor-resumed.txt"), &resumed)?;
    ledger.pass("7b-editor-resume", "raw mode intact after resume (composer still takes input)");
    // Clear the draft so step 8's captures stay canonical (tmux's key
    // name is BSpace — "Backspace" would be typed literally).
    for _ in 0..20 {
        key(SESSION, "BSpace");
    }

    // -- step 8: kill + reboot → tabs restored ------------------------------
    // Distinct titles make the restored tabs visibly distinguishable: the
    // driver renames both sessions (`session.renamed` → the running TUI
    // updates its tab titles → tabs.json persists them).
    for (id, title) in [(&session1, "Alpha notes"), (&session2, "Beta second")] {
        let (status, body) = api(
            &url,
            &password,
            "PATCH",
            &format!("/api/session/{id}"),
            Some(&format!("{{\"title\":\"{title}\"}}")),
        )
        .await?;
        anyhow::ensure!(status == 204, "step 8: rename {id} failed ({status}): {body}");
    }
    let renamed = wait_capture(SESSION, "Alpha notes", 10, 1).await?;
    std::fs::write(artifacts.join("step-08-renamed-live.txt"), &renamed)?;

    let tabs_json = home.join(".local/state/opencode/local/tui/tabs.json");
    let mut persisted = String::new();
    for _ in 0..10 {
        if tabs_json.is_file() {
            persisted = std::fs::read_to_string(&tabs_json).unwrap_or_default();
            if persisted.contains(&session1) && persisted.contains(&session2) {
                break;
            }
        }
        sleep(1).await;
    }
    std::fs::write(artifacts.join("step-08-tabs-json.txt"), &persisted)?;
    anyhow::ensure!(
        persisted.contains(&session1) && persisted.contains(&session2),
        "step 8: tabs.json does not hold both sessions: {persisted}"
    );

    leader(SESSION, "q");
    wait_tui_gone(15).await?;
    let exit_capture = capture(SESSION);
    std::fs::write(artifacts.join("step-08-tui-exit.txt"), &exit_capture)?;
    ledger.pass("8-tui-exit", "leader+q exited the TUI cleanly (pane gone)");

    let stderr_log2 = artifacts.join("tui-stderr-2.txt");
    let raw_log2 = artifacts.join("pane-raw-2.log");
    launch_tui(&cli, &url, &password, &home, &project, &stderr_log2, &raw_log2, None)?;
    // The restored first tab is the route; the FIRST-connect boot resync
    // (session.list + message.list per restored tab) replays its
    // transcript from the server (the D4 fix — no reconnect needed).
    let mut restored = String::new();
    for _ in 0..30 {
        sleep(2).await;
        restored = capture(SESSION);
        if restored.contains("The notes file is present.") {
            break;
        }
    }
    std::fs::write(artifacts.join("step-08-reboot-restored.txt"), &restored)?;
    anyhow::ensure!(
        restored.contains("Alpha notes"),
        "step 8: the rebooted TUI did not restore the tab-1 route/title:\n{restored}"
    );
    anyhow::ensure!(
        restored.contains("The notes file is present."),
        "step 8: the restored transcript stayed empty (the boot resync must fetch \
         message.list per restored tab on the first connect):\n{restored}"
    );
    ledger.pass("8-transcripts", "restored tabs' transcripts hydrated on the first connect");
    // The strip shows the OTHER restored tab's title in the same capture.
    anyhow::ensure!(
        restored.contains("Beta second"),
        "step 8: the restored strip does not show tab 2's title:\n{restored}"
    );
    ledger.pass("8-strip-restored", "the restored strip shows both renamed tabs");
    leader(SESSION, "2");
    let restored2 = wait_capture(SESSION, "Beta second", 8, 1).await?;
    std::fs::write(artifacts.join("step-08-tab2-restored.txt"), &restored2)?;
    anyhow::ensure!(
        restored2.contains("Beta second"),
        "step 8: leader+2 did not switch to the restored tab 2:\n{restored2}"
    );
    leader(SESSION, "1");
    ledger.pass(
        "8-persistence",
        "tabs + route restored from tabs.json; quick slots still switch restored tabs",
    );

    // -- step 9: clean exit + SIGTERM-safe server shutdown ------------------
    leader(SESSION, "q");
    wait_tui_gone(15).await?;
    let gone = Command::new("tmux").args(["has-session", "-t", SESSION]).output();
    anyhow::ensure!(
        gone.map_or(true, |out| !out.status.success()),
        "step 9: the TUI session survived leader+q"
    );
    ledger.pass("9-tui-exit", "second instance exited cleanly with the server still up");

    let started = Instant::now();
    server.stop().await?;
    let shutdown = started.elapsed();
    std::fs::write(
        artifacts.join("step-09-server-stop.txt"),
        format!("SIGTERM shutdown elapsed: {shutdown:?}\n"),
    )?;
    anyhow::ensure!(
        shutdown < Duration::from_secs(5),
        "step 9: server SIGTERM shutdown took {shutdown:?} (>= 5s)"
    );
    ledger.pass("9-server-stop", format!("server exited in {shutdown:?} (< 5s)"));

    // -- evidence copies next to the goldens --------------------------------
    let goldens = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens/m2");
    std::fs::create_dir_all(&goldens)?;
    for entry in std::fs::read_dir(&artifacts)? {
        let entry = entry?;
        if entry.path().is_file() {
            let _ = std::fs::copy(entry.path(), goldens.join(entry.file_name()));
        }
    }

    // -- the ledger ----------------------------------------------------------
    println!("\n[m2] ===== M2 daily-work ledger =====");
    for (step, passed, note) in &ledger.rows {
        println!("[m2] {} {step}: {note}", if *passed { "PASS " } else { "FIND" });
    }
    let findings = ledger.rows.iter().filter(|(_, passed, _)| !passed).count();
    println!("[m2] {} rows, {findings} findings/skips", ledger.rows.len());
    eprintln!("[m2] PASS — captures in {} + goldens/m2/", artifacts.display());
    Ok(())
}
