//! Session lane — port of `packages/core/src/session/*` (core-runner-map
//! §1.2, §1.8, §2.1, §2.8).
//!
//! Wave A scope: the durable session projection stack —
//!
//! - [`inbox`]: pending-only inbox (`session_inbox` rows) with
//!   steer-first promotion, control reordering and compaction splicing;
//! - [`message_updater`]: the `session_message` part-state machine driven
//!   by every message-level durable event;
//! - [`projector`]: all `bus.project` handlers (projector.ts:434-788) plus
//!   the `session.usage.updated` fan-out subscription;
//! - [`store`]: `session_v2` reads/list/claim lifecycle and idempotent
//!   `session.created` publication;
//! - [`execution`]: the per-key run coordinator (doorbell + successor
//!   starts) and the execution service (write-ahead claim, terminal
//!   classification).
//!
//! Wave B plugs the real LLM drain into [`execution::SessionExecution`];
//! Wave A drives it with injected fakes.

pub mod diff;
pub mod execution;
pub mod fork;
pub mod inbox;
pub mod message_updater;
pub mod projector;
pub mod prompt;
pub mod revert;
pub mod runner;
pub mod store;
pub mod subagent_completion;
pub mod subagent_job;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_diff;
#[cfg(test)]
mod tests_parity;
#[cfg(test)]
mod tests_prompt_attachment;
#[cfg(test)]
mod tests_transfer;
pub mod transfer;

/// Errors from [`remove_tree`].
#[derive(Debug, thiserror::Error)]
pub enum RemoveTreeError {
    #[error(transparent)]
    Database(#[from] crate::database::error::DbError),
    #[error(transparent)]
    Bus(#[from] crate::bus::BusError),
}

/// How long [`remove_tree`] waits for an interrupted execution to go
/// idle before deleting anyway. Unlike the `wait` endpoint (whose whole
/// purpose is blocking until idle), deletion must proceed even if a
/// drain wedges — the row delete is idempotent and the interrupt already
/// signalled shutdown.
const REMOVE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// `Session.remove` (ref session.ts:367-377): interrupt → await idle →
/// (transport close) → recursively delete child sessions → clear the
/// session environment → publish `session.deleted` per session.
///
/// Transport close has no separate call here: the drain's interrupt
/// cleanup already closes the session's model transport channel
/// (`runner/drain.rs` `close_transport` on unwind), and the bounded idle
/// wait below waits out that cleanup before deletion proceeds. The bus
/// map cleanup (`bus.remove(sessionID)` in the ref) is covered by the
/// routing layer's reaction to `session.deleted`.
pub async fn remove_tree(
    store: &SessionStore,
    execution: &SessionExecution,
    session_env: &crate::env::SessionEnv,
    bus: &crate::bus::Bus,
    session_id: &str,
) -> std::result::Result<(), RemoveTreeError> {
    // Interrupt first so a running drain stops writing new rows for a
    // session that is about to disappear (ref `execution.interrupt`,
    // default options: no reason, no resume steering).
    execution.interrupt(session_id, InterruptOptions::default()).await;
    let _ = tokio::time::timeout(REMOVE_IDLE_TIMEOUT, execution.await_idle(session_id)).await;
    // Recursively delete children BEFORE the parent, mirroring the ref's
    // `forEach(children, (child) => remove(child.id))` — otherwise the
    // children would be orphaned with a dangling parent_id.
    let children = store
        .list(ListInput { parent_id: Some(Some(session_id.to_owned())), ..ListInput::default() })
        .await?;
    for child in children {
        Box::pin(remove_tree(store, execution, session_env, bus, &child.id)).await?;
    }
    // `environments.clear(sessionID)` — the Rust seam is the engine's
    // shared `SessionEnv` map.
    session_env.clear(session_id);
    store.remove(bus, session_id).await?;
    Ok(())
}

pub use execution::{
    CancelJobs, Continuation, DrainOutcome, InterruptOptions, RunError, SessionExecution,
    SessionRunCoordinator, Terminal,
};
pub use fork::{fork, ForkError, ForkOutcome};
pub use inbox::{InboxError, Promotable, Scope, SessionInbox};
pub use message_updater::SessionMessageUpdater;
pub use projector::{install_projectors, SessionAlreadyProjected};
pub use runner::{
    CompactMechanism, CompactOutcome, LlmClient, LlmEventStream, ModelError, ModelResolver,
    RecordedUsage, ResolvedModel, RouteLlmClient, SessionUsage,
};
pub use store::{
    CreateInput, CreateOutcome, ListAnchor, ListDirection, ListInput, ListOrder, MessagesInput,
    ResolvedProject, SessionStore,
};
pub use transfer::{import, ImportError, ImportInput};
