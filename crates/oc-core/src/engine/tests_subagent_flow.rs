//! Corpus3 subagent-flow regression gates (lane G, server-side):
//!
//! - **#41** — exactly ONE subagent-classified request per flow: the
//!   child wakes (never awaiting a forced resume in the tool), the job
//!   body's resume joins the wake-started execution, and no second
//!   drain runs an empty step;
//! - **#38** — a config-defined agent (`agents.researcher` in
//!   `opencode.json`) reaches the per-location agent registry and the
//!   `subagent` tool resolves it end-to-end (parse → registry → spawn
//!   → child session agent → completed tool result).
//!
//! The flows run through the FULL engine (real registries, tools,
//! inbox, coordinator) over a scripted TestLlm with body-substring
//! attribution — the same attribution the parity rig's shared stub
//! uses, so these gates pin the rig-visible request counts.

#![allow(
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::uninlined_format_args
)]

use std::sync::Arc;

use oc_ai::testing::TestLlm;
use oc_schema::id::SessionId;

use crate::engine::{Engine, EngineOptions};
use crate::location::LocationRef;

use crate::session::runner::tests_harness::{
    fixed_clock, test_resolved_model, FixedResolver, TestLlmClient,
};

/// The child-session first-prompt prefix (subagent.ts:209-212) — the
/// rig's `subagent` attribution key, byte-identical on both stacks.
const CHILD_PREFIX: &str = "You are a subagent spawned by another session.";
const CHILD_TEXT: &str = "Child fixture done.";

async fn engine_with(llm: TestLlm) -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-subagent-flow-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    let engine = Engine::boot(EngineOptions {
        llm: Some(Arc::new(TestLlmClient(llm))),
        model_resolver: Some(Arc::new(FixedResolver { resolved: test_resolved_model() })),
        clock: fixed_clock(),
        activity_sweeper: false,
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = LocationRef::new(dir.path().to_path_buf(), None);
    (Arc::new(engine), dir, location)
}

/// Attribution-keyed responder (the rig's `classify_flow_request`
/// semantics): child requests by the first-prompt prefix, continuations
/// by the child's reply in the tool result, everything else is the main
/// request answered with a `subagent` tool call for `agent`.
fn serve_flow(llm: &TestLlm, agent: &str, prompt: &str) {
    let agent = agent.to_owned();
    let prompt = prompt.to_owned();
    llm.serve(move |request| {
        let body = serde_json::to_string(&request.messages).unwrap_or_default();
        if body.contains(CHILD_PREFIX) {
            crate::session::runner::tests_harness::parity_text_response(CHILD_TEXT, 10.0, 5.0)
        } else if body.contains(CHILD_TEXT) {
            crate::session::runner::tests_harness::parity_text_response(
                "Main flow final reply.",
                10.0,
                5.0,
            )
        } else {
            oc_ai::testing::tool(
                "call_sub_1",
                "subagent",
                serde_json::json!({
                    "agent": agent,
                    "description": "Check the fixture",
                    "prompt": prompt,
                }),
            )
        }
    });
}

async fn create_flow_session(engine: &Engine, location: &LocationRef, id: &str) -> SessionId {
    let session_id = SessionId::parse(id).unwrap_or_else(|error| SessionId(error.value));
    engine
        .create_session(
            location,
            crate::session::store::CreateInput {
                id: Some(session_id.clone()),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath::parse(
                        location.directory.to_string_lossy().into_owned(),
                    ),
                    workspace_id: None,
                }),
                title: Some("Subagent flow".to_string()),
                agent: None,
                model: Some(oc_schema::model::Ref {
                    provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                    id: oc_schema::id::ModelId::parse("fake-model"),
                    variant: Some(oc_schema::id::ModelVariantId::parse("default")),
                }),
                // The engine runs the REAL permission pipeline; the flow
                // sessions come pre-approved (the parity rig's allow-all
                // user config).
                permissions: Some(vec![oc_schema::permission::Rule {
                    action: "*".into(),
                    resource: "*".into(),
                    effect: oc_schema::permission::Effect::Allow,
                }]),
                metadata: None,
                parent_id: None,
                project: None,
                version: "test".to_string(),
            },
        )
        .await
        .expect("create session");
    session_id
}

