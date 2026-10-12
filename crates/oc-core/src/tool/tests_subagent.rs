//! Ported `tool-subagent.test.ts` cases (Wave D). Every model-facing
//! string is asserted byte-exact against v2.

#![allow(clippy::uninlined_format_args)]

use std::sync::Arc;
use std::time::Duration;

use oc_schema::agent::{Info as AgentInfo, Mode as AgentMode};
use oc_schema::id::{AgentId, AgentName, MessageId, SessionId};
use oc_schema::model;
use oc_schema::permission::{Effect, Rule};
use oc_schema::session_inbox::Info as InboxInfo;
use serde_json::json;

use crate::bus::{Bus, BusOptions, PublishOptions};
use crate::database::Database;
use crate::location::{ToolCallContext, ToolRunFailure, ToolRunResult};
use crate::permission::{BroadcastEventSink, MemorySavedGrantStore, Permission, RuleResolver};
use crate::session::inbox::SessionInbox;
use crate::session::projector::install_projectors;
use crate::session::store::{CreateInput, CreateOutcome, ListInput, SessionStore};
use crate::session::subagent_job::SubagentJob;
use crate::subagent::{ChildExecution, JobControl, MemoryJobs, ModelEntry, ModelSource, NO_TEXT};

use super::subagent::{SubagentDeps, SubagentInput, SubagentTool};

const CHILD_TEXT: &str = "child final response";

fn completed_output(session_id: &str) -> String {
    format!("<subagent sessionID=\"{session_id}\" state=\"completed\">\n{CHILD_TEXT}\n</subagent>")
}

fn model_ref(id: &str) -> model::Ref {
    model::Ref {
        id: oc_schema::id::ModelId::parse(id),
        provider_id: oc_schema::id::ProviderId::parse("test"),
        variant: Some(oc_schema::id::ModelVariantId::parse("default")),
    }
}

fn override_model() -> model::Ref {
    model::Ref {
        id: oc_schema::id::ModelId::parse("override"),
        provider_id: oc_schema::id::ProviderId::parse("test"),
        variant: Some(oc_schema::id::ModelVariantId::parse("fast")),
    }
}

// -----------------------------------------------------------------------
// fixtures
// -----------------------------------------------------------------------

struct Harness {
    tool: SubagentTool,
    store: SessionStore,
    inbox: SessionInbox,
    bus: Bus,
    jobs: Arc<MemoryJobs>,
    depth: Arc<std::sync::atomic::AtomicU64>,
    progress: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
}

async fn harness() -> Harness {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-subagent-")
        .tempdir_in("/tmp/opencode")
        .expect("temp dir");
    let db_path = dir.path().join("opencode.db");
    let db = Database::open_file(db_path.clone()).await.expect("db");
    std::mem::forget(dir);
    let bus = Bus::new(db.clone(), BusOptions::deterministic());
    install_projectors(&bus);
    let store = SessionStore::new(db.clone());
    let inbox = SessionInbox::new(db.clone(), bus.clone());
    let jobs = Arc::new(MemoryJobs::new());
    let execution: Arc<dyn ChildExecution> = Arc::new(FakeExecution);
    let subagents = Arc::new(SubagentJob::new(
        store.clone(),
        inbox.clone(),
        Arc::clone(&execution),
        Arc::clone(&jobs) as Arc<dyn JobControl>,
    ));
    let depth = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let depth_reader = {
        let depth = Arc::clone(&depth);
        Arc::new(move || depth.load(std::sync::atomic::Ordering::SeqCst))
            as Arc<dyn Fn() -> u64 + Send + Sync>
    };
    let progress: Arc<parking_lot::Mutex<Vec<serde_json::Value>>> = Arc::default();
    let progress_sink = Arc::clone(&progress);
    let agents: Arc<dyn Fn() -> Vec<AgentInfo> + Send + Sync> = Arc::new(agents);
    let models: Arc<dyn ModelSource> = Arc::new(catalog_models());
    let deps = SubagentDeps {
        store: store.clone(),
        inbox: inbox.clone(),
        execution,
        jobs: Arc::clone(&jobs) as Arc<dyn JobControl>,
        subagents,
        models,
        permission: permission(),
        depth: depth_reader,
        agents,
        progress: Some(Arc::new(move |_session, metadata| {
            progress_sink.lock().push(metadata);
        })),
        interrupt: None,
    };
    install_harness(db.clone(), bus.clone(), db_path);
    Harness { tool: SubagentTool::new(deps), store, inbox, bus, jobs, depth, progress }
}

