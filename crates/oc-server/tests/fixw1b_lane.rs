//! FIXW1B lane battery (scan-cd register fixes, backend half):
//!
//! * **CD-12** — `GET /api/command` surfaces config `commands` maps AND
//!   `{command,commands}/**/*.md` discovery; `POST /api/session/:id/command`
//!   executes a config command's template (positional substitution) and
//!   404s unknown names.
//! * **CD-34** — a REAL plugin's `ctx.command.add` registrations surface
//!   through the production boot (`boot_and_serve`), and executing one
//!   dispatches into the guest (marker proof).
//! * **CD-1** — `POST /api/session/:id/prompt` returns while the run is
//!   still settling against a held-open provider stream (v2's
//!   fire-and-forget `execution.wake` semantics).
//!
//! CD-2 (fork parent_id) and CD-13 (global AGENTS.md v2 dir) carry their
//! E2E assertions in `be1_lane.rs` and `oc-core/src/engine/tests.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oc_client::Client;
use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::handlers;
use oc_server::lifecycle::ServeOptions;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// A minimal successful chat-completion SSE (one text delta + stop).
const OK_SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
    data: {\"id\":\"cmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"A greeting\"}}]}\n\n\
    data: {\"id\":\"cmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n\
    data: [DONE]\n\n";

/// The provider config: an openai-compatible custom provider pointed at
/// the in-test mock.
fn user_config(provider_port: u16) -> String {
    format!(
        r#"{{
            "model": "mock/mock-1",
            "providers": {{
                "mock": {{
                    "name": "Mock",
                    "npm": "@ai-sdk/openai-compatible",
                    "settings": {{
                        "baseURL": "http://127.0.0.1:{provider_port}/v1",
                        "apiKey": "sk-dummy"
                    }},
                    "models": {{
                        "mock-1": {{ "name": "Mock 1" }}
                    }}
                }}
            }}
        }}"#
    )
}

async fn read_one_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_subsequence(&buf, b"\r\n\r\n") {
            break position;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Serves one SSE completion per request, forever.
async fn sse_provider(listener: TcpListener, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    loop {
        let (mut stream, _) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(_) => return,
            },
            _ = shutdown.changed() => return,
        };
        if read_one_request(&mut stream).await.is_some() {
            let _ = stream.write_all(OK_SSE.as_bytes()).await;
        }
        let _ = stream.shutdown().await;
    }
}

/// Accepts requests and NEVER answers (the held-open stream — CD-1's
/// SLOWSTREAM class).
async fn hung_provider(listener: TcpListener) {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else { return };
        if read_one_request(&mut stream).await.is_some() {
            // Hold the connection open; the response never arrives.
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    }
}

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    _workspace: PathBuf,
    _data: tempfile::TempDir,
}

async fn boot(workspace: PathBuf) -> TestServer {
    let data = tempfile::tempdir().expect("data tempdir");
    std::env::set_current_dir(&workspace).expect("chdir to workspace");
    let engine =
        oc_core::engine::Engine::boot(oc_core::engine::EngineOptions::new(
            data.path().join("data"),
            data.path().join("home"),
        ))
        .await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, state, _workspace: workspace, _data: data }
}

async fn create_session(client: &Client, directory: &Path) -> String {
    client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: directory.to_string_lossy().into_owned(),
            }),
            ..Default::default()
        })
        .await
        .expect("create session")
        .id
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

// ---------------------------------------------------------------------------
// CD-12: config + markdown command discovery, listing AND execution
// ---------------------------------------------------------------------------

/// Writes the CD-12 fixture layout into `workspace`: a config `commands`
/// map, a `{command,commands}/**/*.md` pair, and noise that must NOT
/// surface.
fn write_command_fixture(workspace: &Path) {
    std::fs::write(
        workspace.join("opencode.json"),
        json!({
            "commands": {
                "cfg-cmd": {
                    "template": "Fix $1 then $2.",
                    "description": "Config command"
                }
            }
        })
        .to_string(),
    )
    .expect("config");
    std::fs::create_dir_all(workspace.join(".opencode").join("command").join("team"))
        .expect("cmd dirs");
    std::fs::write(
        workspace.join(".opencode").join("command").join("md-cmd.md"),
        "---\ndescription: Markdown command\n---\nSummarize.",
    )
    .expect("md-cmd");
    std::fs::write(
        workspace.join(".opencode").join("command").join("team").join("plan.md"),
        "---\ndescription: Team plan\n---\nPlan $ARGUMENTS now.",
    )
    .expect("team/plan");
    // Noise: outside the source dirs, wrong suffix.
    std::fs::write(workspace.join(".opencode").join("notes.md"), "not a command")
        .expect("noise");
}

