//! G-128 — the spring/tween animation engine (ref `ui/animation.ts`,
//! v2.0.24).
//!
//! The ref's `createAnimatable` couples a solid signal to a
//! `setInterval(…, 16)` task pump. The port's equivalent is
//! TIME-DERIVED and pump-sampled: [`Animatable::advance`] takes the
//! caller's clock (the store's 100 ms pump tick — G-49a pins that
//! cadence and forbids global 40 ms wakes), derives the delta from
//! the LAST sample, and reports whether the value still moves (the
//! caller keeps redrawing while any animatable reports `true`).
//! Bounded deviation (G-128): animation granularity is limited to
//! the pump tick cadence (100 ms), never the ref's ~16 ms frame
//! clock; springs advance on wall-clock deltas so a missed tick does
//! not slow the trajectory, only its sampling.
//!
//! The math is byte-faithful to the ref:
//!
//! * `advanceSpring` (animation.ts:139-163): a decaying harmonic
//!   integrator with `frequency = 2π / (visualDuration × 1.2)`,
//!   `decay = exp(−frequency × delta)` and the ref's exact
//!   velocity/value update order;
//! * `advanceTween` (:165-176): normalized progress eased from
//!   `from` toward `target` (`smoothstep` default, :37);
//! * rest detection at `restDelta`/`restSpeed` (0.002 defaults,
//!   :31-33), the 0.05 s delta cap (:76), and the settle→`jump`
//!   finish (the value SNAPS to target when the rest thresholds
//!   pass, :78-81).

use std::collections::BTreeMap;

/// `smoothstep` (animation.ts:37) — the default tween ease.
#[must_use]
pub fn smoothstep(progress: f64) -> f64 {
    progress * progress * (3.0 - 2.0 * progress)
}

/// `smootherstep` (tab-pulse.ts — the shimmer/DIM consumer's ease).
#[must_use]
pub fn smootherstep(progress: f64) -> f64 {
    let t = progress.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// `spring({visualDuration, restDelta, restSpeed})` (animation.ts:29-35).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringTransition {
    pub visual_duration: f64,
    pub rest_delta: f64,
    pub rest_speed: f64,
}

impl SpringTransition {
    #[must_use]
    pub fn new(visual_duration: f64) -> Self {
        Self { visual_duration, rest_delta: 0.002, rest_speed: 0.002 }
    }
}

/// `tween({duration, ease})` (animation.ts:37-42).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenTransition {
    /// Seconds (the ref's `duration` is seconds against `now/1_000`).
    pub duration: f64,
    /// The ease fn (`smoothstep` default).
    pub ease: fn(f64) -> f64,
}

/// `Transition` (animation.ts:15-19).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transition {
    Spring(SpringTransition),
    Tween(TweenTransition),
}

/// One value's integrator state (`ValueState`, animation.ts:21-27).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ValueState {
    value: f64,
    target: f64,
    velocity: f64,
    from: f64,
}

/// `createAnimatable(initial, {transition})` — the multi-key scalar
/// form (the port's consumers animate scalars: widths, opacities).
/// Map-ordered for deterministic reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Animatable {
    transition: Transition,
    state: BTreeMap<String, ValueState>,
    /// The last sample's clock (ms) — `previous` (animation.ts:70).
    previous_ms: Option<f64>,
    /// The tween start clock (ms) — `started` (:69).
    started_ms: f64,
}

impl Animatable {
    /// `createAnimatable` over `(key, value)` pairs.
    #[must_use]
    pub fn new(initial: &[(&str, f64)], transition: Transition, started_ms: f64) -> Self {
        let state = initial
            .iter()
            .map(|&(key, value)| {
                (key.to_owned(), ValueState { value, target: value, velocity: 0.0, from: value })
            })
            .collect();
        Self { transition, state, previous_ms: None, started_ms }
    }

