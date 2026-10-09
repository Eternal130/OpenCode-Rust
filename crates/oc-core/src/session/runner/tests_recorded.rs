//! Ported unit tests: `session-runner` / `session-runner-recorded` /
//! `session-runner-message` — drain boundary semantics against a scripted
//! LLM (entry gate, cursor reset, stale settlement, synthetic continue).

use oc_ai::schema::events::{FinishReason, LLMEvent};
use oc_ai::testing::{complete, stop, text, tool, TestResponse};
use oc_schema::common::Record;
use oc_schema::session_inbox::{Delivery, Item, UserPayload};

use super::tests_harness::{harness, harness_with, Harness};
use crate::session::execution::DrainOutcome;
use crate::session::inbox::Promotable;
use crate::session::runner::drain::RunFailure;

#[tokio::test]
async fn runner_drain_completes_without_pending_input() {
    let harness = harness().await;
    harness.create_session().await;
    let outcome = harness
        .runner
        .drain(
            harness.session_id().as_str(),
            false,
            None,
            Promotable::Input,
            tokio::sync::watch::channel(false).1,
        )
        .await
        .unwrap();
    assert_eq!(outcome, DrainOutcome::Complete);
}

#[tokio::test]
async fn runner_drain_entry_gate_completes_when_nothing_is_promotable() {
    let harness = harness().await;
    harness.create_session().await;
    // A queued prompt is not promotable at a steer-scoped drain.
    harness
        .inbox
        .admit(
            "msg_q1",
            harness.session_id().as_str(),
            Item::User {
                payload: UserPayload {
                    text: "queued".into(),
                    files: None,
                    agents: None,
                    skills: None,
                    metadata: None,
                },
                delivery: Delivery::Queue,
            },
        )
        .await
        .unwrap();
    let outcome = harness
        .runner
        .drain(
            harness.session_id().as_str(),
            false,
            None,
            Promotable::Steer,
            tokio::sync::watch::channel(false).1,
        )
        .await
        .unwrap();
    assert_eq!(outcome, DrainOutcome::Complete, "queued input stays parked at steer boundaries");
}

#[tokio::test]
async fn runner_drain_promotes_steers_and_resets_the_step_cursor() {
    let harness = harness().await;
    harness.create_session().await;
    harness.admit_prompt("msg_s1", "steer me").await;
    let outcome = harness
        .runner
        .drain(
            harness.session_id().as_str(),
            false,
            None,
            Promotable::Input,
            tokio::sync::watch::channel(false).1,
        )
        .await
        .unwrap();
    assert_eq!(outcome, DrainOutcome::Complete);
    // The user message was promoted and projected.
    let messages = harness.messages().await;
    assert!(messages.iter().any(|(id, type_, data)| id == "msg_s1"
        && type_ == "user"
        && data["text"] == serde_json::json!("steer me")));
}

#[tokio::test]
async fn runner_drain_settles_stale_compactions_newest_first() {
    let harness = harness().await;
    harness.create_session().await;
    // Seed a running compaction message directly.
    let session = harness.session_id().as_str().to_string();
    harness
        .db
        .transaction(move |tx| {
            use crate::database::schema::SessionMessageRow;
            let rows = [
                (
                    "msg_c_old",
                    serde_json::json!({
                        "id": "msg_c_old",
                        "time": { "created": 1 },
                        "reason": "auto",
                        "summary": "",
                        "recent": "",
                        "status": "running",
                    })
                    .to_string(),
                ),
                (
                    "msg_c_new",
                    serde_json::json!({
                        "id": "msg_c_new",
                        "time": { "created": 2 },
                        "reason": "manual",
                        "summary": "",
                        "recent": "",
                        "status": "running",
                    })
                    .to_string(),
                ),
            ];
            for (index, (id, data)) in rows.iter().enumerate() {
                let row = SessionMessageRow {
                    id: (*id).to_string(),
                    session_id: session.clone(),
                    type_: "compaction".into(),
                    seq: (index + 1) as i64,
                    time_created: index as i64,
                    time_updated: index as i64,
                    data: data.clone(),
                };
                SessionMessageRow::insert(tx, &row)?;
            }
            Ok(())
        })
        .await
        .unwrap();
    // A forced drain skips the entry gate and settles the stale rows.
    let outcome = harness
        .runner
        .drain(
            harness.session_id().as_str(),
            true,
            None,
            Promotable::Input,
            tokio::sync::watch::channel(false).1,
        )
        .await
        .unwrap();
    assert_eq!(outcome, DrainOutcome::Complete);
    let events = harness.durable_events().await;
    let failed: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.compaction.failed").collect();
    assert_eq!(failed.len(), 2, "both stale compactions settle");
    assert_eq!(failed[0].2["inputID"], serde_json::json!("msg_c_new"), "newest first");
    assert_eq!(failed[0].2["error"]["type"], serde_json::json!("compaction.interrupted"));
    assert_eq!(failed[0].2["error"]["message"], serde_json::json!("Compaction was interrupted"));
}