/// The caller agent carries v2's default allow-all grant (the TS fixture
/// pushes `{ action: "*", resource: "*", effect: "allow" }` onto the
/// calling agent).
#[derive(Default)]
struct FixedRules;

#[async_trait::async_trait]
impl RuleResolver for FixedRules {
    async fn resolve(
        &self,
        _session_id: &SessionId,
        _agent: Option<&AgentId>,
    ) -> Result<
        (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
        crate::permission::PermissionError,
    > {
        Ok((
            Some(vec![Rule { action: "*".into(), resource: "*".into(), effect: Effect::Allow }]),
            None,
        ))
    }
}

fn permission() -> Permission {
    Permission::new(
        Arc::new(oc_util::determinism::SequentialIdGen::new()),
        Arc::new(oc_util::determinism::FixedClock::epoch()),
        Arc::new(BroadcastEventSink::new(16)),
        Arc::new(MemorySavedGrantStore::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
        )),
        Arc::new(FixedRules),
        oc_schema::id::ProjectId::global(),
        None,
    )
}

struct FakeModels {
    entries: Vec<ModelEntry>,
}

fn catalog_models() -> FakeModels {
    FakeModels {
        entries: vec![
            ModelEntry {
                provider_id: "test".into(),
                id: "child".into(),
                variants: vec!["default".into()],
            },
            ModelEntry {
                provider_id: "test".into(),
                id: "parent".into(),
                variants: vec!["default".into()],
            },
            ModelEntry {
                provider_id: "test".into(),
                id: "override".into(),
                variants: vec!["fast".into()],
            },
            ModelEntry { provider_id: "test".into(), id: "plain".into(), variants: vec![] },
        ],
    }
}

impl ModelSource for FakeModels {
    fn model(&self, provider_id: &str, model_id: &str) -> Option<ModelEntry> {
        self.entries
            .iter()
            .find(|entry| entry.provider_id == provider_id && entry.id == model_id)
            .cloned()
    }
}

/// Fake child execution: `resume` publishes one completed assistant step
/// carrying the canned child text (the TS test `executionNode.complete`),
/// or fails for sessions titled with "fail".
struct FakeExecution;

