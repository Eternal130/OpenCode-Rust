//! The Wave 5-0 acceptance gates through the composition root:
//!
//! 1. the full-stack e2e — real tools + snapshot + forms + AGENTS.md
//!    injection — a scripted TestLlm session whose read tool touches a NEW
//!    directory injects the synthetic instruction exactly once;
//! 2. the Wave-B e2e shape (the full tool-call turn sequence) passes
//!    THROUGH the root;
//! 3. the interrupt-during-tool parity golden (guard region);
//! 4. tool-output truncation through the real pipeline;
//! 5. the durable `permission.rejected` error type.

#![allow(
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructors consume their parts
    clippy::too_many_lines, // the assembly table mirrors the v2 layer graph
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity, // hook/executor seams match the lib's
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown, // TS identifiers in prose stay unquoted
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening, // assembly order is deliberate
    clippy::large_futures, // boot/assemble mirror the v2 layer composition
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation,
    clippy::elidable_lifetime_names,
    clippy::question_mark // the early-return shape mirrors the TS branch
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use oc_ai::testing::TestLlm;
use oc_schema::id::SessionId;

use crate::engine::{Engine, EngineOptions};
use crate::location::{LocationRef, ToolExecute, ToolRegistration, ToolRunFailure, ToolRunResult};

use crate::session::runner::tests_harness::{
    fixed_clock, parity_text_response, test_resolved_model, FixedResolver, TestLlmClient,
};

/// Boots an engine over a sandboxed project with a scripted LLM and the
/// test model resolver.
async fn engine_with(llm: TestLlm) -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-engine-")
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

async fn create_titled_session(engine: &Engine, location: &LocationRef, id: &str) -> SessionId {
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
                title: Some("Engine test".to_string()),
                agent: None,
                model: Some(oc_schema::model::Ref {
                    provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                    id: oc_schema::id::ModelId::parse("fake-model"),
                    variant: Some(oc_schema::id::ModelVariantId::parse("default")),
                }),
                // The engine runs the REAL permission pipeline; the e2e
                // sessions come pre-approved (the parity scenarios'
                // allow-all user).
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
    session_id: &SessionId,
) -> Vec<(String, i64, serde_json::Value)> {
    use futures::StreamExt;
    let mut stream = engine.bus.log(crate::bus::LogInput {
        aggregate_id: session_id.to_string(),
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

/// Polls until the predicate over the durable events holds (the engine's
/// side effects — AGENTS.md injection, settle rows — are fire-and-forget
/// tasks).
async fn wait_for_events(
    engine: &Engine,
    session_id: &SessionId,
    predicate: impl Fn(&[(String, i64, serde_json::Value)]) -> bool,
) -> Vec<(String, i64, serde_json::Value)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let events = durable_events(engine, session_id).await;
        if predicate(&events) {
            return events;
        }
        if std::time::Instant::now() > deadline {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

// ===========================================================================
// Gate 2: the full-stack e2e (AGENTS.md injection exactly once)
// ===========================================================================

#[tokio::test]
async fn engine_e2e_read_tool_injects_agents_md_synthetic_exactly_once() {
    let llm = TestLlm::new();
    llm.push([
        oc_ai::testing::tool("call_1", "read", serde_json::json!({ "path": "sub/hello.txt" })),
        parity_text_response("Read done.", 20.0, 8.0),
    ]);
    let (engine, dir, location) = engine_with(llm).await;

    // A NEW directory below the location root: its AGENTS.md is NOT part
    // of the initial discovery (which walks UP from the location).
    std::fs::create_dir_all(dir.path().join("sub")).expect("subdir");
    std::fs::write(dir.path().join("sub").join("hello.txt"), "hello world").expect("file");
    std::fs::write(dir.path().join("sub").join("AGENTS.md"), "Sub-specific guidance.")
        .expect("agents");

    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_titled_session(&engine, &location, "ses_engine_agents").await;
    engine
        .prompt(&location, &session_id, "msg_user_1", "read sub/hello.txt")
        .await
        .expect("prompt exit");

    // The synthetic injection lands exactly once.
    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.synthetic")
    })
    .await;
    let synthetic: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.synthetic").collect();
    assert_eq!(synthetic.len(), 1, "exactly one synthetic: {events:#?}");
    let expected_path = dir.path().join("sub").join("AGENTS.md");
    assert_eq!(
        synthetic[0].2["metadata"]["instruction"]["paths"],
        serde_json::json!([expected_path.to_string_lossy().into_owned()]),
        "the ledger path is the absolute AGENTS.md"
    );
    assert_eq!(
        synthetic[0].2["description"],
        serde_json::json!("Loaded sub/AGENTS.md"),
        "description relative to the project root"
    );
    assert!(
        synthetic[0].2["text"].as_str().unwrap().contains("Instructions from: "),
        "the body carries the injected instruction text"
    );

    // Normal turn events: the read tool executed and the turn completed.
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    assert!(types.contains(&"session.tool.called"), "{types:?}");
    assert!(types.contains(&"session.tool.success"), "{types:?}");
    assert!(types.contains(&"session.step.ended"), "{types:?}");
    assert_eq!(types.last(), Some(&"session.execution.succeeded"));

    // The tool result content is the read output (the real tool ran).
    let success =
        events.iter().find(|(type_, _, _)| type_ == "session.tool.success").expect("tool success");
    assert_eq!(success.2["id"], serde_json::json!("call_1"));
    assert!(
        success.2["content"][0]["text"].as_str().unwrap().contains("hello world"),
        "the read leaf produced the file content"
    );
}

#[tokio::test]
async fn engine_e2e_second_read_does_not_reinject() {
    let llm = TestLlm::new();
    llm.push([
        oc_ai::testing::tool("call_1", "read", serde_json::json!({ "path": "sub/hello.txt" })),
        parity_text_response("Read done.", 20.0, 8.0),
        oc_ai::testing::tool("call_2", "read", serde_json::json!({ "path": "sub/hello.txt" })),
        parity_text_response("Read again.", 21.0, 8.0),
    ]);
    let (engine, dir, location) = engine_with(llm).await;
    std::fs::create_dir_all(dir.path().join("sub")).expect("subdir");
    std::fs::write(dir.path().join("sub").join("hello.txt"), "hello world").expect("file");
    std::fs::write(dir.path().join("sub").join("AGENTS.md"), "Sub-specific guidance.")
        .expect("agents");

    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_titled_session(&engine, &location, "ses_engine_agents2").await;
    engine.prompt(&location, &session_id, "msg_user_1", "read it").await.expect("first exit");
    wait_for_events(&engine, &session_id, |events| {
        events.iter().filter(|(type_, _, _)| type_ == "session.tool.success").count() == 1
    })
    .await;

    engine
        .prompt(&location, &session_id, "msg_user_2", "read it again")
        .await
        .expect("second exit");
    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().filter(|(type_, _, _)| type_ == "session.tool.success").count() == 2
    })
    .await;
    let injections = events.iter().filter(|(type_, _, _)| type_ == "session.synthetic").count();
    assert_eq!(injections, 1, "the ledger suppresses the second injection");
}