#[tokio::test]
async fn cd12_command_list_surfaces_config_and_markdown_sources() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_command_fixture(workspace.path());
    let server = boot(workspace.path().to_path_buf()).await;

    let listed = server.client.command().list(&Default::default()).await.expect("command.list");
    let names: Vec<(String, Option<String>)> = listed
        .data
        .iter()
        .map(|command| (command.name.clone(), command.description.clone()))
        .collect();
    let flat: Vec<&str> = names.iter().map(|(name, _)| name.as_str()).collect();
    assert!(flat.contains(&"cfg-cmd"), "config command listed: {flat:?}");
    assert!(flat.contains(&"md-cmd"), "flat markdown command listed: {flat:?}");
    assert!(flat.contains(&"team/plan"), "nested markdown id keeps its path: {flat:?}");
    assert!(!flat.contains(&"notes"), "noise outside {{command,commands}}/** is not listed");
    let cfg = names.iter().find(|(name, _)| name == "cfg-cmd").expect("cfg-cmd");
    assert_eq!(cfg.1.as_deref(), Some("Config command"));
    let md = names.iter().find(|(name, _)| name == "md-cmd").expect("md-cmd");
    assert_eq!(md.1.as_deref(), Some("Markdown command"));
}

#[tokio::test]
#[allow(unused_assignments)] // poll loop overwrites the accumulator before reading
async fn cd12_command_execute_expands_template_and_unknown_404s() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_command_fixture(workspace.path());
    let provider_listener = TcpListener::bind("127.0.0.1:0").await.expect("provider bind");
    let port = provider_listener.local_addr().unwrap().port();
    std::fs::write(workspace.path().join("opencode.json"), user_config(port)).expect("config");
    // Re-add the commands map the provider config just replaced.
    let mut config: Value =
        serde_json::from_str(&user_config(port)).expect("parse provider config");
    config["commands"] = json!({
        "cfg-cmd": {"template": "Fix $1 then $2.", "description": "Config command"}
    });
    std::fs::write(workspace.path().join("opencode.json"), config.to_string()).expect("config");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        sse_provider(provider_listener, shutdown_rx).await;
    });
    let server = boot(workspace.path().to_path_buf()).await;
    let session_id = create_session(&server.client, workspace.path()).await;

    // Template execution: positional substitution ($2 = last absorbs rest).
    server
        .client
        .session()
        .command(
            &session_id,
            &oc_client::types::SessionCommandBody {
                name: "cfg-cmd".to_owned(),
                text: "a b c".to_owned(),
                ..Default::default()
            },
        )
        .await
        .expect("cfg-cmd executes");
    // Plain markdown template + input appends the input.
    server
        .client
        .session()
        .command(
            &session_id,
            &oc_client::types::SessionCommandBody {
                name: "md-cmd".to_owned(),
                text: "the text".to_owned(),
                ..Default::default()
            },
        )
        .await
        .expect("md-cmd executes");

    let mut texts: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let messages = server
            .client
            .message()
            .list(&session_id, &oc_client::types::SessionMessageListQuery::default())
            .await
            .expect("message list");
        texts = messages
            .data
            .iter()
            .filter_map(|message| match message {
                oc_client::types::SessionMessageInfo::User(user) => Some(user.text.clone()),
                _ => None,
            })
            .collect::<Vec<String>>();
        if texts.len() >= 2 || Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        texts.iter().any(|text| text.contains("Fix a then b c.")),
        "cfg-cmd template expanded (last placeholder absorbs the rest): {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("Summarize.") && text.contains("the text")),
        "plain template appends the input: {texts:?}"
    );

    // Unknown command: the documented 404.
    let unknown = server
        .client
        .session()
        .command(
            &session_id,
            &oc_client::types::SessionCommandBody {
                name: "no-such-command".to_owned(),
                text: String::new(),
                ..Default::default()
            },
        )
        .await;
    assert!(unknown.is_err(), "unknown command 404s");
    let _ = shutdown_tx.send(true);
}

