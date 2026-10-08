//! Port of `run-coordinator.ts` + `execution.ts` (runner-map §1.8).
//!
//! # Coordinator
//!
//! One execution is a busy period for one key: one task that drains from
//! the first wake until the key would stay idle. `pending_wake` is the
//! doorbell — work recorded during the execution rings it with the scope
//! that work needs, and the execution loop drains again instead of ending
//! ("input" subsumes "steer"). A doorbell that survives the loop (rung
//! after the loop decided to end, or during failure/interruption cleanup)
//! starts a fresh execution for the remaining work at settle. See
//! `docs/runner-concurrency.md` §5 for the mapping contract:
//!
//! - interrupt = `Notify::notify_one` on a per-execution stop bell
//!   (fire-and-forget; the drain future is dropped at its next await);
//! - settlement (the `settled` hook + successor start + waiter release)
//!   runs uninterruptibly after the drain loop exits;
//! - waiters observe the execution's whole exit (drain exit composed with
//!   settlement defects) through a `watch` channel.
//!
//! # Execution service
//!
//! Write-ahead claim: `session.execution.started` publishes with a commit
//! hook flipping the DB claim inside the same transaction. Terminals
//! release it — except shutdown interruption, which keeps the claim so the
//! next server start resumes the turn. Terminal classification:
//! success ⇒ `succeeded`; interrupts-only ⇒ `interrupted{reason ?? "shutdown"}`;
//! else `failed{toSessionError}`. Non-shutdown interrupts cancel session
//! jobs (job model is Wave D; [`CancelJobs`] is the typed hook).

#![allow(
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructor consumes its parts
    clippy::field_reassign_with_default, // PublishOptions::default + commit hook
)]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use oc_schema::common::SessionError;
use oc_schema::session_event::InterruptReason;
use parking_lot::Mutex;
use tokio::sync::{watch, Notify};

use crate::bus::{Bus, PublishOptions};
use crate::session::inbox::{Promotable, SessionInbox};
use crate::session::store::SessionStore;

/// A drain continuation (`{step}`, runner-map §1.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Continuation {
    pub step: u32,
}

/// The `SessionRunner.drain` result (`llm.ts` `DrainResult`).
#[derive(Debug, Clone, PartialEq)]
pub enum DrainOutcome {
    Complete,
    Moved { continuation: Option<Continuation> },
    Reloaded { force: bool, continuation: Option<Continuation> },
}

/// Drain failures (`SessionRunner.RunError` classification): typed failure,
/// interruption, or a settlement/publication defect (`Effect.die`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RunError {
    #[error("drain failed: {}", .0.error_type)]
    Failed(SessionError),
    #[error("drain interrupted")]
    Interrupted,
    #[error("defect: {0}")]
    Defect(String),
}

impl RunError {
    /// `toSessionError` (to-session-error.ts:87-88 fallback).
    #[must_use]
    pub fn to_session_error(&self) -> SessionError {
        match self {
            RunError::Failed(error) => error.clone(),
            RunError::Interrupted => SessionError {
                error_type: "aborted".into(),
                message: "Interrupted".into(),
                status: None,
                response: None,
            },
            RunError::Defect(message) => SessionError {
                error_type: "unknown".into(),
                message: message.clone(),
                status: None,
                response: None,
            },
        }
    }
}

/// The execution's exit observed by waiters.
pub type Exit = Result<(), RunError>;

type SharedExit = Option<Exit>;

struct Execution {
    /// Stop bell (one permit ⇒ the drain loop breaks as interrupted).
    stop: Arc<Notify>,
    /// Waiter fan-out: `Some(exit)` once settled.
    done_tx: watch::Sender<SharedExit>,
    scope: Promotable,
    pending_wake: Option<Promotable>,
    stopping: bool,
    /// The drain loop exited; settlement runs (run-coordinator's
    /// `owner === undefined` settlement window).
    loop_done: bool,
    interruption_reason: Option<InterruptReason>,
}

/// The injectable drain (`options.drain`). Wave B plugs the real LLM
/// runner; Wave A tests inject fakes.
pub type DrainFn =
    Arc<dyn Fn(String, bool, Option<Continuation>, Promotable) -> DrainFuture + Send + Sync>;