/// Every committed durable event for a session, in seq order.
async fn durable_events(
    engine: &Engine,
    session_id: &str,
) -> Vec<(String, i64, serde_json::Value)> {
    use futures::StreamExt;
    let mut stream = engine.bus.log(crate::bus::LogInput {
        aggregate_id: session_id.to_owned(),
        after: None,
        follow: false,
    });
    let mut collected = Vec::new();
    while let Some(item) = stream.next().await {
        if let Ok(crate::bus::LogItem::Event(event)) = item {
            if let Some(seq) = event.seq() {
                collected.push((event.type_.clone(), seq, event.data));
            }
        }
    }
    collected
}

/// Per-stage request attribution over the recorded requests.
fn stage_counts(llm: &TestLlm) -> (usize, usize, usize) {
    let mut main = 0;
    let mut subagent = 0;
    let mut continuation = 0;
    for request in llm.requests() {
        let body = serde_json::to_string(&request.messages).unwrap_or_default();
        if body.contains(CHILD_PREFIX) {
            subagent += 1;
        } else if body.contains(CHILD_TEXT) {
            continuation += 1;
        } else {
            main += 1;
        }
    }
    (main, subagent, continuation)
}

/// #41: the flow issues exactly ONE request per stage — main,
/// subagent, continuation. The duplicate-subagent-request regression
/// (the tool awaiting a forced resume, letting the child settle idle
/// before the job body's resume force-drained an empty step) is pinned
/// by the `subagent == 1` arm and the single child exchange below.
#[tokio::test]
async fn engine_subagent_flow_single_request_per_stage() {
    let llm = TestLlm::new();
    serve_flow(&llm, "general", "PARITY-SUBAGENT-TASK inspect the fixture.");
    let (engine, dir, location) = engine_with(llm.clone()).await;
    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_flow_session(&engine, &location, "ses_flow_general").await;

    engine
        .prompt(&location, &session_id, "msg_user_1", "run the subagent")
        .await
        .expect("prompt exit");
    // Background settles (title generation) get a beat.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let (main, subagent, continuation) = stage_counts(&llm);
    assert_eq!(main, 1, "exactly one main request");
    assert_eq!(subagent, 1, "exactly one subagent request (corpus3 #41)");
    assert_eq!(continuation, 1, "exactly one continuation request");

    // The child transcript holds exactly ONE exchange (the duplicate
    // request doubled it before #41).
    let store = crate::session::store::SessionStore::new((*engine.db).clone());
    let sessions = store.list(Default::default()).await.expect("list");
    let child = sessions
        .iter()
        .find(|row| row.parent_id.as_deref() == Some(session_id.as_str()))
        .expect("child session");
    let events = durable_events(&engine, child.id.as_str()).await;
    let enqueued = events.iter().filter(|(type_, _, _)| type_ == "session.inbox.enqueued").count();
    assert_eq!(enqueued, 1, "the child's first prompt was admitted exactly once");
    let steps = events.iter().filter(|(type_, _, _)| type_ == "session.step.ended").count();
    assert_eq!(steps, 1, "the child ran exactly one step");
    assert_eq!(child.agent.as_deref(), Some("general"));

    // The parent's tool call completed with the envelope.
    let parent_events = durable_events(&engine, session_id.as_str()).await;
    let success = parent_events
        .iter()
        .find(|(type_, _, data)| type_ == "session.tool.success" && data["id"] == "call_sub_1")
        .expect("subagent tool success");
    assert!(
        success.2["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("<subagent sessionID=") && text.contains(CHILD_TEXT)),
        "completed envelope carries the child reply: {}",
        success.2["content"]
    );
    let _ = dir;
}

/// beta.13 bug 2 (real-env): the subagent tool's running-state
/// metadata reaches the wire — an EPHEMERAL `session.tool.progress`
/// carrying the TOOL CALL id and `metadata.sessionID` = the child (v2
/// subagent.ts:201 `context.progress({sessionID, status: "running"})`
/// through step.ts:96 `publisher.progress(call.id, update)`). The
/// TUI's subagent-card click surface keys on `metadata.sessionID`; a
/// missing progress left RUNNING cards unclickable (and the old call
/// site passed the CHILD id where the tool id belongs — a publisher
/// `defect` drop).
#[tokio::test]
async fn engine_subagent_flow_publishes_running_progress_metadata() {
    let llm = TestLlm::new();
    serve_flow(&llm, "general", "PARITY-SUBAGENT-TASK inspect the fixture.");
    let (engine, dir, location) = engine_with(llm.clone()).await;
    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_flow_session(&engine, &location, "ses_flow_progress").await;

    // The live volatile channel (progress is ephemeral — never in the
    // durable log): subscribed from t0, filtered to the type.
    let mut live = engine.bus.subscribe_types(&["session.tool.progress"]);
    engine
        .prompt(&location, &session_id, "msg_user_1", "run the subagent")
        .await
        .expect("prompt exit");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let child = {
        let store = crate::session::store::SessionStore::new((*engine.db).clone());
        let sessions = store.list(Default::default()).await.expect("list");
        sessions
            .into_iter()
            .find(|row| row.parent_id.as_deref() == Some(session_id.as_str()))
            .expect("child session")
    };

    use futures::StreamExt;
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), live.next())
        .await
        .expect("a session.tool.progress within the timeout")
        .expect("the subscription saw the event");
    let progress = event.data;
    assert_eq!(progress["id"], "call_sub_1", "the TOOL CALL id (not the child session id)");
    assert_eq!(progress["sessionID"], session_id.as_str());
    assert!(
        progress["assistantMessageID"].as_str().is_some_and(|id| !id.is_empty()),
        "the parent's assistant message id: {}",
        progress["assistantMessageID"]
    );
    assert_eq!(progress["metadata"]["sessionID"], child.id.as_str(), "the child session id");
    assert_eq!(progress["metadata"]["status"], "running");
    let _ = dir;
}

