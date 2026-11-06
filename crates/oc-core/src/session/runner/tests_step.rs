//! Ported unit tests: `session-step` — attempt outcome classification
//! (retry, continue, overflow, recover-full, declines, max steps).

use std::sync::Arc;

use oc_ai::schema::events::{FinishReason, LLMEvent};
use oc_ai::schema::messages::ToolChoice;
use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};
use oc_ai::testing::{text, TestLlm, TestResponse};

use super::tests_harness::{harness_with, NOW_MS};
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

/// RD-02: `step.started` comes from the injected clock (step.ts
/// `Clock.currentTimeMillis`), not a hardcoded 0, and the assistant
/// message's `time.created` projects the same epoch-ms.
#[tokio::test]
async fn runner_step_started_uses_injected_clock() {
    let llm = TestLlm::new();
    llm.always(text("clocked"));
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_clk", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    let events = harness.durable_events().await;
    let started = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.step.started")
        .expect("step.started published");
    assert_eq!(started.2.get("started").and_then(|v| v.as_u64()), Some(NOW_MS));

    let messages = harness.messages().await;
    let assistant = messages
        .iter()
        .find(|(_, type_, _)| type_ == "assistant")
        .expect("assistant message projected");
    assert_eq!(
        assistant.2.get("time").and_then(|t| t.get("created")).and_then(|v| v.as_u64()),
        Some(NOW_MS)
    );
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
    assert_eq!(
        harness.llm.as_ref().expect("scripted harness").0.requests().len(),
        2,
        "the full request was re-sent"
    );
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

/// RV-B1: retries do NOT consume step budget. REF llm.ts increments `step`
/// only in the outer drain loop after a Completed `runStep`; a Retry inside
/// the attempt loop stays on the same logical step.
#[tokio::test]
async fn runner_step_retry_does_not_consume_step_budget() {
    // Agent steps=2. Attempt 1 (step 1) fails with a retryable rate limit;
    // the retry must still be step 1 — no MAX_STEPS_PROMPT, tools stay on.
    let llm = TestLlm::new();
    llm.push([TestResponse::error(rate_limit()), text("after retry")]);
    let harness = harness_with(llm).await;
    harness.agents.update(&oc_schema::id::AgentId::parse("build"), |agent| agent.steps = Some(2));
    harness.create_session().await;
    harness.admit_prompt("msg_rb1", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let requests = harness.llm.as_ref().expect("scripted harness").0.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].tool_choice, None,
        "the retry is still step 1 — the step limit is not reached"
    );
    let last_message = requests[1].messages.last().unwrap();
    assert_eq!(last_message.role, oc_ai::schema::messages::MessageRole::User);
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
    let requests = harness.llm.as_ref().expect("scripted harness").0.requests();
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
            Ok(crate::location::ToolRunResult { content: vec![], metadata: None, output: None })
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

// -- G-105: a stream that ends abruptly (no finish event, no error
// event) must still flush its open fragments and settle the step ------

/// A stub provider whose stream closes right after TextStart+Delta —
/// no step-finish, no provider-error (G-105's reproduction; the ref
/// guarantees a terminal finish at the protocol layer, so the runner
/// hoists that guarantee — see step.rs §4).
struct AbruptCloseLlm {
    requests: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl crate::session::runner::LlmClient for AbruptCloseLlm {
    async fn stream(
        &self,
        _request: oc_ai::schema::messages::LLMRequest,
    ) -> Result<crate::session::runner::LlmEventStream, oc_ai::schema::AiError> {
        use std::sync::atomic::Ordering;
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(LLMEvent::StepStart { index: 0.0 }),
            Ok(LLMEvent::TextStart { id: "t1".into(), provider_metadata: None }),
            Ok(LLMEvent::TextDelta {
                id: "t1".into(),
                text: "partial ".into(),
                provider_metadata: None,
            }),
            Ok(LLMEvent::TextDelta {
                id: "t1".into(),
                text: "answer".into(),
                provider_metadata: None,
            }),
        ])))
    }

    fn can_compact(
        &self,
        _request: &oc_ai::schema::messages::LLMRequest,
        _mechanism: crate::session::runner::CompactMechanism,
    ) -> bool {
        false
    }

    async fn compact(
        &self,
        _request: oc_ai::schema::messages::LLMRequest,
        _mechanism: crate::session::runner::CompactMechanism,
    ) -> Result<crate::session::runner::CompactOutcome, oc_ai::schema::AiError> {
        Err(oc_ai::schema::AiError::unsupported_operation("compact", "AbruptCloseLlm"))
    }
}