    /// `animate(next)` (:84-98): a same-target call is a no-op; a new
    /// target restarts the integrators from the CURRENT values. The
    /// first `advance` after an `animate` seeds `previous` (the ref
    /// resets `started`/`previous` at animate time — the port defers
    /// to the next pump sample, same wall-clock semantics).
    pub fn animate(&mut self, now_ms: f64, next: &[(&str, f64)]) {
        let same_target = next
            .iter()
            .all(|&(key, target)| self.state.get(key).is_some_and(|state| state.target == target))
            && next.len() == self.state.len();
        if same_target {
            return;
        }
        self.started_ms = now_ms;
        self.previous_ms = Some(now_ms);
        for &(key, target) in next {
            let Some(state) = self.state.get_mut(key) else { continue };
            state.target = target;
            state.from = state.value;
        }
    }

    /// `jump(next)` (:100-105): snap — values, targets and velocities
    /// land immediately, nothing schedules.
    pub fn jump(&mut self, next: &[(&str, f64)]) {
        for &(key, value) in next {
            self.state.insert(
                key.to_owned(),
                ValueState { value, target: value, velocity: 0.0, from: value },
            );
        }
        self.previous_ms = None;
    }

    /// `step(now)` (:74-83): advance every value by the elapsed
    /// delta (capped at 0.05 s, animation.ts:76) and report whether
    /// the animatable still moves. A settled spring/tween FINISHES by
    /// snapping to target (`jump(target)`, :78-79).
    pub fn advance(&mut self, now_ms: f64) -> bool {
        let Some(previous) = self.previous_ms else {
            self.previous_ms = Some(now_ms);
            return self.moving();
        };
        let delta = (now_ms - previous).max(0.0) / 1000.0;
        self.previous_ms = Some(now_ms);
        let delta = delta.min(0.05);
        let moving = match self.transition {
            Transition::Spring(spec) => {
                let mut moving = false;
                for state in self.state.values_mut() {
                    let (value, velocity) =
                        advance_spring(spec, state.value, state.velocity, state.target, delta);
                    state.value = value;
                    state.velocity = velocity;
                    moving |= (value - state.target).abs() > spec.rest_delta
                        || velocity.abs() > spec.rest_speed;
                }
                moving
            }
            Transition::Tween(spec) => {
                let progress =
                    ((now_ms - self.started_ms) / 1000.0 / spec.duration.max(0.001)).min(1.0);
                let eased = (spec.ease)(progress);
                for state in self.state.values_mut() {
                    state.value = state.from + (state.target - state.from) * eased;
                    state.velocity = 0.0;
                }
                progress < 1.0
            }
        };
        if !moving {
            // The ref's finish: `jump(target)` — snap past the rest
            // thresholds so reads land on the exact target.
            let targets: Vec<(String, f64)> =
                self.state.iter().map(|(key, state)| (key.clone(), state.target)).collect();
            for (key, target) in targets {
                if let Some(state) = self.state.get_mut(&key) {
                    state.value = target;
                    state.velocity = 0.0;
                }
            }
        }
        moving
    }

    /// Whether any value sits off its target (`settled()`'s inverse).
    #[must_use]
    pub fn moving(&self) -> bool {
        match self.transition {
            Transition::Spring(spec) => self.state.values().any(|state| {
                (state.value - state.target).abs() > spec.rest_delta
                    || state.velocity.abs() > spec.rest_speed
            }),
            Transition::Tween(_) => true,
        }
    }

    /// `value()[key]` — the animated read (target once settled).
    #[must_use]
    pub fn value(&self, key: &str) -> Option<f64> {
        self.state.get(key).map(|state| if self.moving() { state.value } else { state.target })
    }
}

/// `advanceSpring` for one value (animation.ts:139-163, scalar arm):
/// the ref's exact update order —
/// `nextVelocity = velocity + frequency × offset`;
/// `value ← target + (offset + nextVelocity × delta) × decay`;
/// `velocity ← (velocity − frequency × nextVelocity × delta) × decay`.
#[must_use]
pub fn advance_spring(
    spec: SpringTransition,
    value: f64,
    velocity: f64,
    target: f64,
    delta: f64,
) -> (f64, f64) {
    let frequency = std::f64::consts::TAU / (spec.visual_duration.max(0.001) * 1.2);
    let offset = value - target;
    let decay = std::f64::consts::E.powf(-frequency * delta);
    let next_velocity = velocity + frequency * offset;
    let value = target + (offset + next_velocity * delta) * decay;
    let velocity = (velocity - frequency * next_velocity * delta) * decay;
    (value, velocity)
}

