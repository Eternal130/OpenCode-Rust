//! TOOLS-EXEC parity regression (ledger TOOL-D1): tool structured output
//! (`Tool.NormalizedResult.output`) must survive the execution pipeline.
//!
//! v2 semantics (ref: packages/core/src/tool.ts:135-160,
//! packages/core/src/tool/runtime.ts:45-59):
//!   - a leaf's declared `output` is encoded at the execute boundary;
//!   - `tool.execute.after` receives it in the result JSON (plugins can
//!     observe and mutate it);
//!   - `normalizeContent(afterEvent.result.content, afterEvent.result.output)`
//!     re-derives emptied content from the (possibly mutated) output;
//!   - the normalized result carries `output` back to the runner — Code
//!     Mode nested calls read it instead of the joined text
//!     (packages/core/src/codemode/tool.ts:103).
//!
//! Wire boundary pinned on purpose (ref:
//! packages/schema/src/session-message.ts:138-144): `ToolStateCompleted`
//! has NO `output` field — the client-facing session event stays
//! content+metadata. These tests pin the pipeline seams (hook event,
//! normalize fallback, runner-visible result), not the client wire.
//!
//! Hermetic: sandboxed engine roots under /tmp/opencode, no network.

use std::sync::Arc;

use oc_core::engine::{Engine, EngineOptions};
use oc_core::hooks::{fn_hook, HookOptions, ToolOutcome};
use oc_core::location::LocationRef;
use oc_core::session::store::CreateInput as SessionCreateInput;
use oc_schema::id::SessionId;
use serde_json::{json, Value};

/// Boots an engine over a sandboxed project (same shape as the
/// `tool_runtime` TOOLS-EXEC tests).
async fn engine_sandbox() -> (Arc<Engine>, tempfile::TempDir, LocationRef) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-parity-tools-exec-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    std::fs::create_dir_all(dir.path().join(".git")).expect("git marker");
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
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

/// Creates a session pre-approved for every tool action (the engine runs
/// the real permission pipeline).
async fn allow_all_session(engine: &Engine, location: &LocationRef, id: &str) -> SessionId {
    let session_id =
        SessionId::parse(id.to_string()).unwrap_or_else(|error| SessionId(error.value));
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
                title: Some("Structured output test".to_string()),
                agent: None,
                model: Some(model_ref()),
                metadata: None,
                permissions: Some(vec![oc_schema::permission::Rule {
                    action: "*".into(),
                    resource: "*".into(),
                    effect: oc_schema::permission::Effect::Allow,
                }]),
                parent_id: None,
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

/// The `write` leaf's declared output shape (write.rs:
/// `{operation, target, resource, existed}` — v2 write plugin).
fn expect_write_output(output: &Value) {
    let object = output.as_object().unwrap_or_else(|| {
        panic!("declared output must be an object, got: {output}");
    });
    assert_eq!(object.get("operation"), Some(&json!("write")), "output: {output}");
    assert_eq!(object.get("existed"), Some(&json!(false)), "output: {output}");
    assert!(
        object
            .get("resource")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("structured-output"),
        "output resource names the written file: {output}"
    );
}

/// TOOL-D1: the declared structured output of a tool survives the
/// execution pipeline — the `tool.execute.after` event carries it
/// (tool.ts:142-146) and a plugin that empties the content array gets the
/// stringified output back through `normalizeContent` (tool.ts:150,
/// runtime.ts:259-263).
#[tokio::test]
async fn structured_output_dropped() {
    // -- Section 1: the after-hook event JSON carries the output ----------
    let (engine, dir, location) = engine_sandbox().await;
    let services = engine.location(location.clone()).await.expect("assemble");
    let session = allow_all_session(&engine, &location, "ses_tool_d1_capture").await;

    let captured = Arc::new(parking_lot::Mutex::new(Vec::<Value>::new()));
    let sink = Arc::clone(&captured);
    let _keep = services
        .graph
        .hooks
        .register_tool_execute_after(
            fn_hook(move |event: &mut oc_core::hooks::ToolExecuteAfterEvent| {
                if event.tool == "write" {
                    if let ToolOutcome::Completed { result } = &event.outcome {
                        sink.lock().push(result.clone());
                    }
                }
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;

    let executor = services.graph.tools.executor("write").expect("write executor");
    let result = executor
        .execute_with_context(
            &json!({ "path": "structured-output.txt", "content": "payload" }),
            &call_context(&session),
        )
        .await
        .expect("write runs");
    assert!(
        std::fs::read_to_string(dir.path().join("structured-output.txt")).is_ok(),
        "the write itself succeeded"
    );

    let captured_json = {
        let captured = captured.lock();
        assert_eq!(captured.len(), 1, "exactly one after event for write");
        captured[0].clone()
    };
    let output = captured_json.get("output").cloned().unwrap_or_else(|| {
        panic!("after-hook result carries the declared output: {captured_json}")
    });
    expect_write_output(&output);

    // The runner-visible result carries the declared output through the
    // widened `ToolRunResult.output` seam (TOOL-D1; v2 NormalizedResult
    // flows to the runner — Code Mode nested calls read it,
    // codemode/tool.ts:103).
    let output = result.output.as_ref().unwrap_or_else(|| {
        panic!("ToolRunResult carries the declared output; content: {:?}", result.content)
    });
    expect_write_output(output);

    // -- Section 2: a plugin emptying content re-derives it from output ---
    let (engine, _dir, location) = engine_sandbox().await;
    let services = engine.location(location.clone()).await.expect("assemble");
    let session = allow_all_session(&engine, &location, "ses_tool_d1_normalize").await;

    let _keep = services
        .graph
        .hooks
        .register_tool_execute_after(
            fn_hook(|event: &mut oc_core::hooks::ToolExecuteAfterEvent| {
                if event.tool == "write" {
                    if let ToolOutcome::Completed { result } = &mut event.outcome {
                        // Suppress the model-facing content; v2's
                        // normalizeContent falls back to stringify(output).
                        result["content"] = json!([]);
                    }
                }
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;

    let executor = services.graph.tools.executor("write").expect("write executor");
    let result = executor
        .execute_with_context(
            &json!({ "path": "structured-output.txt", "content": "payload" }),
            &call_context(&session),
        )
        .await
        .expect("write runs");
    let text = match result.content.first() {
        Some(oc_schema::tool::Content::Text { text }) => text.clone(),
        other => panic!(
            "normalizeContent re-derives emptied content from the declared output; got: {other:?}"
        ),
    };
    let reparsed: Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| panic!("fallback text is the stringified output: {text}"));
    expect_write_output(&reparsed);
}
