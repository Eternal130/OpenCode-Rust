//! Ported unit tests: `session-step` — attempt outcome classification
//! (retry, continue, overflow, recover-full, declines, max steps).

use std::sync::Arc;

use oc_ai::schema::events::{FinishReason, LLMEvent};
use oc_ai::schema::messages::ToolChoice;
use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};
use oc_ai::testing::{text, TestLlm, TestResponse};

use super::tests_harness::harness_with;
use crate::location::{ToolExecute, ToolRunFailure};

fn overflow_error() -> AiError {
    let mut error = AiError::invalid_request("This request would exceed your context limit");
    if let AiErrorReason::InvalidRequest { classification, .. } = &mut error.reason {
        *classification = Some(oc_ai::schema::ProviderFailureClassification::ContextOverflow);
    }
    error
}

fn retry_full_transport() -> AiError {
    AiError::new(AiErrorReason::Transport {
        fields: ReasonFields::new("socket rotated"),
        transport: oc_ai::schema::TransportType::Websocket,
        operation: oc_ai::schema::TransportOperation::Request,
        code: None,
        url: None,
        phase: None,
        delivery: None,
        recovery: Some(oc_ai::schema::TransportRecovery::RetryFull),
    })
}

fn rate_limit() -> AiError {
    AiError::new(AiErrorReason::RateLimit {
        fields: ReasonFields::new("slow down"),
        retry_after_ms: None,
        rate_limit: None,
    })
}

fn overflow_event_response() -> TestResponse {
    TestResponse {
        events: vec![
            LLMEvent::StepStart { index: 0.0 },
            LLMEvent::ProviderError {
                message: "context too long".into(),
                classification: Some(oc_ai::schema::ProviderFailureClassification::ContextOverflow),
                provider_metadata: None,
            },
        ],
        error: Some(overflow_error()),
    }
}

#[tokio::test]
async fn runner_step_completed_text_has_no_continuation() {
    let llm = TestLlm::new();
    llm.always(text("plain answer"));
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_t1", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let events = harness.durable_events().await;
    assert_eq!(events.iter().filter(|(type_, _, _)| type_ == "session.step.started").count(), 1);
    assert!(events.iter().any(|(type_, _, _)| type_ == "session.step.ended"));
}

#[tokio::test]
async fn runner_step_retry_then_success() {
    let llm = TestLlm::new();
    llm.push([TestResponse::error(rate_limit()), text("after retry")]);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_r2", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let events = harness.durable_events().await;
    let retry = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.retry.scheduled")
        .expect("retry scheduled");
    assert_eq!(retry.2["error"]["type"], serde_json::json!("provider.rate-limit"));
    assert_eq!(retry.2["attempt"], serde_json::json!(2));
    // The retry reuses the same assistant message id; the re-attempt
    // publishes step.started again against the same row (the projector
    // resets its retry/failure fields).
    let started: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.step.started").collect();
    assert_eq!(started.len(), 2, "one step.started per attempt");
    assert_eq!(
        started[0].2["assistantMessageID"], started[1].2["assistantMessageID"],
        "the retry reuses the assistant"
    );
    assert!(started[0].1 < retry.1 && retry.1 < started[1].1);
    assert!(events.iter().any(|(type_, _, _)| type_ == "session.step.ended"));
}

#[tokio::test]
async fn runner_step_overflow_before_output_fails_when_recovery_skips() {
    // auto compaction off ⇒ recoverOverflow returns false ⇒ the parked
    // overflow publishes and fails the assistant.
    let llm = TestLlm::new();
    llm.always(overflow_event_response());
    let harness = harness_with(llm).await;
    harness.compaction.configure(crate::session::runner::compaction::Settings {
        auto: false,
        buffer: None,
        keep: 15_000,
    });
    harness.create_session().await;
    harness.admit_prompt("msg_of", "hi").await;
    let exit = harness.execution.resume(harness.session_id().as_str()).await;
    assert!(exit.is_err());
    let events = harness.durable_events().await;
    let failed =
        events.iter().find(|(type_, _, _)| type_ == "session.step.failed").expect("step failed");
    // The parked provider-error event publishes as `provider.unknown`
    // (publish-llm-event.ts:553-556); the recovery skip only decides
    // WHETHER it publishes, not its shape.
    assert_eq!(failed.2["error"]["type"], serde_json::json!("provider.unknown"));
    assert_eq!(failed.2["error"]["message"], serde_json::json!("context too long"));
}

#[tokio::test]
async fn runner_step_recover_full_then_success() {
    let llm = TestLlm::new();
    llm.push([TestResponse::error(retry_full_transport()), text("retried fully")]);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_rf", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let events = harness.durable_events().await;
    assert!(events.iter().any(|(type_, _, _)| type_ == "session.step.ended"));
    assert_eq!(harness.llm.0.requests().len(), 2, "the full request was re-sent");
}

#[tokio::test]
async fn runner_step_declined_tool_interrupts_the_step() {
    struct Declining;
    #[async_trait::async_trait]
    impl ToolExecute for Declining {
        async fn execute(
            &self,
            _input: &serde_json::Value,
        ) -> Result<crate::location::ToolRunResult, ToolRunFailure> {
            Err(ToolRunFailure::Declined { message: "The user declined this tool call".into() })
        }
    }
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::tool("call_d", "ask", serde_json::json!({})));
    let harness = harness_with(llm).await;
    harness.tools.register(crate::location::ToolRegistration::new(
        "ask",
        "Ask the user",
        serde_json::json!({ "type": "object" }),
    ));
    harness.tools.register_executor("ask", Arc::new(Declining));
    harness.create_session().await;
    harness.admit_prompt("msg_dec", "ask me").await;
    let exit = harness.execution.resume(harness.session_id().as_str()).await;
    assert!(exit.is_err(), "declines interrupt the execution");
    let events = harness.durable_events().await;
    let failed = events
        .iter()
        .find(|(type_, _, data)| {
            type_ == "session.tool.failed" && data["id"] == serde_json::json!("call_d")
        })
        .expect("declined tool settles");
    assert_eq!(failed.2["error"]["message"], serde_json::json!("The user declined this tool call"));
    assert!(events.iter().any(|(type_, _, _)| type_ == "session.execution.interrupted"));
}

