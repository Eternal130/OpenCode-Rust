//! ACP integration battery (R-03): spawns the real `opencode acp`
//! binary (stdio mode) against a stub ACP client built with the same
//! `agent-client-protocol` crate (the `yolo_one_shot_client.rs`
//! pattern), backed by a `FakeLlm` completions server (the
//! `tools/parity` `FakeLlm` pattern).
//!
//! Coverage:
//! * `initialize` — exact capabilities handshake (`service.ts:106-130`).
//! * `session/new` + `session/prompt` — the update stream incl. a
//!   `tool_call` and a permission request → `allow_once` → `end_turn`.
//! * `session/list` — list + cursor-error mapping (service.ts:164-189).
//! * `session/cancel` — mid-prompt cancel → stopReason cancelled.

#![allow(clippy::too_many_lines)]

use std::collections::VecDeque;
use std::fmt::Write as FmtWrite;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    ClientCapabilities, ContentBlock, ElicitationCapabilities, ElicitationFormCapabilities,
    InitializeRequest, InitializeResponse, ListSessionsRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Client, ConnectTo, ConnectionTo};
use futures::channel::mpsc as futures_mpsc;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc as tokio_mpsc, Mutex};

// ---------------------------------------------------------------------
// The fake LLM (parity::scenario::FakeLlm pattern)
// ---------------------------------------------------------------------

/// An openai-compatible SSE server serving a fixed sequence of
/// completion scripts, one per HTTP request (each request is a fresh
/// connection; the n-th request gets the n-th script, clamped to the
/// last).
struct FakeLlm {
    port: u16,
    #[allow(dead_code)]
    requests: Arc<Mutex<VecDeque<String>>>,
}

/// One write-then-pause step of a completion script.
struct Chunk {
    body: String,
    pause_ms: u64,
}

impl FakeLlm {
    /// `scripts` serve the TOOLED completion requests in order (the
    /// agent's prompt loop); tool-less requests (the session title
    /// generator) always get a plain one-line reply so they never
    /// consume a scripted turn.
    async fn start(scripts: Vec<Vec<Chunk>>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind fake llm");
        let port = listener.local_addr().expect("addr").port();
        let counter = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(VecDeque::new()));
        let scripts = Arc::new(scripts);
        let request_log = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { return };
                let counter = Arc::clone(&counter);
                let scripts = Arc::clone(&scripts);
                let request_log = Arc::clone(&request_log);
                tokio::spawn(async move {
                    serve_completion(socket, counter, scripts, request_log).await;
                });
            }
        });
        Self { port, requests }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn provider_config(&self) -> String {
        format!(
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
            self.base()
        )
    }
}

