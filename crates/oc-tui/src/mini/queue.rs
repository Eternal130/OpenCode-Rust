//! The serial prompt queue — port of
//! `packages/tui/src/mini/runtime.queue.ts`.
//!
//! Prompts arrive from the footer (user types and hits enter); local
//! operations drain one at a time. Ordinary prompts submitted during
//! an ACTIVE ordinary turn are admitted straight to the server's
//! durable queue instead of waiting (`runtime.queue.ts:256-276`). The
//! queue also owns the local session commands (`/new`, `/compact`),
//! empty-prompt rejection, and the turn-duration bookkeeping the
//! statusline shows (`runtime.queue.ts:210-221`).
//!
//! The v2 queue is an event-driven promise graph; this port keeps the
//! same decision table as a pure state machine the runtime loop steps
//! (`submit` → [`QueueAction`]).

use super::prompt_shared::{is_compact_command, is_exit_command, is_new_command};

/// A submitted prompt (`RunPrompt`, text subset).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunPrompt {
    pub text: String,
    /// `mode: "shell"` rows (the `!`-prefixed shell passthrough).
    pub shell: bool,
    /// `delivery` for admitted queued prompts (`queue`/`steer`).
    pub delivery: Option<&'static str>,
}

/// What `submit` decided (`runtime.queue.ts:243-294`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    /// Empty (or shell-empty) input is dropped (`runtime.queue.ts:246`).
    Rejected,
    /// `/exit` `/quit` `/q` `:q` close the footer (`runtime.queue.ts:251`).
    Exit,
    /// `/new` — no session factory means the status hint
    /// "new sessions unavailable" (`runtime.queue.ts:98-107`).
    NewSession { unavailable: bool },
    /// `/compact` (status "compacting session",
    /// `runtime.queue.ts:128-144`).
    Compact,
    /// An ordinary prompt over an idle queue: run it now.
    Run { prompt: RunPrompt },
    /// An ordinary prompt submitted while an ordinary turn is active:
    /// admitted as durable queued work on the server
    /// (`runtime.queue.ts:256-276`).
    Admit { prompt: RunPrompt, delivery: &'static str },
}

/// The queue's live state (`State`, `runtime.queue.ts:33-39`).
#[derive(Debug, Default)]
pub struct PromptQueue {
    queue: Vec<RunPrompt>,
    active: Option<RunPrompt>,
    /// Whether `onNewSession` is wired (`input.onNewSession`).
    pub can_new_session: bool,
}

impl PromptQueue {
    #[must_use]
    pub fn new(can_new_session: bool) -> Self {
        Self { queue: Vec::new(), active: None, can_new_session }
    }

    /// Is an ordinary (non-shell) turn mid-flight?
    #[must_use]
    pub fn ordinary_active(&self) -> bool {
        self.active.as_ref().is_some_and(|active| !active.shell)
    }

    /// `submit` (`runtime.queue.ts:243-294`).
    #[must_use]
    pub fn submit(&mut self, prompt: RunPrompt) -> QueueAction {
        if prompt.text.trim().is_empty() {
            return QueueAction::Rejected;
        }
        if !prompt.shell && is_exit_command(&prompt.text) {
            return QueueAction::Exit;
        }
        if !prompt.shell && is_new_command(&prompt.text) {
            return QueueAction::NewSession { unavailable: !self.can_new_session };
        }
        if !prompt.shell && is_compact_command(&prompt.text) {
            return QueueAction::Compact;
        }
        if self.ordinary_active() {
            // Durable admission with a fresh message id
            // (`runtime.queue.ts:266-269`); delivery defaults to
            // "queue".
            let delivery = prompt.delivery.unwrap_or("queue");
            return QueueAction::Admit {
                prompt: RunPrompt { delivery: Some(delivery), ..prompt },
                delivery,
            };
        }
        QueueAction::Run { prompt }
    }