#[async_trait::async_trait]
impl ChildExecution for FakeExecution {
    async fn resume(&self, session_id: &str) -> Result<(), String> {
        // The fake reads the session title through the shared test DB —
        // reached via the task-local HARNESS_DB installed by `run`.
        let title = current_title(session_id);
        if let Some(title) = title {
            if title.contains("fail") {
                return Err("No model is available for session".to_string());
            }
        }
        let session = SessionId::parse(session_id.to_string()).expect("session id");
        let assistant = MessageId::new();
        let model = model_ref("child");
        let bus = current_bus().expect("bus");
        publish(
            &bus,
            "session.step.started",
            json!({
                "sessionID": session, "assistantMessageID": assistant, "agent": "reviewer",
                "model": model, "started": 0,
            }),
        )
        .await;
        publish(
            &bus,
            "session.text.started",
            json!({ "sessionID": session, "assistantMessageID": assistant, "ordinal": 0 }),
        )
        .await;
        publish(
            &bus,
            "session.text.ended",
            json!({ "sessionID": session, "assistantMessageID": assistant, "ordinal": 0, "text": CHILD_TEXT }),
        )
        .await;
        publish(
            &bus,
            "session.step.ended",
            json!({
                "sessionID": session, "assistantMessageID": assistant, "finish": "stop",
                "cost": 0.0, "tokens": { "input": 0, "output": 0, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            }),
        )
        .await;
        Ok(())
    }

    fn wake(&self, _session_id: &str) {}

    async fn interrupt(&self, _session_id: &str) {}
}

thread_local! {
    static HARNESS_CTX: std::cell::RefCell<Option<(Bus, std::path::PathBuf)>> =
        const { std::cell::RefCell::new(None) };
}

fn install_harness(_db: Database, bus: Bus, db_path: std::path::PathBuf) {
    HARNESS_CTX.with(|slot| *slot.borrow_mut() = Some((bus, db_path)));
}

fn current_bus() -> Option<Bus> {
    HARNESS_CTX.with(|slot| slot.borrow().as_ref().map(|(bus, _)| bus.clone()))
}

/// Sync title read over a second sqlite connection (the async database
/// cannot block inside the current-thread test runtime).
fn current_title(session_id: &str) -> Option<String> {
    HARNESS_CTX.with(|slot| {
        let guard = slot.borrow();
        let path = guard.as_ref().map(|(_, path)| path.clone())?;
        let connection = rusqlite::Connection::open(path).ok()?;
        connection
            .query_row(
                "SELECT title FROM session_v2 WHERE id = ?1",
                rusqlite::params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
    })
}

async fn publish(bus: &Bus, type_: &str, data: serde_json::Value) {
    let def = crate::bus::registry::by_type_latest(type_)
        .cloned()
        .unwrap_or_else(|| panic!("definition {type_}"));
    let _ = bus.publish(&def, data, PublishOptions::default()).await;
}

fn agent(id: &str, mode: AgentMode, configured: Option<model::Ref>) -> AgentInfo {
    let mut info = AgentInfo::default_for(AgentId::parse(id));
    info.name = AgentName(id.to_string());
    info.mode = mode;
    info.model = configured;
    info
}

fn agents() -> Vec<AgentInfo> {
    let allow_all = oc_schema::permission::Ruleset::from(vec![Rule {
        action: "*".into(),
        resource: "*".into(),
        effect: Effect::Allow,
    }]);
    vec![
        AgentInfo { permissions: allow_all.clone(), ..agent("build", AgentMode::Primary, None) },
        AgentInfo {
            permissions: allow_all.clone(),
            ..agent("reviewer", AgentMode::Subagent, Some(model_ref("child")))
        },
        AgentInfo { permissions: allow_all, ..agent("fallback", AgentMode::Subagent, None) },
        agent("primary", AgentMode::Primary, None),
        AgentInfo { hidden: true, ..agent("hidden", AgentMode::Subagent, None) },
    ]
}

async fn create_session(
    harness: &Harness,
    parent: Option<&str>,
    title: &str,
    model: Option<model::Ref>,
    agent: Option<&str>,
) -> SessionId {
    let parent_row = match parent {
        Some(parent) => Some(harness.store.get(parent).await.unwrap().expect("parent")),
        None => None,
    };
    let directory = parent_row
        .as_ref()
        .map(|row| row.directory.clone())
        .unwrap_or_else(|| "/tmp/opencode".to_string());
    let outcome = harness
        .store
        .create(
            &harness.bus,
            CreateInput {
                id: None,
                parent_id: parent.map(|value| SessionId::parse(value).expect("parent id")),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath(directory),
                    workspace_id: None,
                }),
                title: Some(title.to_string()),
                agent: agent.map(str::to_string),
                model,
                metadata: None,
                permissions: None,
                project: None,
                version: "2".to_string(),
            },
        )
        .await
        .expect("create session");
    match outcome {
        CreateOutcome::Created(row) | CreateOutcome::Existing(row) => {
            SessionId::parse(row.id).unwrap_or_else(|err| SessionId(err.value))
        }
    }
}

async fn run(
    harness: &Harness,
    session: &SessionId,
    input: serde_json::Value,
) -> Result<ToolRunResult, ToolRunFailure> {
    let context = ToolCallContext {
        session_id: session.clone(),
        agent: AgentId::parse("build"),
        message_id: "msg_x".to_string(),
        call_id: "call_x".to_string(),
    };
    let decoded = SubagentInput::decode(&input).expect("decode");
    harness.tool.execute(&decoded, &context).await.map(super::subagent::settle)
}