// ===========================================================================
// Gate 2 (b): the Wave-B e2e sequence through the root
// ===========================================================================

#[tokio::test]
async fn engine_e2e_full_tool_call_turn_sequence_through_the_root() {
    let llm = TestLlm::new();
    llm.push([
        oc_ai::testing::tool("call_1", "read", serde_json::json!({ "path": "hello.txt" })),
        parity_text_response("Root read done.", 20.0, 8.0),
    ]);
    let (engine, dir, location) = engine_with(llm).await;
    std::fs::write(dir.path().join("hello.txt"), "root file").expect("file");

    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_titled_session(&engine, &location, "ses_engine_sequence").await;
    engine.prompt(&location, &session_id, "msg_user_1", "read hello").await.expect("exit");

    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.execution.succeeded")
    })
    .await;
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "session.created",
            "session.inbox.enqueued",
            "session.execution.started",
            "session.instructions.updated",
            "session.inbox.delivered",
            "session.step.started",
            "session.tool.input.started",
            "session.tool.input.ended",
            "session.tool.called",
            "session.step.streamed",
            "session.tool.success",
            "session.step.ended",
            "session.step.started",
            "session.text.started",
            "session.text.ended",
            "session.step.streamed",
            "session.step.ended",
            "session.execution.succeeded",
        ],
        "the Wave-B e2e sequence, through the root"
    );
    // Sequence numbers are gapless in commit order.
    let seqs: Vec<i64> = events.iter().map(|(_, seq, _)| *seq).collect();
    assert_eq!(seqs, (0..i64::try_from(events.len()).unwrap_or(0)).collect::<Vec<_>>());
}

// ===========================================================================
// Gate 1: the interrupt-during-tool parity golden
// ===========================================================================

/// A tool that blocks until dropped (the interrupt bell drops it).
struct BlockingTool {
    gate: Arc<tokio::sync::Notify>,
    dropped: Arc<AtomicUsize>,
}

struct BlockingGuard(Arc<AtomicUsize>);

impl Drop for BlockingGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl ToolExecute for BlockingTool {
    async fn execute(&self, _input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        let _guard = BlockingGuard(Arc::clone(&self.dropped));
        self.gate.notified().await;
        Ok(ToolRunResult {
            content: vec![oc_schema::tool::Content::Text { text: "unreachable".into() }],
            metadata: None,
        })
    }
}

#[tokio::test]
async fn engine_e2e_interrupt_during_tool_settles_full_parity_sequence() {
    let llm = TestLlm::new();
    llm.push([oc_ai::testing::tool("call_1", "block", serde_json::json!({ "wait": true }))]);
    let (engine, _dir, location) = engine_with(llm).await;

    let services = engine.location(location.clone()).await.expect("assemble");
    let gate = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    services.graph.tools.register(ToolRegistration::new(
        "block",
        "Blocks until interrupted",
        serde_json::json!({ "type": "object", "properties": { "wait": { "type": "boolean" } } }),
    ));
    services
        .graph
        .tools
        .register_executor("block", Arc::new(BlockingTool { gate, dropped: dropped.clone() }));

    let session_id = create_titled_session(&engine, &location, "ses_engine_interrupt").await;
    let prompt_engine = Arc::clone(&engine);
    let prompt_location = location.clone();
    let prompt_session = session_id.clone();
    let prompt = tokio::spawn(async move {
        let _ = prompt_engine
            .prompt(&prompt_location, &prompt_session, "msg_user_1", "block forever")
            .await;
    });

    // Wait until the tool call is durably recorded, then interrupt.
    let pre = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.tool.called")
    })
    .await;
    assert!(pre.iter().any(|(type_, _, _)| type_ == "session.tool.called"));

    let interrupted = engine.interrupt(&location, &session_id).await;
    assert!(interrupted, "an active execution was interrupted");
    let _ = prompt.await;

    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.execution.interrupted")
    })
    .await;

    // The FULL durable sequence per v2 order: tool.failed with the exact
    // TOOLS_INTERRUPTED payload, step.failed STEP_INTERRUPTED, then the
    // terminal execution.interrupted.
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    let tool_failed = types
        .iter()
        .position(|type_| *type_ == "session.tool.failed")
        .expect("durable tool failure");
    let step_failed = types
        .iter()
        .position(|type_| *type_ == "session.step.failed")
        .expect("durable step failure");
    let execution_interrupted = types
        .iter()
        .position(|type_| *type_ == "session.execution.interrupted")
        .expect("terminal interruption");
    assert!(tool_failed < step_failed, "{types:?}");
    assert!(step_failed < execution_interrupted, "{types:?}");

    let tool_failure = &events[tool_failed].2;
    assert_eq!(tool_failure["id"], serde_json::json!("call_1"), "the live tool settles as failed");
    assert_eq!(
        tool_failure["error"],
        serde_json::json!({
            "type": "aborted",
            "message": "Tool execution interrupted",
        }),
        "the exact TOOLS_INTERRUPTED durable error"
    );
    let step_failure = &events[step_failed].2;
    assert_eq!(
        step_failure["error"]["message"],
        serde_json::json!("Step interrupted"),
        "STEP_INTERRUPTED"
    );
    assert_eq!(
        events[execution_interrupted].2["reason"],
        serde_json::json!("user"),
        "a user interrupt carries the user reason"
    );

    // No row stays `running`: the assistant's tool part settled with the
    // error, not a live status.
    let messages = engine
        .messages(
            &location,
            crate::session::store::MessagesInput {
                session_id: session_id.to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("messages");
    let mut running = 0;
    for row in &messages {
        let data: serde_json::Value = serde_json::from_str(&row.data).unwrap_or_default();
        for part in data["content"].as_array().unwrap_or(&Vec::new()) {
            if part["type"] == "tool" {
                let status = part["state"]["status"].as_str().unwrap_or_default();
                if status == "running" {
                    running += 1;
                }
            }
        }
    }
    assert_eq!(running, 0, "no eternal running tool parts");

    // The blocked fiber observed the bell (its future was dropped).
    assert!(
        dropped.load(Ordering::SeqCst) >= 1,
        "the tool fiber was interrupted, not run to completion"
    );
}

// ===========================================================================
// Gate 3: tool-output truncation through the real pipeline
// ===========================================================================

struct BigOutputTool;

#[async_trait::async_trait]
impl ToolExecute for BigOutputTool {
    async fn execute(&self, _input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        // > 50KiB of lines — over both the line and byte budgets.
        let line = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let text: Vec<String> = (0..1_200).map(|index| format!("{line} #{index}")).collect();
        Ok(ToolRunResult {
            content: vec![oc_schema::tool::Content::Text { text: text.join("\n") }],
            metadata: None,
        })
    }
}

#[tokio::test]
async fn engine_e2e_tool_output_over_50kib_truncates_with_spill_and_marker() {
    let llm = TestLlm::new();
    llm.push([
        oc_ai::testing::tool("call_1", "big", serde_json::json!({})),
        parity_text_response("Big done.", 10.0, 5.0),
    ]);
    let (engine, _dir, location) = engine_with(llm).await;

    let services = engine.location(location.clone()).await.expect("assemble");
    services.graph.tools.register(ToolRegistration::new(
        "big",
        "Returns a big payload",
        serde_json::json!({ "type": "object", "properties": {} }),
    ));
    services.graph.tools.register_executor("big", Arc::new(BigOutputTool));

    let session_id = create_titled_session(&engine, &location, "ses_engine_truncate").await;
    engine.prompt(&location, &session_id, "msg_user_1", "go big").await.expect("exit");

    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.execution.succeeded")
    })
    .await;
    let success =
        events.iter().find(|(type_, _, _)| type_ == "session.tool.success").expect("tool success");
    let text: String = success.2["content"]
        .as_array()
        .expect("content array")
        .iter()
        .filter_map(|part| part["text"].as_str().map(str::to_string))
        .collect::<String>();
    assert!(text.len() < 60 * 1024, "the settled payload is bounded");
    let marker_start =
        text.find("[showing lines 1-").expect("the exact marker rides the bounded content");
    let marker_line = &text[marker_start..];
    assert!(marker_line.starts_with("[showing lines 1-"), "exact marker prefix: {marker_line}");
    assert!(
        marker_line.contains("of 1200; full output saved to "),
        "exact marker body: {marker_line}"
    );
    assert!(marker_line.ends_with(']'), "exact marker suffix: {marker_line}");
    // The spill file exists with the full output.
    let spill = marker_line.rsplit("saved to ").next().unwrap_or_default().trim_end_matches(']');
    let spilled = std::fs::read_to_string(spill).expect("spill file exists");
    assert!(spilled.contains("#1199"), "the spill keeps the whole output");
    assert!(spilled.len() > 50 * 1024, "the original exceeded the budget");
}