/// Boxed drain future.
pub type DrainFuture = Pin<Box<dyn Future<Output = Result<DrainOutcome, RunError>> + Send>>;
/// `options.started`.
pub type StartedFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<(), RunError>> + Send>> + Send + Sync>;
/// `options.settled` — runs for every exit before waiters resolve.
pub type SettledFn = Arc<
    dyn Fn(
            String,
            Exit,
            Option<InterruptReason>,
        ) -> Pin<Box<dyn Future<Output = Result<(), RunError>> + Send>>
        + Send
        + Sync,
>;

/// `SessionRunCoordinator.make` options.
#[derive(Default)]
pub struct CoordinatorOptions {
    pub drain: Option<DrainFn>,
    pub started: Option<StartedFn>,
    pub settled: Option<SettledFn>,
}

/// Serializes execution per key while allowing different keys to run
/// concurrently (`run-coordinator.ts` `Coordinator`). Cloning shares all
/// state.
#[derive(Clone)]
pub struct SessionRunCoordinator {
    inner: Arc<CoordinatorInner>,
}

struct CoordinatorInner {
    executions: Mutex<HashMap<String, Arc<Mutex<Execution>>>>,
    drain: DrainFn,
    started: Option<StartedFn>,
    settled: Option<SettledFn>,
}

impl Default for SessionRunCoordinator {
    fn default() -> Self {
        Self::new(CoordinatorOptions::default())
    }
}

impl SessionRunCoordinator {
    /// Builds the coordinator (`SessionRunCoordinator.make`).
    #[must_use]
    pub fn new(options: CoordinatorOptions) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner {
                executions: Mutex::new(HashMap::new()),
                drain: options.drain.unwrap_or_else(|| {
                    Arc::new(|_, _, _, _| Box::pin(async { Ok(DrainOutcome::Complete) }))
                }),
                started: options.started,
                settled: options.settled,
            }),
        }
    }

    /// Snapshots keys with an execution owned by this coordinator.
    #[must_use]
    pub fn active(&self) -> Vec<String> {
        self.inner.executions.lock().keys().cloned().collect()
    }

    /// Checks ownership for one key, including cleanup and terminal
    /// settlement.
    #[must_use]
    pub fn is_active(&self, key: &str) -> bool {
        self.inner.executions.lock().contains_key(key)
    }

    /// Starts an execution while idle, or joins the active execution and
    /// returns its exit. A stopping execution refuses joiners: wait out
    /// its cleanup, then run fresh.
    pub async fn run(&self, key: &str) -> Exit {
        loop {
            let execution = {
                let mut executions = self.inner.executions.lock();
                match executions.get(key).cloned() {
                    Some(execution) => execution,
                    None => start_locked(
                        &mut executions,
                        self.inner.clone(),
                        key,
                        true,
                        Promotable::Input,
                    ),
                }
            };
            if !execution.lock().stopping {
                return await_done(&execution).await;
            }
            // Stopping: wait out this execution's exit, then run fresh.
            let _ = await_done(&execution).await;
        }
    }

    /// Rings the doorbell: an idle key starts an execution; an active one
    /// drains again before settling. Coalesced wakes keep the widest scope
    /// ("input" subsumes "steer").
    pub fn wake(&self, key: &str, scope: Promotable) {
        let mut executions = self.inner.executions.lock();
        match executions.get(key).cloned() {
            Some(execution) => {
                let mut execution = execution.lock();
                // Coalesced wakes keep the widest scope: "input" subsumes "steer".
                execution.pending_wake = Some(match execution.pending_wake {
                    Some(Promotable::Input) => Promotable::Input,
                    _ => scope,
                });
            }
            None => {
                start_locked(&mut executions, self.inner.clone(), key, false, scope);
            }
        }
    }

    /// Stops the active execution and clears its doorbell. No-op when
    /// idle. Resolves once the interruption is *accepted* (not when
    /// cleanup settles); returns whether an active execution was
    /// interrupted. When `await_settlement`, also waits out this
    /// execution's cleanup and settled hook.
    pub async fn interrupt(
        &self,
        key: &str,
        reason: Option<InterruptReason>,
        await_settlement: bool,
    ) -> (bool, Option<watch::Receiver<SharedExit>>) {
        let outcome = {
            let executions = self.inner.executions.lock();
            let Some(execution) = executions.get(key).cloned() else {
                return (false, None);
            };
            let mut execution = execution.lock();
            if execution.stopping {
                return (false, Some(execution.done_tx.subscribe()));
            }
            if execution.loop_done {
                // Settlement window: the terminal outcome is already
                // decided, so no reason attaches — but the interrupt still
                // claims the recorded wakes so settle does not start a
                // dead-intent successor.
                execution.pending_wake = None;
                return (false, Some(execution.done_tx.subscribe()));
            }
            execution.stopping = true;
            // Wakes recorded so far belong to the interrupted intent; the
            // interrupt claims them. Wakes arriving during cleanup are new
            // admissions and restart normally at settle.
            execution.pending_wake = None;
            execution.interruption_reason = reason;
            // Fire and forget: nobody benefits from waiting out cleanup here.
            execution.stop.notify_one();
            (true, Some(execution.done_tx.subscribe()))
        };
        if await_settlement {
            if let Some(receiver) = &outcome.1 {
                let _ = await_receiver(receiver).await;
            }
        }
        outcome
    }

    /// Resolves once no execution is active for the key. Returns
    /// immediately when already idle and never starts work.
    pub async fn await_idle(&self, key: &str) {
        loop {
            let execution = self.inner.executions.lock().get(key).cloned();
            let Some(execution) = execution else { return };
            let _ = await_done(&execution).await;
        }
    }

    /// Interrupts every active execution (v2's scope-close cleanup — the
    /// graceful twin of an unclean death; terminals read as shutdown).
    pub fn interrupt_all(&self) {
        let executions = self.inner.executions.lock();
        for execution in executions.values() {
            let mut execution = execution.lock();
            if execution.stopping || execution.loop_done {
                continue;
            }
            execution.stopping = true;
            execution.pending_wake = None;
            execution.interruption_reason = None;
            execution.stop.notify_one();
        }
    }
}

