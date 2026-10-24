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