fn metadata_session(result: &ToolRunResult) -> String {
    result
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("sessionID"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

async fn user_texts(harness: &Harness, session: &str) -> Vec<String> {
    harness
        .inbox
        .list(session)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|item| match item {
            InboxInfo::User { payload, .. } => Some(payload.text),
            _ => None,
        })
        .collect()
}

async fn child_count(harness: &Harness, parent: &str) -> usize {
    harness
        .store
        .list(ListInput { parent_id: Some(Some(parent.to_string())), ..ListInput::default() })
        .await
        .unwrap()
        .len()
}

async fn wait_for_synthetic(harness: &Harness, parent: &str) -> Vec<InboxInfo> {
    for _ in 0..400 {
        let found: Vec<InboxInfo> = harness
            .inbox
            .list(parent)
            .await
            .unwrap()
            .into_iter()
            .filter(|item| matches!(item, InboxInfo::Synthetic { .. }))
            .collect();
        if !found.is_empty() {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no synthetic completion arrived");
}

/// Re-installs the thread-local harness context (tests may run after the
/// constructor installed it; kept for explicitness at call sites).
fn install_harness_setup(_harness: &Harness) {}

// -----------------------------------------------------------------------
// cases
// -----------------------------------------------------------------------

#[tokio::test]
async fn subagent_tool_rejects_primary_and_unknown_agents() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;

    let error = run(
        &harness,
        &parent,
        json!({ "agent": "primary", "description": "primary", "prompt": "should fail" }),
    )
    .await
    .expect_err("primary rejected");
    match error {
        ToolRunFailure::Error { message, .. } => {
            assert_eq!(message, "Agent primary cannot run as a subagent");
        }
        other => panic!("expected error failure, got {other:?}"),
    }

    let error = run(
        &harness,
        &parent,
        json!({ "agent": "ghost", "description": "ghost", "prompt": "nope" }),
    )
    .await
    .expect_err("unknown rejected");
    match error {
        ToolRunFailure::Error { message, .. } => assert_eq!(message, "Unknown agent: ghost"),
        other => panic!("expected error failure, got {other:?}"),
    }
    assert_eq!(child_count(&harness, parent.as_str()).await, 0);
}

#[tokio::test]
async fn subagent_tool_prevents_nested_subagents_by_default() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let root = create_session(&harness, None, "root", None, None).await;
    let parent =
        create_session(&harness, Some(root.as_str()), "parent", Some(model_ref("parent")), None)
            .await;

    let error = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "nested", "prompt": "should fail" }),
    )
    .await
    .expect_err("depth guard");
    match error {
        ToolRunFailure::Error { message, .. } => assert_eq!(
            message,
            "Subagent depth limit reached (1). Increase \"experimental.subagent_depth\" to allow nested subagents."
        ),
        other => panic!("expected error failure, got {other:?}"),
    }
    assert_eq!(child_count(&harness, parent.as_str()).await, 0);

    // Configured depth 2 allows one nested level.
    harness.depth.store(2, std::sync::atomic::Ordering::SeqCst);
    let settled = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "nested", "prompt": "should run" }),
    )
    .await
    .expect("nested allowed");
    let child = metadata_session(&settled);
    assert!(!child.is_empty());
    assert_eq!(
        settled.metadata.as_ref().unwrap().get("status").and_then(serde_json::Value::as_str),
        Some("completed")
    );
    let row = harness.store.get(&child).await.unwrap().expect("child row");
    assert_eq!(row.parent_id.as_deref(), Some(parent.as_str()));
}

