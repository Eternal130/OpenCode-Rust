//! Retry scheduling.
//!
//! Two layers live here:
//!
//! - [`RetryConfig::next_delay`] ports the session-runner schedule from
//!   `packages/core/src/session/runner/retry.ts`: exponential backoff from 2s
//!   capped at 10s per gap, 10 retries, ±20% jitter, with a provider-requested
//!   `retry-after` floor capped at 15 minutes.
//! - [`retry_with`] ports the generic helper from `packages/util/src/retry.ts`
//!   (3 attempts, 500ms base, factor 2, 10s cap, caller-owned retry predicate).

use std::thread;
use std::time::Duration;

use crate::determinism::RngSource;

/// Upper bound for provider-requested retry delays (15 minutes).
pub const RETRY_AFTER_MAX: Duration = Duration::from_mins(15);

/// Timeout failures earn far fewer retries than other transient errors
/// (`MAX_TIMEOUT_RETRIES` in the session runner).
pub const MAX_TIMEOUT_RETRIES: u32 = 3;

/// Lower jitter bound applied to every scheduled delay.
pub const JITTER_MIN: f64 = 0.8;

/// Upper jitter bound applied to every scheduled delay.
pub const JITTER_MAX: f64 = 1.2;

/// Session-runner backoff schedule: 2s exponential, 10s cap, 10 retries.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Delay before the first retry.
    pub base: Duration,
    /// Largest delay any single gap may reach.
    pub cap: Duration,
    /// Number of failed attempts that may be retried (the schedule serves
    /// exactly `max_attempts` delays; the runner tries `max_attempts + 1`
    /// times in total, matching `Schedule.recurs(10)`).
    pub max_attempts: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self { base: Duration::from_secs(2), cap: Duration::from_secs(10), max_attempts: 10 }
    }
}

impl RetryConfig {
    /// Unjittered backoff for the gap after `attempt` (1-based index of the
    /// attempt that just failed): `base * 2^(attempt-1)`, capped at `cap`.
    #[must_use]
    pub fn backoff(&self, attempt: u32) -> Duration {
        let base = self.base.as_millis();
        let cap = self.cap.as_millis();
        let mut delay = base;
        for _ in 1..attempt {
            delay = (delay * 2).min(cap);
            if delay == cap {
                break;
            }
        }
        Duration::from_millis(u64::try_from(delay.min(cap)).unwrap_or(u64::MAX))
    }

    /// True while `attempt` (1-based failed-attempt index) may still be
    /// retried. The default budget serves ten gaps (2, 4, 8, then 10×7 = 84s
    /// of waiting), matching `Schedule.recurs(10)`.
    #[must_use]
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt > 0 && attempt <= self.max_attempts
    }

    /// Delay to wait after `attempt` (1-based failed-attempt index) before the
    /// next try, or `None` when the retry budget is exhausted.
    ///
    /// Mirrors `schedule.pipe(Schedule.jittered, modifyDelay(...))`: the raw
    /// exponential delay is scaled by a random factor in
    /// `[JITTER_MIN, JITTER_MAX]`, then floored at `retry_after` (itself capped
    /// at [`RETRY_AFTER_MAX`]), then rounded up to whole milliseconds.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn next_delay(
        &self,
        attempt: u32,
        retry_after: Option<Duration>,
        rng: &dyn RngSource,
    ) -> Option<Duration> {
        if !self.should_retry(attempt) {
            return None;
        }
        let raw_ms = self.backoff(attempt).as_secs_f64() * 1000.0;
        let span = JITTER_MAX - JITTER_MIN;
        let factor = JITTER_MIN + span * rng.next_f64().clamp(0.0, 1.0);
        let jittered = raw_ms * factor;
        let floor =
            retry_after.map(|value| value.min(RETRY_AFTER_MAX)).unwrap_or_default().as_secs_f64()
                * 1000.0;
        let delay_ms = jittered.max(floor).ceil();
        Some(Duration::from_millis(delay_ms.max(0.0) as u64))
    }
}

/// Options for [`retry_with`], mirroring `RetryOptions` in
/// `packages/util/src/retry.ts`.
#[derive(Debug, Clone)]
pub struct RetryOptions {
    /// Total attempts (the first try included). Defaults to 3.
    pub attempts: u32,
    /// Delay before the first retry. Defaults to 500ms.
    pub delay: Duration,
    /// Multiplier applied per attempt. Defaults to 2.
    pub factor: f64,
    /// Largest delay between attempts. Defaults to 10s.
    pub max_delay: Duration,
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self {
            attempts: 3,
            delay: Duration::from_millis(500),
            factor: 2.0,
            max_delay: Duration::from_secs(10),
        }
    }
}