    /// The drain loop claims the next prompt (`drain`,
    /// `runtime.queue.ts:85-241`): the caller passes the action it
    /// obtained from `submit` back in when the turn starts.
    pub fn turn_started(&mut self, prompt: RunPrompt) {
        self.active = Some(prompt);
    }

    /// The turn finished: emit duration status, clear active
    /// (`runtime.queue.ts:210-222`).
    pub fn turn_finished(&mut self) {
        self.active = None;
    }

    /// Pending local prompts (the queue the drain loop empties).
    #[must_use]
    pub fn pending(&self) -> &[RunPrompt] {
        &self.queue
    }

    /// Queue a prompt for the drain loop (the plain `state.queue.push`
    /// branch, `runtime.queue.ts:278`).
    pub fn push(&mut self, prompt: RunPrompt) {
        self.queue.push(prompt);
    }

    /// Pop the next queued prompt for the drain loop.
    pub fn pop(&mut self) -> Option<RunPrompt> {
        if self.ordinary_active() || self.queue.is_empty() {
            return None;
        }
        Some(self.queue.remove(0))
    }
}

/// The duration label for a finished turn (`Locale.duration` via
/// `runtime.queue.ts:211`): human units, sub-second shows `<1s`.
#[must_use]
pub fn duration_label(elapsed_ms: u128) -> String {
    if elapsed_ms < 1000 {
        return "<1s".to_owned();
    }
    let seconds = elapsed_ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str) -> RunPrompt {
        RunPrompt { text: text.into(), ..RunPrompt::default() }
    }

    #[test]
    fn empty_prompts_are_rejected() {
        let mut queue = PromptQueue::new(false);
        assert_eq!(queue.submit(prompt("   ")), QueueAction::Rejected);
        assert_eq!(queue.submit(prompt("")), QueueAction::Rejected);
    }

    #[test]
    fn exit_commands_close() {
        let mut queue = PromptQueue::new(true);
        assert_eq!(queue.submit(prompt("/exit")), QueueAction::Exit);
        assert_eq!(queue.submit(prompt(":q")), QueueAction::Exit);
    }

    #[test]
    fn new_and_compact_dispatch() {
        let mut queue = PromptQueue::new(true);
        assert_eq!(queue.submit(prompt("/new")), QueueAction::NewSession { unavailable: false });
        assert_eq!(queue.submit(prompt("/compact")), QueueAction::Compact);
        let mut bare = PromptQueue::new(false);
        assert_eq!(bare.submit(prompt("/new")), QueueAction::NewSession { unavailable: true });
    }

    #[test]
    fn prompts_over_active_turns_admit_as_durable_queue_work() {
        let mut queue = PromptQueue::new(true);
        let first = queue.submit(prompt("first"));
        assert!(matches!(first, QueueAction::Run { .. }));
        queue.turn_started(prompt("first"));
        let second = queue.submit(prompt("second"));
        match second {
            QueueAction::Admit { delivery, .. } => assert_eq!(delivery, "queue"),
            other => panic!("expected admission, got {other:?}"),
        }
        // Exit/compact still work over an active turn.
        assert_eq!(queue.submit(prompt("/compact")), QueueAction::Compact);
        queue.turn_finished();
        assert!(matches!(queue.submit(prompt("third")), QueueAction::Run { .. }));
    }

    #[test]
    fn drain_pops_in_order_when_idle() {
        let mut queue = PromptQueue::new(true);
        queue.push(prompt("a"));
        queue.push(prompt("b"));
        assert_eq!(queue.pop().map(|item| item.text), Some("a".into()));
        assert_eq!(queue.pop().map(|item| item.text), Some("b".into()));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn duration_labels() {
        assert_eq!(duration_label(50), "<1s");
        assert_eq!(duration_label(1500), "1s");
        assert_eq!(duration_label(65_000), "1m5s");
        assert_eq!(duration_label(3_600_000), "1h0m");
    }
}