fn start_locked(
    executions: &mut HashMap<String, Arc<Mutex<Execution>>>,
    inner: Arc<CoordinatorInner>,
    key: &str,
    force: bool,
    scope: Promotable,
) -> Arc<Mutex<Execution>> {
    let (done_tx, _) = watch::channel(None);
    let execution = Arc::new(Mutex::new(Execution {
        stop: Arc::new(Notify::new()),
        done_tx,
        scope,
        pending_wake: None,
        stopping: false,
        loop_done: false,
        interruption_reason: None,
    }));
    executions.insert(key.to_string(), Arc::clone(&execution));
    let task_key = key.to_string();
    tokio::spawn(run_execution(inner, task_key, Arc::clone(&execution), force, scope));
    execution
}

async fn await_done(execution: &Arc<Mutex<Execution>>) -> Exit {
    let receiver = execution.lock().done_tx.subscribe();
    await_receiver(&receiver).await
}

async fn await_receiver(receiver: &watch::Receiver<SharedExit>) -> Exit {
    let mut receiver = receiver.clone();
    loop {
        if let Some(exit) = receiver.borrow().clone() {
            return exit;
        }
        if receiver.changed().await.is_err() {
            // Sender dropped without a value (never happens today).
            return Ok(());
        }
    }
}

/// One execution: started hook → drain loop (doorbell trampoline) →
/// uninterruptible settlement → successor start → waiter release.
async fn run_execution(
    inner: Arc<CoordinatorInner>,
    key: String,
    execution: Arc<Mutex<Execution>>,
    force: bool,
    initial_scope: Promotable,
) {
    // The leading yield lets wake()/run() observe the execution before the
    // first drain settles (run-coordinator.ts:94-96).
    tokio::task::yield_now().await;

    let mut exit: Exit = Ok(());
    let mut force = force;
    let mut scope = initial_scope;

    // started runs uninterruptibly before the first drain.
    if let Some(started) = &inner.started {
        if let Err(err) = started(key.clone()).await {
            exit = Err(err);
        }
    }

    while exit.is_ok() {
        let stop = execution.lock().stop.clone();
        let drain = (inner.drain)(key.clone(), force, None, scope);
        let drained = tokio::select! {
            biased;
            () = stop.notified() => Err(RunError::Interrupted),
            outcome = drain => outcome.map(|_| ()),
        };
        if let Err(err) = drained {
            exit = Err(err);
            break;
        }

        // Doorbell check under the execution lock (the "yieldNow
        // trampoline"): stopping ends the loop as interrupted; a recorded
        // wake re-drains with its (widest) scope.
        let next = {
            let mut state = execution.lock();
            if state.stopping {
                None
            } else if let Some(pending) = state.pending_wake.take() {
                state.scope = pending;
                Some(pending)
            } else {
                None
            }
        };
        match next {
            None => {
                if execution.lock().stopping {
                    exit = Err(RunError::Interrupted);
                }
                break;
            }
            Some(pending) => {
                scope = pending;
                force = false;
                tokio::task::yield_now().await;
            }
        }
    }

    // Settlement window opens: late interrupts only claim wakes.
    let reason = {
        let mut state = execution.lock();
        state.loop_done = true;
        state.interruption_reason.take()
    };

    // Settled hook — uninterruptible; its defects replace the exit but
    // never skip settle.
    if let Some(settled) = &inner.settled {
        if let Err(err) = settled(key.clone(), exit.clone(), reason).await {
            exit = Err(err);
        }
    }

    // settle: a surviving doorbell starts the successor BEFORE waiters
    // resolve (run-coordinator.ts:116-120).
    {
        let mut executions = inner.executions.lock();
        let pending_wake = execution.lock().pending_wake.take();
        let (same, still_mapped) = match executions.get(&key) {
            Some(current) => (Arc::ptr_eq(current, &execution), true),
            None => (false, false),
        };
        if still_mapped && same {
            match pending_wake {
                Some(scope) => {
                    start_locked(&mut executions, inner.clone(), &key, false, scope);
                }
                None => {
                    executions.remove(&key);
                }
            }
        }
    }
    let _ = execution.lock().done_tx.send(Some(exit));
}