async fn serve_completion(
    mut socket: tokio::net::TcpStream,
    counter: Arc<AtomicUsize>,
    scripts: Arc<Vec<Vec<Chunk>>>,
    request_log: Arc<Mutex<VecDeque<String>>>,
) {
    // Read the request head + body (small, content-length framed).
    let mut buffer = vec![0u8; 65536];
    let mut seen = 0;
    loop {
        match tokio::time::timeout(Duration::from_secs(10), socket.read(&mut buffer[seen..])).await
        {
            Ok(Ok(0) | Err(_)) | Err(_) => break,
            Ok(Ok(n)) => {
                seen += n;
                let head_end = buffer[..seen]
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4);
                if let Some(head_end) = head_end {
                    let head = String::from_utf8_lossy(&buffer[..head_end]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.trim() == "content-length" {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if seen >= head_end + length {
                        break;
                    }
                }
            }
        }
    }
    let request = String::from_utf8_lossy(&buffer[..seen]).into_owned();
    let tooled = request.contains("\"tools\":");
    request_log.lock().await.push_back(request);
    // The counter counts only TOOLED requests (the scripted agent turns);
    // tool-less requests (title generation) always take the last script.
    let index = if tooled {
        counter.fetch_add(1, Ordering::SeqCst).min(scripts.len() - 2)
    } else {
        scripts.len() - 1
    };
    for chunk in &scripts[index] {
        let _ = socket.write_all(chunk.body.as_bytes()).await;
        if chunk.pause_ms > 0 {
            tokio::time::sleep(Duration::from_millis(chunk.pause_ms)).await;
        }
    }
    let _ = socket.shutdown().await;
}

/// A completion script: text deltas then stop.
fn text_script(parts: &[&str]) -> Vec<Chunk> {
    let mut script = String::from("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
    script.push_str("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n");
    for part in parts {
        write!(
            script,
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
            serde_json::to_string(part).unwrap()
        )
        .expect("script write");
    }
    script.push_str(
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}\n\ndata: [DONE]\n\n",
    );
    vec![Chunk { body: script, pause_ms: 0 }]
}

/// A completion script: one tool call (openai index-keyed deltas), for
/// the permission flow.
fn tool_script(tool: &str, arguments: &serde_json::Value) -> Vec<Chunk> {
    let arguments = arguments.to_string();
    let tool = tool.to_owned();
    let mut script = String::from("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
    script.push_str("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n");
    write!(
        script,
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{{\"name\":\"{tool}\",\"arguments\":\"\"}}}}]}}}}]}}\n\n"
    )
    .expect("script write");
    write!(
        script,
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":{}}}}}]}}}}]}}\n\n",
        serde_json::to_string(&arguments).unwrap()
    )
    .expect("script write");
    script.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n");
    script.push_str("data: [DONE]\n\n");
    vec![Chunk { body: script, pause_ms: 0 }]
}

/// The tool-less request script (session title generation).
fn title_script() -> Vec<Chunk> {
    text_script(&["Test session"])
}

/// A slow script: one delta, then a long pause before finishing — the
/// cancel window (kept for future live-delta streaming; the current
/// server projects deltas at stream end).
#[allow(dead_code)]
fn slow_script() -> Vec<Chunk> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"starting\"}}]}\n\n".to_owned();
    let tail = "data: {\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_owned();
    vec![Chunk { body: head, pause_ms: 8_000 }, Chunk { body: tail, pause_ms: 0 }]
}

// ---------------------------------------------------------------------
// The sandbox + binary spawn
// ---------------------------------------------------------------------

struct AcpChild {
    child: Child,
    #[allow(dead_code)]
    sandbox: PathBuf,
    stderr_path: PathBuf,
}

fn read_stderr(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_acp(sandbox: &std::path::Path) -> AcpChild {
    let stderr_path = sandbox.join("acp.stderr.log");
    let stderr_file = std::fs::File::create(&stderr_path).expect("stderr log");
    let project = sandbox.join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    let home = sandbox.join("home");
    for dir in ["data", "state", "cache", "config"] {
        std::fs::create_dir_all(home.join(dir)).expect("home dir");
    }
    std::fs::create_dir_all(sandbox.join("tmp")).expect("tmp dir");
    let child = Command::new(env!("CARGO_BIN_EXE_opencode-rust"))
        .arg("acp")
        .current_dir(&project)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", &home)
        .env("OPENCODE_TEST_HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("TMPDIR", sandbox.join("tmp"))
        .env("OPENCODE_CONFIG", sandbox.join("opencode.json"))
        .env("RUST_LOG", "warn")
        .env("OPENCODE_PRINT_LOGS", "1")
        .env("OPENCODE_ACP_DEBUG", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("spawn opencode acp");
    AcpChild { child, sandbox: sandbox.to_path_buf(), stderr_path }
}

/// The tail of the sandbox's acp stderr log (standalone server logs
/// ride the same stream when `OPENCODE_PRINT_LOGS=1`).
fn dump_fake_requests(fake: &FakeLlm) -> String {
    let requests = fake.requests.try_lock();
    match requests {
        Ok(log) => log
            .iter()
            .rev()
            .take(3)
            .rev()
            .enumerate()
            .map(|(index, request)| format!("--- fake request {index} ---\n{request}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => "<locked>".to_owned(),
    }
}

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opencode/acp-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    dir
}

// ---------------------------------------------------------------------
// The stub client transport (ndjson over the child's pipes)
// ---------------------------------------------------------------------

/// A no-cap ndjson transport over the child's stdin/stdout (the agent
/// side enforces its own cap).
struct ChildNdJson {
    sink: WriterSink,
    stream: LineStream,
}

impl ChildNdJson {
    fn spawn(child: &mut Child) -> Self {
        let stdout = child.stdout.take().expect("stdout");
        let stdin = child.stdin.take().expect("stdin");
        Self { sink: WriterSink::spawn(stdin), stream: LineStream::spawn(stdout) }
    }
}

impl<R: agent_client_protocol::Role> ConnectTo<R> for ChildNdJson {
    async fn connect_to(
        self,
        client: impl ConnectTo<R::Counterpart>,
    ) -> Result<(), agent_client_protocol::Error> {
        agent_client_protocol::ConnectTo::<R>::connect_to(
            agent_client_protocol::Lines::new(self.sink, self.stream),
            client,
        )
        .await
    }
}

struct WriterSink {
    tx: mpsc::Sender<String>,
}

impl WriterSink {
    fn spawn<W: Write + Send + 'static>(write: W) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut write = std::io::BufWriter::new(write);
            for line in rx {
                if write.write_all(line.as_bytes()).is_err()
                    || write.write_all(b"\n").is_err()
                    || write.flush().is_err()
                {
                    return;
                }
            }
        });
        Self { tx }
    }
}

impl futures::Sink<String> for WriterSink {
    type Error = std::io::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, item: String) -> Result<(), Self::Error> {
        self.tx.send(item).map_err(|_| std::io::Error::other("writer gone"))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

struct LineStream {
    rx: futures_mpsc::UnboundedReceiver<std::io::Result<String>>,
}

impl LineStream {
    fn spawn<R: Read + Send + 'static>(read: R) -> Self {
        let (tx, rx) = futures_mpsc::unbounded();
        std::thread::spawn(move || {
            let reader = BufReader::new(read);
            for line in reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        if tx.unbounded_send(Ok(line)).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
        Self { rx }
    }
}

impl futures::Stream for LineStream {
    type Item = std::io::Result<String>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_next_unpin(cx)
    }
}

// ---------------------------------------------------------------------
// The stub client
// ---------------------------------------------------------------------

type Updates = Arc<Mutex<Vec<SessionNotification>>>;

struct ClientDrive {
    updates: Updates,
    permission_tx: tokio_mpsc::UnboundedSender<RequestPermissionRequest>,
    permission_rx:
        std::sync::Mutex<Option<tokio_mpsc::UnboundedReceiver<RequestPermissionRequest>>>,
    /// When set, the stub HOLDS its permission answers until notified
    /// (the cancel test needs a parked ask).
    hold_permissions: Option<Arc<tokio::sync::Notify>>,
}

impl ClientDrive {
    fn new() -> Self {
        let (permission_tx, permission_rx) = tokio_mpsc::unbounded_channel();
        Self {
            updates: Arc::new(Mutex::new(Vec::new())),
            permission_tx,
            permission_rx: std::sync::Mutex::new(Some(permission_rx)),
            hold_permissions: None,
        }
    }
}

/// Drive the stub client against the spawned agent until `run` returns.
async fn drive_client<F>(child: &mut Child, drive: ClientDrive, run: F)
where
    F: AsyncFnOnce(
        ConnectionTo<agent_client_protocol::Agent>,
    ) -> Result<(), agent_client_protocol::Error>,
{
    let transport = ChildNdJson::spawn(child);
    let updates = Arc::clone(&drive.updates);
    let permission_tx = drive.permission_tx.clone();
    let result = Client
        .builder()
        .name("acp-test-client")
        .on_receive_notification(
            {
                let updates = Arc::clone(&updates);
                async move |notification: SessionNotification, _cx| {
                    updates.lock().await.push(notification);
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let permission_tx = permission_tx.clone();
                let hold = drive.hold_permissions.clone();
                async move |request: RequestPermissionRequest, responder, cx| {
                    // Default stub behavior: allow once; the cancel test
                    // parks the answer until it releases the gate (off
                    // the dispatch loop so responses still route).
                    let permission_tx = permission_tx.clone();
                    let option = request
                        .options
                        .iter()
                        .find(|option| option.option_id.0.as_ref() == "once")
                        .cloned();
                    let _ = permission_tx.send(request);
                    if let Some(hold) = hold.clone() {
                        cx.spawn(async move {
                            hold.notified().await;
                            let _ = responder.respond(match option {
                                Some(option) => RequestPermissionResponse::new(
                                    RequestPermissionOutcome::Selected(
                                        SelectedPermissionOutcome::new(option.option_id),
                                    ),
                                ),
                                None => RequestPermissionResponse::new(
                                    RequestPermissionOutcome::Cancelled,
                                ),
                            });
                            Ok(())
                        })
                        .map_err(|error| {
                            agent_client_protocol::Error::internal_error().data(format!("{error}"))
                        })?;
                        return Ok(());
                    }
                    match option {
                        Some(option) => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option.option_id,
                            )),
                        )),
                        None => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, run)
        .await;
    if let Err(error) = result {
        panic!("acp client connection failed: {error}");
    }
}

/// The full-capabilities handshake request (initialize with form
/// elicitation + terminal-auth meta).
fn initialize_request() -> InitializeRequest {
    let mut meta = serde_json::Map::new();
    meta.insert("terminal-auth".to_owned(), serde_json::Value::Bool(true));
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(
            ClientCapabilities::default().elicitation(
                ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()),
            ),
        )
        .meta(meta)
}

async fn wait_for<F>(updates: &Updates, predicate: F) -> SessionNotification
where
    F: Fn(&SessionNotification) -> bool,
{
    let deadline = Duration::from_secs(30);
    let started = std::time::Instant::now();
    loop {
        let found =
            updates.lock().await.iter().find(|notification| predicate(notification)).cloned();
        if let Some(found) = found {
            return found;
        }
        if started.elapsed() > deadline {
            let collected: Vec<String> = updates
                .lock()
                .await
                .iter()
                .map(|notification| format!("{:?}", notification.update))
                .collect();
            panic!("timed out waiting for update; saw:\n{}", collected.join("\n"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

/// `initialize` returns the exact v2 capabilities (service.ts:118-129).
#[tokio::test]
async fn initialize_advertises_exact_capabilities() {
    let sandbox = sandbox("init");
    let fake = FakeLlm::start(vec![text_script(&["hi"]), title_script()]).await;
    std::fs::write(sandbox.join("opencode.json"), fake.provider_config()).unwrap();
    let mut child = spawn_acp(&sandbox);
    let drive = ClientDrive::new();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    drive_client(&mut child.child, drive, async move |cx| {
        let response: InitializeResponse =
            cx.send_request(initialize_request()).block_task().await.expect("initialize");
        // protocolVersion 1.
        assert_eq!(response.protocol_version, ProtocolVersion::V1);
        // agentCapabilities (service.ts:120-126).
        let capabilities = &response.agent_capabilities;
        assert!(capabilities.load_session);
        assert!(capabilities.mcp_capabilities.http);
        assert!(!capabilities.mcp_capabilities.sse);
        assert!(capabilities.prompt_capabilities.embedded_context);
        assert!(capabilities.prompt_capabilities.image);
        let session = &capabilities.session_capabilities;
        assert!(session.list.is_some());
        assert!(session.delete.is_some());
        assert!(session.additional_directories.is_some());
        assert!(session.fork.is_some());
        assert!(session.resume.is_some());
        assert!(session.close.is_some());
        // _meta opencode/child-session-updates: true.
        assert_eq!(
            capabilities.meta.as_ref().and_then(|meta| meta.get("opencode/child-session-updates")),
            Some(&serde_json::Value::Bool(true))
        );
        // authMethods (service.ts:108-117).
        assert_eq!(response.auth_methods.len(), 1);
        assert_eq!(response.auth_methods[0].id().0.as_ref(), "opencode-login");
        assert_eq!(response.auth_methods[0].name(), "Login with opencode");
        assert_eq!(
            response.auth_methods[0].description(),
            Some("Run `opencode auth login` in the terminal")
        );
        // agentInfo.
        assert_eq!(response.agent_info.as_ref().expect("agent info").name, "OpenCode");
        let version = response.agent_info.as_ref().expect("agent info").version.clone();
        assert_ne!(version.as_str(), "", "agentInfo.version is filled");
        let _ = ready_tx.send(());
        Ok(())
    })
    .await;
    ready_rx.await.expect("initialize flow");
    // Client capabilities parsed (child-session-updates meta absent here
    // → false; capabilities.rs:11).
    drop(child);
}

/// `session/new` + `session/prompt` with a tool call: the update stream
/// (`agent_message_chunk`, `tool_call`, `tool_call_update`), the permission
/// request → allow once, and the `end_turn` stop reason.
#[tokio::test]
async fn prompt_streams_updates_tool_call_and_permission() {
    let sandbox = sandbox("prompt");
    // Request 1: the write tool call. Request 2 (after the tool result):
    // plain text + stop.
    let fake = FakeLlm::start(vec![
        tool_script("read", &serde_json::json!({ "path": "secret.env" })),
        text_script(&["all done"]),
        title_script(),
    ])
    .await;
    let fake = Arc::new(fake);
    std::fs::write(sandbox.join("opencode.json"), fake.provider_config()).unwrap();
    let mut child = spawn_acp(&sandbox);
    // The default agent's rules ask for `read *.env` — a deterministic
    // permission ask for the ACP bridge.
    std::fs::create_dir_all(sandbox.join("project")).unwrap();
    std::fs::write(sandbox.join("project/secret.env"), "TOKEN=acp-test\n").unwrap();
    let stderr_path = child.stderr_path.clone();
    let fake_for_panic = Arc::clone(&fake);
    let drive = ClientDrive::new();
    let updates = Arc::clone(&drive.updates);
    let mut permission_rx = drive.permission_rx.lock().unwrap().take().expect("permission rx");
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    drive_client(&mut child.child, drive, async move |cx| {
        let _initialize: InitializeResponse =
            cx.send_request(initialize_request()).block_task().await.expect("initialize");
        let cwd = sandbox.join("project");
        drop(sandbox);
        let session = cx
            .send_request(NewSessionRequest::new(cwd.clone()))
            .block_task()
            .await
            .expect("session/new");
        assert!(!session.session_id.0.is_empty());
        // configOptions: model + mode at minimum (config-option.ts:22-68).
        let options = session.config_options.expect("config options");
        assert!(options.iter().any(|option| option.id == "model".into()));
        assert!(options.iter().any(|option| option.id == "mode".into()));

        let (prompt_done_tx, prompt_done_rx) = tokio::sync::oneshot::channel();
        let prompt_cx = cx.clone();
        let prompt_session = session.session_id.clone();
        let prompt = tokio::spawn(async move {
            let response = prompt_cx
                .send_request(PromptRequest::new(
                    prompt_session,
                    vec![ContentBlock::Text(TextContent::new("write the file"))],
                ))
                .block_task()
                .await
                .expect("prompt");
            let _ = prompt_done_tx.send(());
            response
        });

        // The permission request must arrive with the pending tool call.
        let asked = match tokio::time::timeout(Duration::from_secs(30), permission_rx.recv()).await
        {
            Ok(Some(asked)) => asked,
            outcome => {
                let collected: Vec<String> = updates
                    .lock()
                    .await
                    .iter()
                    .map(|notification| format!("{:?}", notification.update))
                    .collect();
                panic!(
                    "no permission request ({:?}); updates so far:\n{}\nacp stderr:\n{}\nfake requests:\n{}",
                    outcome.map(|option| option.is_some()),
                    collected.join("\n"),
                    read_stderr(&stderr_path),
                    dump_fake_requests(&fake_for_panic),
                );
            }
        };
        let tool_call = &asked.tool_call;
        assert_eq!(
            tool_call.fields.status,
            Some(agent_client_protocol::schema::v1::ToolCallStatus::Pending)
        );
        assert!(tool_call.fields.title.is_some(), "permission carries a titled pending update");
        assert_eq!(asked.options.len(), 3);
        let option_ids: Vec<String> =
            asked.options.iter().map(|option| option.option_id.0.as_ref().to_owned()).collect();
        assert_eq!(option_ids, vec!["once".to_owned(), "always".to_owned(), "reject".to_owned()]);

        // Wait for the tool call update + completion in the stream.
        let _message = wait_for(&updates, |notification| {
            matches!(
                notification.update,
                agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(_)
            )
        })
        .await;
        let _tool = wait_for(&updates, |notification| {
            matches!(
                notification.update,
                agent_client_protocol::schema::v1::SessionUpdate::ToolCall(_)
                    | agent_client_protocol::schema::v1::SessionUpdate::ToolCallUpdate(_)
            )
        })
        .await;
        let completed = wait_for(&updates, |notification| {
            matches!(
                &notification.update,
                agent_client_protocol::schema::v1::SessionUpdate::ToolCallUpdate(update)
                    if update.fields.status
                        == Some(agent_client_protocol::schema::v1::ToolCallStatus::Completed)
            )
        })
        .await;
        assert_eq!(completed.session_id, session.session_id);

        let _ = prompt_done_rx.await;
        let response = prompt.await.expect("prompt task");
        assert_eq!(response.stop_reason, agent_client_protocol::schema::v1::StopReason::EndTurn);
        // The read tool ran after allow-once: its output carries the file.
        let completed_json =
            serde_json::to_value(&completed).unwrap_or(serde_json::Value::Null);
        let output = serde_json::to_string(&completed_json).unwrap_or_default();
        assert!(output.contains("acp-test"), "read output in {output}");
        let _ = cwd;
        let _ = done_tx.send(());
        Ok(())
    })
    .await;
    tokio::time::timeout(Duration::from_secs(60), done_rx)
        .await
        .expect("prompt flow completes")
        .expect("done signal");
    drop(child);
}

/// `session/list` returns the created sessions and maps an invalid
/// cursor to an invalid-params error on the cursor field
/// (service.ts:164-189).
#[tokio::test]
async fn session_list_and_cursor_errors() {
    let sandbox = sandbox("list");
    let fake = FakeLlm::start(vec![text_script(&["hi"]), title_script()]).await;
    std::fs::write(sandbox.join("opencode.json"), fake.provider_config()).unwrap();
    let mut child = spawn_acp(&sandbox);
    let drive = ClientDrive::new();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    drive_client(&mut child.child, drive, async move |cx| {
        let _initialize: InitializeResponse =
            cx.send_request(initialize_request()).block_task().await.expect("initialize");
        let cwd = sandbox.join("project");
        drop(sandbox);
        let first = cx
            .send_request(NewSessionRequest::new(cwd.clone()))
            .block_task()
            .await
            .expect("session/new 1");
        let second = cx
            .send_request(NewSessionRequest::new(cwd.clone()))
            .block_task()
            .await
            .expect("session/new 2");

        let page = cx
            .send_request(ListSessionsRequest::new().cwd(cwd.clone()))
            .block_task()
            .await
            .expect("session/list");
        let ids: Vec<String> =
            page.sessions.iter().map(|session| session.session_id.to_string()).collect();
        assert!(ids.contains(&first.session_id.to_string()), "first session listed");
        assert!(ids.contains(&second.session_id.to_string()), "second session listed");
        for session in &page.sessions {
            assert_eq!(session.cwd, cwd, "cwd passthrough");
            assert!(
                session.title.is_some() || session.updated_at.is_some(),
                "title fallback or timestamp"
            );
        }

        // v2 passes the HTTP cursor through whenever the page is non-empty:
        // acp/service.ts:187 `...(page.cursor.next ? { nextCursor: page.cursor.next } : {})`,
        // and the session list emits cursors from the first/last row of every
        // non-empty page with no has_more check (HTTP-H2/H6 semantics).
        // Pagination terminates on the EMPTY page, not on a absent cursor.
        if page.sessions.len() < 100 {
            let cursor = page
                .next_cursor
                .as_deref()
                .expect("non-empty page carries nextCursor (v2 pass-through)");
            let next = cx
                .send_request(ListSessionsRequest::new().cwd(cwd.clone()).cursor(cursor))
                .block_task()
                .await
                .expect("session/list page 2");
            assert!(next.sessions.is_empty(), "pagination terminates on the empty page (v2)");
        }

        // An invalid cursor maps to InvalidRequest on the cursor field
        // (client.ts:21-23).
        let invalid = cx
            .send_request(ListSessionsRequest::new().cwd(cwd).cursor("not-a-cursor"))
            .block_task()
            .await;
        match invalid {
            Err(error) => {
                assert_eq!(error.code, agent_client_protocol::schema::v1::ErrorCode::InvalidParams);
                let field = error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("field"))
                    .and_then(serde_json::Value::as_str);
                assert_eq!(field, Some("cursor"));
            }
            Ok(unexpected) => panic!("expected cursor error, got {unexpected:?}"),
        }
        let _ = done_tx.send(());
        Ok(())
    })
    .await;
    tokio::time::timeout(Duration::from_secs(60), done_rx)
        .await
        .expect("list flow completes")
        .expect("done signal");
    drop(child);
}

/// `session/cancel` during a parked permission ask resolves the prompt
/// with stopReason cancelled (translate.ts:465; the ask's reject
/// fallback, turn.ts:118-131).
#[tokio::test]
async fn cancel_mid_prompt_returns_cancelled() {
    let sandbox = sandbox("cancel");
    // Request 1: a read of a `.env` file — the default agent asks.
    // Request 2 (never reached cleanly): text.
    let fake = FakeLlm::start(vec![
        tool_script("read", &serde_json::json!({ "path": "secret.env" })),
        text_script(&["after"]),
        title_script(),
    ])
    .await;
    std::fs::write(sandbox.join("opencode.json"), fake.provider_config()).unwrap();
    let mut child = spawn_acp(&sandbox);
    std::fs::create_dir_all(sandbox.join("project")).unwrap();
    std::fs::write(sandbox.join("project/secret.env"), "TOKEN=acp-test\n").unwrap();
    let stderr_path = child.stderr_path.clone();
    let mut drive = ClientDrive::new();
    let hold = Arc::new(tokio::sync::Notify::new());
    drive.hold_permissions = Some(Arc::clone(&hold));
    let updates = Arc::clone(&drive.updates);
    let mut permission_rx = drive.permission_rx.lock().unwrap().take().expect("permission rx");
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    drive_client(&mut child.child, drive, async move |cx| {
        let _initialize: InitializeResponse =
            cx.send_request(initialize_request()).block_task().await.expect("initialize");
        let cwd = sandbox.join("project");
        drop(sandbox);
        let session =
            cx.send_request(NewSessionRequest::new(cwd)).block_task().await.expect("session/new");

        let prompt_cx = cx.clone();
        let prompt_session = session.session_id.clone();
        let prompt = tokio::spawn(async move {
            prompt_cx
                .send_request(PromptRequest::new(
                    prompt_session,
                    vec![ContentBlock::Text(TextContent::new("read the env"))],
                ))
                .block_task()
                .await
                .expect("prompt")
        });

        // The permission ask arrives and parks (the stub holds it).
        let asked = tokio::time::timeout(Duration::from_secs(30), permission_rx.recv())
            .await
            .expect("permission request while held")
            .expect("channel");
        let _ = asked;

        // Cancel the turn while the ask is parked.
        cx.send_notification(agent_client_protocol::schema::v1::CancelNotification::new(
            session.session_id.clone(),
        ))
        .expect("cancel notification");

        let response = tokio::time::timeout(Duration::from_secs(30), prompt)
            .await
            .expect("prompt settles after cancel")
            .expect("prompt task");
        assert_eq!(response.stop_reason, agent_client_protocol::schema::v1::StopReason::Cancelled);
        // Release the held stub answer (it may or may not have lost the
        // race; the connection stays healthy either way).
        hold.notify_waiters();
        let _ = updates;
        let _ = done_tx.send(());
        Ok(())
    })
    .await;
    tokio::time::timeout(Duration::from_secs(60), done_rx)
        .await
        .expect("cancel flow completes")
        .expect("done signal");
    let _ = stderr_path;
    drop(child);
}