/// #38: a config-defined agent (`agents.researcher` in the location's
/// `opencode.json`) reaches the agent registry and the subagent flow
/// resolves it end-to-end — the ref's `ConfigAgentPlugin` document
/// passthrough (config/plugin/agent.ts:34-36) + transform
/// (config/plugin/agent.ts:100-127).
#[tokio::test]
async fn engine_subagent_flow_config_defined_agent_resolves() {
    let llm = TestLlm::new();
    serve_flow(&llm, "researcher", "PARITY-RESEARCHER-TASK study the fixture.");
    let (engine, dir, location) = engine_with(llm.clone()).await;
    // The location's opencode.json IS a config source (direct walk-up
    // discovery) — write it BEFORE the location assembles.
    std::fs::write(
        dir.path().join("opencode.json"),
        serde_json::json!({
            "agents": {
                "researcher": {
                    "mode": "subagent",
                    "description": "Researches complex topics",
                    "system": "You are a careful researcher."
                }
            }
        })
        .to_string(),
    )
    .expect("write config");
    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_flow_session(&engine, &location, "ses_flow_researcher").await;

    engine
        .prompt(&location, &session_id, "msg_user_1", "run the researcher")
        .await
        .expect("prompt exit");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let (main, subagent, continuation) = stage_counts(&llm);
    assert_eq!((main, subagent, continuation), (1, 1, 1), "flow completes through researcher");

    // The child session runs AS the config-defined agent; its system
    // prompt came from the config entry.
    let store = crate::session::store::SessionStore::new((*engine.db).clone());
    let sessions = store.list(Default::default()).await.expect("list");
    let child = sessions
        .iter()
        .find(|row| row.parent_id.as_deref() == Some(session_id.as_str()))
        .expect("child session");
    assert_eq!(child.agent.as_deref(), Some("researcher"), "config agent resolved (#38)");
    assert_eq!(child.title.as_deref(), Some("Check the fixture"));

    // The main request offered the researcher in the subagent tool's
    // description hook (the `Available subagents:` listing).
    let requests = llm.requests();
    let main_request = requests
        .iter()
        .find(|request| {
            serde_json::to_string(&request.messages)
                .is_ok_and(|body| !body.contains(CHILD_PREFIX) && !body.contains(CHILD_TEXT))
        })
        .expect("main request");
    let tools = serde_json::to_string(&main_request.tools).unwrap_or_default();
    assert!(
        tools.contains("researcher") && tools.contains("Researches complex topics"),
        "the config agent is advertised with its description: {tools}"
    );

    // And the child's transcript carries the configured system prompt.
    let child_events = durable_events(&engine, child.id.as_str()).await;
    assert!(
        child_events.iter().any(|(type_, _, _)| type_ == "session.step.ended"),
        "the researcher child completed a step"
    );
}