// ---------------------------------------------------------------------------
// SessionExecution
// ---------------------------------------------------------------------------

/// Wave D job-model hook: cancels a session's background jobs.
#[async_trait::async_trait]
pub trait CancelJobs: Send + Sync {
    async fn cancel(&self, session_id: &str);
}

/// `SessionExecution.interrupt` options.
#[derive(Debug, Clone, Default)]
pub struct InterruptOptions {
    pub resume: bool,
    pub reason: Option<InterruptReason>,
    pub await_settlement: bool,
}

/// `SessionExecution.terminal` (execution.ts:50-54).
#[must_use]
pub fn terminal(exit: &Exit, reason: Option<InterruptReason>) -> Terminal {
    match exit {
        Ok(()) => Terminal::Succeeded,
        Err(RunError::Interrupted) => {
            Terminal::Interrupted { reason: reason.unwrap_or(InterruptReason::Shutdown) }
        }
        Err(error) => Terminal::Failed { error: error.to_session_error() },
    }
}

/// The terminal classification result.
#[derive(Debug, Clone, PartialEq)]
pub enum Terminal {
    Succeeded,
    Interrupted { reason: InterruptReason },
    Failed { error: SessionError },
}

/// Process-local execution: drains run in this process through the
/// injected drain (`SessionExecution.layer`).
pub struct SessionExecution {
    inbox: SessionInbox,
    coordinator: SessionRunCoordinator,
}

