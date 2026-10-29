//! Port of `session/runner/llm.ts` — the drain outer loop +
//! `advanceToStep` boundary state machine + `runStep` logical-step policy
//! (core-runner-map §1.1, §1.3), wired as the injectable drain for
//! [`crate::session::execution::SessionExecution`].

#![allow(clippy::too_many_lines)]

use std::collections::HashMap;
use std::sync::Arc;

use oc_schema::id::{MessageId, SessionId};
use oc_schema::session_inbox::Info as InboxInfo;
use oc_schema::session_message::Info as MessageInfo;
use oc_util::determinism::{Clock, IdGen, RngSource};
use parking_lot::Mutex;

use crate::bus::{Bus, PublishOptions};
use crate::database::Database;
use crate::session::execution::{Continuation, DrainOutcome, RunError};
use crate::session::inbox::{Promotable, SessionInbox};
use crate::session::runner::compaction::{
    Reason as CompactionReason, SessionCompaction, Trigger as CompactionTrigger,
};
use crate::session::runner::context::{self, Loaded, SessionContext};
use crate::session::runner::model_request::{prepare_primary, SessionModelRequest};
use crate::session::runner::prompt_prepare::PromptPrepare;
use crate::session::runner::step::{AttemptFailure, AttemptInput, Outcome, StepDeps};
use crate::session::runner::title::SessionTitle;
use crate::session::runner::LlmClient;
use crate::session::store::SessionStore;

/// `CONTINUE_AFTER_INCOMPLETE_STREAM` (llm.ts:35-36).
pub const CONTINUE_AFTER_INCOMPLETE_STREAM: &str = "The previous response was interrupted. Continue from where you left off without repeating completed content.";

/// Resolves once the shutdown watch is (or becomes) signalled — the
/// interrupt signal compaction awaits race against (RD-11).
async fn shutdown_signal(shutdown: &tokio::sync::watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    let mut watched = shutdown.clone();
    let _ = watched.changed().await;
}

/// `StepFailedError`.
#[derive(Debug, Clone, PartialEq)]
pub struct StepFailed(pub oc_schema::common::SessionError);

/// One drain boundary result (`DrainResult.Ready` carries the loaded
/// context).
enum Boundary {
    Ready(Box<Loaded>, u32),
    Complete,
    Moved { continuation: Option<Continuation> },
    Reloaded { force: bool, continuation: Option<Continuation> },
}

/// The session runner service (llm.ts `layer`).
pub struct SessionRunner {
    pub(crate) db: Database,
    pub(crate) bus: Bus,
    inbox: SessionInbox,
    #[allow(dead_code)]
    store: SessionStore,
    pub(crate) context: Arc<SessionContext>,
    compaction: Arc<SessionCompaction>,
    title: Arc<SessionTitle>,
    pub(crate) llm: Arc<dyn LlmClient>,
    pub(crate) requests: Arc<SessionModelRequest>,
    /// The one-shot generation service (`session/generate.ts`) — exposed
    /// for the server's `session.generate` route; never mutates sessions.
    pub(crate) generate_service: Arc<crate::session::runner::SessionGenerate>,
    clock: Arc<dyn Clock>,
    rng: Arc<dyn RngSource>,
    ids: Arc<dyn IdGen>,
    /// Whether the runner's location has closed (reload boundary).
    location_closed: Arc<dyn Fn() -> bool + Send + Sync>,
    /// The snapshot service for step settlements (`None` disables the
    /// bracket; Wave C1).
    snapshot: Option<Arc<dyn crate::snapshot::SnapshotService>>,
    /// The tool-output budget service (`None` publishes raw tool results).
    tool_output: Option<Arc<crate::tool_output::ToolOutput>>,
    /// The model-input image limits (RV-G3): applied to settled tool
    /// content via `normalizeImages`. REF defaults when not configured.
    image_limits: crate::image::Limits,
    /// `FiberMap`-equivalent for title generation: one live task per
    /// session, `onlyIfMissing`.
    title_tasks: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// The `SessionPrompt.prepare` seam (P01/P02): `session.prompt`
    /// dispatch + `@skill` resolution on the consume boundary. `None`
    /// = not attached (the pre-seam behavior).
    prompt_prepare: Option<Arc<PromptPrepare>>,
}