// ===========================================================================
// Gate 4: the durable permission.rejected error type
// ===========================================================================

struct DeniedTool;

#[async_trait::async_trait]
impl ToolExecute for DeniedTool {
    async fn execute(&self, _input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        Err(ToolRunFailure::PermissionRejected {
            message: "Permission denied: denied_tool".into(),
            metadata: None,
        })
    }
}

#[tokio::test]
async fn engine_e2e_permission_blocked_tool_fails_with_permission_rejected() {
    let llm = TestLlm::new();
    llm.push([
        oc_ai::testing::tool("call_1", "denied_tool", serde_json::json!({})),
        parity_text_response("Handled.", 10.0, 5.0),
    ]);
    let (engine, _dir, location) = engine_with(llm).await;

    let services = engine.location(location.clone()).await.expect("assemble");
    services.graph.tools.register(ToolRegistration::new(
        "denied_tool",
        "Always permission-blocked",
        serde_json::json!({ "type": "object", "properties": {} }),
    ));
    services.graph.tools.register_executor("denied_tool", Arc::new(DeniedTool));

    let session_id = create_titled_session(&engine, &location, "ses_engine_permtype").await;
    engine.prompt(&location, &session_id, "msg_user_1", "call it").await.expect("exit");

    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.tool.failed")
    })
    .await;
    let failure =
        events.iter().find(|(type_, _, _)| type_ == "session.tool.failed").expect("tool failure");
    assert_eq!(
        failure.2["error"],
        serde_json::json!({
            "type": "permission.rejected",
            "message": "Permission denied: denied_tool",
        }),
        "the durable error type distinguishes permission rejections"
    );
}

// ===========================================================================
// Config-lane skill discovery (finding 7)
// ===========================================================================

#[test]
fn engine_skills_table_discovers_config_skill_paths() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let skills = dir.path().join("skills");
    std::fs::create_dir_all(skills.join("review")).expect("skill dir");
    std::fs::create_dir_all(skills.join("drafting")).expect("skill dir");
    std::fs::write(
        skills.join("review").join("SKILL.md"),
        "---\nname: Review\ndescription: Review code carefully\n---\n# Review guidance",
    )
    .expect("skill file");
    std::fs::write(
        skills.join("drafting").join("SKILL.md"),
        "---\nname: Drafting\ndescription: Draft documents\nmetadata:\n  opencode/autoinvoke: false\n---\n# Drafting guidance",
    )
    .expect("skill file");
    // A bare `<skill>.md` at the source root.
    std::fs::write(skills.join("quick.md"), "---\nname: Quick\n---\nQuick and dirty")
        .expect("bare md");

    let loaded = crate::config::Loader::new(
        dir.path().to_path_buf(),
        crate::config::GlobalDirs { config: dir.path().join("cfg"), home: dir.path().join("home") },
    )
    .load();
    let mut with_document = loaded.clone();
    with_document.entries.push(oc_schema::config::Entry::Document(Box::new(
        oc_schema::config::Document {
            info: serde_json::from_value(serde_json::json!({ "skills": ["./skills"] }))
                .expect("info"),
            path: None,
        },
    )));
    let table = crate::engine::skills::ConfigSkillTable::discover(
        &with_document,
        dir.path(),
        &dir.path().join("home"),
    );
    let review =
        crate::tool::skill::SkillTable::get(&table, "review").expect("the SKILL.md directory id");
    assert_eq!(review.name.0, "Review");
    assert_eq!(review.description.as_deref(), Some("Review code carefully"));
    assert!(review.content.contains("# Review guidance"));
    let drafting = crate::tool::skill::SkillTable::get(&table, "drafting").expect("drafting");
    assert_eq!(drafting.auto_invoke, Some(false), "metadata opencode/autoinvoke");
    let quick =
        crate::tool::skill::SkillTable::get(&table, "quick").expect("the bare <skill>.md id");
    assert_eq!(quick.name.0, "Quick");
}

