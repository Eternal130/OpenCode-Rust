//! Port of `runner/step.ts` — one attempt's streaming, tool, and durable
//! settlement state machine (core-runner-map §1.4, finalization order 1–13;
//! runner-concurrency.md §2 tool fibers and §4 the uninterruptible
//! finalization region).
//!
//! Concurrency mapping: the provider stream and the tool-outcome channel
//! feed ONE single-writer loop; tool fibers are `tokio::spawn`ed tasks that
//! send their settled exit over an mpsc channel (never touching the
//! publisher record), so settlement marks happen in the loop's
//! single-threaded order. The finalization region runs after the stream
//! exits inside this same task; because the drain future's external
//! cancellation drops it at await boundaries, Wave A's coordinator treats
//! that drop as the interrupt — the finalization ordering contract holds
//! for every path the runner itself observes.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use futures::StreamExt;
use oc_ai::schema::events::LLMEvent;
use oc_ai::schema::messages::ToolChoice;
use oc_ai::schema::ProviderFailureClassification;
use oc_schema::common::{FinishReason, Record, SessionError};
use oc_schema::id::{AgentId, MessageId, SessionId};
use tokio::sync::mpsc;

use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};

use crate::bus::Bus;
use crate::location::{ToolRunFailure, ToolRunResult};
use crate::session::runner::model_request::{Prepared, ToolCallData};
use crate::session::runner::publisher::{
    LlmEventPublisher, PublisherError, SettlementDetails, ToolFailureScope,
};
use crate::session::runner::retry::Decision;
use crate::session::runner::{
    is_context_overflow_failure, is_interrupted_stream, is_retry_full_transport, is_retryable,
    to_session_error, unknown_finish_error, LlmClient, ResolvedModel, SessionUsage,
};

/// `TOOLS_INTERRUPTED` (step.ts:61).
#[must_use]
pub fn tools_interrupted() -> SessionError {
    SessionError {
        error_type: "aborted".into(),
        message: "Tool execution interrupted".into(),
        status: None,
        response: None,
    }
}

/// `STEP_INTERRUPTED` (step.ts:62).
#[must_use]
pub fn step_interrupted() -> SessionError {
    SessionError {
        error_type: "aborted".into(),
        message: "Step interrupted".into(),
        status: None,
        response: None,
    }
}

/// `RESULT_MISSING` (step.ts:63).
#[must_use]
pub fn result_missing() -> SessionError {
    SessionError {
        error_type: "tool.result-missing".into(),
        message: "Provider did not return a tool result".into(),
        status: None,
        response: None,
    }
}

/// The step attempt outcome (step.ts `Outcome`).
#[derive(Debug, Clone)]
pub enum Outcome {
    Completed { needs_continuation: bool },
    Retry { error: SessionError, decision: Decision },
    Continue { error: SessionError, decision: Decision },
    RecoverFull,
    Compacted,
}

/// How the attempt failed beyond its typed outcome (`Effect.failCause` +
/// interrupts + defects).
#[derive(Debug, Clone)]
pub enum AttemptFailure {
    /// `Effect.interrupt` — declines with an open location, or a step
    /// interrupt observed upstream.
    Interrupted,
    /// `failCause(stream.cause)` — the provider stream failed.
    Stream(AiError),
    /// `failCause(tools.failure)` — a tool fiber died (task panic).
    ToolDie(AiError),
    /// `StepFailedError` — a durably recorded step failure.
    StepFailed(SessionError),
    /// Publisher invariant violation (`Effect.die`).
    Defect(String),
}

impl From<PublisherError> for AttemptFailure {
    fn from(error: PublisherError) -> Self {
        AttemptFailure::Defect(error.0)
    }
}

/// One locally executed tool call's settled exit.
#[derive(Debug, Clone)]
enum ToolExit {
    Success {
        id: String,
        name: String,
        result: ToolRunResult,
    },
    Error {
        id: String,
        error: SessionError,
        metadata: Option<Record>,
    },
    Declined {
        id: String,
        message: String,
    },
    /// The per-tool shutdown channel fired mid-execution.
    Interrupted,
}

/// The attempt input (step.ts `Input`).
pub struct AttemptInput {
    pub session_id: SessionId,
    pub assistant_message_id: MessageId,
    pub agent: AgentId,
    pub model: ResolvedModel,
    pub prepared: Prepared,
    /// The runner's retry decider — `(cause, error, proposed)`.
    pub retry: Arc<
        dyn Fn(AiError, SessionError, bool) -> futures::future::BoxFuture<'static, Decision>
            + Send
            + Sync,
    >,
    pub recover_continuation: bool,
    /// Invoked only before durable output: `compact(reason: "overflow")`
    /// completed?
    pub recover_overflow: Arc<dyn Fn() -> futures::future::BoxFuture<'static, bool> + Send + Sync>,
    pub is_location_closed: Arc<dyn Fn() -> bool + Send + Sync>,
}