impl SessionRunner {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        db: Database,
        bus: Bus,
        inbox: SessionInbox,
        store: SessionStore,
        context: Arc<SessionContext>,
        compaction: Arc<SessionCompaction>,
        title: Arc<SessionTitle>,
        llm: Arc<dyn LlmClient>,
        requests: Arc<SessionModelRequest>,
        clock: Arc<dyn Clock>,
        rng: Arc<dyn RngSource>,
        ids: Arc<dyn IdGen>,
        location_closed: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            db: db.clone(),
            bus,
            inbox,
            store,
            context: Arc::clone(&context),
            compaction,
            title,
            llm: Arc::clone(&llm),
            requests: Arc::clone(&requests),
            generate_service: Arc::new(crate::session::runner::SessionGenerate::new(
                db.clone(),
                Arc::clone(&context),
                Arc::clone(&requests),
                Arc::clone(&llm),
            )),
            clock,
            rng,
            ids,
            location_closed,
            snapshot: None,
            tool_output: None,
            image_limits: crate::image::Limits::default(),
            title_tasks: Mutex::new(HashMap::new()),
            prompt_prepare: None,
        }
    }

    /// The location's compaction service (CONFIG-C1: the engine's
    /// `ConfigCompactionPlugin` wiring — `compaction.auto` /
    /// `keep.tokens` / `buffer` from the location's config documents —
    /// is observable through `compaction().settings()`).
    #[must_use]
    pub fn compaction(&self) -> &Arc<SessionCompaction> {
        &self.compaction
    }

    /// Attaches the `SessionPrompt.prepare` seam (the engine's
    /// production wiring — `session.prompt` hook dispatch + `@skill`
    /// resolution over the location's skill table; guest hooks ride
    /// the same registry seam, PLUG-D21).
    #[must_use]
    pub fn with_prompt_prepare(mut self, prepare: Arc<PromptPrepare>) -> Self {
        self.prompt_prepare = Some(prepare);
        self
    }

    /// The attached `SessionPrompt.prepare` seam, if any (the HTTP
    /// prompt handler calls [`PromptPrepare::prepare_input`] on the
    /// admission path — submit-time hook dispatch per ref
    /// prompt.ts:29-52 — before admitting the inbox row).
    #[must_use]
    pub fn prompt_prepare(&self) -> Option<&Arc<PromptPrepare>> {
        self.prompt_prepare.as_ref()
    }

    /// One-shot generation over the session context (`session.generate`
    /// route backing) — no session side effects.
    pub fn generation(&self) -> &Arc<crate::session::runner::SessionGenerate> {
        &self.generate_service
    }

    /// The runner's LLM client seam (the one-shot `experimental.generate`
    /// route streams through it directly).
    pub fn llm(&self) -> &Arc<dyn LlmClient> {
        &self.llm
    }

    /// Attaches the snapshot service (Wave C1): step settlements capture
    /// the bracket and publish `files` diffs.
    #[must_use]
    pub fn with_snapshot(mut self, snapshot: Arc<dyn crate::snapshot::SnapshotService>) -> Self {
        self.snapshot = Some(snapshot);
        self
    }

    /// Attaches the shared tool-output budget service: settled tool
    /// results pass through `toolOutput.truncate` (step.ts:122) before
    /// the terminal publish.
    #[must_use]
    pub fn with_tool_output(mut self, service: Arc<crate::tool_output::ToolOutput>) -> Self {
        self.tool_output = Some(service);
        self
    }

    /// Attaches the model-input image limits (RV-G3, image.ts): settled
    /// tool content passes through `normalizeImages` after the output
    /// budget.
    #[must_use]
    pub fn with_image_limits(mut self, limits: crate::image::Limits) -> Self {
        self.image_limits = limits;
        self
    }

    /// The injectable drain for
    /// [`crate::session::execution::SessionExecution::new`].
    #[must_use]
    pub fn drain_fn(self: &Arc<Self>) -> crate::session::execution::DrainFn {
        let runner = Arc::clone(self);
        Arc::new(move |session_id, force, continuation, promotable, shutdown| {
            let runner = Arc::clone(&runner);
            Box::pin(async move {
                runner
                    .drain(&session_id, force, continuation, promotable, shutdown)
                    .await
                    .map_err(run_error_of)
            })
        })
    }

    /// `drain` (llm.ts:54-196).
    pub async fn drain(
        &self,
        session_id: &str,
        force: bool,
        continuation: Option<Continuation>,
        promotable: Promotable,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<DrainOutcome, RunFailure> {
        let mut force = force;
        let mut continuing = continuation.is_some();
        let mut step = continuation.map(|continuation| continuation.step).unwrap_or(1);
        let mut entering = true;

        // Entry gate (llm.ts:61-66).
        if !force && !continuing {
            let pending = self.inbox.next_promotable(session_id, Promotable::Input).await?;
            let Some(pending) = pending else { return Ok(DrainOutcome::Complete) };
            let control = matches!(pending, InboxInfo::Compaction { .. } | InboxInfo::Move { .. });
            let queued = matches!(
                pending,
                InboxInfo::User { delivery: oc_schema::session_inbox::Delivery::Queue, .. }
                    | InboxInfo::Synthetic {
                        delivery: oc_schema::session_inbox::Delivery::Queue,
                        ..
                    }
            );
            if promotable == Promotable::Steer && queued && !control {
                return Ok(DrainOutcome::Complete);
            }
        }
        self.settle_stale_compactions(session_id).await?;
        self.settle_stale_tool_calls(session_id).await?;

        // Main loop (llm.ts:188-195): advance to a Ready boundary, run one
        // step, continue.
        loop {
            let next = self
                .advance_to_step(
                    session_id, force, continuing, step, entering, promotable, &shutdown,
                )
                .await?;
            match next {
                Boundary::Ready(context, ready_step) => {
                    let needs_continuation = self.run_step(context, ready_step, &shutdown).await?;
                    step = ready_step + 1;
                    force = false;
                    entering = false;
                    continuing = needs_continuation;
                }
                Boundary::Complete => return Ok(DrainOutcome::Complete),
                Boundary::Moved { continuation } => {
                    return Ok(DrainOutcome::Moved { continuation });
                }
                Boundary::Reloaded { force, continuation } => {
                    return Ok(DrainOutcome::Reloaded { force, continuation });
                }
            }
        }
    }

    /// `advanceToStep` (llm.ts:71-186) — the uninterruptible boundary loop.
    #[allow(clippy::too_many_arguments)]
    async fn advance_to_step(
        &self,
        session_id: &str,
        mut force: bool,
        continuing: bool,
        mut step: u32,
        mut entering: bool,
        promotable: Promotable,
        shutdown: &tokio::sync::watch::Receiver<bool>,
    ) -> Result<Boundary, RunFailure> {
        loop {
            // 1. Location closed ⇒ close the transport, Reloaded.
            if (self.location_closed)() {
                // modelTransport.close (REF llm.ts:78) — the session's
                // WebSocket channel closes with the location.
                self.llm.close_transport(session_id).await;
                return Ok(Boundary::Reloaded {
                    force,
                    continuation: continuing.then_some(Continuation { step }),
                });
            }
            // 2. Peek under the per-session inbox mutex.
            let pending = self
                .inbox
                .serialized(session_id, async {
                    let scope = if entering || !continuing { Promotable::Input } else { Promotable::Steer };
                    let next = self.inbox.next_promotable(session_id, scope).await?;
                    match &next {
                        Some(InboxInfo::Compaction { id, .. }) => {
                            // Eager compaction-started publish: the inbox
                            // item becomes the running message immediately.
                            let delivered = self
                                .bus
                                .publish(
                                    def("session.inbox.delivered"),
                                    serde_json::json!({ "sessionID": session_id, "inboxID": id.as_str() }),
                                    PublishOptions::default(),
                                )
                                .await;
                            if let Err(error) = delivered {
                                return Err(RunFailure::Defect(format!(
                                    "failed to publish inbox delivered: {error}"
                                )));
                            }
                            let started = self
                                .bus
                                .publish(
                                    def("session.compaction.started"),
                                    serde_json::json!({
                                        "sessionID": session_id,
                                        "reason": "manual",
                                        "recent": "",
                                        "inputID": id.as_str(),
                                    }),
                                    PublishOptions::default(),
                                )
                                .await;
                            if let Err(error) = started {
                                return Err(RunFailure::Defect(format!(
                                    "failed to publish compaction started: {error}"
                                )));
                            }
                        }
                        Some(InboxInfo::Move { id, payload, .. }) => {
                            // Close transport (REF llm.ts:95
                            // modelTransport.close), then InboxDelivered + Moved.
                            self.llm.close_transport(session_id).await;
                            let delivered = self
                                .bus
                                .publish(
                                    def("session.inbox.delivered"),
                                    serde_json::json!({ "sessionID": session_id, "inboxID": id.as_str() }),
                                    PublishOptions::default(),
                                )
                                .await;
                            if let Err(error) = delivered {
                                return Err(RunFailure::Defect(format!(
                                    "failed to publish inbox delivered: {error}"
                                )));
                            }
                            let moved = self
                                .bus
                                .publish(
                                    def("session.moved"),
                                    serde_json::json!({
                                        "sessionID": session_id,
                                        "location": serde_json::to_value(&payload.location).unwrap_or_default(),
                                        "projectID": payload.project_id,
                                        "subpath": payload.subpath,
                                    }),
                                    PublishOptions::default(),
                                )
                                .await;
                            if let Err(error) = moved {
                                return Err(RunFailure::Defect(format!(
                                    "failed to publish session moved: {error}"
                                )));
                            }
                        }
                        _ => {}
                    }
                    Ok(next)
                })
                .await?;
            // 3. Cursor reset rule: steers preserve the step counter.
            let steer_delivered = matches!(
                &pending,
                Some(InboxInfo::User { delivery: oc_schema::session_inbox::Delivery::Steer, .. })
                    | Some(InboxInfo::Synthetic {
                        delivery: oc_schema::session_inbox::Delivery::Steer,
                        ..
                    })
            );
            if !continuing && !steer_delivered {
                entering = true;
                step = 1;
            }
            // 4. Move pending ⇒ Moved (execution re-drains the same
            //    continuation).
            if matches!(pending, Some(InboxInfo::Move { .. })) {
                return Ok(Boundary::Moved {
                    continuation: continuing.then_some(Continuation { step }),
                });
            }
            // 5. Manual compaction.
            if let Some(InboxInfo::Compaction { id, .. }) = &pending {
                let outcome = self.manual_compaction(session_id, id.as_str(), shutdown).await;
                match outcome {
                    Ok(()) => {
                        force = false;
                        continue;
                    }
                    Err(error) => {
                        // Failure ⇒ Compaction.Failed + rethrow.
                        let failure = match &error {
                            RunFailure::Interrupted => oc_schema::common::SessionError {
                                error_type: "aborted".into(),
                                message: "Compaction cancelled".into(),
                                status: None,
                                response: None,
                            },
                            other => oc_schema::common::SessionError {
                                error_type: "compaction.failed".into(),
                                message: other.to_string(),
                                status: None,
                                response: None,
                            },
                        };
                        let _ = self
                            .bus
                            .publish(
                                def("session.compaction.failed"),
                                serde_json::json!({
                                    "sessionID": session_id,
                                    "reason": "manual",
                                    "inputID": id.as_str(),
                                    "error": serde_json::to_value(&failure).unwrap_or_default(),
                                }),
                                PublishOptions::default(),
                            )
                            .await;
                        return Err(error);
                    }
                }
            }
            // 6. Complete check.
            let queued = matches!(
                &pending,
                Some(InboxInfo::User { delivery: oc_schema::session_inbox::Delivery::Queue, .. })
                    | Some(InboxInfo::Synthetic {
                        delivery: oc_schema::session_inbox::Delivery::Queue,
                        ..
                    })
            );
            if !force
                && !continuing
                && (pending.is_none() || (queued && promotable == Promotable::Steer))
            {
                return Ok(Boundary::Complete);
            }
            // 7. Ready path: prepare prompts (the `session.prompt` +
            //    `@skill` seam fires here — before delivery and the
            //    model turn), prepare context, promote, fork titles,
            //    load.
            self.prepare_session_prompts(session_id).await?;
            let selected = self.context.prepare_context(session_id).await?;
            let promoted = self
                .inbox
                .promote(
                    session_id,
                    if entering && !continuing { promotable } else { Promotable::Steer },
                )
                .await
                .map_err(|error| RunFailure::Defect(format!("promote failed: {error}")))?;
            // A control admitted during context preparation owns this
            // boundary.
            if promoted.is_none() {
                continue;
            }
            if promoted.unwrap_or(0) > 0 {
                if selected.session.parent_id.is_none()
                    && crate::session::runner::title::is_untitled(&selected.session)
                {
                    self.fork_title(session_id);
                }
                step = 1;
            }
            let loaded = self.context.load(&selected).await?;
            return Ok(Boundary::Ready(Box::new(loaded), step));
        }
    }

    /// `SessionPrompt.prepare` on the consume boundary (RUNTIME-P01 /
    /// P02, core/session/prompt.ts:38-84): fires `session.prompt`
    /// (copy-in/copy-out per ADR-0005) and resolves `@skill`
    /// attachments for every not-yet-prepared pending user row,
    /// BEFORE the promote path delivers them — the projected user
    /// message and the model turn both see the prepared prompt. A
    /// failed prepare cancels the poisoned row (v2 never admits a
    /// prompt whose prepare failed) and fails the drain with the
    /// error surfaced to the session.
    async fn prepare_session_prompts(&self, session_id: &str) -> Result<(), RunFailure> {
        let Some(prepare) = self.prompt_prepare.as_ref() else { return Ok(()) };
        if !prepare.is_armed() {
            return Ok(());
        }
        let parsed =
            SessionId::parse(session_id.to_string()).unwrap_or_else(|error| SessionId(error.value));
        if let Err((id, error)) = prepare.prepare_pending(&parsed).await {
            // Cancel the poisoned pending row (best-effort: a bus
            // failure still surfaces the prepare error below).
            let _ = self.inbox.cancel(session_id, &id).await;
            return Err(RunFailure::surfaced(oc_schema::common::SessionError {
                error_type: "prompt.failed".into(),
                message: format!("prompt {id} rejected: {error}"),
                status: None,
                response: None,
            }));
        }
        Ok(())
    }

    /// The manual-compaction run (llm.ts:113-157): preview context, then
    /// `compact(reason: "manual", inputID)`.
    async fn manual_compaction(
        &self,
        session_id: &str,
        input_id: &str,
        shutdown: &tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), RunFailure> {
        let selected = self.context.select(session_id).await?;
        let model = self.context.resolve_model(&selected.session).map_err(|error| {
            RunFailure::unsurfaced(oc_schema::common::SessionError {
                error_type: "provider.no-route".into(),
                message: error.to_string(),
                status: None,
                response: None,
            })
        })?;
        let boundary = match context::provenance(&model) {
            Some(provenance) => context::Boundary::Provenance(provenance),
            None => context::Boundary::Local,
        };
        let messages = context::history_preview(&self.db, session_id, &boundary)
            .await
            .map_err(|error| RunFailure::Defect(format!("history preview failed: {error}")))?;
        let initial =
            context::InstructionState::initial(&self.db, &selected.instructions, session_id)
                .await
                .map_err(|error| {
                    RunFailure::Defect(format!("instruction initial failed: {error}"))
                })?;
        let loaded = Loaded {
            session: selected.session.clone(),
            agent_id: selected.agent_id.clone(),
            agent: selected.agent.clone(),
            model,
            initial,
            messages,
            tools: selected.tools.clone(),
        };
        let trigger = CompactionTrigger {
            reason: CompactionReason::Manual,
            context: loaded,
            input_id: Some(
                MessageId::parse(input_id.to_string())
                    .unwrap_or_else(|error| MessageId(error.value)),
            ),
        };
        // RD-11 (ref compaction.ts:664-672): an interrupt settles a manual
        // compaction immediately — the drain's own inbox-item cleanup
        // publishes the failure, so here we only stop awaiting. Without
        // this race the running compaction row lingers until a later
        // drain's `settle_stale_compactions` backstop.
        let outcome = tokio::select! {
            outcome = self.compaction.compact(trigger) => outcome,
            () = shutdown_signal(shutdown) => {
                // F3 (B9): REF clears per-session spent via
                // `Effect.ensuring`, which also runs on interrupt — the
                // dropped compact future skipped its own tail.
                self.compaction.clear_spent(session_id);
                return Err(RunFailure::Interrupted);
            }
        };
        match outcome {
            crate::session::runner::compaction::CompactStatus::Completed => Ok(()),
            crate::session::runner::compaction::CompactStatus::Skipped => Ok(()),
            crate::session::runner::compaction::CompactStatus::Failed { error } => {
                // The compaction service already published
                // Compaction.Failed; surface the failure to the drain. The
                // compaction message itself carries the error (the
                // message updater projects it), so the failure is visible.
                Err(RunFailure::surfaced(error))
            }
        }
    }

    /// `runStep` (llm.ts:206-297): owns logical Step policy; each attempt
    /// owns its streaming, tools, and durable settlement. Returns the
    /// step's `needsContinuation` (the drain's next `continuing`).
    async fn run_step(
        &self,
        first: Box<Loaded>,
        step: u32,
        shutdown: &tokio::sync::watch::Receiver<bool>,
    ) -> Result<bool, RunFailure> {
        let session_id = SessionId::parse(first.session.id.clone())
            .unwrap_or_else(|error| SessionId(error.value));
        let mut assistant_message_id = self.new_message_id();
        let mut initial: Option<Loaded> = Some(*first);
        let recover_overflow = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mut recover_continuation = true;
        // The retry policy is PER STEP (llm.ts creates it once per
        // `runStep`): its schedule counter caps the whole retry ladder
        // (MAX_RECURS). Constructing it inside the attempt loop reset the
        // counter every attempt — a retryable failure (dead endpoint)
        // retried forever instead of surfacing after the 10th attempt.
        let policy = Arc::new(crate::session::runner::retry::RetryPolicy::new(
            session_id.clone(),
            Arc::clone(&self.rng),
        ));
        loop {
            // Reuse boundary preparation once; retries refresh context
            // without delivering more input.
            let loaded = match initial.take() {
                Some(loaded) => loaded,
                None => {
                    let selected = self.context.prepare_context(session_id.as_str()).await?;
                    self.context.load(&selected).await?
                }
            };
            // Auto compaction before every attempt.
            // RD-11 (ref compaction.ts:664-672 `Effect.onInterrupt`): an
            // interrupted non-manual compaction emits
            // `compaction.interrupted` immediately — never left to the
            // next drain's `settle_stale_compactions` backstop.
            let outcome = tokio::select! {
                outcome = self.compaction.compact(CompactionTrigger {
                    reason: CompactionReason::Auto,
                    context: loaded.clone(),
                    input_id: None,
                }) => outcome,
                () = shutdown_signal(shutdown) => {
                    // F3 (B9): `Effect.ensuring` semantics — the dropped
                    // compact future's spent tail never ran, clear it here.
                    self.compaction.clear_spent(session_id.as_str());
                    let _ = self
                        .bus
                        .publish(
                            def("session.compaction.failed"),
                            serde_json::json!({
                                "sessionID": session_id.as_str(),
                                "reason": "auto",
                                "error": {
                                    "type": "compaction.interrupted",
                                    "message": "Compaction was interrupted",
                                },
                            }),
                            PublishOptions::default(),
                        )
                        .await;
                    return Err(RunFailure::Interrupted);
                }
            };
            match outcome {
                crate::session::runner::compaction::CompactStatus::Failed { error } => {
                    return Err(RunFailure::surfaced(error));
                }
                crate::session::runner::compaction::CompactStatus::Completed => {
                    assistant_message_id = self.new_message_id();
                    continue;
                }
                crate::session::runner::compaction::CompactStatus::Skipped => {}
            }
            let step_limit_reached =
                loaded.agent.steps.is_some_and(|steps| u64::from(step) >= steps);
            let (measured, estimated) =
                crate::session::runner::compaction::estimate_prompt(&loaded);
            let prepared = prepare_primary(
                self.requests.as_ref(),
                &loaded,
                None,
                step_limit_reached,
                Some(crate::session::runner::model_request::InputTokens {
                    measured: measured as f64,
                    estimated: estimated as f64,
                }),
            )
            .await
            .map_err(|error| {
                RunFailure::unsurfaced(oc_schema::common::SessionError {
                    error_type: "provider.no-route".into(),
                    message: error.to_string(),
                    status: None,
                    response: None,
                })
            })?;

            // The retry decider wires the attempt's prepared hook into the
            // per-step policy (hoisted above the loop — the schedule
            // counter must survive across attempts).
            let policy = Arc::clone(&policy);
            let retry_hook = Arc::clone(&prepared.retry);
            let retry_agent = loaded.agent_id.clone();
            let retry_model = loaded.model.reference.clone();
            let retry_decider = Arc::new(
                move |cause: oc_ai::schema::AiError,
                      error: oc_schema::common::SessionError,
                      proposed: bool| {
                    let policy = Arc::clone(&policy);
                    let hook = Arc::clone(&retry_hook);
                    let agent = retry_agent.clone();
                    let model = retry_model.clone();
                    Box::pin(async move {
                        policy.decide(&cause, &error, &agent, &model, proposed, Some(hook)).await
                    })
                        as futures::future::BoxFuture<
                            'static,
                            crate::session::runner::retry::Decision,
                        >
                },
            );

            let compaction = Arc::clone(&self.compaction);
            let recover_overflow_context = loaded.clone();
            let recover_overflow_flag = Arc::clone(&recover_overflow);
            let recover_overflow_run = Arc::new(move || {
                let compaction = Arc::clone(&compaction);
                let context = recover_overflow_context.clone();
                let enabled = Arc::clone(&recover_overflow_flag);
                Box::pin(async move {
                    if !enabled.load(std::sync::atomic::Ordering::SeqCst) {
                        return false;
                    }
                    matches!(
                        compaction
                            .compact(CompactionTrigger {
                                reason: CompactionReason::Overflow,
                                context,
                                input_id: None,
                            })
                            .await,
                        crate::session::runner::compaction::CompactStatus::Completed
                    )
                }) as futures::future::BoxFuture<'static, bool>
            });
            let location_closed = Arc::clone(&self.location_closed);
            let deps = StepDeps {
                bus: self.bus.clone(),
                llm: Arc::clone(&self.llm),
                snapshot: self.snapshot.clone(),
                tool_output: self.tool_output.clone(),
                image_limits: Some(self.image_limits),
                shutdown: shutdown.clone(),
                clock: Arc::clone(&self.clock),
            };
            let input = AttemptInput {
                session_id: session_id.clone(),
                assistant_message_id: assistant_message_id.clone(),
                agent: loaded.agent_id.clone(),
                model: loaded.model.clone(),
                prepared,
                retry: retry_decider,
                recover_continuation,
                recover_overflow: recover_overflow_run,
                is_location_closed: location_closed,
            };
            let outcome = crate::session::runner::step::attempt(&deps, input).await;
            // REF llm.ts: the step counter only advances in the outer drain
            // loop after a Completed run — retries, Continue, Compacted, and
            // RecoverFull stay on the same logical step (budget intact).
            match map_attempt_failure(outcome) {
                Err(failure) => return Err(failure),
                Ok(Outcome::Completed { needs_continuation }) => return Ok(needs_continuation),
                Ok(Outcome::Retry { error, decision }) => {
                    self.wait_retry(&session_id, &assistant_message_id, &error, &decision).await?;
                }
                Ok(Outcome::Continue { error, decision }) => {
                    self.wait_retry(&session_id, &assistant_message_id, &error, &decision).await?;
                    let _ = self
                        .bus
                        .publish(
                            def("session.synthetic"),
                            serde_json::json!({
                                "sessionID": session_id.as_str(),
                                "text": CONTINUE_AFTER_INCOMPLETE_STREAM,
                            }),
                            PublishOptions::default(),
                        )
                        .await;
                    assistant_message_id = self.new_message_id();
                }
                Ok(Outcome::Compacted) => {
                    recover_overflow.store(false, std::sync::atomic::Ordering::SeqCst);
                    assistant_message_id = self.new_message_id();
                }
                Ok(Outcome::RecoverFull) => {
                    recover_continuation = false;
                }
            }
        }
    }

    /// `retry.wait` — RetryScheduled then sleep the remainder.
    async fn wait_retry(
        &self,
        session_id: &SessionId,
        assistant_message_id: &MessageId,
        error: &oc_schema::common::SessionError,
        decision: &crate::session::runner::retry::Decision,
    ) -> Result<(), RunFailure> {
        let scheduled = u64::try_from(self.clock.unix_ms().max(0)).unwrap_or(0);
        let _ = self
            .bus
            .publish(
                def("session.retry.scheduled"),
                serde_json::json!({
                    "sessionID": session_id.as_str(),
                    "assistantMessageID": assistant_message_id.as_str(),
                    "attempt": decision.attempt,
                    "at": scheduled + decision.delay_ms,
                    "error": serde_json::to_value(error).unwrap_or_default(),
                }),
                PublishOptions::default(),
            )
            .await;
        let now = self.clock.unix_ms();
        let remaining = ((scheduled as i64) + (decision.delay_ms as i64) - now).max(0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(remaining)).await;
        Ok(())
    }

    /// `FiberMap.run(titles, sessionID, title.generate, {onlyIfMissing})`:
    /// title generation starts once input is visible and must not delay
    /// model execution.
    fn fork_title(&self, session_id: &str) {
        let mut tasks = self.title_tasks.lock();
        if let Some(task) = tasks.get(session_id) {
            if !task.is_finished() {
                return;
            }
        }
        let title = Arc::clone(&self.title);
        let id = session_id.to_string();
        let handle = tokio::spawn(async move {
            title.generate(&id).await;
        });
        tasks.insert(session_id.to_string(), handle);
    }

    fn new_message_id(&self) -> MessageId {
        MessageId::parse(self.ids.new_id("msg")).unwrap_or_else(|error| MessageId(error.value))
    }

    // -----------------------------------------------------------------------
    // Stale settlement (llm.ts:299-352)
    // -----------------------------------------------------------------------

    /// A process death skips compaction finalizers: settle running
    /// compaction messages newest-first.
    async fn settle_stale_compactions(&self, session_id: &str) -> Result<(), RunFailure> {
        let rows: Vec<(String, String)> = self
            .db
            .transaction({
                let id = session_id.to_string();
                move |tx| {
                    let mut statement = tx.conn().prepare(
                        "SELECT id, data FROM session_message WHERE session_id = ?1 \
                         AND type = 'compaction' \
                         AND json_extract(data, '$.status') = 'running' \
                         ORDER BY seq DESC",
                    )?;
                    let rows = statement
                        .query_map(rusqlite::params![id], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(rows)
                }
            })
            .await
            .map_err(|error| {
                RunFailure::Defect(format!("stale compaction read failed: {error}"))
            })?;
        for (id, data) in rows {
            let value: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
            let reason =
                value.get("reason").and_then(serde_json::Value::as_str).unwrap_or("manual");
            let _ = self
                .bus
                .publish(
                    def("session.compaction.failed"),
                    serde_json::json!({
                        "sessionID": session_id,
                        "reason": reason,
                        "inputID": id,
                        "error": {
                            "type": "compaction.interrupted",
                            "message": "Compaction was interrupted",
                        },
                    }),
                    PublishOptions::default(),
                )
                .await;
        }
        Ok(())
    }

    /// Streaming/running tool parts are interrupted tool executions.
    async fn settle_stale_tool_calls(&self, session_id: &str) -> Result<(), RunFailure> {
        let messages = self
            .context_messages(session_id)
            .await
            .map_err(|error| RunFailure::Defect(format!("context read failed: {error}")))?;
        for message in messages {
            let MessageInfo::Assistant { id: message_id, content, .. } = &message else { continue };
            for part in content {
                let oc_schema::session_message::AssistantContent::Tool {
                    id: tool_id,
                    name,
                    executed,
                    state,
                    ..
                } = part
                else {
                    continue;
                };
                let running_metadata = match state {
                    oc_schema::session_message::ToolState::Running { metadata, .. } => {
                        Some(metadata)
                    }
                    _ => None,
                };
                let stale = matches!(
                    state,
                    oc_schema::session_message::ToolState::Streaming { .. }
                        | oc_schema::session_message::ToolState::Running { .. }
                );
                if !stale {
                    continue;
                }
                let child_id = running_metadata.and_then(|metadata| {
                    if name == "subagent" {
                        metadata.get("sessionID").and_then(serde_json::Value::as_str)
                    } else {
                        None
                    }
                });
                let mut data = serde_json::json!({
                    "sessionID": session_id,
                    "assistantMessageID": message_id.as_str(),
                    "id": tool_id,
                    "error": {
                        "type": "aborted",
                        "message": match child_id {
                            Some(child) => format!("Tool execution interrupted: {name} (sessionID: {child})"),
                            None => format!("Tool execution interrupted: {name}"),
                        },
                    },
                    "executed": *executed == Some(true),
                });
                if let Some(metadata) = running_metadata {
                    if !metadata.is_empty() {
                        data["metadata"] =
                            serde_json::Value::Object(metadata.clone().into_iter().collect());
                    }
                }
                let _ = self
                    .bus
                    .publish(def("session.tool.failed"), data, PublishOptions::default())
                    .await;
            }
        }
        Ok(())
    }

    async fn context_messages(
        &self,
        session_id: &str,
    ) -> crate::database::error::Result<Vec<MessageInfo>> {
        context::history_load(&self.db, session_id, &context::Boundary::Latest).await
    }
}

fn def(type_: &str) -> &'static crate::bus::EventDefinition {
    crate::bus::registry::by_type_latest(type_)
        .unwrap_or_else(|| panic!("no definition for {type_}"))
}