// ===========================================================================
// Built-in skills plugin (gate-6 F3) — plugin/skill.ts port
// ===========================================================================

#[test]
fn engine_builtin_skills_seed_the_table() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let loaded = crate::config::Loader::new(
        dir.path().to_path_buf(),
        crate::config::GlobalDirs { config: dir.path().join("cfg"), home: dir.path().join("home") },
    )
    .load();
    let table = crate::engine::skills::ConfigSkillTable::discover(
        &loaded,
        dir.path(),
        &dir.path().join("home"),
    );

    let opencode =
        crate::tool::skill::SkillTable::get(&table, "opencode").expect("built-in opencode");
    assert_eq!(opencode.name.0, "OpenCode");
    assert!(opencode
        .description
        .as_deref()
        .unwrap_or_default()
        .starts_with("Use this skill for any question about OpenCode itself"));
    assert_eq!(opencode.path.as_str(), "/builtin/opencode.md");
    assert_eq!(opencode.content, crate::engine::skills::builtin::OPENCODE_MD);
    assert_eq!(opencode.auto_invoke, None);

    let report = crate::tool::skill::SkillTable::get(&table, "report").expect("built-in report");
    assert_eq!(report.name.0, "Report");
    assert!(report.description.as_deref().unwrap_or_default().contains("opencode issue"));
    assert_eq!(report.path.as_str(), "/builtin/report.md");
    // The diagnostics snapshot mirrors the TS field set.
    let content = &report.content;
    assert!(content.starts_with(crate::engine::skills::builtin::REPORT_MD));
    assert!(content.contains("## Runtime Diagnostics Snapshot"));
    assert!(content
        .contains(&format!("- opencode version: {}", crate::engine::skills::builtin::APP_VERSION)));
    assert_eq!(crate::engine::skills::builtin::APP_VERSION, "2.0.24");
    assert!(content
        .contains(&format!("- install/channel: {}", crate::engine::skills::builtin::APP_CHANNEL)));
    assert!(content.contains("- OS: "));
    assert!(content.contains("- Terminal: "));
    assert!(content.contains("- Shell: "));
    assert!(content.contains("- Active plugins: None found in config"));
}

#[test]
fn engine_builtin_report_lists_configured_plugins_verbatim_and_sorted() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let document = oc_schema::config::Document {
        info: serde_json::from_value(serde_json::json!({
            "plugins": [
                "package-plugin",
                "-disabled",
                "local.ts",
                { "package": "package-plugin", "options": { "enabled": true } }
            ]
        }))
        .expect("info"),
        path: None,
    };
    let loaded = crate::config::Loaded {
        sources: crate::config::discovery::Sources::default(),
        entries: vec![oc_schema::config::Entry::Document(Box::new(document))],
        diagnostics: Vec::new(),
    };
    let table = crate::engine::skills::ConfigSkillTable::discover(
        &loaded,
        dir.path(),
        &dir.path().join("home"),
    );
    let report = crate::tool::skill::SkillTable::get(&table, "report").expect("report");
    // Reference expectation (skill.test.ts): entries verbatim, duplicates
    // and remove directives included, lexicographically sorted.
    assert!(
        report
            .content
            .contains("- Active plugins: -disabled, local.ts, package-plugin, package-plugin"),
        "report content:\n{}",
        report.content
    );
}

#[test]
fn engine_builtin_skills_lose_id_collisions_to_config() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let skills = dir.path().join("skills");
    std::fs::create_dir_all(skills.join("opencode")).expect("skill dir");
    std::fs::write(
        skills.join("opencode").join("SKILL.md"),
        "---\nname: Custom\ndescription: A config-defined opencode skill\n---\n# Custom",
    )
    .expect("skill file");
    let loaded = crate::config::Loader::new(
        dir.path().to_path_buf(),
        crate::config::GlobalDirs { config: dir.path().join("cfg"), home: dir.path().join("home") },
    )
    .load();
    let mut with_document = loaded.clone();
    with_document.entries.push(oc_schema::config::Entry::Document(Box::new(
        oc_schema::config::Document {
            info: serde_json::from_value(serde_json::json!({ "skills": ["./skills"] }))
                .expect("info"),
            path: None,
        },
    )));
    let table = crate::engine::skills::ConfigSkillTable::discover(
        &with_document,
        dir.path(),
        &dir.path().join("home"),
    );
    // Config wins on collision (v2 editor.add overwrite precedence).
    let collision =
        crate::tool::skill::SkillTable::get(&table, "opencode").expect("the config opencode");
    assert_eq!(collision.name.0, "Custom");
    assert!(collision.content.contains("# Custom"));
    // The built-in report is untouched.
    let report = crate::tool::skill::SkillTable::get(&table, "report").expect("report");
    assert!(report.content.contains("## Runtime Diagnostics Snapshot"));
}

// ===========================================================================
// Gate-5 re-review fixes: cold-assembly dedup + bundle background-task
// shutdown
// ===========================================================================

#[tokio::test]
async fn engine_concurrent_cold_location_calls_share_one_bundle() {
    // Two concurrent get-misses must produce ONE assembly: both callers
    // receive the same Arc (gate-5 re-review, cold-assembly race).
    let llm = TestLlm::new();
    llm.always(parity_text_response("ok", 1.0, 1.0));
    let (engine, _dir, location) = engine_with(llm).await;
    let (first, second) =
        tokio::join!(engine.location(location.clone()), engine.location(location.clone()));
    let first = first.expect("first assembly");
    let second = second.expect("second assembly");
    assert!(Arc::ptr_eq(&first, &second), "concurrent cold location() calls must share one bundle");
    // A later call still resolves the incumbent.
    let third = engine.location(location.clone()).await.expect("incumbent");
    assert!(Arc::ptr_eq(&first, &third));
}