/// `advanceTween` for one value (animation.ts:165-176).
#[must_use]
pub fn advance_tween(spec: TweenTransition, from: f64, target: f64, progress: f64) -> f64 {
    let eased = (spec.ease)(progress.clamp(0.0, 1.0));
    from + (target - from) * eased
}

/// The RetryProvider's timing constants (retry-provider.tsx:19-21):
/// the FADE half-duration and the RESIZE midpoint, both ms.
pub const RETRY_FADE_MS: f64 = 80.0;
pub const RETRY_RESIZE_MS: f64 = 60.0;
/// `DURATION = FADE * 2 + RESIZE` (retry-provider.tsx:22).
pub const RETRY_DURATION_MS: f64 = RETRY_FADE_MS * 2.0 + RETRY_RESIZE_MS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_and_snaps_to_target() {
        let spec = SpringTransition::new(0.2);
        let mut value = 0.0f64;
        let mut velocity = 0.0f64;
        for _ in 0..200 {
            let (next_value, next_velocity) =
                advance_spring(spec, value, velocity, 1.0, 1.0 / 60.0);
            value = next_value;
            velocity = next_velocity;
        }
        assert!((value - 1.0).abs() <= spec.rest_delta, "converged, got {value}");
        assert!(velocity.abs() <= spec.rest_speed);
    }

    #[test]
    fn animatable_advances_on_the_pump_clock_and_finishes_by_snapping() {
        let mut animatable = Animatable::new(
            &[("width", 22.0)],
            Transition::Spring(SpringTransition::new(0.2)),
            0.0,
        );
        animatable.animate(100.0, &[("width", 32.0)]);
        let mut moving = false;
        let mut now = 100.0;
        for _ in 0..100 {
            now += 100.0; // the 100 ms pump tick (G-49a)
            moving = animatable.advance(now);
        }
        assert!(!moving, "settled within 10 s of pump ticks");
        assert_eq!(animatable.value("width"), Some(32.0), "snapped to target");
    }

    #[test]
    fn same_target_animate_is_a_noop_and_jump_lands() {
        let mut animatable = Animatable::new(
            &[("opacity", 1.0)],
            Transition::Spring(SpringTransition::new(0.1)),
            0.0,
        );
        animatable.animate(10.0, &[("opacity", 1.0)]);
        assert!(!animatable.moving());
        animatable.animate(10.0, &[("opacity", 0.0)]);
        assert!(animatable.moving());
        animatable.jump(&[("opacity", 0.5)]);
        assert_eq!(animatable.value("opacity"), Some(0.5));
        assert!(!animatable.moving());
    }

    #[test]
    fn tween_eases_between_edges() {
        let spec = TweenTransition { duration: 0.1, ease: smoothstep };
        assert_eq!(advance_tween(spec, 0.0, 10.0, 0.0), 0.0);
        assert_eq!(advance_tween(spec, 0.0, 10.0, 1.0), 10.0);
        let mid = advance_tween(spec, 0.0, 10.0, 0.5);
        assert!((mid - 5.0).abs() < 1e-9, "smoothstep is symmetric, got {mid}");
        // The tween finishes: progress reaches 1 and reports done.
        let mut animatable = Animatable::new(&[("x", 0.0)], Transition::Tween(spec), 0.0);
        animatable.animate(0.0, &[("x", 4.0)]);
        assert!(animatable.advance(50.0), "halfway: still moving");
        assert!((animatable.value("x").unwrap() - 2.0).abs() < 1e-9, "halfway of 0→4");
        assert!(!animatable.advance(150.0), "past duration: done");
        assert_eq!(animatable.value("x"), Some(4.0));
    }

    #[test]
    fn retry_provider_constants_match_the_ref() {
        assert_eq!(RETRY_FADE_MS, 80.0);
        assert_eq!(RETRY_RESIZE_MS, 60.0);
        assert_eq!(RETRY_DURATION_MS, 220.0);
    }
}