#[tokio::test]
async fn subagent_tool_foreground_run_and_completed_wrapper() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;

    let settled = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "review", "prompt": "review this", "model": "", "sessionID": "" }),
    )
    .await
    .expect("foreground run");

    let child = metadata_session(&settled);
    assert_eq!(
        settled.content,
        vec![oc_schema::tool::Content::Text { text: completed_output(&child) }]
    );
    assert_eq!(
        serde_json::to_value(settled.metadata.as_ref().unwrap()).unwrap(),
        json!({ "sessionID": child, "status": "completed" })
    );
    // progress({sessionID: child.id, status: "running"})
    let progress = harness.progress.lock().clone();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0], json!({ "sessionID": child, "status": "running" }));
    // Child identity: reviewer + agent-configured model, same directory.
    let row = harness.store.get(&child).await.unwrap().expect("child");
    assert_eq!(row.parent_id.as_deref(), Some(parent.as_str()));
    assert_eq!(row.agent.as_deref(), Some("reviewer"));
    assert_eq!(row.directory, "/tmp/opencode");
    let decoded = crate::subagent::decode_row_model(&row).expect("model");
    assert_eq!(decoded.id.0, "child");
    // First-prompt prefix EXACT.
    assert_eq!(
        user_texts(&harness, &child).await,
        vec!["You are a subagent spawned by another session.\nreview this".to_string()]
    );

    // Fallback agent inherits the parent's model.
    let fallback = run(
        &harness,
        &parent,
        json!({ "agent": "fallback", "description": "fallback", "prompt": "fallback" }),
    )
    .await
    .expect("fallback run");
    let fallback_child = metadata_session(&fallback);
    let row = harness.store.get(&fallback_child).await.unwrap().expect("fallback child");
    let decoded = crate::subagent::decode_row_model(&row).expect("model");
    assert_eq!(decoded.id.0, "parent");
}

#[tokio::test]
async fn subagent_tool_continues_existing_child() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;

    let first = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "review", "prompt": "review this" }),
    )
    .await
    .expect("first run");
    let child = metadata_session(&first);
    let second = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "follow up", "prompt": "continue this", "sessionID": child, "model": "" }),
    )
    .await
    .expect("second run");

    assert_eq!(metadata_session(&second), child);
    assert_eq!(child_count(&harness, parent.as_str()).await, 1, "continuation reuses the child");
    let row = harness.store.get(&child).await.unwrap().expect("child");
    assert_eq!(row.title.as_deref(), Some("review"));
    assert_eq!(
        user_texts(&harness, &child).await,
        vec![
            "You are a subagent spawned by another session.\nreview this".to_string(),
            "continue this".to_string(),
        ]
    );
    assert_eq!(
        second.content,
        vec![oc_schema::tool::Content::Text { text: completed_output(&child) }]
    );
}

