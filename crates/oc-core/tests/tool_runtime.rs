//! TOOLS-EXEC regressions (ledger TOOL-A1, TOOL-A2, TOOL-C3, TOOL-C4,
//! TOOL-C5): the `opencode` tool namespace survives registration and the
//! effective names are advertised, `session_move` actually moves the
//! session through the move inbox, and the config `shell` +
//! `experimental.portable_shell_scanner` / `experimental.subagent_depth`
//! keys are consumed by the production engine assembly (setting a value
//! changes observable behavior).
//!
//! Hermetic: sandboxed engine roots under /tmp/opencode, config written
//! into the sandbox project, no network (`catalog_fetch` stays off).

use std::sync::Arc;

use oc_core::engine::{Engine, EngineOptions};
use oc_core::location::{LocationRef, ToolExecute as _};
use oc_core::session::store::CreateInput as SessionCreateInput;
use oc_schema::id::SessionId;
use serde_json::{json, Value};

/// Boots an engine over a sandboxed project, optionally writing a
/// project config document first (`<project>/.opencode/opencode.json`).
async fn engine_with_config(config: Option<Value>) -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-tools-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    if let Some(config) = config {
        std::fs::create_dir_all(dir.path().join(".opencode")).expect("config dir");
        std::fs::write(
            dir.path().join(".opencode").join("opencode.json"),
            serde_json::to_string(&config).expect("config json"),
        )
        .expect("write config");
    }
    let engine = Engine::boot(EngineOptions {
        activity_sweeper: false,
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = LocationRef::new(dir.path().to_path_buf(), None);
    (Arc::new(engine), dir, location)
}

fn model_ref() -> oc_schema::model::Ref {
    oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse("parityfake"),
        id: oc_schema::id::ModelId::parse("fake-model"),
        variant: Some(oc_schema::id::ModelVariantId::parse("default")),
    }
}

/// Creates a session pre-approved for every tool action (the engine
/// runs the real permission pipeline).
async fn allow_all_session(
    engine: &Engine,
    location: &LocationRef,
    id: &str,
    parent: Option<SessionId>,
) -> SessionId {
    let session_id = SessionId::parse(id.to_string()).unwrap_or_else(|error| SessionId(error.value));
    engine
        .create_session(
            location,
            SessionCreateInput {
                id: Some(session_id.clone()),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath::parse(
                        location.directory.to_string_lossy().into_owned(),
                    ),
                    workspace_id: None,
                }),
                title: Some("Tools runtime test".to_string()),
                agent: None,
                model: Some(model_ref()),
                metadata: None,
                permissions: Some(vec![oc_schema::permission::Rule {
                    action: "*".into(),
                    resource: "*".into(),
                    effect: oc_schema::permission::Effect::Allow,
                }]),
                parent_id: parent,
                project: None,
                version: "test".to_string(),
            },
        )
        .await
        .expect("create session");
    session_id
}

fn call_context(session: &SessionId) -> oc_core::location::ToolCallContext {
    oc_core::location::ToolCallContext {
        session_id: session.clone(),
        agent: oc_schema::id::AgentId::parse("build"),
        message_id: String::new(),
        call_id: String::new(),
    }
}

// ---------------------------------------------------------------------------
// TOOL-A1: the opencode namespace survives registration
// ---------------------------------------------------------------------------

/// The three `opencode` tools register with their namespace intact: the
/// registry rows carry `namespace = opencode`, the model-facing name is
/// the effective `opencode_*` form, and the Code Mode catalog lists the
/// qualified dotted paths.
#[tokio::test]
async fn opencode_namespace() {
    let (engine, _dir, location) = engine_with_config(None).await;
    let services = engine.location(location.clone()).await.expect("assemble");
    let tools = &services.graph.tools;

    for (bare, effective) in [
        ("session_rename", "opencode_session_rename"),
        ("session_move", "opencode_session_move"),
        ("models", "opencode_models"),
    ] {
        let registration = tools.get(bare).unwrap_or_else(|| panic!("{bare} registered"));
        assert_eq!(
            registration.namespace.as_deref(),
            Some("opencode"),
            "{bare} keeps its namespace (registry.rs must not clear it)"
        );
        assert!(
            tools.executor(bare).is_some(),
            "{bare} has a pipeline executor under its bare name"
        );
        assert_eq!(
            oc_core::tool::registry::effective_name(registration.namespace.as_deref(), bare),
            effective,
            "the advertised tool name is the effective name"
        );
    }

    // Code Mode catalog: qualified dotted paths under the namespace.
    let entries = oc_core::tool::codemode::code_mode_entries(tools);
    let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
    for path in ["opencode.session_rename", "opencode.session_move", "opencode.models"] {
        assert!(paths.contains(&path), "code mode catalog must list {path}; got {paths:?}");
    }
}