impl SessionExecution {
    /// Assembles the service with lifecycle publication hooks wired to
    /// the bus around an injectable drain.
    #[must_use]
    pub fn new(
        store: SessionStore,
        bus: Bus,
        inbox: SessionInbox,
        jobs: Option<Arc<dyn CancelJobs>>,
        drain: DrainFn,
    ) -> Self {
        let service = Arc::new(ExecutionHooks { bus: bus.clone(), jobs: jobs.clone() });
        let coordinator = SessionRunCoordinator::new(CoordinatorOptions {
            drain: Some(wrap_drain(store.clone(), drain)),
            started: Some(Arc::new({
                let service = Arc::clone(&service);
                move |session_id| {
                    let service = Arc::clone(&service);
                    Box::pin(async move { service.started(&session_id).await })
                }
            })),
            settled: Some(Arc::new({
                let service = Arc::clone(&service);
                move |session_id, exit, reason| {
                    let service = Arc::clone(&service);
                    Box::pin(async move { service.settled(&session_id, exit, reason).await })
                }
            })),
        });
        Self { inbox, coordinator }
    }

    /// Snapshots active execution owned by this process.
    pub fn active(&self) -> Vec<String> {
        self.coordinator.active()
    }

    /// Checks process-local ownership, including interruption cleanup and
    /// terminal settlement.
    pub fn is_active(&self, session_id: &str) -> bool {
        self.coordinator.is_active(session_id)
    }

    /// Starts execution while idle or joins the active execution.
    pub async fn resume(&self, session_id: &str) -> Exit {
        self.coordinator.run(session_id).await
    }

    /// Registers newly recorded work; repeated wakeups may coalesce.
    pub fn wake(&self, session_id: &str, scope: Promotable) {
        self.coordinator.wake(session_id, scope);
    }

    /// Interrupt active work owned by this process; idle interruption is a
    /// no-op. Returns whether an active execution was interrupted.
    pub async fn interrupt(&self, session_id: &str, options: InterruptOptions) -> bool {
        let (interrupted, _receiver) =
            self.coordinator.interrupt(session_id, options.reason, options.await_settlement).await;
        if !options.resume {
            return interrupted;
        }
        // Resume steering input and between-turn control work from the
        // interrupted intent. Queued next-turn prompts stay parked: a
        // steer-scoped drain never promotes them, and a control item
        // behind a queued prompt waits its turn.
        if let Ok(Some(next)) = self.inbox.next_promotable(session_id, Promotable::Input).await {
            let steer_next = matches!(
                &next,
                oc_schema::session_inbox::Info::Compaction { .. }
                    | oc_schema::session_inbox::Info::Move { .. }
                    | oc_schema::session_inbox::Info::User {
                        delivery: oc_schema::session_inbox::Delivery::Steer,
                        ..
                    }
                    | oc_schema::session_inbox::Info::Synthetic {
                        delivery: oc_schema::session_inbox::Delivery::Steer,
                        ..
                    }
            );
            if steer_next {
                self.coordinator.wake(session_id, Promotable::Steer);
            }
        }
        interrupted
    }

    /// Resolves once this process owns no active execution for the session.
    pub async fn await_idle(&self, session_id: &str) {
        self.coordinator.await_idle(session_id).await;
    }

    /// Graceful teardown: interrupt everything (shutdown terminals keep
    /// their claims for restart resume).
    pub fn shutdown(&self) {
        self.coordinator.interrupt_all();
    }
}

/// The drain wrapper matching Moved/Reloaded re-drain rules (execution.ts
/// :87-110): Moved re-drains without force carrying the continuation;
/// Reloaded re-drains with its own force flag.
fn wrap_drain(store: SessionStore, drain: DrainFn) -> DrainFn {
    Arc::new(move |session_id, force, continuation, promotable| {
        let store = store.clone();
        let drain = Arc::clone(&drain);
        Box::pin(async move {
            let session = store
                .get(&session_id)
                .await
                .map_err(|err| RunError::Defect(format!("session read failed: {err}")))?;
            if session.is_none() {
                return Err(RunError::Defect(format!("Session not found: {session_id}")));
            }
            let mut outcome = drain(session_id.clone(), force, continuation, promotable).await?;
            loop {
                match outcome {
                    DrainOutcome::Complete => return Ok(DrainOutcome::Complete),
                    DrainOutcome::Moved { continuation } => {
                        outcome =
                            drain(session_id.clone(), false, continuation, promotable).await?;
                    }
                    DrainOutcome::Reloaded { force, continuation } => {
                        outcome =
                            drain(session_id.clone(), force, continuation, promotable).await?;
                    }
                }
            }
        })
    })
}

