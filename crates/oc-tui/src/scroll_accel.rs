//! G-102 — the scroll-wheel acceleration model.
//!
//! Port of `packages/tui/src/util/scroll.ts` +
//! `@opentui/core`'s `MacOSScrollAccel` (src/lib/scroll-acceleration.ts,
//! core 0.6.1): the wheel step per event comes from the live config —
//!
//! * `scroll.acceleration: true` → the macOS-style accel streak model
//!   (quick bursts ramp 1×→6×, relaxed gestures stay at 1×);
//! * else `scroll.speed: N` → a constant N rows a notch;
//! * the default → 3 rows a notch (the pre-G-102 behavior).
//!
//! The acceleration math is byte-faithful to the core class:
//! a 3-sample moving window of inter-event intervals (≥6 ms apart,
//! streaks reset after 150 ms idle), velocity =
//! `100ms / avgInterval`, multiplier = `1 + 0.8·(exp(v/3) − 1)`
//! clamped to 6.

use std::collections::VecDeque;

/// The accel model's constants (the core class's fields).
const HISTORY_SIZE: usize = 3;
const STREAK_TIMEOUT_MS: i64 = 150;
const MIN_TICK_INTERVAL_MS: i64 = 6;
const REFERENCE_INTERVAL_MS: f64 = 100.0;
/// `opts` defaults: A = 0.8, tau = 3, maxMultiplier = 6.
const A: f64 = 0.8;
const TAU: f64 = 3.0;
const MAX_MULTIPLIER: f64 = 6.0;

/// `CustomSpeedScroll` (scroll.ts:9-15): a constant rows-per-notch
/// model (`tick` always returns the configured speed).
#[derive(Debug, Clone, PartialEq)]
pub struct CustomSpeedScroll {
    pub speed: f64,
}

/// `MacOSScrollAccel` (core's scroll-acceleration.ts): the streak
/// model over inter-event intervals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacOsScrollAccel {
    last_tick_ms: Option<i64>,
    velocity_history: VecDeque<i64>,
}

impl MacOsScrollAccel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

#[allow(clippy::cast_precision_loss)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
    /// The multiplier for this event (the core `tick`, default opts).
    #[must_use]
    pub fn tick(&mut self, now_ms: i64) -> f64 {
        let Some(last) = self.last_tick_ms else {
            self.last_tick_ms = Some(now_ms);
            self.velocity_history.clear();
            return 1.0;
        };
        let dt = now_ms - last;
        if dt > STREAK_TIMEOUT_MS {
            self.last_tick_ms = Some(now_ms);
            self.velocity_history.clear();
            return 1.0;
        }
        if dt < MIN_TICK_INTERVAL_MS {
            return 1.0;
        }
        self.last_tick_ms = Some(now_ms);
        self.velocity_history.push_back(dt);
        if self.velocity_history.len() > HISTORY_SIZE {
            self.velocity_history.pop_front();
        }
        let avg_interval = self.velocity_history.iter().copied().sum::<i64>() as f64
            / self.velocity_history.len() as f64;
        let velocity = REFERENCE_INTERVAL_MS / avg_interval;
        let x = velocity / TAU;
        let multiplier = 1.0 + A * (x.exp() - 1.0);
        multiplier.min(MAX_MULTIPLIER)
    }

    /// The core `reset` (a fresh streak).
    pub fn reset(&mut self) {
        self.last_tick_ms = None;
        self.velocity_history.clear();
    }
}

/// `getScrollAcceleration` (scroll.ts:20-28): the model the live
/// config selects — acceleration FIRST (ref order), else the speed
/// override, else the 3-rows default.
#[derive(Debug, Clone, PartialEq)]
pub enum ScrollAcceleration {
    MacOs(MacOsScrollAccel),
    CustomSpeed(CustomSpeedScroll),
}