#[tokio::test]
async fn engine_invalidate_stops_the_bundle_background_tasks() {
    // The per-location shutdown token (the MCP cache-refresh loop + the
    // reconcile observe it) is registered with the bundle and removed —
    // rung — by invalidate, so a superseded bundle's loops stop instead
    // of leaking (gate-5 re-review, MCP cache loop leak).
    let llm = TestLlm::new();
    llm.always(parity_text_response("ok", 1.0, 1.0));
    let (engine, _dir, location) = engine_with(llm).await;
    engine.location(location.clone()).await.expect("assembly");
    assert!(
        engine.location_shutdown.get(&location).is_some(),
        "the bundle registers its background-task shutdown token"
    );
    engine.invalidate(&location).await;
    assert!(
        engine.location_shutdown.get(&location).is_none(),
        "invalidate rings and removes the token"
    );
    // The location re-assembles cleanly afterwards.
    engine.location(location.clone()).await.expect("re-assembly");
    assert!(engine.location_shutdown.get(&location).is_some());
}

// ===========================================================================
// Per-location provider wiring (v2 resolves providers per location —
// `provider.ts` reads the layer's Config): each location's config-seeded
// custom providers + model default overlay the engine-level bindings.
// ===========================================================================

use crate::database::schema::SessionRow;
use crate::provider_catalog::facade::{CustomModel, CustomProvider};
use oc_schema::config::ModelSelection;
use oc_schema::id::{ModelId, ProviderId};
use oc_schema::model::Ref as ModelRef;

/// A project dir with `opencode.json` + a git marker under `root`.
fn overlay_project(root: &std::path::Path, name: &str, config: Option<&str>) -> LocationRef {
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join(".git")).expect("project dir");
    if let Some(config) = config {
        std::fs::write(dir.join("opencode.json"), config).expect("project config");
    }
    LocationRef::new(dir, None)
}

/// A session row with an optional pinned model ref.
fn overlay_row(model: Option<ModelRef>) -> SessionRow {
    let mut row =
        SessionRow::new("ses_probe", "prj_probe", "/proj", "probe", "2.0.24").expect("row");
    row.model = model.map(|reference| serde_json::to_string(&reference).expect("model json"));
    row
}

fn model_ref(provider: &str, model: &str) -> ModelRef {
    ModelRef { provider_id: ProviderId::parse(provider), id: ModelId::parse(model), variant: None }
}

fn selection(provider: &str, model: &str) -> ModelSelection {
    ModelSelection {
        provider_id: ProviderId::parse(provider),
        model: ModelId::parse(model),
        variant: None,
    }
}

fn custom(provider: &str, model: &str) -> CustomProvider {
    CustomProvider {
        id: provider.to_owned(),
        name: provider.to_owned(),
        base_url: format!("https://{provider}.test/v1"),
        api_key: Some("probe-key".into()),
        headers: None,
        body: None,
        models: vec![CustomModel { id: model.to_owned(), name: None, headers: None, body: None }],
    }
}

/// Boots a resolver-default engine (no injection) over a sandbox root —
/// the daemon shape: engine-level bindings come only from the options
/// (boot/global config), everything else resolves per location.
async fn overlay_engine(
    root: &std::path::Path,
    custom_providers: Vec<CustomProvider>,
    default_model: Option<ModelSelection>,
) -> Engine {
    Engine::boot(EngineOptions {
        custom_providers,
        default_model,
        activity_sweeper: false,
        clock: fixed_clock(),
        ..EngineOptions::new(root.join("data"), root.join("home"))
    })
    .await
}

#[tokio::test]
async fn per_location_overlay_two_projects_resolve_their_own() {
    let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let alpha = overlay_project(
        root.path(),
        "alpha",
        Some(
            r#"{
                "model": { "providerID": "alpha-probe", "model": "alpha-one" },
                "providers": {
                    "alpha-probe": {
                        "settings": { "baseURL": "https://alpha.test/v1", "apiKey": "k" },
                        "models": { "alpha-one": { "name": "Alpha One" } }
                    }
                }
            }"#,
        ),
    );
    let beta = overlay_project(
        root.path(),
        "beta",
        Some(
            r#"{
                "model": { "providerID": "beta-probe", "model": "beta-one" },
                "providers": {
                    "beta-probe": {
                        "settings": { "baseURL": "https://beta.test/v1", "apiKey": "k" },
                        "models": { "beta-one": { "name": "Beta One" } }
                    }
                }
            }"#,
        ),
    );
    let engine = overlay_engine(root.path(), Vec::new(), None).await;

    let alpha_services = engine.location(alpha.clone()).await.expect("alpha assembles");
    let beta_services = engine.location(beta.clone()).await.expect("beta assembles");

    // Each project's own default (config `model`) resolves through its own
    // overlay — a session with no pinned model.
    let resolved = alpha_services.resolver.resolve(&overlay_row(None)).expect("alpha default");
    assert_eq!(resolved.reference.provider_id.0, "alpha-probe");
    assert_eq!(resolved.reference.id.0, "alpha-one");
    let resolved = beta_services.resolver.resolve(&overlay_row(None)).expect("beta default");
    assert_eq!(resolved.reference.provider_id.0, "beta-probe");
    assert_eq!(resolved.reference.id.0, "beta-one");

    // Explicit refs resolve on their own project and NOT on the sibling's
    // (the overlay is per-location, the engine-level bindings are empty).
    assert!(alpha_services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("alpha-probe", "alpha-one"))
        .is_ok());
    assert!(
        alpha_services
            .resolver
            .resolve_selected(&overlay_row(None), &model_ref("beta-probe", "beta-one"))
            .is_err(),
        "beta's provider is not visible at alpha"
    );
    assert!(beta_services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("beta-probe", "beta-one"))
        .is_ok());

    // A pinned model takes the same per-location path.
    let mut pinned = overlay_row(Some(model_ref("beta-probe", "beta-one")));
    pinned.directory = beta.directory.to_string_lossy().into_owned();
    let resolved = beta_services.resolver.resolve(&pinned).expect("pinned beta model");
    assert_eq!(resolved.reference.provider_id.0, "beta-probe");
}

