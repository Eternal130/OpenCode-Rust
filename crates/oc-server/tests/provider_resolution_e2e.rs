//! The beta.3.1 hotfix money test: the user's EXACT setup, end to end
//! through the real `oc-server` bin.
//!
//! Config (migrated from v2, worked in v2 daily): provider map KEY
//! lowercase `own` carrying the display name "Own", top-level
//! `model: "Own/glm-5.3"` — the reference cased after the NAME, not the
//! key. Pre-fix the turn died with
//! `session.execution.failed {provider.no-route, "Model unavailable: Own/"}`
//! (the model segment dropped, nothing in the transcript, nothing in the
//! server log).
//!
//! Post-fix, with the provider's baseURL UNREACHABLE (the failure path):
//!
//! 1. `POST /api/session/:id/prompt` → 200 (the turn outcome is a
//!    session result, not an HTTP error);
//! 2. resolution SUCCEEDS — the failure is `provider.transport` (the
//!    endpoint is down), NOT `provider.no-route`;
//! 3. the error part is visible in `GET /api/session/:id/message` (an
//!    assistant message carrying `error` + `finish: "error"`);
//! 4. exactly ONE engine-level stderr line (`[session] turn failed:`)
//!    carrying the session id and the resolved model id — captured from
//!    the real process's stderr.
//!
//! Hermetic: sandboxed HOME/XDG roots, port 0, loopback-only unreachable
//! baseURL, no network.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use tokio::io::AsyncBufReadExt as _;

/// The user's config shape: lowercase provider key `own`, name "Own",
/// mixed-case model reference, dummy-but-unreachable baseURL.
const USER_CONFIG: &str = r#"{
    "model": "Own/glm-5.3",
    "providers": {
        "own": {
            "name": "Own",
            "package": "@opencode/ai/providers/anthropic",
            "settings": {
                "baseURL": "http://127.0.0.1:9/v1",
                "apiKey": "sk-dummy"
            },
            "models": {
                "glm-5.3": { "name": "GLM 5.3" }
            }
        }
    }
}"#;