#[tokio::test]
async fn subagent_tool_background_steers_running_child() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;
    // Pre-existing running child job: block() would wait forever, so the
    // background path must not block on it.
    let child = create_session(
        &harness,
        Some(parent.as_str()),
        "review",
        Some(model_ref("child")),
        Some("reviewer"),
    )
    .await;
    let jobs = Arc::clone(&harness.jobs);
    let blocked_child = child.clone();
    tokio::spawn(async move {
        jobs.start(crate::subagent::JobStart {
            id: blocked_child.as_str().to_string(),
            job_type: "subagent".to_string(),
            title: Some("review".to_string()),
            recovery: None,
            run: Box::pin(std::future::pending()),
        })
        .await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let result = run(
        &harness,
        &parent,
        json!({
            "agent": "reviewer", "description": "follow up", "prompt": "continue while running",
            "sessionID": child, "background": true,
        }),
    )
    .await
    .expect("background steering");

    assert_eq!(
        serde_json::to_value(result.metadata.as_ref().unwrap()).unwrap(),
        json!({ "sessionID": child.as_str(), "status": "running" })
    );
    assert_eq!(
        user_texts(&harness, child.as_str()).await,
        vec!["continue while running".to_string()]
    );
    let info = harness.jobs.get(child.as_str()).await.expect("job");
    assert_eq!(info.status, crate::subagent::JobStatus::Running);
}

#[tokio::test]
async fn subagent_tool_rejects_unrelated_children_and_switches_agents() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;
    let other_parent =
        create_session(&harness, None, "other", Some(model_ref("parent")), None).await;
    let unrelated = create_session(
        &harness,
        Some(other_parent.as_str()),
        "other review",
        None,
        Some("reviewer"),
    )
    .await;
    let switched = create_session(
        &harness,
        Some(parent.as_str()),
        "fallback review",
        Some(model_ref("parent")),
        Some("fallback"),
    )
    .await;

    let missing = SessionId::new();
    let error = run(&harness, &parent, json!({ "agent": "reviewer", "description": "follow up", "prompt": "continue", "sessionID": missing }))
        .await
        .expect_err("missing");
    match error {
        ToolRunFailure::Error { message, .. } => {
            assert_eq!(message, format!("Subagent session not found: {missing}"));
        }
        other => panic!("expected error failure, got {other:?}"),
    }

    let error = run(&harness, &parent, json!({ "agent": "reviewer", "description": "follow up", "prompt": "continue", "sessionID": parent }))
        .await
        .expect_err("parent id");
    match error {
        ToolRunFailure::Error { message, .. } => {
            assert_eq!(
                message,
                format!("Session {} is not a child of the current session", parent.as_str())
            );
        }
        other => panic!("expected error failure, got {other:?}"),
    }

    let error = run(&harness, &parent, json!({ "agent": "reviewer", "description": "follow up", "prompt": "continue", "sessionID": unrelated }))
        .await
        .expect_err("unrelated child");
    match error {
        ToolRunFailure::Error { message, .. } => {
            assert_eq!(
                message,
                format!("Session {} is not a child of the current session", unrelated.as_str())
            );
        }
        other => panic!("expected error failure, got {other:?}"),
    }

    // Switching agents re-switches agent and model.
    let settled = run(&harness, &parent, json!({ "agent": "reviewer", "description": "follow up", "prompt": "continue", "sessionID": switched }))
        .await
        .expect("switched run");
    assert_eq!(metadata_session(&settled), switched.as_str());
    let row = harness.store.get(switched.as_str()).await.unwrap().expect("row");
    assert_eq!(row.agent.as_deref(), Some("reviewer"));
    let model = crate::subagent::decode_row_model(&row).expect("model");
    assert_eq!(model.id.0, "child", "switched agent's configured model applies");

    // Switching to an agent without a configured model keeps the child's model.
    let settled = run(&harness, &parent, json!({ "agent": "fallback", "description": "follow up", "prompt": "continue", "sessionID": switched }))
        .await
        .expect("modelless switch");
    assert_eq!(metadata_session(&settled), switched.as_str());
    let row = harness.store.get(switched.as_str()).await.unwrap().expect("row");
    assert_eq!(row.agent.as_deref(), Some("fallback"));
    let model = crate::subagent::decode_row_model(&row).expect("model");
    assert_eq!(model.id.0, "child");
}

#[tokio::test]
async fn subagent_tool_model_resolution_and_failures() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;
    let call = |model: &str, session_id: Option<&str>| {
        let harness = &harness;
        let parent = parent.clone();
        let model = model.to_string();
        let session_id = session_id.map(str::to_string);
        async move {
            let mut input = json!({ "agent": "reviewer", "description": "review", "prompt": "review this", "model": model });
            if let Some(session_id) = session_id {
                input["sessionID"] = json!(session_id);
            }
            run(harness, &parent, input).await
        }
    };

    // Explicit model beats the agent's configured model.
    let spawned = call("test/override#fast", None).await.expect("override spawn");
    let child = metadata_session(&spawned);
    let row = harness.store.get(&child).await.unwrap().expect("row");
    assert_eq!(crate::subagent::decode_row_model(&row).unwrap(), override_model());

    // Continuing with a model switches the child even when the agent is
    // unchanged; the omitted variant normalizes to "default".
    let continued = call("test/override", Some(&child)).await.expect("override continue");
    assert_eq!(metadata_session(&continued), child);
    let row = harness.store.get(&child).await.unwrap().expect("row");
    assert_eq!(
        crate::subagent::decode_row_model(&row).unwrap(),
        model::Ref {
            id: oc_schema::id::ModelId::parse("override"),
            provider_id: oc_schema::id::ProviderId::parse("test"),
            variant: Some(oc_schema::id::ModelVariantId::parse("default")),
        }
    );

    let failures = [
        ("not-a-ref", "Invalid model \"not-a-ref\". Use \"providerID/modelID\" or \"providerID/modelID#variant\"."),
        ("test/missing", "Model \"test/missing\" is not available. Use the models tool to see what is available."),
        ("test/override#slow", "Variant \"slow\" is not available for \"test/override\". Available: fast."),
        ("test/plain#high", "Model \"test/plain\" has no variants. Omit the variant."),
    ];
    for (model, message) in failures {
        let error = call(model, None).await.expect_err("model failure");
        match error {
            ToolRunFailure::Error { message: got, .. } => assert_eq!(got, message, "model {model}"),
            other => panic!("expected error failure for {model}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn subagent_tool_returns_child_failures_as_tool_errors() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;

    let error = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "fail review", "prompt": "please fail" }),
    )
    .await
    .expect_err("child failure");
    match error {
        ToolRunFailure::Error { message, .. } => {
            // Failure surfaces keep the sessionID visible.
            assert!(message.starts_with("Subagent failed (sessionID: "), "{message}");
            assert!(message.contains("): No model is available for session"), "{message}");
        }
        other => panic!("expected error failure, got {other:?}"),
    }
}