/// Messages whose failures are considered transient network blips, from
/// `packages/util/src/retry.ts`.
const TRANSIENT_MESSAGES: &[&str] = &[
    "load failed",
    "network connection was lost",
    "network request failed",
    "failed to fetch",
    "econnreset",
    "econnrefused",
    "etimedout",
    "socket hang up",
];

/// True when a lowercased error message (with any transport `code` appended)
/// signals a transient network failure.
#[must_use]
pub fn is_transient_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    TRANSIENT_MESSAGES.iter().any(|needle| lower.contains(needle))
}

/// Runs `operation` with exponential backoff, retrying only failures accepted
/// by `retry_if`. The thread sleeps between attempts; callers with async
/// runtimes should drive [`RetryConfig`] themselves.
///
/// # Errors
///
/// Returns the last error produced by `operation`, either because the retry
/// predicate rejected it or the attempt budget ran out.
pub fn retry_with<T, E>(
    options: &RetryOptions,
    mut retry_if: impl FnMut(&E) -> bool,
    mut operation: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let attempts = options.attempts.max(1);
    let mut backoff = options.delay;
    for attempt in 0..attempts {
        match operation() {
            Ok(value) => return Ok(value),
            Err(err) => {
                if attempt + 1 == attempts || !retry_if(&err) {
                    return Err(err);
                }
                thread::sleep(backoff.min(options.max_delay));
                backoff = Duration::from_secs_f64(
                    (backoff.as_secs_f64() * options.factor).min(options.max_delay.as_secs_f64()),
                );
            }
        }
    }
    unreachable!("attempt count is at least one")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::determinism::{Clock, FixedClock, SequentialRng};
    use std::sync::Arc;

    fn fixed_jitter(value: f64) -> impl RngSource {
        struct Fixed(f64);
        impl RngSource for Fixed {
            fn next_f64(&self) -> f64 {
                self.0
            }
            fn next_u128(&self) -> u128 {
                0
            }
        }
        Fixed(value)
    }

    #[test]
    fn default_backoff_series() {
        let config = RetryConfig::default();
        let series: Vec<Duration> = (1..=12).map(|attempt| config.backoff(attempt)).collect();
        assert_eq!(
            series,
            vec![
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
            ]
        );
    }

    #[test]
    fn default_series_totals_eighty_four_seconds() {
        // 2 + 4 + 8 + 7*10 = 84s of waiting across the 10 retry gaps.
        let config = RetryConfig::default();
        let total: Duration = (1..=10).map(|attempt| config.backoff(attempt)).sum();
        assert_eq!(total, Duration::from_secs(84));
    }

    #[test]
    fn custom_backoff_caps_quickly() {
        let config = RetryConfig {
            base: Duration::from_millis(100),
            cap: Duration::from_millis(250),
            max_attempts: 5,
        };
        assert_eq!(config.backoff(1), Duration::from_millis(100));
        assert_eq!(config.backoff(2), Duration::from_millis(200));
        assert_eq!(config.backoff(3), Duration::from_millis(250));
        assert_eq!(config.backoff(4), Duration::from_millis(250));
    }

    #[test]
    fn should_retry_bounds() {
        let config = RetryConfig::default();
        assert!(!config.should_retry(0));
        assert!(config.should_retry(1));
        assert!(config.should_retry(10));
        assert!(!config.should_retry(11));
    }

    #[test]
    fn next_delay_is_none_outside_budget() {
        let config = RetryConfig::default();
        let rng = fixed_jitter(1.0);
        assert!(config.next_delay(0, None, &rng).is_none());
        assert!(config.next_delay(11, None, &rng).is_none());
        assert!(config.next_delay(10, None, &rng).is_some());
        assert!(config.next_delay(9, None, &rng).is_some());
    }

    #[test]
    fn next_delay_applies_jitter_bounds() {
        let config = RetryConfig::default();
        // attempt 1 => raw 2s; jitter factor spans [0.8, 1.2].
        let low = config.next_delay(1, None, &fixed_jitter(0.0)).unwrap();
        let high = config.next_delay(1, None, &fixed_jitter(1.0)).unwrap();
        assert_eq!(low, Duration::from_millis(1600));
        assert_eq!(high, Duration::from_millis(2400));
        // attempt 4 hits the 10s cap before jitter.
        let capped = config.next_delay(4, None, &fixed_jitter(1.0)).unwrap();
        assert_eq!(capped, Duration::from_secs(12));
    }

    #[test]
    fn next_delay_floors_at_retry_after() {
        let config = RetryConfig::default();
        let delay = config.next_delay(1, Some(Duration::from_secs(9)), &fixed_jitter(0.0)).unwrap();
        assert_eq!(delay, Duration::from_secs(9));
        // Jittered value already above the floor stays untouched.
        let delay = config.next_delay(3, Some(Duration::from_secs(1)), &fixed_jitter(1.0)).unwrap();
        assert_eq!(delay, Duration::from_millis(9600));
    }

    #[test]
    fn next_delay_caps_retry_after_at_fifteen_minutes() {
        let config = RetryConfig::default();
        let delay =
            config.next_delay(1, Some(Duration::from_secs(3600)), &fixed_jitter(1.0)).unwrap();
        assert_eq!(delay, RETRY_AFTER_MAX);
    }

    #[test]
    fn next_delay_ceils_to_whole_milliseconds() {
        let config = RetryConfig {
            base: Duration::from_millis(3),
            cap: Duration::from_millis(10),
            max_attempts: 4,
        };
        // 3ms * 0.8..1.2 stays whole; force a fractional via factor mid-range.
        let delay = config.next_delay(1, None, &fixed_jitter(0.5)).unwrap();
        assert_eq!(delay, Duration::from_millis(3)); // 3 * 1.0 = 3ms
        let rng = fixed_jitter(0.25); // factor 0.9 => 2.7ms => ceil 3ms
        assert_eq!(config.next_delay(1, None, &rng).unwrap(), Duration::from_millis(3));
        let rng = fixed_jitter(0.75); // factor 1.1 => 3.3ms => ceil 4ms
        assert_eq!(config.next_delay(1, None, &rng).unwrap(), Duration::from_millis(4));
    }

    #[test]
    fn next_delay_uses_injected_rng() {
        let config = RetryConfig::default();
        let rng = Arc::new(SequentialRng::new());
        let first = config.next_delay(1, None, rng.as_ref()).unwrap();
        let second = config.next_delay(1, None, rng.as_ref()).unwrap();
        // Sequential values 0.0 and 0.000001 map to distinct factors.
        assert_ne!(first, second);
        assert!(first <= Duration::from_millis(1601));
    }

    #[test]
    fn retry_with_succeeds_after_transient_failures() {
        let options =
            RetryOptions { attempts: 3, delay: Duration::ZERO, ..RetryOptions::default() };
        let mut calls = 0;
        let value = retry_with(
            &options,
            |err: &String| is_transient_message(err),
            || {
                calls += 1;
                if calls < 3 {
                    Err("Failed to fetch".to_string())
                } else {
                    Ok("ready")
                }
            },
        )
        .unwrap();
        assert_eq!(value, "ready");
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_with_stops_on_non_transient_error() {
        let options =
            RetryOptions { attempts: 3, delay: Duration::ZERO, ..RetryOptions::default() };
        let mut calls = 0;
        let err = retry_with(
            &options,
            |err: &String| is_transient_message(err),
            || {
                calls += 1;
                Err::<(), _>("invalid response".to_string())
            },
        )
        .unwrap_err();
        assert_eq!(err, "invalid response");
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_with_honors_custom_predicate_and_budget() {
        let options =
            RetryOptions { attempts: 2, delay: Duration::ZERO, ..RetryOptions::default() };
        let mut calls = 0;
        let err = retry_with(
            &options,
            |_| true,
            || {
                calls += 1;
                Err::<(), _>("retry me".to_string())
            },
        )
        .unwrap_err();
        assert_eq!(err, "retry me");
        assert_eq!(calls, 2);
    }

    #[test]
    fn retry_with_backoff_growth() {
        let options = RetryOptions {
            attempts: 4,
            delay: Duration::from_millis(1),
            factor: 2.0,
            max_delay: Duration::from_millis(2),
        };
        let mut calls = 0;
        let start = std::time::Instant::now();
        retry_with(
            &options,
            |err: &String| is_transient_message(err),
            || {
                calls += 1;
                Err::<(), _>("etimedout".to_string())
            },
        )
        .unwrap_err();
        // 1ms + 2ms (capped) sleeps: growth stops at max_delay.
        assert_eq!(calls, 4);
        assert!(start.elapsed() >= Duration::from_millis(3));
    }

    #[test]
    fn transient_message_matrix() {
        assert!(is_transient_message("Failed to fetch"));
        assert!(is_transient_message("ECONNRESET while reading"));
        assert!(is_transient_message("socket hang up"));
        assert!(!is_transient_message("getaddrinfo ENOTFOUND"));
        assert!(!is_transient_message("invalid response"));
        assert!(!is_transient_message(""));
    }

    #[test]
    fn constants_match_runner_policy() {
        assert_eq!(RETRY_AFTER_MAX, Duration::from_mins(15));
        assert_eq!(MAX_TIMEOUT_RETRIES, 3);
        // A pinned clock keeps the determinism seams usable from this module.
        assert_eq!(FixedClock::epoch().unix_ms(), 0);
    }
}