/// The runner's drain failure channel, mapped onto
/// [`crate::session::execution::RunError`].
///
/// Drain failures beyond the typed outcome, classified for the terminal
/// event: whether the error already surfaced on an assistant message
/// (`session.step.failed` from the step publisher) decides the synthetic
/// error-part emission in `settled` (pre-step failures — model
/// resolution, context select — never do).
#[derive(Debug, Clone, PartialEq)]
pub enum RunFailure {
    Failed { error: oc_schema::common::SessionError, surfaced: bool },
    Interrupted,
    Defect(String),
}

impl RunFailure {
    /// A failure whose error is already visible in the message stream.
    #[must_use]
    pub fn surfaced(error: oc_schema::common::SessionError) -> Self {
        Self::Failed { error, surfaced: true }
    }

    /// A failure the message stream never saw (pre-step: the model never
    /// resolved, no assistant message exists).
    #[must_use]
    pub fn unsurfaced(error: oc_schema::common::SessionError) -> Self {
        Self::Failed { error, surfaced: false }
    }
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunFailure::Failed { error, .. } => write!(f, "drain failed: {}", error.message),
            RunFailure::Interrupted => write!(f, "drain interrupted"),
            RunFailure::Defect(message) => write!(f, "defect: {message}"),
        }
    }
}