#[tokio::test]
async fn subagent_tool_background_result_text_and_completion_notice() {
    let harness = harness().await;
    install_harness_setup(&harness);
    let parent = create_session(&harness, None, "parent", Some(model_ref("parent")), None).await;

    let settled = run(
        &harness,
        &parent,
        json!({ "agent": "reviewer", "description": "background review", "prompt": "review", "background": true }),
    )
    .await
    .expect("background launch");
    let child = metadata_session(&settled);
    assert_eq!(
        serde_json::to_value(settled.metadata.as_ref().unwrap()).unwrap(),
        json!({ "sessionID": child, "status": "running" })
    );
    // DO-NOT-POLL text EXACT.
    let expected = [
        format!("The subagent is working in the background (sessionID: {child}). You will be notified automatically when it finishes."),
        "DO NOT sleep, poll for progress, ask the subagent for status, or duplicate this subagent's work; avoid working with the same files or topics it is using.".to_string(),
        "Work on non-overlapping tasks, or briefly tell the user what you launched and end your response.".to_string(),
    ]
    .join("\n");
    assert_eq!(settled.content, vec![oc_schema::tool::Content::Text { text: expected }]);

    // The completion synthetic arrives in the parent, envelope EXACT.
    let synthetic = wait_for_synthetic(&harness, parent.as_str()).await;
    assert_eq!(synthetic.len(), 1, "one durable completion");
    let InboxInfo::Synthetic { payload, .. } = &synthetic[0] else {
        panic!("synthetic item");
    };
    let child_ref = child.clone();
    assert_eq!(
        payload.text,
        format!(
            "<subagent sessionID=\"{child_ref}\" state=\"completed\" description=\"background review\">\n{CHILD_TEXT}\n</subagent>"
        )
    );
    assert_eq!(payload.description.as_deref(), Some("background review"));
    assert_eq!(
        serde_json::to_value(payload.metadata.as_ref()).unwrap(),
        json!({ "source": "subagent", "childID": child_ref, "agent": "reviewer", "state": "completed" })
    );
}

#[tokio::test]
async fn subagent_tool_description_lists_available_subagents() {
    let base = super::subagent::DESCRIPTION;
    let with = super::subagent::SubagentTool::description_for(
        &agents(),
        &oc_schema::permission::Ruleset::default(),
    );
    assert!(with.starts_with(base));
    assert!(with.contains("\n\nAvailable subagents:\n- fallback: This subagent should only be called when explicitly requested."));
    assert!(with
        .contains("\n- reviewer: This subagent should only be called when explicitly requested."));
    assert!(!with.contains("- primary:"), "primary agents are not listed");
    assert!(!with.contains("- hidden:"), "hidden agents are not listed");
    assert!(!with.contains("- build:"), "the selected primary agent is not listed");
    // Sorted by id.
    let fallback = with.find("- fallback").unwrap();
    let reviewer = with.find("- reviewer").unwrap();
    assert!(fallback < reviewer);
}

#[tokio::test]
async fn subagent_tool_no_text_fallback() {
    assert_eq!(NO_TEXT, "Subagent completed without a text response.");
}

// probe appended temporarily into tests_subagent.rs