// ---------------------------------------------------------------------------
// TOOL-A2: session_move actually moves the session
// ---------------------------------------------------------------------------

/// The LLM-facing `session_move` tool admits a `move` inbox item for the
/// current session and wakes it; the drain delivers it and the session
/// row lands in the destination directory (`session.moved` durable).
#[tokio::test]
async fn session_move_tool() {
    let (engine, dir, location) = engine_with_config(None).await;
    let services = engine.location(location.clone()).await.expect("assemble");
    let session = allow_all_session(&engine, &location, "ses_tool_move", None).await;

    let destination = dir.path().join("moved-here");
    std::fs::create_dir_all(&destination).expect("destination");

    let executor = services.graph.tools.executor("session_move").expect("session_move executor");
    let outcome = executor
        .execute_with_context(
            &json!({ "directory": destination.to_string_lossy() }),
            &call_context(&session),
        )
        .await;
    let result = outcome.unwrap_or_else(|failure| {
        panic!("session_move must succeed, got: {}", failure.message())
    });
    let text = serde_json::to_string(&result.content).unwrap_or_default();
    assert!(
        text.contains("Moved session"),
        "the tool reports the move; content: {text}"
    );

    // The admitted move is delivered (drain → InboxDelivered + Moved);
    // the projector repoints the session row at the destination.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut moved = false;
    while std::time::Instant::now() < deadline {
        if let Ok(Some(row)) = services.store.get(session.as_str()).await {
            if row.directory == destination.to_string_lossy() {
                moved = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(moved, "the session row must move to the destination directory");

    let events = durable_events(&engine, &session).await;
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    assert!(types.contains(&"session.inbox.delivered"), "events: {types:?}");
    assert!(types.contains(&"session.moved"), "events: {types:?}");
}

/// Every committed durable event for a session, in seq order.
async fn durable_events(engine: &Engine, session: &SessionId) -> Vec<(String, i64, Value)> {
    use futures::StreamExt;
    let mut stream = engine.bus.log(oc_core::bus::LogInput {
        aggregate_id: session.to_string(),
        after: None,
        follow: false,
    });
    let mut collected = Vec::new();
    while let Some(item) = stream.next().await {
        if let Ok(oc_core::bus::LogItem::Event(event)) = item {
            if let Some(seq) = event.seq() {
                collected.push((event.type_.clone(), seq, event.data));
            }
        }
    }
    collected
}

// ---------------------------------------------------------------------------
// TOOL-C3: config `shell` is consumed
// ---------------------------------------------------------------------------

/// A configured shell (a fixture executable in the sandbox) is used by
/// BOTH the service-level default resolve and the shell tool's compat
/// resolve — the spawned command runs through it.
#[tokio::test]
async fn config_shell_key() {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-tools-c3-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    std::fs::create_dir_all(dir.path().join(".opencode")).expect("config dir");
    // A fixture "shell" that can only be reached through the config key:
    // the environment fallback would never resolve to this sandbox path.
    let fixture = dir.path().join("fixture-shell.sh");
    std::fs::write(&fixture, "#!/bin/sh\nexec /bin/sh \"$@\"\n").expect("fixture");
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&fixture).expect("stat").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fixture, permissions).expect("chmod");
    }
    std::fs::write(
        dir.path().join(".opencode").join("opencode.json"),
        serde_json::to_string(&json!({ "shell": fixture.to_string_lossy() })).expect("json"),
    )
    .expect("write config");

    let engine = Engine::boot(EngineOptions {
        activity_sweeper: false,
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = LocationRef::new(dir.path().to_path_buf(), None);
    let services = engine.location(location.clone()).await.expect("assemble");

    // Service path (`Shell.create` with no explicit shell): the config
    // key is the default resolve input.
    let info = services
        .shell
        .create(
            oc_schema::shell::CreateInput {
                command: "printf via-config".into(),
                cwd: Some(dir.path().to_string_lossy().to_string()),
                timeout: Some(10_000),
                metadata: None,
            },
            None,
            None,
            None,
        )
        .await
        .expect("service create");
    assert_eq!(
        info.shell,
        fixture.to_string_lossy().to_string(),
        "config `shell` must be the service-level default shell"
    );

    // Tool path (compat priority): the shell tool spawns through the
    // configured shell as well.
    let session = allow_all_session(&engine, &location, "ses_tool_c3", None).await;
    let executor = services.graph.tools.executor("shell").expect("shell executor");
    let outcome = executor
        .execute_with_context(
            &json!({ "command": "printf tool-path", "workdir": dir.path().to_string_lossy() }),
            &call_context(&session),
        )
        .await
        .expect("shell tool runs");
    let output = serde_json::to_string(&outcome.content).unwrap_or_default();
    assert!(output.contains("tool-path"), "tool output: {output}");
    let shell_id = outcome.metadata.as_ref().and_then(|metadata| {
        metadata.get("shellID").and_then(Value::as_str).map(str::to_string)
    });
    let shell_id = shell_id.expect("shellID metadata");
    let shell_id =
        oc_schema::id::ShellId::parse(shell_id).unwrap_or_else(|error| oc_schema::id::ShellId(error.value));
    let tool_info = services.shell.get(&shell_id).expect("shell info");
    assert_eq!(
        tool_info.shell,
        fixture.to_string_lossy().to_string(),
        "config `shell` must drive the tool's compat resolve too"
    );
}

// ---------------------------------------------------------------------------
// TOOL-C4: experimental.portable_shell_scanner is consumed
// ---------------------------------------------------------------------------

/// With the experimental flag on, the shell tool parses commands through
/// the portable scanner — a command the portable scanner refuses as
/// opaque fails the tool; with the flag off (default) the legacy
/// tree-sitter scanner runs the same command fine.
#[tokio::test]
async fn portable_scanner_flag() {
    // Flag off (default): `alias x='evil'` parses (legacy) and runs.
    let (engine, dir, location) = engine_with_config(None).await;
    let services = engine.location(location.clone()).await.expect("assemble");
    let session = allow_all_session(&engine, &location, "ses_tool_c4_off", None).await;
    let executor = services.graph.tools.executor("shell").expect("shell executor");
    let outcome = executor
        .execute_with_context(
            &json!({ "command": "alias x='evil'", "workdir": dir.path().to_string_lossy() }),
            &call_context(&session),
        )
        .await;
    assert!(
        outcome.is_ok(),
        "legacy scanner accepts the command: {}",
        outcome.err().map(|failure| failure.message().to_string()).unwrap_or_default()
    );

    // Flag on: the portable scanner marks `alias x='evil'` opaque
    // (dynamic execution) and the tool fails before spawning.
    let (engine, dir, location) =
        engine_with_config(Some(json!({ "experimental": { "portable_shell_scanner": true } })))
            .await;
    let services = engine.location(location.clone()).await.expect("assemble");
    let session = allow_all_session(&engine, &location, "ses_tool_c4_on", None).await;
    let executor = services.graph.tools.executor("shell").expect("shell executor");
    let failure = executor
        .execute_with_context(
            &json!({ "command": "alias x='evil'", "workdir": dir.path().to_string_lossy() }),
            &call_context(&session),
        )
        .await
        .expect_err("portable scanner refuses the opaque command");
    assert!(
        failure.message().contains("Portable shell scanner cannot analyze command"),
        "the portable scanner error surfaces: {}",
        failure.message()
    );
}

// ---------------------------------------------------------------------------
// TOOL-C5: experimental.subagent_depth is consumed
// ---------------------------------------------------------------------------

/// A child session (depth 1) cannot spawn a subagent at the default
/// depth limit of 1; raising `experimental.subagent_depth` to 2 lets the
/// depth gate pass (the next failure is agent resolution, not depth).
#[tokio::test]
async fn subagent_depth() {
    async fn run(config: Option<Value>, agent: &str) -> String {
        let (engine, _dir, location) = engine_with_config(config).await;
        let services = engine.location(location.clone()).await.expect("assemble");
        let parent = allow_all_session(&engine, &location, "ses_c5_parent", None).await;
        let child =
            allow_all_session(&engine, &location, "ses_c5_child", Some(parent.clone())).await;
        let executor = services.graph.tools.executor("subagent").expect("subagent executor");
        executor
            .execute_with_context(
                &json!({ "agent": agent, "description": "depth probe", "prompt": "noop" }),
                &call_context(&child),
            )
            .await
            .err()
            .map(|failure| failure.message().to_string())
            .unwrap_or_default()
    }

    let default = run(None, "nope").await;
    assert!(
        default.starts_with("Subagent depth limit reached (1)"),
        "default depth limit is 1; got: {default}"
    );

    let raised = run(Some(json!({ "experimental": { "subagent_depth": 2 } })), "nope").await;
    assert!(
        raised.starts_with("Unknown agent: nope"),
        "depth 2 passes the gate and reaches agent resolution; got: {raised}"
    );
}