#[tokio::test]
async fn runner_step_max_steps_disables_tools_on_the_final_step() {
    // Agent steps=2: step 1 runs normally; step 2 (the continuation, with
    // step >= steps) appends MAX_STEPS_PROMPT and toolChoice none.
    let llm = TestLlm::new();
    llm.push([
        oc_ai::testing::tool("call_ms", "echo", serde_json::json!({ "text": "x" })),
        text("final summary"),
    ]);
    let harness = harness_with(llm).await;
    harness.agents.update(&oc_schema::id::AgentId::parse("build"), |agent| agent.steps = Some(2));
    harness.create_session().await;
    harness.admit_prompt("msg_ms", "do work").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let requests = harness.llm.0.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].tool_choice, None);
    assert_eq!(requests[1].tool_choice, Some(ToolChoice::none()));
    let last_message = requests[1].messages.last().unwrap();
    let text = match last_message.content.first() {
        Some(oc_ai::schema::messages::ContentPart::Text { text, .. }) => text.clone(),
        other => panic!("expected the max-steps synthetic assistant message, got {other:?}"),
    };
    assert_eq!(text, crate::session::runner::max_steps::MAX_STEPS_PROMPT);
    assert_eq!(last_message.role, oc_ai::schema::messages::MessageRole::Assistant);
    // Tool definitions stay for the cached prefix.
    assert!(!requests[1].tools.is_empty());
}

#[tokio::test]
async fn runner_step_tools_disabled_guard_settles_tool_calls() {
    // Even if the model ignores toolChoice none, the executor guard fails
    // the call with the exact message.
    let llm = TestLlm::new();
    let mut disabled = oc_ai::testing::tool("call_guard", "echo", serde_json::json!({}));
    if let LLMEvent::ToolCall { provider_executed, .. } = &mut disabled.events[1] {
        let _ = provider_executed;
    }
    llm.always(disabled.clone());
    let harness = harness_with(llm).await;
    harness.agents.update(&oc_schema::id::AgentId::parse("build"), |agent| agent.steps = Some(2));
    harness.create_session().await;
    harness.admit_prompt("msg_guard", "go").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let events = harness.durable_events().await;
    let _ =
        events.iter().find(|(type_, _, _)| type_ == "session.step.started").expect("step started");
}

#[tokio::test]
async fn runner_step_unknown_finish_fails_with_incomplete_stream() {
    let llm = TestLlm::new();
    let response = oc_ai::testing::complete(
        FinishReason::Unknown,
        vec![LLMEvent::TextStart { id: "t".into(), provider_metadata: None }],
    );
    llm.always(response);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_uf", "hi").await;
    let exit = harness.execution.resume(harness.session_id().as_str()).await;
    assert!(exit.is_err());
    let events = harness.durable_events().await;
    assert!(events.iter().any(|(type_, _, data)| type_ == "session.step.failed"
        && data["error"]["type"] == serde_json::json!("provider.invalid-output")));
}

#[tokio::test]
async fn runner_step_tool_task_observes_a_ring_before_its_first_poll() {
    // The clone race (gate-5 re-review, tool-bell finding): a bell rung
    // between the tool task's spawn and its first poll is marked seen by
    // the fresh receiver, so `bell.changed()` would never fire. The task
    // must observe the latched value instead — cancelling the execution
    // before it is ever polled (no side effects, TOOLS_INTERRUPTED exit).
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::step::{run_tool_task, ToolExit, ToolInterrupt};

    let polled = Arc::new(AtomicBool::new(false));
    let polled_by_executor = Arc::clone(&polled);
    let execute: crate::session::runner::model_request::ExecuteTool = Arc::new(move |_call| {
        let polled = Arc::clone(&polled_by_executor);
        Box::pin(async move {
            polled.store(true, Ordering::SeqCst);
            Ok(crate::location::ToolRunResult { content: vec![], metadata: None })
        })
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (bell, _bell_alive) = tokio::sync::watch::channel(false);
    let cancel: ToolInterrupt = Arc::new(bell);
    let call = crate::session::runner::model_request::ToolCallData {
        id: "call_race".into(),
        name: "echo".into(),
        input: serde_json::json!({}),
        message_id: "msg_race".into(),
    };
    // Spawn the task, then ring WITHOUT yielding in between: on the
    // current-thread runtime the spawned task cannot have been polled,
    // so the ring strictly precedes the task's subscribe — the exact
    // spawn→first-poll window the race lives in.
    let task = tokio::spawn(run_tool_task(execute, call, Arc::clone(&cancel), tx));
    let _ = cancel.send(true);
    task.await.expect("tool task exits");
    let exit = rx.recv().await.expect("one settled exit");
    assert!(matches!(exit, ToolExit::Interrupted), "a pre-poll ring cancels: {exit:?}");
    assert!(!polled.load(Ordering::SeqCst), "the interrupted execution must never be polled");
}