impl ScrollAcceleration {
    #[must_use]
    pub fn from_config(acceleration: bool, speed: Option<f64>) -> Self {
        if acceleration {
            return Self::MacOs(MacOsScrollAccel::new());
        }
        Self::CustomSpeed(CustomSpeedScroll { speed: speed.unwrap_or(3.0) })
    }

#[allow(clippy::cast_possible_truncation)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
    /// The wheel step (rows, whole numbers — a fractional row cannot
    /// move a cell grid; the ref's scrollbox keeps the fraction in its
    /// own accumulator, a bounded deviation documented here).
    #[must_use]
    pub fn tick(&mut self, now_ms: i64) -> i64 {
        let step = match self {
            Self::MacOs(model) => model.tick(now_ms),
            Self::CustomSpeed(model) => model.speed,
        };
        // The multiplier floors at 1 row; a configured speed of 0
        // stays 0 (the config row's minimum is 0.25, so this only
        // guards degenerate values).
        step.round().max(0.0) as i64
    }

    pub fn reset(&mut self) {
        match self {
            Self::MacOs(model) => model.reset(),
            Self::CustomSpeed(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default model is the constant 3-rows step (the pre-G-102
    /// wheel math preserved).
    #[test]
    fn default_is_constant_three_rows() {
        let mut model = ScrollAcceleration::from_config(false, None);
        assert_eq!(model.tick(0), 3);
        assert_eq!(model.tick(500), 3);
    }

    /// `scroll.speed` overrides the step (live value).
    #[test]
    fn speed_override_applies() {
        let mut model = ScrollAcceleration::from_config(false, Some(5.0));
        assert_eq!(model.tick(0), 5);
    }

    /// Acceleration wins over speed in the config order (scroll.ts:
    /// 20-28 checks `acceleration` first).
    #[test]
    fn acceleration_takes_precedence() {
        let model = ScrollAcceleration::from_config(true, Some(9.0));
        assert!(matches!(model, ScrollAcceleration::MacOs(_)));
    }

    /// The accel math: an isolated event, then a fast streak ramps the
    /// multiplier toward the 6× cap; a slow streak stays near 1×.
    #[test]
    fn macos_accel_ramps_on_fast_streaks() {
        let mut model = MacOsScrollAccel::new();
        // First tick: no history → 1.
        assert!((model.tick(1000) - 1.0).abs() < f64::EPSILON);
        // A fast streak: 15ms apart events.
        let mut now = 1000i64;
        let mut last = 1.0f64;
        for _ in 0..10 {
            now += 15;
            let value = model.tick(now);
            assert!(value >= last, "the streak ramps: {value} >= {last}");
            last = value;
        }
        // 15ms → velocity ≈ 6.67 → x ≈ 2.22 → 1+0.8·(e^2.22−1) ≈ 7.6
        // clamped to 6.
        assert!((last - MAX_MULTIPLIER).abs() < 1e-9);
        // An idle gap resets the streak to 1.
        now += STREAK_TIMEOUT_MS + 1;
        assert!((model.tick(now) - 1.0).abs() < f64::EPSILON);
    }

    /// A relaxed streak (60ms apart → velocity 1.67 → ≈1.34×) stays
    /// precise; sub-6ms coalesced events return 1 without advancing.
    #[test]
    fn macos_accel_stays_low_on_slow_streaks() {
        let mut model = MacOsScrollAccel::new();
        let _ = model.tick(0);
        let mut now = 0i64;
        let mut peak = 1.0f64;
        for _ in 0..8 {
            now += 60;
            peak = peak.max(model.tick(now));
        }
        assert!(peak < 2.0, "slow streaks stay near the base, got {peak}");
        // Coalesced (dt < 6ms): no history advance, base multiplier.
        now += 3;
        assert!((model.tick(now) - 1.0).abs() < f64::EPSILON);
        // reset() clears the streak.
        model.reset();
        assert!((model.tick(now + 1000) - 1.0).abs() < f64::EPSILON);
    }

    /// The whole-number step the wheel path consumes.
    #[test]
    fn enum_tick_rounds_to_whole_rows() {
        let mut model = ScrollAcceleration::from_config(true, None);
        assert_eq!(model.tick(0), 1);
    }
}