/// Removes the sandbox however the test exits.
struct SandboxDrop(PathBuf);
impl Drop for SandboxDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one sequential end-to-end acceptance scenario
async fn case_mismatched_provider_resolves_and_failed_turn_surfaces_everywhere() {
    let root = PathBuf::from(format!("/tmp/opencode/oc-server-resolve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    let stderr_file = root.join("stderr.log");
    for dir in [&home, &project] {
        std::fs::create_dir_all(dir).expect("sandbox dir");
    }
    std::fs::write(project.join("opencode.json"), USER_CONFIG).expect("write user config");
    let _sandbox = SandboxDrop(root.clone());

    // The real server process, stderr captured to a file (the daemon-log
    // seam this hotfix's log line rides).
    let stderr = std::fs::File::create(&stderr_file).expect("stderr sink");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_oc-server"))
        .args(["serve", "--port", "0"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("OPENCODE_TEST_HOME", &home)
        .env_remove("OPENCODE_PASSWORD")
        .env_remove("OPENCODE_CONFIG")
        .env_remove("OPENCODE_DB")
        .env_remove("OPENCODE_DEBUG_ERRORS")
        .current_dir(&project)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .expect("spawn oc-server");

    // The ready line carries the ephemeral port.
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut line = String::new();
    let url = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            line.clear();
            let read = stdout.read_line(&mut line).await.expect("stdout");
            assert!(read != 0, "server exited before printing its URL");
            if let Some(rest) = line.trim().strip_prefix("server listening on ") {
                return rest.to_owned();
            }
        }
    })
    .await
    .expect("server ready line");
    let base = url.trim().trim_end_matches('/').to_owned();
    let client = oc_client::Client::new(&format!("{base}/")).expect("client");

    // Create a session in the project (no pinned model — the config's
    // `Own/glm-5.3` default drives resolution, the user's exact path).
    let created = client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: project.to_string_lossy().into_owned(),
            }),
            title: Some("money test".to_owned()),
            ..Default::default()
        })
        .await
        .expect("session.create");
    let session_id = created.id.clone();

    // (1) prompt → 200: the failed turn is a session outcome, not an
    // HTTP error (the response resolves after the execution settles —
    // INCLUDING v2's retry schedule for a retryable transport failure:
    // 2s→10s backoff over 10 attempts, ≈67–101s jittered, so the call
    // legitimately takes a while against a dead endpoint).
    tokio::time::timeout(
        Duration::from_secs(300),
        client.session().prompt(
            &session_id,
            &oc_client::types::SessionPromptBody { text: "hello".to_owned(), ..Default::default() },
        ),
    )
    .await
    .expect("prompt settles within the retry budget")
    .expect("session.prompt must succeed (the turn outcome is not an HTTP error)");

    // (2)+(3) resolution succeeded and the error part is visible in the
    // message list: an assistant message with `error` (the retry loop
    // backs the model call's own retry policy).
    let assistant = wait_for_errored_assistant(&client, &session_id).await;
    let error = assistant.error.as_ref().expect("assistant error part");
    assert_ne!(
        error.type_.as_str(),
        "provider.no-route",
        "the mixed-case reference must resolve against the lowercase key"
    );
    assert_eq!(
        error.type_.as_str(),
        "provider.transport",
        "the unreachable baseURL fails at transport: {:?}",
        error.message
    );
    assert!(
        !error.message.contains("Model unavailable"),
        "no resolution error leaked: {:?}",
        error.message
    );
    assert_eq!(
        assistant.finish,
        Some(oc_client::types::SessionMessageAssistantFinish::Error),
        "the v2 failed-turn finish marker"
    );
    // The recorded model is the canonical config key pair, not `Own/...`.
    assert_eq!(assistant.model.provider_id, "own");
    assert_eq!(assistant.model.id, "glm-5.3");

    // (4) exactly ONE engine-level stderr line, with the session id and
    // the resolved model id (the canonical config keys, not `Own/`).
    let stderr_text = wait_for(Duration::from_secs(10), || {
        let text = std::fs::read_to_string(&stderr_file).unwrap_or_default();
        async move { text.contains("[session] turn failed:").then_some(text) }.boxed()
    })
    .await;
    let failure_lines: Vec<&str> =
        stderr_text.lines().filter(|line| line.contains("[session] turn failed:")).collect();
    assert_eq!(failure_lines.len(), 1, "one stderr line per failed turn: {stderr_text}");
    let failure_line = failure_lines[0];
    assert!(failure_line.contains(session_id.as_str()), "session id: {failure_line}");
    assert!(failure_line.contains("own/glm-5.3"), "the resolved model id: {failure_line}");
    assert!(failure_line.contains("provider.transport"), "the error type: {failure_line}");

    child.kill().await.expect("kill server");
    let _ = child.wait().await;
}

/// Polls the message list until a non-null assistant error part lands.
async fn wait_for_errored_assistant(
    client: &oc_client::Client,
    session_id: &str,
) -> oc_client::types::SessionMessageAssistant {
    wait_for(Duration::from_secs(15), || {
        let client = client.clone();
        let session_id = session_id.to_owned();
        async move {
            let messages = client
                .message()
                .list(
                    &session_id,
                    &oc_client::types::SessionMessageListQuery {
                        limit: Some("50".to_owned()),
                        ..Default::default()
                    },
                )
                .await
                .expect("message list");
            messages.data.into_iter().find_map(|message| match message {
                oc_client::types::SessionMessageInfo::Assistant(assistant) => {
                    let has_error =
                        assistant.error.as_ref().is_some_and(|error| !error.message.is_empty());
                    has_error.then_some(assistant)
                }
                _ => None,
            })
        }
        .boxed()
    })
    .await
}

/// Retries a probe until it returns `Some` or the budget runs out.
async fn wait_for<T>(
    budget: Duration,
    mut probe: impl FnMut() -> futures::future::BoxFuture<'static, Option<T>>,
) -> T {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(value) = probe().await {
            return value;
        }
        assert!(Instant::now() < deadline, "retry budget exhausted");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