impl From<crate::session::runner::context::SelectError> for RunFailure {
    fn from(error: crate::session::runner::context::SelectError) -> Self {
        use crate::session::runner::context::SelectError;
        match error {
            SelectError::LocationMismatch(_) => RunFailure::Interrupted,
            SelectError::NotFound(session) => {
                RunFailure::Defect(format!("Session not found: {session}"))
            }
            other => RunFailure::unsurfaced(other.to_session_error()),
        }
    }
}

impl From<crate::session::runner::ModelError> for RunFailure {
    fn from(error: crate::session::runner::ModelError) -> Self {
        RunFailure::unsurfaced(oc_schema::common::SessionError {
            error_type: "provider.no-route".into(),
            message: error.to_string(),
            status: None,
            response: None,
        })
    }
}

fn run_error_of(failure: RunFailure) -> RunError {
    match failure {
        RunFailure::Failed { error, surfaced } => RunError::Failed { error, surfaced },
        RunFailure::Interrupted => RunError::Interrupted,
        RunFailure::Defect(message) => RunError::Defect(message),
    }
}

impl From<crate::database::error::DbError> for RunFailure {
    fn from(error: crate::database::error::DbError) -> Self {
        RunFailure::Defect(error.to_string())
    }
}

fn map_attempt_failure(outcome: Result<Outcome, AttemptFailure>) -> Result<Outcome, RunFailure> {
    match outcome {
        Ok(outcome) => Ok(outcome),
        Err(AttemptFailure::Interrupted) => Err(RunFailure::Interrupted),
        // A stream failure always passes `failAssistant` + `publishStepFailure`
        // first (step.rs steps 7-11, mirroring v2 step.ts:197,239): the
        // assistant message already carries the error.
        Err(AttemptFailure::Stream(error)) => {
            Err(RunFailure::surfaced(crate::session::runner::to_session_error(&error)))
        }
        // A tool-fiber die settles tool parts but the turn error itself
        // never reaches an assistant message — synthetic emission applies.
        Err(AttemptFailure::ToolDie(error)) => {
            Err(RunFailure::unsurfaced(crate::session::runner::to_session_error(&error)))
        }
        Err(AttemptFailure::StepFailed(error)) => Err(RunFailure::surfaced(error)),
        Err(AttemptFailure::Defect(message)) => Err(RunFailure::Defect(message)),
    }
}