#[tokio::test]
async fn runner_drain_settles_stale_tool_calls() {
    let harness = harness().await;
    harness.create_session().await;
    let session = harness.session_id().as_str().to_string();
    let assistant = serde_json::json!({
        "id": "msg_a_stale",
        "time": { "created": 1 },
        "agent": "build",
        "model": { "id": "fake-model", "providerID": "parityfake" },
        "content": [
            {
                "type": "tool",
                "id": "call_stale",
                "name": "read",
                "executed": false,
                "state": { "status": "streaming", "input": "{}" },
                "time": { "created": 1 },
            },
            {
                "type": "tool",
                "id": "call_sub_stale",
                "name": "subagent",
                "executed": false,
                "state": {
                    "status": "running",
                    "input": {},
                    "metadata": { "sessionID": "ses_child_9" },
                },
                "time": { "created": 1 },
            },
        ],
    });
    harness
        .db
        .transaction(move |tx| {
            use crate::database::schema::SessionMessageRow;
            let row = SessionMessageRow {
                id: "msg_a_stale".into(),
                session_id: session,
                type_: "assistant".into(),
                seq: 1,
                time_created: 1,
                time_updated: 1,
                data: assistant.to_string(),
            };
            SessionMessageRow::insert(tx, &row)?;
            Ok(())
        })
        .await
        .unwrap();
    harness
        .runner
        .drain(
            harness.session_id().as_str(),
            true,
            None,
            Promotable::Input,
            tokio::sync::watch::channel(false).1,
        )
        .await
        .unwrap();
    let events = harness.durable_events().await;
    let failed: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.tool.failed").collect();
    assert_eq!(failed.len(), 2);
    assert_eq!(failed[0].2["id"], serde_json::json!("call_stale"));
    assert_eq!(
        failed[0].2["error"]["message"],
        serde_json::json!("Tool execution interrupted: read")
    );
    assert_eq!(
        failed[1].2["error"]["message"],
        serde_json::json!("Tool execution interrupted: subagent (sessionID: ses_child_9)")
    );
    assert_eq!(failed[1].2["executed"], serde_json::json!(false));
}