struct ExecutionHooks {
    bus: Bus,
    jobs: Option<Arc<dyn CancelJobs>>,
}

impl ExecutionHooks {
    /// Publishes `session.execution.started` with the claim commit hook —
    /// the durable write-ahead intent lands in the SAME transaction.
    async fn started(&self, session_id: &str) -> Result<(), RunError> {
        let data = serde_json::json!({ "sessionID": session_id });
        let def = crate::bus::registry::by_type_latest("session.execution.started")
            .unwrap_or_else(|| panic!("no session.execution.started definition"));
        let claimed = session_id.to_string();
        let mut options = PublishOptions::default();
        options.commit = Some(Box::new(move |tx: &mut crate::database::Tx<'_>, _| {
            tx.execute(
                "UPDATE session_v2 SET time_suspended = ?2, time_updated = time_updated \
                 WHERE id = ?1 AND time_suspended IS NULL",
                rusqlite::params![claimed, crate::database::migrations::now_ms()],
            )?;
            Ok(())
        }));
        self.bus
            .publish(def, data, options)
            .await
            .map(|_| ())
            .map_err(|err| RunError::Defect(format!("failed to publish execution started: {err}")))
    }

    /// One terminal observation per busy period, covering every coalesced
    /// drain (execution.ts:118-147).
    async fn settled(
        &self,
        session_id: &str,
        exit: Exit,
        reason: Option<InterruptReason>,
    ) -> Result<(), RunError> {
        let outcome = terminal(&exit, reason);
        match outcome {
            Terminal::Succeeded => {
                self.publish_terminal(session_id, "session.execution.succeeded", None, true).await
            }
            Terminal::Interrupted { reason } => {
                // Deliberate stops release the claim; shutdown keeps it for
                // restart continuity.
                if reason != InterruptReason::Shutdown {
                    if let Some(jobs) = &self.jobs {
                        jobs.cancel(session_id).await;
                    }
                }
                let mut data = serde_json::json!({ "sessionID": session_id });
                data["reason"] = serde_json::json!(match reason {
                    InterruptReason::User => "user",
                    InterruptReason::Shutdown => "shutdown",
                    InterruptReason::Superseded => "superseded",
                    InterruptReason::Inactivity => "inactivity",
                });
                self.publish_event(
                    session_id,
                    "session.execution.interrupted",
                    data,
                    reason != InterruptReason::Shutdown,
                )
                .await
            }
            Terminal::Failed { error } => {
                let data = serde_json::json!({
                    "sessionID": session_id,
                    "error": serde_json::to_value(&error).unwrap_or_default(),
                });
                self.publish_event(session_id, "session.execution.failed", data, true).await
            }
        }
    }

    async fn publish_terminal(
        &self,
        session_id: &str,
        type_: &str,
        data: Option<serde_json::Value>,
        release: bool,
    ) -> Result<(), RunError> {
        let data = data.unwrap_or_else(|| serde_json::json!({ "sessionID": session_id }));
        self.publish_event(session_id, type_, data, release).await
    }

    async fn publish_event(
        &self,
        session_id: &str,
        type_: &str,
        data: serde_json::Value,
        release: bool,
    ) -> Result<(), RunError> {
        let def = crate::bus::registry::by_type_latest(type_)
            .unwrap_or_else(|| panic!("no {type_} definition"));
        let mut options = PublishOptions::default();
        if release {
            let released = session_id.to_string();
            options.commit = Some(Box::new(move |tx: &mut crate::database::Tx<'_>, _| {
                tx.execute(
                    "UPDATE session_v2 SET time_suspended = NULL, resume_attempts = 0, \
                     time_updated = time_updated WHERE id = ?1",
                    rusqlite::params![released],
                )?;
                Ok(())
            }));
        }
        self.bus
            .publish(def, data, options)
            .await
            .map(|_| ())
            .map_err(|err| RunError::Defect(format!("failed to publish {type_}: {err}")))
    }
}
