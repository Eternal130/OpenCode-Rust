//! Port of `runner/retry.ts` — the exponential/jittered schedule, the
//! `session.retry` hook override, and `wait()` (RetryScheduled + sleep).

use std::sync::Arc;
use std::time::Duration;

use oc_schema::common::SessionError;
use oc_schema::id::{AgentId, MessageId, SessionId};
use oc_schema::model;
use oc_util::determinism::{Clock, RngSource};

use oc_ai::schema::AiError;
use oc_ai::schema::AiErrorReason;

use crate::bus::{Bus, PublishOptions};
use crate::hooks::{RetryDecision as HookDecision, SessionRetryEvent};

/// Base delay floor/cap (retry.ts:42-44): exponential from 2s, capped at
/// 10s per gap, 10 retries max — 2, 4, 8, then 10 × 7.
const BASE_DELAY_MS: u64 = 2_000;
const DELAY_CAP_MS: u64 = 10_000;
const MAX_RECURS: u32 = 10;

/// Jitter window: ±20% (≈84s of raw waiting lands in 67–101s).
const JITTER_MIN: f64 = 0.8;
const JITTER_MAX: f64 = 1.2;

/// Bound provider-requested delays so a hostile retry-after cannot stall a
/// session for hours.
const RETRY_AFTER_MAX_MS: f64 = 15.0 * 60.0 * 1000.0;

/// A timed-out attempt already waited minutes; cap those well below the
/// general allowance (retry.ts:58-60).
const MAX_TIMEOUT_RETRIES: u32 = 3;

/// `SessionRunnerRetry.Decision` — `{retry: false}` or
/// `{retry: true, attempt, delay}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub retry: bool,
    pub attempt: u32,
    /// Milliseconds to wait before the next attempt.
    pub delay_ms: u64,
}

impl Decision {
    #[must_use]
    pub fn no_retry() -> Self {
        Self { retry: false, attempt: 0, delay_ms: 0 }
    }
}

/// `retryAfter` — provider-requested delay floor for rate limits and
/// provider-internal failures.
fn retry_after_ms(cause: &AiError) -> Option<f64> {
    match &cause.reason {
        AiErrorReason::RateLimit { retry_after_ms, .. }
        | AiErrorReason::ProviderInternal { retry_after_ms, .. } => {
            retry_after_ms.map(|ms| ms.min(RETRY_AFTER_MAX_MS))
        }
        _ => None,
    }
}

fn is_timeout(cause: &AiError) -> bool {
    matches!(&cause.reason, AiErrorReason::Transport { code: Some(code), .. } if code == "Timeout")
}

/// The raw schedule gap for the n-th retry (1-indexed): doubling from 2s,
/// capped at 10s.
#[must_use]
pub fn base_delay(recursion: u32) -> u64 {
    let exp = BASE_DELAY_MS.saturating_mul(1_u64 << recursion.saturating_sub(1).min(31));
    exp.min(DELAY_CAP_MS)
}

/// One retry-hook caller (`Input.hook`) — receives the event mutably so a
/// plugin decision override propagates back to the schedule.
pub type RetryHook = Arc<
    dyn for<'a> Fn(&'a mut SessionRetryEvent) -> futures::future::BoxFuture<'a, ()> + Send + Sync,
>;

/// The same shape stored on [`crate::session::runner::model_request::Prepared`].
pub type PreparedRetryHook = RetryHook;

/// `SessionRunnerRetry.policy` + `make` — per-step retry state. The
/// schedule counters are atomics so the decider future stays `Send` while
/// shared across the step's closures.
pub struct RetryPolicy {
    session_id: SessionId,
    attempt: std::sync::atomic::AtomicU32,
    timeouts: std::sync::atomic::AtomicU32,
    rng: Arc<dyn RngSource>,
}

impl RetryPolicy {
    pub fn new(session_id: SessionId, rng: Arc<dyn RngSource>) -> Self {
        Self {
            session_id,
            attempt: std::sync::atomic::AtomicU32::new(1),
            timeouts: std::sync::atomic::AtomicU32::new(0),
            rng,
        }
    }

    /// `decide` (retry.ts:69-91): advances the schedule, applies the
    /// timeout cap, fires the `session.retry` hook, and honors its
    /// override.
    pub async fn decide(
        &self,
        cause: &AiError,
        error: &SessionError,
        agent: &AgentId,
        model: &model::Ref,
        proposed: bool,
        hook: Option<RetryHook>,
    ) -> Decision {
        use std::sync::atomic::Ordering;
        let recursion = self.attempt.load(Ordering::SeqCst);
        if recursion > MAX_RECURS {
            return Decision::no_retry();
        }
        let base = base_delay(recursion);
        // Jittered gap (deterministic through the injected rng).
        let factor = JITTER_MIN + (JITTER_MAX - JITTER_MIN) * self.rng.next_f64();
        let mut delay = ((base as f64) * factor).ceil() as u64;
        // Provider-requested floor.
        if let Some(minimum) = retry_after_ms(cause) {
            delay = delay.max(minimum.ceil() as u64);
        }
        let attempt = self.attempt.fetch_add(1, Ordering::SeqCst) + 1;
        if is_timeout(cause) {
            self.timeouts.fetch_add(1, Ordering::SeqCst);
        }
        let timeouts = self.timeouts.load(Ordering::SeqCst);
        let mut event = SessionRetryEvent {
            session_id: self.session_id.clone(),
            agent: agent.clone(),
            model: model.clone(),
            error: serde_json::to_value(error).unwrap_or_default(),
            attempt,
            decision: if proposed && timeouts <= MAX_TIMEOUT_RETRIES {
                HookDecision::Retry { delay_ms: delay }
            } else {
                HookDecision::NoRetry
            },
        };
        if let Some(hook) = &hook {
            (hook)(&mut event).await;
        }
        match event.decision {
            HookDecision::NoRetry => Decision::no_retry(),
            HookDecision::Retry { delay_ms } => {
                // A hook-supplied delay is a u64 (always finite and
                // non-negative); it replaces the schedule gap directly.
                Decision { retry: true, attempt, delay_ms }
            }
        }
    }

    /// `wait` (retry.ts:97-113): publish RetryScheduled, then sleep the
    /// remainder.
    pub async fn wait(
        &self,
        bus: &Bus,
        clock: &Arc<dyn Clock>,
        assistant_message_id: &MessageId,
        error: &SessionError,
        decision: &Decision,
        sleep: &dyn Fn(Duration) -> futures::future::BoxFuture<'static, ()>,
    ) -> Result<(), String> {
        let scheduled = u64::try_from(clock.unix_ms().max(0)).unwrap_or(0);
        let data = serde_json::json!({
            "sessionID": self.session_id.as_str(),
            "assistantMessageID": assistant_message_id.as_str(),
            "attempt": decision.attempt,
            "at": scheduled + decision.delay_ms,
            "error": serde_json::to_value(error).unwrap_or_default(),
        });
        let def = crate::bus::registry::by_type_latest("session.retry.scheduled")
            .unwrap_or_else(|| panic!("no session.retry.scheduled definition"));
        bus.publish(def, data, PublishOptions::default())
            .await
            .map(|_| ())
            .map_err(|err| format!("publish session.retry.scheduled failed: {err}"))?;
        let now = clock.unix_ms();
        let remaining = (scheduled as i64 + decision.delay_ms as i64 - now).max(0) as u64;
        (sleep)(Duration::from_millis(remaining)).await;
        Ok(())
    }
}
