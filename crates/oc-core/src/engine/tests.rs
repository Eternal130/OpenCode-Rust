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