/// The step dependencies (step.ts `make`: bus + llm + snapshots).
pub struct StepDeps {
    pub bus: Bus,
    pub llm: Arc<dyn LlmClient>,
    /// The snapshot service (Wave C1); `None` disables the snapshot
    /// bracket (captures resolve to `None`, files stays absent).
    pub snapshot: Option<Arc<dyn crate::snapshot::SnapshotService>>,
}

enum LoopItem {
    Outcome(Option<ToolExit>),
    Event(Option<Result<LLMEvent, AiError>>),
}

/// The per-tool interrupt bell (runner-concurrency.md §2): fire-then-join,
/// interrupted executions report `TOOLS_INTERRUPTED` through the outcome
/// channel so classification stays in one place.
type ToolInterrupt = Arc<tokio::sync::Notify>;

/// `SessionStep.attempt` (step.ts:72-279).
pub async fn attempt(deps: &StepDeps, input: AttemptInput) -> Result<Outcome, AttemptFailure> {
    // startSnapshot (step.ts:73) — the attempt's snapshot bracket; `None`
    // when snapshots are disabled (capture degrades to None).
    let start_snapshot = match deps.snapshot.as_ref() {
        Some(service) => service.capture().await,
        None => None,
    };
    let publisher = LlmEventPublisher::new(
        deps.bus.clone(),
        input.session_id.as_str().to_string(),
        input.assistant_message_id.clone(),
        input.agent.clone(),
        input.model.reference.clone(),
        input.model.provider_metadata_key().to_string(),
        start_snapshot.clone(),
        0,
    );
    let tools_disabled = matches!(input.prepared.request.tool_choice, Some(ToolChoice::None {}));

    let (outcome_tx, mut outcome_rx) = mpsc::unbounded_channel::<ToolExit>();
    let cancel: ToolInterrupt = Arc::new(tokio::sync::Notify::new());
    let mut tool_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    let mut parked_overflow: Option<LLMEvent> = None;
    let mut publisher = publisher;

    // The single-writer loop: mark synchronously before every publish.
    let mut stream =
        deps.llm.stream(input.prepared.request.clone()).await.map_err(AttemptFailure::Stream)?;
    let mut stream_error: Option<AiError> = None;
    loop {
        let next = tokio::select! {
            biased;
            outcome = outcome_rx.recv() => LoopItem::Outcome(outcome),
            event = stream.next() => LoopItem::Event(event),
        };
        match next {
            LoopItem::Outcome(Some(exit)) => {
                settle_tool_exit(&mut publisher, exit).await?;
            }
            LoopItem::Outcome(None) => {}
            LoopItem::Event(Some(Ok(event))) => {
                if parked_overflow.is_some() || publisher.has_provider_error() {
                    continue;
                }
                if let LLMEvent::ProviderError { classification: Some(classification), .. } = &event
                {
                    if *classification == ProviderFailureClassification::ContextOverflow
                        && !publisher.record().output_started
                    {
                        // Park the failure: not published until after the
                        // overflow-recovery decision (§1.4 "Overflow
                        // pre-first-token").
                        parked_overflow = Some(event);
                        continue;
                    }
                }
                publisher.publish(&event).await?;
                if let LLMEvent::ToolCall {
                    id, name, input: call_input, provider_executed, ..
                } = &event
                {
                    if provider_executed.unwrap_or(false) {
                        continue;
                    }
                    // A local execution starts only after its Tool.Called
                    // publication completes (it just did, above).
                    if tools_disabled {
                        // The executeTool guard (step.ts:89-91).
                        publisher
                            .fail_tool(
                                id,
                                SessionError {
                                    error_type: "tool.execution".into(),
                                    message: "Tools are disabled after the maximum agent steps"
                                        .into(),
                                    status: None,
                                    response: None,
                                },
                                None,
                            )
                            .await?;
                        continue;
                    }
                    let execute = Arc::clone(&input.prepared.execute_tool);
                    let call = ToolCallData {
                        id: id.clone(),
                        name: name.clone(),
                        input: call_input.clone(),
                        message_id: input.assistant_message_id.as_str().to_string(),
                    };
                    let tx = outcome_tx.clone();
                    let cancel = Arc::clone(&cancel);
                    tool_tasks.push(tokio::spawn(async move {
                        let execution = execute(call.clone());
                        let outcome = tokio::select! {
                            biased;
                            () = cancel.notified() => ToolExit::Interrupted,
                            outcome = execution => match outcome {
                                Ok(result) => ToolExit::Success {
                                    id: call.id,
                                    name: call.name,
                                    result,
                                },
                                Err(ToolRunFailure::Error { message, metadata }) => ToolExit::Error {
                                    id: call.id,
                                    error: SessionError {
                                        error_type: "tool.execution".into(),
                                        message,
                                        status: None,
                                        response: None,
                                    },
                                    metadata,
                                },
                                Err(ToolRunFailure::Declined { message }) => ToolExit::Declined {
                                    id: call.id,
                                    message,
                                },
                            },
                        };
                        let _ = tx.send(outcome);
                    }));
                }
            }
            LoopItem::Event(Some(Err(error))) => {
                stream_error = Some(error);
                // The provider stream is still consumed to its end after a
                // failure event; read on until the stream closes.
            }
            LoopItem::Event(None) => break,
        }
    }

    // Finalization (uninterruptibleMask; step.ts:136-275).
    // 1. The stream exited; publisher.hasStarted() ⇒ streamed().
    if parked_overflow.is_none() && publisher.has_started() {
        publisher.streamed().await?;
    }

    // 2. Join the tool fibers, then drain their outcomes.
    let mut tool_die: Option<AiError> = None;
    for task in tool_tasks.drain(..) {
        if let Err(join_error) = task.await {
            // Die-reasons-only: a panicked tool fiber is a defect.
            tool_die = Some(AiError::new(AiErrorReason::UnknownProvider {
                fields: ReasonFields::new(format!("Tool execution died: {join_error}")),
            }));
        }
    }
    drop(outcome_tx);
    let mut exits: Vec<ToolExit> = Vec::new();
    while let Some(exit) = outcome_rx.recv().await {
        exits.push(exit);
    }
    for exit in &exits {
        settle_tool_exit(&mut publisher, exit.clone()).await?;
    }

    // 2b. Classify the tool exits (step.ts:288-311).
    let declines: Vec<(String, String)> = exits
        .iter()
        .filter_map(|exit| match exit {
            ToolExit::Declined { id, message } => Some((id.clone(), message.clone())),
            _ => None,
        })
        .collect();
    let tools_interrupted_flag = exits.iter().any(|exit| matches!(exit, ToolExit::Interrupted));

    // 3. Overflow recovery.
    let overflow =
        parked_overflow.is_some() || stream_error.as_ref().is_some_and(is_context_overflow_failure);
    if !publisher.record().output_started && overflow && (input.recover_overflow)().await {
        return Ok(Outcome::Compacted);
    }

    if let Some(event) = parked_overflow.take() {
        publisher.publish(&event).await?;
    }
    // 4. unknownFinish ⇒ InvalidProviderOutput incomplete-stream.
    let record = publisher.record();
    let unknown_finish = if stream_error.is_none()
        && record.finish.as_ref().map(|s| s.finish) == Some(FinishReason::Unknown)
    {
        Some(unknown_finish_error())
    } else {
        None
    };
    let llm_failure: Option<AiError> = stream_error.clone().or(unknown_finish);
    let llm_error: Option<SessionError> =
        llm_failure.as_ref().filter(|_| !record.provider_failed).map(to_session_error);

    // 5. RecoverFull.
    if input.recover_continuation
        && llm_failure.as_ref().is_some_and(is_retry_full_transport)
        && !record.output_started
    {
        return Ok(Outcome::RecoverFull);
    }

    // 6. Retry decision.
    let retry_decision = match (&llm_failure, &llm_error) {
        (Some(failure), Some(error)) if !is_context_overflow_failure(failure) => {
            let proposed =
                is_retryable(failure) || (record.output_started && is_interrupted_stream(failure));
            Some((input.retry)(failure.clone(), error.clone(), proposed).await)
        }
        _ => None,
    };
    if let (Some(_), Some(error), Some(decision)) = (&llm_failure, &llm_error, &retry_decision) {
        if decision.retry && !record.output_started {
            // Retry state projects onto the existing assistant, even before
            // it has produced output.
            publisher.start_assistant().await?;
            return Ok(Outcome::Retry { error: error.clone(), decision: *decision });
        }
    }
    // 7. failAssistant.
    if let Some(error) = &llm_error {
        publisher.fail_assistant(error.clone()).await?;
    }

    // 8. Declines.
    for (id, message) in &declines {
        let error = SessionError {
            error_type: "aborted".into(),
            message: if (input.is_location_closed)() {
                "Interaction cancelled because the location shut down".to_string()
            } else {
                message.clone()
            },
            status: None,
            response: None,
        };
        publisher.fail_tool(id, error, None).await?;
    }
    // 9. Tool failure / step interrupt.
    let interrupted = !declines.is_empty() || tools_interrupted_flag;
    let tool_failure = if interrupted {
        Some(tools_interrupted())
    } else if let Some(die) = &tool_die {
        Some(to_session_error(die))
    } else if record.provider_failed {
        Some(tools_interrupted())
    } else {
        None
    };
    if let Some(error) = tool_failure {
        publisher.fail_unsettled_tools(error, ToolFailureScope::All).await?;
    }
    if interrupted {
        publisher.fail_assistant(step_interrupted()).await?;
    }

    // 10. Missing hosted results.
    if llm_error.is_some() || (stream_error.is_none() && !record.provider_failed) {
        let missing =
            publisher.fail_unsettled_tools(result_missing(), ToolFailureScope::Hosted).await?;
        if missing && llm_error.is_none() && record.finish.is_none() {
            publisher.fail_assistant(result_missing()).await?;
        }
    }

    // 11. Durable settlement (step.ts:226-251): capture the end snapshot;
    // files = [] when the trees are equal, the diff otherwise, absent when
    // either capture failed.
    let record = publisher.record();
    if record.finish.is_some() || record.failure.is_some() {
        let snapshot = match deps.snapshot.as_ref() {
            Some(service) => service.capture().await,
            None => None,
        };
        let files: Option<Vec<String>> = match (&start_snapshot, &snapshot) {
            (Some(from), Some(to)) if from == to => Some(Vec::new()),
            (Some(from), Some(to)) => match deps.snapshot.as_ref() {
                Some(service) => service
                    .files(from, to)
                    .await
                    .ok()
                    .map(|files| files.into_iter().map(|path| path.0).collect()),
                None => None,
            },
            _ => None,
        };
        let details: SettlementDetails = record
            .finish
            .as_ref()
            .map(|settlement| SettlementDetails {
                cost: Some(SessionUsage::calculate_cost(&input.model.cost, &settlement.tokens)),
                tokens: Some(settlement.tokens.clone()),
                snapshot: snapshot.clone(),
                files: files.clone(),
            })
            .unwrap_or_default();
        if record.failure.is_some() {
            publisher.publish_step_failure(&details).await?;
        } else {
            publisher.publish_step_ended(&details).await?;
        }
    }

    // 12. Continue outcome.
    if let (Some(_), Some(error), Some(decision)) = (&llm_failure, &llm_error, &retry_decision) {
        if decision.retry && record.output_started && declines.is_empty() && !tools_interrupted_flag
        {
            return Ok(Outcome::Continue { error: error.clone(), decision: *decision });
        }
    }

    // 13. Returns.
    if let Some(error) = stream_failed(stream_error.as_ref(), tool_die.as_ref()) {
        return Err(AttemptFailure::Stream(error));
    }
    if !declines.is_empty() {
        if (input.is_location_closed)() {
            return Ok(Outcome::Completed { needs_continuation: true });
        }
        return Err(AttemptFailure::Interrupted);
    }
    if let (true, Some(die)) = (tools_interrupted_flag, tool_die) {
        return Err(AttemptFailure::ToolDie(die));
    }
    if let Some(failure) = record.failure {
        return Err(AttemptFailure::StepFailed(failure));
    }
    Ok(Outcome::Completed { needs_continuation: !tools_disabled && record.needs_continuation })
}

/// `failTool`/`toolExecution` for one settled exit (declines and interrupts
/// settle during finalization).
async fn settle_tool_exit(
    publisher: &mut LlmEventPublisher,
    exit: ToolExit,
) -> Result<(), PublisherError> {
    match exit {
        ToolExit::Success { id, name, result } => {
            // toolOutput.truncate — TODO(wave-C): the 2000-line / 50KiB
            // tool-output budget lands with the tool lane.
            publisher.tool_execution(&id, &name, &result).await
        }
        ToolExit::Error { id, error, metadata } => {
            publisher.fail_tool(&id, error, metadata).await.map(|_| ())
        }
        ToolExit::Declined { .. } | ToolExit::Interrupted => Ok(()),
    }
}

fn stream_failed(stream_error: Option<&AiError>, tool_die: Option<&AiError>) -> Option<AiError> {
    stream_error.cloned().or_else(|| tool_die.cloned())
}