// ---------------------------------------------------------------------------
// CD-1: /prompt returns before run settlement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cd1_prompt_returns_while_the_provider_stream_is_held_open() {
    let workspace = tempfile::tempdir().expect("workspace");
    let provider_listener = TcpListener::bind("127.0.0.1:0").await.expect("provider bind");
    let port = provider_listener.local_addr().unwrap().port();
    std::fs::write(workspace.path().join("opencode.json"), user_config(port)).expect("config");
    tokio::spawn(async move {
        hung_provider(provider_listener).await;
    });
    let server = boot(workspace.path().to_path_buf()).await;
    let session_id = create_session(&server.client, workspace.path()).await;

    // CD-1: the HTTP response returns the Inbox info immediately — the
    // old resume-join held it for the whole provider stream (≥90 s in
    // the scan's SLOWSTREAM probe).
    let started = Instant::now();
    let admitted = tokio::time::timeout(Duration::from_secs(3), server.client.session().prompt(
        &session_id,
        &oc_client::types::SessionPromptBody { text: "hello".to_owned(), ..Default::default() },
    ))
    .await
    .expect("prompt returns without waiting for the run (CD-1)")
    .expect("prompt admits");
    assert!(started.elapsed() < Duration::from_secs(3), "prompt was fire-and-forget");
    assert!(!admitted.id.is_empty(), "the Inbox info crossed the wire");

    // The run itself DID start (the wake registered the work): poll the
    // engine's execution ownership for this session.
    let location = oc_core::location::LocationRef::new(workspace.path().to_path_buf(), None);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut active = false;
    loop {
        if let Ok(services) = server.state.engine.location(location.clone()).await {
            if services.execution.active().contains(&session_id) {
                active = true;
                break;
            }
        }
        if Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(active, "the execution is running while the response already returned");
}

// ---------------------------------------------------------------------------
// CD-34: real plugin command registrations through the production boot
// ---------------------------------------------------------------------------

/// The v2-shape test plugin: two `ctx.command.add` registrations, one
/// writing a marker with the invocation (the dispatch proof).
const CMD_PLUGIN: &str = r#"
import { writeFileSync } from "node:fs";
import { join } from "node:path";
function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}
export default {
  id: "cmdplugin",
  async setup(ctx) {
    await ctx.command.add({
      name: "plug.hello",
      description: "Plugin hello",
      execute: async (input) => {
        writeFileSync(
          marker("plug-hello.txt"),
          `${input.sessionID}:${input.prompt?.text ?? ""}`,
        );
        return "done";
      },
    });
    await ctx.command.add({
      name: "plug.quiet",
      description: "Quiet twin",
      execute: async () => "quiet",
    });
  },
};
"#;

fn bun_available() -> bool {
    std::process::Command::new("bun").arg("--version").output().is_ok_and(|out| out.status.success())
}

#[tokio::test]
async fn cd34_plugin_commands_surface_and_execute_through_the_real_boot() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let root = PathBuf::from(format!("/tmp/opencode/oc-server-fixw1b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    std::fs::write(project.join("cmdplugin.ts"), CMD_PLUGIN).expect("plugin");
    std::fs::write(
        project.join("opencode.json"),
        json!({ "plugins": ["./cmdplugin.ts"] }).to_string(),
    )
    .expect("config");
    std::env::set_var("OC_PLUGIN_TEST_SANDBOX", root.display().to_string());

    let handle = boot_and_serve(&BootOptions {
        serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
        password: None,
        cors_origins: Vec::new(),
        version: "2.0.24".to_owned(),
        data_root: root.join("data"),
        home: home.clone(),
        config_location: Some(project.clone()),
    })
    .await
    .expect("boot and serve");
    let client = Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // The plugin's registrations surface (the host load settles
    // asynchronously — poll for the command rows).
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut names: Option<Vec<String>> = None;
    loop {
        let listed_names: Vec<String> = client
            .command()
            .list(&Default::default())
            .await
            .ok()
            .map(|listed| listed.data.into_iter().map(|command| command.name).collect())
            .unwrap_or_default();
        if listed_names.contains(&"plug.hello".to_owned())
            && listed_names.contains(&"plug.quiet".to_owned())
        {
            names = Some(listed_names);
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let Some(names) = names else {
        panic!("plugin commands never surfaced through /api/command");
    };
    assert!(
        !names.iter().any(|name| name == "cfg-cmd" || name == "md-cmd"),
        "this project declares no config/markdown commands: {names:?}"
    );

    // Executing dispatches into the guest with the session + text (the
    // marker is the end-to-end proof).
    let session_id = create_session(&client, &project).await;
    client
        .session()
        .command(
            &session_id,
            &oc_client::types::SessionCommandBody {
                name: "plug.hello".to_owned(),
                text: "with args".to_owned(),
                ..Default::default()
            },
        )
        .await
        .expect("plugin command executes");
    let expected_marker = format!("{session_id}:with args");
    let marker = wait_for(Duration::from_secs(10), || {
        std::fs::read_to_string(root.join("plug-hello.txt")).ok().filter(|text| !text.is_empty())
    })
    .await;
    assert_eq!(
        marker.as_deref(),
        Some(expected_marker.as_str()),
        "the guest's execute saw the real session id + text"
    );
    let _ = std::fs::remove_dir_all(&root);
}