#[tokio::test]
async fn per_location_overlay_falls_through_to_engine_bindings() {
    let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    // Engine-level (boot/global config) bindings: a gateway provider and
    // the engine default selection.
    let engine = overlay_engine(
        root.path(),
        vec![custom("gateway-probe", "gw-one")],
        Some(selection("gateway-probe", "gw-one")),
    )
    .await;

    // A config-less location: no overlay, the engine default answers.
    let plain = overlay_project(root.path(), "plain", None);
    let plain_services = engine.location(plain).await.expect("plain assembles");
    let resolved = plain_services.resolver.resolve(&overlay_row(None)).expect("engine default");
    assert_eq!(resolved.reference.provider_id.0, "gateway-probe");

    // A location with providers but NO `model` default: the engine default
    // still answers (overlay NotSelected falls through), and the engine's
    // global provider stays explicitly resolvable at the location.
    let delta = overlay_project(
        root.path(),
        "delta",
        Some(
            r#"{
                "providers": {
                    "delta-probe": {
                        "settings": { "baseURL": "https://delta.test/v1" },
                        "models": { "delta-one": {} }
                    }
                }
            }"#,
        ),
    );
    let delta_services = engine.location(delta).await.expect("delta assembles");
    let resolved =
        delta_services.resolver.resolve(&overlay_row(None)).expect("engine default at delta");
    assert_eq!(resolved.reference.provider_id.0, "gateway-probe");
    let resolved = delta_services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("gateway-probe", "gw-one"))
        .expect("engine-level provider visible at every location");
    assert_eq!(resolved.reference.provider_id.0, "gateway-probe");

    // A location WITH a `model` default: the location default wins over
    // the engine default.
    let epsilon = overlay_project(
        root.path(),
        "epsilon",
        Some(
            r#"{
                "model": { "providerID": "delta-probe", "model": "delta-one" },
                "providers": {
                    "delta-probe": {
                        "settings": { "baseURL": "https://delta.test/v1" },
                        "models": { "delta-one": {} }
                    }
                }
            }"#,
        ),
    );
    let epsilon_services = engine.location(epsilon).await.expect("epsilon assembles");
    let resolved = epsilon_services
        .resolver
        .resolve(&overlay_row(None))
        .expect("location default beats engine default");
    assert_eq!(resolved.reference.provider_id.0, "delta-probe");
    assert_eq!(resolved.reference.id.0, "delta-one");
}

/// The daemon cross-project M2 scenario: an engine booted with project A's
/// config bindings (the pre-overlay boot shape / a `--service` daemon
/// seeded at its boot cwd) serves project B — B's own custom provider and
/// default resolve at B.
#[tokio::test]
async fn per_location_overlay_daemon_booted_at_a_serves_b() {
    let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let project_a = overlay_project(
        root.path(),
        "project-a",
        Some(
            r#"{
                "model": { "providerID": "boot-probe", "model": "boot-one" },
                "providers": {
                    "boot-probe": {
                        "settings": { "baseURL": "https://boot.test/v1" },
                        "models": { "boot-one": {} }
                    }
                }
            }"#,
        ),
    );
    let project_b = overlay_project(
        root.path(),
        "project-b",
        Some(
            r#"{
                "model": { "providerID": "remote-probe", "model": "remote-one" },
                "providers": {
                    "remote-probe": {
                        "settings": { "baseURL": "https://remote.test/v1", "apiKey": "k" },
                        "models": { "remote-one": { "name": "Remote One" } }
                    }
                }
            }"#,
        ),
    );
    // Boot seeding from project A's config (what `boot_and_serve` did
    // before per-location wiring — and still does for engine defaults).
    let loaded = crate::config::Loader::new(
        project_a.directory.clone(),
        crate::config::GlobalDirs {
            config: root.path().join("home/.config/opencode"),
            home: root.path().join("home"),
        },
    )
    .load();
    let providers = crate::provider_catalog::providers_from_config(&loaded);
    let engine = overlay_engine(root.path(), providers.custom, providers.default_model).await;

    let services_b = engine.location(project_b).await.expect("project B assembles");
    // B's custom provider resolves at B through B's OWN config.
    let resolved = services_b
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("remote-probe", "remote-one"))
        .expect("B's custom provider resolves at B");
    assert_eq!(resolved.reference.provider_id.0, "remote-probe");
    assert_eq!(
        resolved.model.route.endpoint.base_url.as_deref(),
        Some("https://remote.test/v1"),
        "B's provider binds B's baseURL"
    );
    // B's default beats the boot location's default.
    let resolved =
        services_b.resolver.resolve(&overlay_row(None)).expect("B's default model resolves at B");
    assert_eq!(resolved.reference.provider_id.0, "remote-probe");
    assert_eq!(resolved.reference.id.0, "remote-one");
    // The boot-time (engine-level) provider stays reachable at B.
    assert!(services_b
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("boot-probe", "boot-one"))
        .is_ok());
}

// ===========================================================================
// beta.3.1 regression: case-mismatched provider key resolution
// (`providers: { own: { name: "Own", ... } }` + `model: "Own/glm-5.3"` →
// `provider.no-route "Model unavailable: Own/"`). Matching is exact-first,
// then case-insensitive; the canonical config keys are recorded.
// ===========================================================================

/// The user's exact config shape: lowercase provider key `own` carrying
/// the display name "Own", model reference cased after the NAME.
const CASE_MIXED_CONFIG: &str = r#"{
    "model": "Own/glm-5.3",
    "providers": {
        "own": {
            "name": "Own",
            "settings": { "baseURL": "https://own.test/v1", "apiKey": "k" },
            "models": { "glm-5.3": { "name": "GLM 5.3" } }
        }
    }
}"#;

#[tokio::test]
async fn per_location_overlay_resolves_case_mismatched_provider_reference() {
    let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let project = overlay_project(root.path(), "casing", Some(CASE_MIXED_CONFIG));
    let engine = overlay_engine(root.path(), Vec::new(), None).await;
    let services = engine.location(project).await.expect("assembles");

    // The configured default (`Own/glm-5.3`) resolves against the
    // provider keyed `own` — no `provider.no-route`.
    let resolved =
        services.resolver.resolve(&overlay_row(None)).expect("case-mismatched default resolves");
    assert_eq!(resolved.reference.provider_id.0, "own", "the canonical config key");
    assert_eq!(resolved.reference.id.0, "glm-5.3");
    assert_eq!(
        resolved.model.route.endpoint.base_url.as_deref(),
        Some("https://own.test/v1"),
        "binds the configured provider's baseURL"
    );

    // An explicit mixed-case reference takes the same path.
    let resolved = services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("Own", "glm-5.3"))
        .expect("explicit mixed-case reference resolves");
    assert_eq!(resolved.reference.provider_id.0, "own");
    assert_eq!(resolved.reference.id.0, "glm-5.3");

    // Fully upper-cased (`OWN/GLM-5.3`): both segments fall back to the
    // case-insensitive match and canonicalize onto the config keys.
    let resolved = services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("OWN", "GLM-5.3"))
        .expect("upper-cased reference resolves");
    assert_eq!(resolved.reference.provider_id.0, "own");
    assert_eq!(resolved.reference.id.0, "glm-5.3");
    assert_eq!(resolved.model.id.as_str(), "glm-5.3", "the route model id is canonical");

    // An exact-case reference still resolves unchanged.
    assert!(services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("own", "glm-5.3"))
        .is_ok());
}

