//! Readiness state machine (`server/src/service-status.ts`).
//!
//! `starting → ready | failed` (only from `starting`); `begin_stopping` is
//! idempotent and wins over everything once entered.

use crate::state::Readiness;

/// Marks the service ready (`starting → ready`; otherwise unchanged).
pub fn ready(sender: &tokio::sync::watch::Sender<Readiness>) {
    transition(sender, |_| Readiness::Ready, |current| *current == Readiness::Starting);
}

/// Marks the service failed (`starting → failed`; otherwise unchanged).
pub fn fail(sender: &tokio::sync::watch::Sender<Readiness>) {
    transition(sender, |_| Readiness::Failed, |current| *current == Readiness::Starting);
}

/// Begins shutdown (`→ stopping`, idempotent).
pub fn begin_stopping(sender: &tokio::sync::watch::Sender<Readiness>) {
    transition(sender, |_| Readiness::Stopping, |_| true);
}

/// Current state snapshot.
#[must_use]
pub fn current(receiver: &tokio::sync::watch::Receiver<Readiness>) -> Readiness {
    *receiver.borrow()
}

fn transition(
    sender: &tokio::sync::watch::Sender<Readiness>,
    next: impl FnOnce(&Readiness) -> Readiness,
    allowed: impl FnOnce(&Readiness) -> bool,
) {
    sender.send_if_modified(|value| {
        if allowed(value) {
            *value = next(value);
            true
        } else {
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::watch;

    fn channel() -> (watch::Sender<Readiness>, watch::Receiver<Readiness>) {
        watch::channel(Readiness::Starting)
    }

    #[test]
    fn server_status_moves_from_starting_to_ready() {
        let (tx, rx) = channel();
        assert_eq!(current(&rx), Readiness::Starting);
        ready(&tx);
        assert_eq!(current(&rx), Readiness::Ready);
    }

    #[test]
    fn server_status_keeps_startup_failure_until_shutdown() {
        let (tx, rx) = channel();
        fail(&tx);
        ready(&tx);
        fail(&tx);
        assert_eq!(current(&rx), Readiness::Failed);
    }

    #[test]
    fn server_status_keeps_stopping_after_shutdown_begins() {
        let (tx, rx) = channel();
        begin_stopping(&tx);
        assert_eq!(current(&rx), Readiness::Stopping);
        begin_stopping(&tx);
        assert_eq!(current(&rx), Readiness::Stopping);
        // ready/fail cannot dislodge stopping.
        ready(&tx);
        fail(&tx);
        assert_eq!(current(&rx), Readiness::Stopping);
    }
}