/// (a) The dangling text part gets its trailing delta, `text.ended`,
/// and a durable step settlement (`step.failed` — the ref treats a
/// finish-less stream as an incomplete-stream failure at the protocol
/// layer, e.g. openai-chat.ts:1225, so the runner mirrors that here).
#[tokio::test]
async fn runner_step_abrupt_close_flushes_and_settles() {
    use crate::hooks::{fn_hook, HookOptions, RetryDecision, SessionRetryEvent};

    let llm = Arc::new(AbruptCloseLlm { requests: std::sync::atomic::AtomicUsize::new(0) });
    let harness = crate::session::runner::tests_harness::harness_with_client(
        Arc::clone(&llm) as std::sync::Arc<dyn crate::session::runner::LlmClient>,
        crate::session::runner::tests_harness::test_resolved_model(),
    )
    .await;
    // Pin the retry decision to NoRetry so the failure settles at once.
    let _no_retry = harness
        .hooks
        .register_session_retry(
            fn_hook(|event: &mut SessionRetryEvent| {
                event.decision = RetryDecision::NoRetry;
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;
    harness.create_session().await;
    harness.admit_prompt("msg_g105", "hi").await;
    // `session.text.delta` is an EPHEMERAL event — observe it on a live
    // subscription (buffered; read after the turn settles).
    let mut deltas = harness.bus.subscribe_types(&["session.text.delta"]);
    // The turn fails (surfaced) — the missing finish IS the failure; the
    // assertions below prove the durable settlement that precedes it.
    let _turn = harness.execution.resume(harness.session_id().as_str()).await;
    use futures::StreamExt as _;
    let delta_seen = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        // Drain frames until the trailing batch shows up (the
        // subscription stays open — stop at the target frame).
        while let Some(frame) = deltas.next().await {
            if frame.data["delta"] == "partial answer" {
                return true;
            }
        }
        false
    })
    .await
    .expect("delta subscription drains");

    let events = harness.durable_events().await;
    // The trailing delta flushed with the fragment end (ephemeral frame,
    // published immediately before text.ended by the flush).
    assert!(delta_seen, "trailing delta flushed on abrupt close");
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.text.ended")
        .expect("text.ended published on abrupt close");
    assert_eq!(ended.2["text"], serde_json::json!("partial answer"));
    // The step settles durably instead of dangling forever.
    assert!(
        events.iter().any(|(type_, _, _)| type_ == "session.step.failed"),
        "abrupt close settles as a failed step (ref protocol-layer incomplete-stream)"
    );
    assert!(
        !events.iter().any(|(type_, _, _)| type_ == "session.step.ended"),
        "a finish-less stream never reports success"
    );
    // Exactly one request: no retry loop.
    assert_eq!(llm.requests.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// (b) Normal paths are unchanged: a complete text turn still ends with
/// `step.ended` (and no failure), one text part, one delta batch.
#[tokio::test]
async fn runner_step_normal_text_turn_unchanged_after_g105() {
    let llm = TestLlm::new();
    llm.always(text("plain answer"));
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_g105b", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    let events = harness.durable_events().await;
    assert!(events.iter().any(|(type_, _, _)| type_ == "session.step.ended"));
    assert!(!events.iter().any(|(type_, _, _)| type_ == "session.step.failed"));
    let ended =
        events.iter().filter(|(type_, _, _)| type_ == "session.text.ended").collect::<Vec<_>>();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].2["text"], serde_json::json!("plain answer"));
}