#[tokio::test]
async fn per_location_overlay_unknown_reference_keeps_both_segments() {
    let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
    let project = overlay_project(root.path(), "unknown", Some(CASE_MIXED_CONFIG));
    let engine = overlay_engine(root.path(), Vec::new(), None).await;
    let services = engine.location(project).await.expect("assembles");

    // Unknown provider: the error carries BOTH segments — the live defect
    // showed `Model unavailable: Own/` (the model id dropped).
    let error = services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("nope", "some-model"))
        .expect_err("unknown provider");
    assert_eq!(error.to_string(), "Model unavailable: nope/some-model");

    // Known provider, unknown model: same both-segments shape.
    let error = services
        .resolver
        .resolve_selected(&overlay_row(None), &model_ref("own", "missing-model"))
        .expect_err("unknown model");
    assert_eq!(error.to_string(), "Model unavailable: own/missing-model");
}

// ===========================================================================
// beta.3.1 regression: failed-turn error surfacing. A turn that fails
// BEFORE any step (model resolution) must still produce the user-visible
// error part (v2 shape: assistant message error via session.step.failed,
// message-updater.ts:273-287) plus the terminal execution.failed, in that
// order, and one stderr line at the failure site.
// ===========================================================================

/// A resolver that always fails — the live defect's engine-level shape
/// (`Model unavailable: own/glm-5.3` on every turn).
struct FailingResolver;

impl crate::session::runner::ModelResolver for FailingResolver {
    fn resolve(
        &self,
        _session: &crate::database::schema::SessionRow,
    ) -> Result<crate::session::runner::ResolvedModel, crate::session::runner::ModelError> {
        Err(crate::session::runner::ModelError::Unavailable("own".into(), "glm-5.3".into()))
    }

    fn resolve_selected(
        &self,
        _session: &crate::database::schema::SessionRow,
        _reference: &ModelRef,
    ) -> Result<crate::session::runner::ResolvedModel, crate::session::runner::ModelError> {
        Err(crate::session::runner::ModelError::Unavailable("own".into(), "glm-5.3".into()))
    }
}

async fn failing_resolution_engine() -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    engine_with_resolver(Arc::new(FailingResolver), oc_ai::testing::TestLlm::new()).await
}

/// Boots an engine over a sandboxed project with an explicit model
/// resolver and LLM (the failing-turn fixtures).
async fn engine_with_resolver(
    resolver: Arc<dyn crate::session::runner::ModelResolver>,
    llm: oc_ai::testing::TestLlm,
) -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-engine-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    let engine = Engine::boot(EngineOptions {
        llm: Some(Arc::new(TestLlmClient(llm))),
        model_resolver: Some(resolver),
        clock: fixed_clock(),
        activity_sweeper: false,
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = LocationRef::new(dir.path().to_path_buf(), None);
    (Arc::new(engine), dir, location)
}

#[tokio::test]
async fn failed_turn_resolution_error_surfaces_error_part_before_idle_failed() {
    let (engine, _dir, location) = failing_resolution_engine().await;
    let session_id = create_titled_session(&engine, &location, "ses_resolve_fail").await;

    let result = engine.prompt(&location, &session_id, "msg_user_1", "hello").await;
    assert!(result.is_err(), "the turn fails");

    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.execution.failed")
    })
    .await;

    // BOTH the error part events AND the idle failed event, in order:
    // the synthetic step pair precedes the terminal (the message updater
    // projects the assistant error message before the idle marker).
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    let step_started = types
        .iter()
        .position(|type_| *type_ == "session.step.started")
        .expect("synthetic step.started creates the error carrier");
    let step_failed = types
        .iter()
        .position(|type_| *type_ == "session.step.failed")
        .expect("synthetic step.failed carries the error");
    let execution_failed = types
        .iter()
        .position(|type_| *type_ == "session.execution.failed")
        .expect("terminal idle failed");
    assert!(step_started < step_failed, "{types:?}");
    assert!(step_failed < execution_failed, "{types:?}");
    assert_eq!(
        types.iter().filter(|type_| **type_ == "session.step.failed").count(),
        1,
        "exactly one error part"
    );

    // The error part carries the resolution failure verbatim.
    let step_failure = &events[step_failed].2;
    assert_eq!(
        step_failure["error"]["type"],
        serde_json::json!("provider.no-route"),
        "the provider.no-route error type"
    );
    assert_eq!(
        step_failure["error"]["message"],
        serde_json::json!("Model unavailable: own/glm-5.3"),
        "the full reference — both segments"
    );

    // The assistant message in the message stream carries the error
    // (the /api/session/:id/message surface).
    let messages = engine
        .messages(
            &location,
            crate::session::store::MessagesInput {
                session_id: session_id.to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("messages");
    let errored = messages.iter().filter(|row| row.type_ == "assistant").find_map(|row| {
        let data: serde_json::Value = serde_json::from_str(&row.data).unwrap_or_default();
        (data["error"]["message"] == serde_json::json!("Model unavailable: own/glm-5.3"))
            .then_some(data)
    });
    let data = errored.expect("an assistant message carries the error");
    assert_eq!(data["finish"], serde_json::json!("error"), "v2 failed-turn finish marker");

    // The engine-level failure log line (the stderr seam): one line with
    // the session id, the model reference, and the error cause.
    let line = crate::session::execution::turn_failure_log_line(
        session_id.as_str(),
        "parityfake/fake-model",
        &oc_schema::common::SessionError {
            error_type: "provider.no-route".into(),
            message: "Model unavailable: own/glm-5.3".into(),
            status: None,
            response: None,
        },
    );
    assert!(line.contains(session_id.as_str()), "session id in the log line: {line}");
    assert!(line.contains("parityfake/fake-model"), "model id in the log line: {line}");
    assert!(line.contains("provider.no-route"), "error type in the log line: {line}");
    assert!(line.contains("Model unavailable: own/glm-5.3"), "cause in the log line: {line}");
}

/// An in-step failure (the model call itself fails) keeps the
/// publisher's own error part — exactly one — followed by the terminal.
#[tokio::test]
async fn failed_turn_stream_error_keeps_single_error_part_before_idle_failed() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.push([oc_ai::testing::TestResponse::error(oc_ai::schema::AiError::authentication(
        "bad key",
    ))]);
    let (engine, _dir, location) =
        engine_with_resolver(Arc::new(FixedResolver { resolved: test_resolved_model() }), llm)
            .await;
    let session_id = create_titled_session(&engine, &location, "ses_stream_fail").await;

    let result = engine.prompt(&location, &session_id, "msg_user_1", "hello").await;
    assert!(result.is_err(), "the model call fails");

    let events = wait_for_events(&engine, &session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.execution.failed")
    })
    .await;
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    assert_eq!(
        types.iter().filter(|type_| **type_ == "session.step.failed").count(),
        1,
        "the publisher's error part — no synthetic duplicate: {types:?}"
    );
    let step_failed = types
        .iter()
        .position(|type_| *type_ == "session.step.failed")
        .expect("publisher step.failed");
    let execution_failed =
        types.iter().position(|type_| *type_ == "session.execution.failed").expect("terminal");
    assert!(step_failed < execution_failed, "{types:?}");
    assert_eq!(events[step_failed].2["error"]["type"], serde_json::json!("provider.auth"));
}