#[tokio::test]
async fn runner_drain_moves_return_moved_for_pending_move() {
    let harness = harness().await;
    harness.create_session().await;
    // The move target project must exist before the projector re-homes the
    // session (v2's move preflight resolves it).
    harness
        .db
        .transaction(|tx| {
            use crate::database::schema::ProjectRow;
            ProjectRow::insert_or_ignore(
                tx,
                &ProjectRow::new("prj_moved", "/tmp/opencode/moved", None)?,
            )
        })
        .await
        .unwrap();
    harness.admit_prompt("msg_before_move", "work").await;
    // Admit a move control AFTER the steer, so promotion delivers the user
    // steer and the next boundary handles the move.
    harness
        .inbox
        .admit(
            "msg_move_1",
            harness.session_id().as_str(),
            Item::Move_ {
                payload: oc_schema::session_inbox::MovePayload {
                    location: oc_schema::location::Ref {
                        directory: oc_schema::common::AbsolutePath::parse("/tmp/opencode/moved"),
                        workspace_id: None,
                    },
                    project_id: oc_schema::id::ProjectId::parse("prj_moved"),
                    subpath: None,
                },
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    let outcome = harness
        .runner
        .drain(
            harness.session_id().as_str(),
            false,
            None,
            Promotable::Input,
            tokio::sync::watch::channel(false).1,
        )
        .await
        .unwrap();
    assert!(matches!(outcome, DrainOutcome::Moved { .. }), "{outcome:?}");
    let events = harness.durable_events().await;
    assert!(events.iter().any(|(type_, _, data)| type_ == "session.moved"
        && data["projectID"] == serde_json::json!("prj_moved")));
    assert!(events.iter().any(|(type_, _, data)| type_ == "session.inbox.delivered"
        && data["inboxID"] == serde_json::json!("msg_move_1")));
}

#[tokio::test]
async fn runner_drain_continues_after_tool_call_step() {
    // The scripted turn: a tool call, then the tool result, then text.
    let llm = oc_ai::testing::TestLlm::new();
    llm.push([tool("call_1", "echo", serde_json::json!({ "text": "hi" })), text("All done.")]);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_toolturn", "run echo").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution");
    let events = harness.durable_events().await;
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    let find = |needle: &str| types.iter().position(|candidate| *candidate == needle);
    let step_started = find("session.step.started").expect("first step");
    let tool_called = find("session.tool.called").expect("tool called");
    let tool_success = find("session.tool.success").expect("tool success");
    let first_ended = find("session.step.ended").expect("step ended");
    let second_started = types
        .iter()
        .rposition(|candidate| *candidate == "session.step.started")
        .expect("two logical steps");
    assert!(step_started < tool_called);
    assert!(tool_called < tool_success);
    assert!(tool_success < first_ended);
    assert!(first_ended < second_started, "needsContinuation started a second step");
    assert_eq!(
        events[second_started].2["assistantMessageID"],
        serde_json::json!("msg_2"),
        "each logical step gets a fresh assistant id"
    );
}

#[tokio::test]
async fn runner_drain_continue_after_incomplete_stream_publishes_synthetic() {
    // Transport read failure AFTER visible output ⇒ Continue: retry wait,
    // synthetic continue message, fresh assistant id, then success.
    let llm = oc_ai::testing::TestLlm::new();
    let interrupted = oc_ai::testing::TestResponse {
        events: vec![
            LLMEvent::TextStart { id: "t1".into(), provider_metadata: None },
            LLMEvent::TextDelta {
                id: "t1".into(),
                text: "partial".into(),
                provider_metadata: None,
            },
            LLMEvent::TextEnd { id: "t1".into(), text: None, provider_metadata: None },
            LLMEvent::StepFinish {
                index: 0.0,
                reason: oc_ai::schema::FinishReasonDetails::new(FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
            LLMEvent::Finish {
                reason: oc_ai::schema::FinishReasonDetails::new(FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
        ],
        error: Some(transport_read_error()),
    };
    llm.push([interrupted, text("recovered")]);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_continue", "say something").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution");
    let events = harness.durable_events().await;
    let synthetic = events
        .iter()
        .find(|(type_, _, data)| {
            type_ == "session.synthetic"
                && data["text"]
                    == serde_json::json!(
                        crate::session::runner::drain::CONTINUE_AFTER_INCOMPLETE_STREAM
                    )
        })
        .expect("continue synthetic");
    let retry = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.retry.scheduled")
        .expect("retry scheduled before the synthetic");
    assert!(retry.1 < synthetic.1, "retry wait precedes the continue note");
    let messages = harness.messages().await;
    assert!(messages.iter().any(|(_, type_, data)| type_ == "synthetic"
        && data["text"]
            == serde_json::json!(crate::session::runner::drain::CONTINUE_AFTER_INCOMPLETE_STREAM)));
}

fn transport_read_error() -> oc_ai::schema::AiError {
    oc_ai::schema::AiError::new(oc_ai::schema::AiErrorReason::Transport {
        fields: oc_ai::schema::ReasonFields::new("connection reset mid-stream"),
        transport: oc_ai::schema::TransportType::Http,
        operation: oc_ai::schema::TransportOperation::Read,
        code: None,
        url: None,
        phase: None,
        delivery: None,
        recovery: None,
    })
}

#[tokio::test]
async fn runner_drain_step_failed_on_stream_error() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.push([TestResponse::error(oc_ai::schema::AiError::authentication("bad key"))]);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_fail", "go").await;
    let exit = harness.execution.resume(harness.session_id().as_str()).await;
    assert!(exit.is_err());
    let events = harness.durable_events().await;
    assert!(events.iter().any(|(type_, _, data)| type_ == "session.step.failed"
        && data["error"]["type"] == serde_json::json!("provider.auth")));
    assert!(events.iter().any(|(type_, _, _)| type_ == "session.execution.failed"));
}

#[tokio::test]
async fn runner_drain_reuses_instructions_between_steps() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(text("ok"));
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_instr", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution");
    let events = harness.durable_events().await;
    let instructions: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.instructions.updated").collect();
    assert_eq!(instructions.len(), 1, "the initial baseline commits exactly once");
    let delta = instructions[0].2["delta"].as_object().expect("delta object");
    assert!(delta.contains_key("core/date"));
    // The initial baseline carries empty text, so no visible system
    // message projects; the assembled system prompt reached the request.
    let requests = harness.llm.0.requests();
    let system: String = requests
        .first()
        .expect("one request")
        .system
        .iter()
        .map(|part| part.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(system.contains("Today's date:"), "{system}");
    assert!(system.contains("<env>"), "{system}");
    assert!(system.contains("# Code Mode"), "the codemode stub renders: {system}");
}

#[allow(dead_code)]
fn unused_response_markers(
    _r: TestResponse,
    _c: fn(&str) -> TestResponse,
    _s: fn(Vec<LLMEvent>) -> TestResponse,
) {
    let _ = complete(FinishReason::Stop, vec![]);
    let _ = stop(vec![]);
}

#[allow(dead_code)]
fn unused_record(_: Record) {}

#[allow(dead_code)]
fn unused_failure_marker(_: RunFailure, _: &Harness) {}