// ---------------------------------------------------------------------------
// P10-DB1: the engine's database path goes through the channel scheme
// ---------------------------------------------------------------------------

/// Env save/restore guard (the path tests' pattern): `Engine::boot` reads
/// the channel through `oc_util::channel::current`, so the test pins the
/// process env while booting.
struct ChannelEnv {
    _guard: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl ChannelEnv {
    fn lock(vars: &'static [&'static str]) -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved = vars.iter().map(|&name| (name, std::env::var(name).ok())).collect();
        for name in vars {
            std::env::remove_var(name);
        }
        Self { _guard: guard, saved }
    }
}

impl Drop for ChannelEnv {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

#[tokio::test]
async fn engine_boot_opens_the_channel_suffixed_database() {
    let vars = &["OPENCODE_CHANNEL", "OPENCODE_DISABLE_CHANNEL_DB", "OPENCODE_DB"];
    for (channel, disable, expected) in [
        (None, None, "opencode.db"),
        (Some("stable"), None, "opencode-stable.db"),
        (Some("stable"), Some("1"), "opencode.db"),
        (Some("beta"), None, "opencode.db"),
    ] {
        let _env = ChannelEnv::lock(vars);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        if let Some(channel) = channel {
            std::env::set_var("OPENCODE_CHANNEL", channel);
        }
        if let Some(disable) = disable {
            std::env::set_var("OPENCODE_DISABLE_CHANNEL_DB", disable);
        }
        let engine = Engine::boot(EngineOptions {
            activity_sweeper: false,
            clock: fixed_clock(),
            ..EngineOptions::new(root.path().join("data"), root.path().join("home"))
        })
        .await;
        let path = engine.db.path().expect("file-backed database").to_path_buf();
        assert_eq!(
            path,
            root.path().join("data").join(expected),
            "channel={channel:?} disable={disable:?}"
        );
        drop(engine);
    }
}

// ===========================================================================
// CD-13 (scan-cd): the global AGENTS.md reads from the SHARED v2 global
// config dir (`.config/opencode`), with the suffixed `opencode-rust`
// dir as a one-generation back-compat read when the v2 file is absent.
// ===========================================================================

/// Asserts the instruction ledger paths of one settled prompt.
async fn instruction_ledger_paths(engine: &Engine, session_id: &SessionId) -> Vec<String> {
    let events = wait_for_events(engine, session_id, |events| {
        events.iter().any(|(type_, _, _)| type_ == "session.synthetic")
    })
    .await;
    events
        .iter()
        .filter(|(type_, _, _)| type_ == "session.synthetic")
        .flat_map(|(_, _, data)| {
            data["metadata"]["instruction"]["paths"].as_array().cloned().unwrap_or_default()
        })
        .filter_map(|path| path.as_str().map(String::from))
        .collect()
}

#[tokio::test]
async fn engine_e2e_global_agents_md_prefers_the_v2_config_dir() {
    let llm = TestLlm::new();
    llm.push([parity_text_response("Done.", 1.0, 1.0)]);
    let (engine, dir, location) = engine_with(llm).await;
    // The synthetic v2-layout home: global instructions at the DOCUMENTED
    // location (`.config/opencode`) AND at our old suffixed dir — the
    // v2 file must win (the suffixed read is back-compat only).
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".config").join("opencode")).expect("v2 dir");
    std::fs::create_dir_all(home.join(".config").join("opencode-rust")).expect("suffixed dir");
    std::fs::write(home.join(".config").join("opencode").join("AGENTS.md"), "V2 global guidance.")
        .expect("v2 agents");
    std::fs::write(
        home.join(".config").join("opencode-rust").join("AGENTS.md"),
        "Suffixed guidance.",
    )
    .expect("suffixed agents");

    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_titled_session(&engine, &location, "ses_cd13_v2").await;
    engine.prompt(&location, &session_id, "msg_user_1", "hello").await.expect("prompt exit");
    let paths = instruction_ledger_paths(&engine, &session_id).await;
    let v2 = home.join(".config").join("opencode").join("AGENTS.md");
    let suffixed = home.join(".config").join("opencode-rust").join("AGENTS.md");
    assert!(
        paths.contains(&v2.to_string_lossy().into_owned()),
        "the v2 global file is read: {paths:?}"
    );
    assert!(
        !paths.contains(&suffixed.to_string_lossy().into_owned()),
        "the suffixed dir loses when both exist: {paths:?}"
    );
}

#[tokio::test]
async fn engine_e2e_global_agents_md_backcompat_reads_the_suffixed_dir() {
    let llm = TestLlm::new();
    llm.push([parity_text_response("Done.", 1.0, 1.0)]);
    let (engine, dir, location) = engine_with(llm).await;
    // Only the old suffixed file exists — it must still be read.
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".config").join("opencode-rust")).expect("suffixed dir");
    std::fs::write(
        home.join(".config").join("opencode-rust").join("AGENTS.md"),
        "Suffixed guidance.",
    )
    .expect("suffixed agents");

    engine.location(location.clone()).await.expect("assemble");
    let session_id = create_titled_session(&engine, &location, "ses_cd13_compat").await;
    engine.prompt(&location, &session_id, "msg_user_1", "hello").await.expect("prompt exit");
    let paths = instruction_ledger_paths(&engine, &session_id).await;
    let suffixed = home.join(".config").join("opencode-rust").join("AGENTS.md");
    assert!(
        paths.contains(&suffixed.to_string_lossy().into_owned()),
        "back-compat: the suffixed-only install keeps its global instructions: {paths:?}"
    );
}
