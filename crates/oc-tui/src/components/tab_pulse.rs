//! The deterministic tab-glow tint math, ported from
//! `component/tab-pulse.tsx:33-80` + `session-tabs.tsx:124-125` +
//! `theme/color.ts` (gate-9 finding 1 — the Tier-A obligation).
//!
//! **Ownership (lane D3-6).** Shared deterministic math library —
//! **not** dead code. The math was absorbed by the solid surfaces,
//! not duplicated into them: live consumers are the tabs strip
//! ([`crate::components::session_tabs_solid`]), the vertical rail
//! ([`crate::components::session_tabs_rail_solid`]), the sidebar
//! glow ([`crate::components::sidebar_solid`]) and the store's
//! [`TabPulseTrack`] descendants ([`crate::store`]). It writes no
//! [`crate::render::Frame`]; multi-family use forbids absorbing it
//! into one consumer, so it remains a top-level compute library.
//!
//! The unread glow is **pure position math, no clock**: a resting
//! (sustain-level) glow paints each cell of a pulse strip as
//!
//! ```text
//! bg(cell) = background + (glowColor − background) · GLOW_OPACITY · intensity(cell)
//! ```
//!
//! and the title characters riding it tint with
//!
//! ```text
//! fg(char) = tint(base, glowColor, 0.12 · intensity(1 + numberWidth + charIndex, tabWidth))
//! ```
//!
//! where `intensity` is a smootherstep falloff clamped to a tail of
//! `min(GLOW_TAIL, max(1, width − 2))` cells. Only the time-varying
//! envelopes (ignition, release swell, sweep, flash, marquee) stay C2 —
//! a golden frame is the sustain state, which is why the corpus bytes
//! are stable.
//!
//! `tint` operates in the reference's float space and rounds per
//! channel at the end (`RGBA.fromInts(round(c · 255))`) — computing in
//! 0..=255 space is linearly identical, which the golden-pinned tests
//! below verify cell-for-cell.

// The tint math rounds per channel by design (`fromInts(round(c))`)
// and indexes cells by `usize` against small f64 tails; clock ms are
// bounded monotonic instants. The pedantic cast lints carry no real
// risk on terminal dimensions/instants.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]
/// A truecolor channel triple (the painter's color vocabulary).
pub type Rgb = (u8, u8, u8);
/// the strip width (`tab-pulse.tsx`).
pub const GLOW_TAIL: f64 = 12.0;
/// `GLOW_OPACITY` — the resting glow's amplitude over the background.
pub const GLOW_OPACITY: f64 = 0.16;
/// The title-tint amplitude (`glowTextColor`: `0.12 · intensity`).
pub const GLOW_TEXT_ALPHA: f64 = 0.12;

/// `clamp` — saturation to `[0, 1]`.
#[must_use]
pub fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// `smoothstep` — the ref `tween()`'s DEFAULT ease (`animation.ts:35`
/// `progress²·(3−2·progress)`; `:50` `ease: options.ease ?? smoothstep`).
/// G-501: the tween faces that pass NO explicit ease — the marquee
/// leading fade (250 ms), the number ignition (0.7 s) and the
/// indicator dissolve (0.18 s) — ride THIS curve (3t²−2t³), not
/// smootherstep/linear.
#[must_use]
pub fn smoothstep(value: f64) -> f64 {
    let v = clamp01(value);
    v * v * (3.0 - 2.0 * v)
}

/// `smootherstep` — the easing the pulse SHAPES ride (the ref's
/// explicit `smootherstep` stages, tab-pulse.tsx:261/276-277).
#[must_use]
pub fn smootherstep(value: f64) -> f64 {
    let v = clamp01(value);
    v * v * v * (v * (v * 6.0 - 15.0) + 10.0)
}

/// `glowIntensityAt(index, tail)` — the resting glow's spatial falloff:
/// full at cells 0-1, then a smootherstep decay over `tail` cells
/// behind cell 1.
#[must_use]
pub fn glow_intensity_at(index: usize, tail: f64) -> f64 {
    let distance = f64::from(u32::try_from(index.saturating_sub(1)).unwrap_or(u32::MAX));
    smootherstep(clamp01(1.0 - distance / tail))
}

/// `unreadGlowIntensity(index, width, maximumTail = GLOW_TAIL)` —
/// [`glow_intensity_at`] with the tail clamped to the strip
/// (`min(maximumTail, max(1, width − 2))`).
#[must_use]
pub fn unread_glow_intensity(index: usize, width: usize, maximum_tail: f64) -> f64 {
    glow_intensity_at(index, pulse_tail(maximum_tail, width))
}

/// The effective pulse tail for a strip of `width` cells
/// (`renderSelf`'s clamp).
#[must_use]
pub fn pulse_tail(glow_tail: f64, width: usize) -> f64 {
    let clamp = f64::from(u32::try_from(width.max(3) - 2).unwrap_or(u32::MAX)).max(1.0);
    glow_tail.min(clamp)
}

/// `tint` (`theme/color.ts`): per-channel linear blend toward
/// `overlay` by `alpha`, rounded once at the end.
#[must_use]
pub fn tint(base: Rgb, overlay: Rgb, alpha: f64) -> Rgb {
    let mix = |b: u8, o: u8| (f64::from(b) + (f64::from(o) - f64::from(b)) * alpha).round();
    (mix(base.0, overlay.0) as u8, mix(base.1, overlay.1) as u8, mix(base.2, overlay.2) as u8)
}

/// `glowTextColor(base, glow, index, width, level = 1)`
/// (`session-tabs.tsx:124`): the title-character tint riding the glow.
#[must_use]
pub fn glow_text_color(base: Rgb, glow: Rgb, index: usize, width: usize, level: f64) -> Rgb {
    tint(base, glow, GLOW_TEXT_ALPHA * unread_glow_intensity(index, width, GLOW_TAIL) * level)
}

/// The resting (sustain) glow's per-cell background —
/// `blendTabPulseColor` with only the glow stage active:
/// `background + (glowColor − background) · GLOW_OPACITY · level ·
/// intensity(cell)`.
#[must_use]
pub fn resting_glow_bg(
    background: Rgb,
    glow_color: Rgb,
    index: usize,
    pulse_width: usize,
    glow_tail: f64,
    level: f64,
) -> Rgb {
    let tail = pulse_tail(glow_tail, pulse_width);
    tint(background, glow_color, GLOW_OPACITY * glow_intensity_at(index, tail) * level)
}

/// The blend for a cell where every stage is idle — plain background
/// (the separator row's inner color when the tab itself does not glow).
#[must_use]
pub fn idle_pulse_bg(background: Rgb) -> Rgb {
    background
}

// ---------------------------------------------------------------------------
// The animated phases (tab-pulse.tsx:31-115 + 125-427) — lane TAIL, the
// user's busy-session findings (ROADMAP 2026-10-16): the clockwise dot
// spinner, the left-to-right cycling sweep, and the envelopes that
// shape them. The envelope levels derive PURELY from a transition-stamp
// track + the injected clock, so every render is stateless and
// reproducible: a frozen clock pins the sustain state (golden frames
// stay byte-stable), a fresh store starts each envelope at its origin.
// ---------------------------------------------------------------------------

/// `RUN_DURATION` — one left-to-right sweep pass (ms).
pub const RUN_DURATION: u64 = 2_800;
/// `RUN_ATTACK` — the sweep's fade-in (ms).
pub const RUN_ATTACK: u64 = 450;
/// `RUN_HEAD`/`RUN_TAIL` — the sweep band's cells ahead/behind the front.
pub const RUN_HEAD: f64 = 4.0;
pub const RUN_TAIL: f64 = 18.0;
/// `RUN_FADE_OUT` — the sweep's note-off fade (ms).
pub const RUN_FADE_OUT: u64 = 500;
/// The sweep band's peak amplitude over the tab background (`0.14`,
/// tab-pulse.tsx renderSelf).
pub const RUN_SWEEP_OPACITY: f64 = 0.14;
/// `COMPLETION_*` — the resolve flash (`attackDecay` peak 1 × 0.18).
pub const COMPLETION_DURATION: u64 = 1_200;
pub const COMPLETION_ATTACK: f64 = 0.12;
pub const COMPLETION_OPACITY: f64 = 0.18;
/// `GLOW_IGNITION_*`/`GLOW_RELEASE_*` — the glow's attack/release shapes.
pub const GLOW_IGNITION_DURATION: u64 = 600;
pub const GLOW_IGNITION_PEAK: f64 = 1.5;
pub const GLOW_IGNITION_ATTACK: f64 = 0.3;
pub const GLOW_RELEASE_DURATION: u64 = 900;
pub const GLOW_RELEASE_ATTACK: f64 = 0.12;
pub const GLOW_RELEASE_PEAK: f64 = 1.25;
/// `GLOW_FADE_OUT` — the drain window inside the release (ms).
pub const GLOW_FADE_OUT_MS: u64 = 200;
/// `EDGE_FLASH_*` (tab-pulse.tsx:42-45) — the prompt-pulse flash: an
/// 800 ms attackDecay one-shot at 0.1 opacity.
pub const EDGE_FLASH_DURATION: u64 = 800;
pub const EDGE_FLASH_ATTACK: f64 = 0.1;
pub const EDGE_FLASH_OPACITY: f64 = 0.1;
/// `PROMPT_FLASH_SCALE` — the flash's restart scale on a `pulse++`
/// (`setPromptPulse` → `edgeFlash.restart(2)`).
pub const PROMPT_FLASH_SCALE: f64 = 2.0;
/// `flashTail` — the rail arms' spatial-falloff prop
/// (session-tabs.tsx:992/1018/1039/1125; the horizontal strip's own
/// mount passes NO tail, so its flash is uniform).
pub const FLASH_TAIL: f64 = 8.0;
/// `createNumberIgnition` (session-tabs.tsx:215-230): a 0.7 s tween
/// from the 0.85 jump down to 0.
pub const IGNITION_DURATION_MS: u64 = 700;
pub const IGNITION_JUMP: f64 = 0.85;
/// `TabIndicator`'s fade (session-tabs.tsx:161-172): the unread
/// marker's 180 ms dissolve (tween).
pub const DISSOLVE_DURATION_MS: u64 = 180;

/// `tabFlashIntensity(index, tail)` (tab-pulse.tsx:91-92) — the
/// flash's spatial falloff, the glow's own `glowIntensityAt`.
#[must_use]
pub fn tab_flash_intensity(index: usize, tail: f64) -> f64 {
    glow_intensity_at(index, tail)
}

/// `intensityAt(index, front, head, tail)` (tab-pulse.tsx:66-69): the
/// sweep band's spatial profile — a `head`-wide smootherstep ramp
/// ahead of the front, a `tail`-long falloff behind it.
#[must_use]
pub fn intensity_at(index: usize, front: f64, head: f64, tail: f64) -> f64 {
    let distance = front - f64::from(index as i32);
    if distance < 0.0 {
        smootherstep(clamp01(1.0 + distance / head))
    } else {
        smootherstep(clamp01(1.0 - distance / tail))
    }
}

/// `coast(value)` (tab-pulse.tsx:70-75): the sweep's eased traversal —
/// a quadratic ramp-in/out around a linear middle.
#[must_use]
pub fn coast(value: f64) -> f64 {
    let ramp = 0.2;
    let v = clamp01(value);
    if v < ramp {
        (v * v) / (2.0 * ramp * (1.0 - ramp))
    } else if v > 1.0 - ramp {
        1.0 - ((1.0 - v) * (1.0 - v)) / (2.0 * ramp * (1.0 - ramp))
    } else {
        (v - ramp / 2.0) / (1.0 - ramp)
    }
}

/// `fadeOut(progress)` = `1 − smootherstep(progress)`.
#[must_use]
pub fn fade_out(progress: f64) -> f64 {
    1.0 - smootherstep(clamp01(progress))
}

/// `attackDecay(progress, attack, peak, rest)` (tab-pulse.tsx:78-81):
/// rise to `peak` over the attack fraction, settle to `rest` after.
#[must_use]
pub fn attack_decay(progress: f64, attack: f64, peak: f64, rest: f64) -> f64 {
    let p = clamp01(progress);
    if p < attack {
        peak * smootherstep(clamp01(p / attack))
    } else {
        peak - (peak - rest) * smootherstep(clamp01((p - attack) / (1.0 - attack)))
    }
}

/// `glowIgnitionLevel` — the glow attack rises past 1 then settles.
#[must_use]
pub fn glow_ignition_level(progress: f64) -> f64 {
    attack_decay(progress, GLOW_IGNITION_ATTACK, GLOW_IGNITION_PEAK, 1.0)
}

/// `glowReleaseSwell` — the diffusing glow swells above its resting
/// level, then decays to nothing.
#[must_use]
pub fn glow_release_swell_shape(progress: f64) -> f64 {
    attack_decay(progress, GLOW_RELEASE_ATTACK, GLOW_RELEASE_PEAK, 0.0)
}

/// `glowReleaseSpread(progress, tail, width)` — how far a resolving
/// glow has diffused: the resting tail spreads across the full width.
// Terminal widths are far below 2^52 — the pedantic precision lint
// carries no real risk (the module's standing allowance).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn glow_release_spread(progress: f64, tail: f64, width: usize) -> f64 {
    tail + smootherstep(clamp01(progress)) * width.max(1) as f64
}

/// `blendTabPulseColor` (tab-pulse.tsx:116-130): the staged blend —
/// glow tint, then sweep, flash and completion each pull the running
/// color the rest of the way. (One argument per stage, the reference
/// signature.)
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn blend_tab_pulse_color(
    background: Rgb,
    glow_color: Rgb,
    running_color: Rgb,
    flash_color: Rgb,
    completion_color: Rgb,
    glow: f64,
    running: f64,
    flash: f64,
    completion: f64,
) -> Rgb {
    let mut out = if glow == 0.0 { background } else { tint(background, glow_color, glow) };
    if running != 0.0 {
        out = tint(out, running_color, running);
    }
    if flash != 0.0 {
        out = tint(out, flash_color, flash);
    }
    if completion != 0.0 {
        out = tint(out, completion_color, completion);
    }
    out
}

/// `elapsed(now, since)` in whole ms (monotonic, saturating).
fn elapsed(now: u64, since: u64) -> u64 {
    now.saturating_sub(since)
}

/// One tab's pulse history — the transition stamps the live path
/// records when a tab's status flips ([`TabPulseTrack::sync`], the
/// tsx prop setters' effect); every envelope level below derives
/// purely from a track + the clock.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
// The ref's track state IS a flat flag set (the tsx props:
// active/complete/attention/glowing/renaming are independent booleans
// on the session-tab status); folding them into enums would diverge
// from the mirrored shape, so the flat bools stay.
#[allow(clippy::struct_excessive_bools)]
pub struct TabPulseTrack {
    /// When the busy sweep began (`setActive(true)`).
    pub active_since: Option<u64>,
    /// When busy ended (the release fade's origin).
    pub idle_since: Option<u64>,
    /// When the unread glow ignited.
    pub glow_since: Option<u64>,
    /// When the glow released.
    pub glow_off_since: Option<u64>,
    /// When the completion flash fired.
    pub completion_since: Option<u64>,
    /// G-503③ — the busy→idle PENDING transition (`setActive(false)`
    /// sets `completionPending = true`, tab-pulse.tsx:347-350): the
    /// one-shot arms only through this gate — `complete` arriving
    /// while the pending lives (before the run release's 500 ms
    /// window ends) fires it (:362-374, :397-403); a complete stamp
    /// at mount or mid-busy fires NOTHING on its own.
    pub completion_pending: bool,
    /// G-479 — the per-session prompt-pulse counter
    /// (`promptPulses`, context/session-tabs.tsx:80/310-315: one per
    /// `session.inbox.enqueued` user item on a non-current tab).
    pub prompt_pulses: u64,
    /// G-479 — the edge flash's origin and scale: a runs flip starts
    /// it at scale 1 (`setActive` → `edgeFlash.start()`), a `pulse++`
    /// restarts it at [`PROMPT_FLASH_SCALE`] (`setPromptPulse` →
    /// `edgeFlash.restart(2)`); a restart mid-flight re-stamps.
    pub flash_since: Option<(u64, f64)>,
    /// G-479 — the number ignition's origin (`createNumberIgnition`:
    /// jump 0.85 then tween to 0 over 0.7 s — retriggered by a runs
    /// flip OR a `pulse++`, session-tabs.tsx:215-230).
    pub ignition_since: Option<u64>,
    /// G-479 — the unread marker's dissolve origin: stamped when the
    /// indicator's `unread` memo falls (not while pending — busy or
    /// attention jumps the fade straight to 0,
    /// session-tabs.tsx:161-172).
    pub dissolve_since: Option<u64>,
    /// G-479 — the dissolve's latched hue was the error unread (the
    /// ref latches the last `unreadColor`, `TabIndicator`'s memo).
    pub dissolve_error: bool,
    /// G-503② — the glow epoch began at MOUNT in sustain (the
    /// `GatedEnvelope` constructor's initial "sustain",
    /// tab-pulse.tsx:274-279: a glow that exists at mount holds
    /// sustain with NO ignition ramp, even with animations off; the
    /// release's note-off level is the sustain's 1). Cleared by a
    /// real post-mount ignition; held through the release.
    glow_mount_sustain: bool,
    /// The last synced `unread` memo (the dissolve stamps only on the
    /// falling edge).
    unread_live: bool,
    /// G-503③ — the last synced `complete` flag (`setComplete`'s
    /// arms fire on the VALUE CHANGE only, tab-pulse.tsx:362-363).
    complete_live: bool,
    /// G-479 — false until the first sync: the ref's per-tab
    /// components CONSTRUCT with the current status (no mount flash —
    /// `createNumberIgnition` seeds `wasRunning`/`promptPulse`, the
    /// `PulseState` constructor does not fire the edge flash), so the
    /// first sync records state without firing the edges.
    armed: bool,
}

impl TabPulseTrack {
    /// Stamp any status transition at `now`. G-457: a re-ON while a
    /// release fade is still armed (`idle_since` set) RETRIGGERS the
    /// note — `active_since` re-stamps and the idle arm clears.
    /// Without this, an OFF/ON pair landing inside one clock step
    /// (the spinner-pin's 80 ms grid coalesces fast transitions —
    /// the store's busy flag flickers false between two mounts of
    /// one render pass) left `idle_since == active_since` and
    /// `sweep_clock` clamped at the constant fade tail: the sweep
    /// band froze while the spinner glyph marched on (the
    /// busy-pulse pressure regression).
    ///
    /// G-479 (va20): `prompt_pulses` is the tab's counter (the flash
    /// restarts at [`PROMPT_FLASH_SCALE`] on a change and the number
    /// ignition retriggers), `unread` is the indicator's memo
    /// (`status.unread && !pending`), `pending` = busy or attention
    /// (a pending indicator jumps the dissolve to 0 instead of
    /// animating).
    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    pub fn sync(
        &mut self,
        runs: bool,
        glow: bool,
        complete: bool,
        prompt_pulses: u64,
        unread: bool,
        pending: bool,
        now: u64,
    ) {
        // G-503③ — `was_runs` is the CURRENT busy state: the sweep
        // attacking/sustaining (`active_since` set AND the release
        // not armed). The old `active_since.is_some()` alone misread
        // every post-idle re-sync as a flip (active_since stays set
        // through the release by design) — each one re-stamped
        // `idle_since` to now, re-arming the completion pending and
        // restarting the release fade forever (the ref's `setActive`
        // no-ops while the value is unchanged, tab-pulse.tsx:338-339).
        let was_runs = self.active_since.is_some() && self.idle_since.is_none();
        // G-503② — the glow epoch's origin: a POST-mount ignition
        // ramps (clears the mount-sustain); the mount sync itself
        // sets the flag right after (below).
        let first_sync = !self.armed;
        if runs != was_runs || (runs && self.idle_since.is_some()) {
            if runs {
                self.active_since = Some(now);
                self.idle_since = None;
                // G-503③ — `setActive(true)` (tab-pulse.tsx:342-346):
                // the completion pulse stops and the pending clears.
                self.completion_since = None;
                self.completion_pending = false;
            } else {
                self.idle_since = Some(now);
                // G-503③ — `setActive(false)` arms the pending
                // (tab-pulse.tsx:347-350): the one-shot can only fire
                // through this gate, never off a bare complete stamp.
                self.completion_pending = true;
            }
        }
        if glow != self.glow_since.is_some() {
            if glow {
                self.glow_since = Some(now);
                self.glow_off_since = None;
                self.glow_mount_sustain = false;
            } else {
                self.glow_off_since = Some(now);
            }
        }
        // G-503② — mount-with-glow: the envelope CONSTRUCTS in
        // sustain (tab-pulse.tsx:274-279) — no ignition ramp; the
        // release's note-off level is the sustain's 1.
        if first_sync && glow {
            self.glow_mount_sustain = true;
        }
        // G-503③ — the completion gate: `setComplete`'s arms
        // (tab-pulse.tsx:362-374) + advance's pending observer
        // (:397-403). The stop/clear runs only on the complete
        // TRUE→FALSE transition (the ref's setter no-ops while the
        // value is unchanged — a mid-busy `false` sync must NOT clear
        // a pending the idle flip just armed); complete with the
        // pending live fires the one-shot. The pending's life ends
        // with the run release (advance's `releaseProgress ===
        // undefined` arm — `RUN_FADE_OUT` after the idle flip); the
        // boundary frame's order puts the fire FIRST (advance checks
        // complete before the release observer), so `<=` fires at
        // exactly the edge.
        if !complete && self.complete_live {
            self.completion_since = None;
            self.completion_pending = false;
        }
        if complete && self.completion_pending {
            let pending_live =
                self.idle_since.is_some_and(|idle| elapsed(now, idle) <= RUN_FADE_OUT);
            if pending_live {
                self.completion_since = Some(now);
            }
            self.completion_pending = false;
        }
        self.complete_live = complete;
        // G-479 — the edge flash: a runs flip STARTS it at scale 1
        // (`Envelope.start()` re-fires on every idle-state flip once
        // the previous flash decayed — G-503①: only a RUNNING clock
        // no-ops, tab-pulse.tsx:136-140; the port's old
        // `flash_since.is_none()` gate left it armed ONCE and never
        // again); a pulse++ RESTARTS it at scale 2. Either edge also
        // retriggers the number ignition.
        // The FIRST sync only records state (the ref constructs its
        // per-tab components with the live status — no mount flash).
        // A re-ON while the release fade is armed is a runs flip too
        // (the ref's active flag toggled false→true; the port's
        // `active_since` stays set through the release — the G-457
        // retrigger clause).
        let runs_flipped = self.armed && (runs != was_runs || (runs && self.idle_since.is_some()));
        let pulses_changed = self.armed && prompt_pulses != self.prompt_pulses;
        if runs_flipped || pulses_changed {
            self.ignition_since = Some(now);
        }
        if pulses_changed {
            self.flash_since = Some((now, PROMPT_FLASH_SCALE));
        } else if runs_flipped && !self.flash_running(now) {
            self.flash_since = Some((now, 1.0));
        }
        // G-479 — the indicator dissolve: unread jumps to 1, pending
        // jumps to 0, and the falling edge (unread → false, not
        // pending) animates 1 → 0 over 180 ms.
        if unread || pending {
            // unread jumps to 1; pending (busy/attention) jumps to 0.
            self.dissolve_since = None;
        } else if self.unread_live {
            self.dissolve_since = Some(now);
        }
        self.unread_live = unread;
        self.prompt_pulses = prompt_pulses;
        self.armed = true;
    }

    /// G-479 — the edge flash's opacity: the 800 ms attackDecay
    /// one-shot × [`EDGE_FLASH_OPACITY`], scaled by 1 (a runs flip)
    /// or [`PROMPT_FLASH_SCALE`] (a pulse++). 0 once it decays away.
    #[must_use]
    pub fn edge_flash(&self, now: u64) -> f64 {
        let Some((since, scale)) = self.flash_since else { return 0.0 };
        let progress = clamp01(elapsed(now, since) as f64 / EDGE_FLASH_DURATION as f64);
        EDGE_FLASH_OPACITY * scale * attack_decay(progress, EDGE_FLASH_ATTACK, 1.0, 0.0)
    }

    /// G-503① — `Envelope.start()`'s no-op gate (tab-pulse.tsx:136-
    /// 140): only a RUNNING clock (decayed strictly before the 800 ms
    /// duration) blocks a re-arm; after the one-shot decays the next
    /// runs flip re-fires it.
    fn flash_running(&self, now: u64) -> bool {
        self.flash_since.is_some_and(|(since, _)| elapsed(now, since) < EDGE_FLASH_DURATION)
    }

    /// G-479 — the number ignition's level: the 0.85 jump tweened
    /// down to 0 over 0.7 s with the ref `tween()`'s DEFAULT
    /// smoothstep ease (G-501: `createNumberIgnition`'s
    /// `tween({duration: 0.7})` passes no ease — animation.ts:50 —
    /// so the value is `0.85·(1 − smoothstep(progress))`, not
    /// linear; a fresh trigger re-stamps, so the tween restarts from
    /// 0.85).
    #[must_use]
    pub fn ignition_level(&self, now: u64) -> f64 {
        let Some(since) = self.ignition_since else { return 0.0 };
        let progress = clamp01(elapsed(now, since) as f64 / IGNITION_DURATION_MS as f64);
        IGNITION_JUMP * (1.0 - smoothstep(progress))
    }

    /// G-479 — the unread marker's dissolve opacity: 1 → 0 over
    /// 180 ms from the stamp with the ref `tween()`'s DEFAULT
    /// smoothstep ease (G-501: `TabIndicator`'s
    /// `tween({duration: 0.18})` — session-tabs.tsx:154-157 — passes
    /// no ease, animation.ts:50), 0 with no stamp. The mid-boost
    /// (`1 − |opacity − 0.8| / 0.2`) is the consumer's to derive.
    #[must_use]
    pub fn dissolve_opacity(&self, now: u64) -> f64 {
        let Some(since) = self.dissolve_since else { return 0.0 };
        let progress = clamp01(elapsed(now, since) as f64 / DISSOLVE_DURATION_MS as f64);
        1.0 - smoothstep(progress)
    }

    /// G-479 — the running sweep's level under the number cell (the
    /// `TabPulse` `onLevel` report: the max of `intensityAt(1, …)`
    /// over both fronts × running; the ref quantizes to 1/32 to damp
    /// signal noise —
    /// the deterministic read keeps the raw value).
    #[must_use]
    pub fn sweep_level_at_number(&self, width: usize, now: u64) -> f64 {
        let running = self.running(now);
        if running <= 0.0 {
            return 0.0;
        }
        let (front_a, front_b) = self.fronts(width, now);
        intensity_at(1, front_a, RUN_HEAD, RUN_TAIL)
            .max(intensity_at(1, front_b, RUN_HEAD, RUN_TAIL))
            * running
    }

    /// The run envelope's level: attack over [`RUN_ATTACK`] to sustain
    /// 1 while busy, then the [`RUN_FADE_OUT`] note-off from the
    /// level held at note-off (`GatedEnvelope.release`).
    #[must_use]
    pub fn running(&self, now: u64) -> f64 {
        let Some(active) = self.active_since else { return 0.0 };
        let Some(idle) = self.idle_since else {
            return smootherstep(clamp01(elapsed(now, active) as f64 / RUN_ATTACK as f64));
        };
        let note_off = smootherstep(clamp01(elapsed(idle, active) as f64 / RUN_ATTACK as f64));
        note_off * fade_out(clamp01(elapsed(now, idle) as f64 / RUN_FADE_OUT as f64))
    }

    /// The sweep clock: coasts from note-on THROUGH the release fade,
    /// stopped once the release completes (`advance`'s `!idle` gate).
    #[must_use]
    pub fn sweep_clock(&self, now: u64) -> u64 {
        let Some(active) = self.active_since else { return 0 };
        let stop = self.idle_since.map_or(u64::MAX, |idle| idle.saturating_add(RUN_FADE_OUT));
        now.min(stop).saturating_sub(active)
    }

    /// The sweep fronts at `now` (see [`fronts_at`]).
    #[must_use]
    pub fn fronts(&self, width: usize, now: u64) -> (f64, f64) {
        fronts_at(self.sweep_clock(now), width)
    }

    /// The glow envelope: ignition past 1 then sustain; on release,
    /// the note-off residue drains over the leading 200 ms (the
    /// release stage's drain shape). G-503②: an epoch that began at
    /// MOUNT holds sustain directly (no ignition ramp) and its
    /// release's note-off level is the sustain's 1.
    #[must_use]
    pub fn glow_level(&self, now: u64) -> f64 {
        if let Some(off) = self.glow_off_since {
            let note_off = if self.glow_mount_sustain {
                1.0
            } else {
                self.glow_since.map_or(1.0, |on| {
                    glow_ignition_level(clamp01(
                        elapsed(off, on) as f64 / GLOW_IGNITION_DURATION as f64,
                    ))
                })
            };
            let drain_fraction = GLOW_FADE_OUT_MS as f64 / GLOW_RELEASE_DURATION as f64;
            let progress = elapsed(now, off) as f64 / GLOW_RELEASE_DURATION as f64;
            return note_off * fade_out(clamp01(progress / drain_fraction));
        }
        if self.glow_mount_sustain {
            return 1.0;
        }
        let Some(on) = self.glow_since else { return 0.0 };
        glow_ignition_level(clamp01(elapsed(now, on) as f64 / GLOW_IGNITION_DURATION as f64))
    }

    /// The completion flash (0.18 amplitude, 1200 ms attackDecay).
    #[must_use]
    pub fn completion(&self, now: u64) -> f64 {
        self.completion_since.map_or(0.0, |at| {
            COMPLETION_OPACITY
                * attack_decay(
                    clamp01(elapsed(now, at) as f64 / COMPLETION_DURATION as f64),
                    COMPLETION_ATTACK,
                    1.0,
                    0.0,
                )
        })
    }

    /// The diffusing swell riding a released glow (above its resting
    /// level, decaying to nothing over the 900 ms release). G-503②:
    /// a mount-sustained glow releases from level 1 (the
    /// `releaseScale` captured at note-off is the sustain's 1).
    #[must_use]
    pub fn glow_release_swell(&self, now: u64) -> f64 {
        let Some(off) = self.glow_off_since else { return 0.0 };
        let progress = clamp01(elapsed(now, off) as f64 / GLOW_RELEASE_DURATION as f64);
        let note_off = if self.glow_mount_sustain {
            1.0
        } else {
            self.glow_since.map_or(1.0, |on| {
                glow_ignition_level(clamp01(
                    elapsed(off, on) as f64 / GLOW_IGNITION_DURATION as f64,
                ))
            })
        };
        glow_release_swell_shape(progress) * note_off.max(1.0)
    }

    /// How far the released glow has diffused (the tail spreads
    /// across the full width as it thins away).
    #[must_use]
    pub fn glow_release_spread(&self, tail: f64, width: usize, now: u64) -> f64 {
        let Some(off) = self.glow_off_since else { return 0.0 };
        glow_release_spread(
            clamp01(elapsed(now, off) as f64 / GLOW_RELEASE_DURATION as f64),
            tail,
            width,
        )
    }
}

/// `PulseState.fronts(width)` (tab-pulse.tsx:404-417): the two sweep
/// fronts — the primary pass coasts left-to-right over [`RUN_DURATION`],
/// a second pass trailing half a cycle behind.
#[must_use]
pub fn fronts_at(sweep_clock: u64, width: usize) -> (f64, f64) {
    let cycles = sweep_clock as f64 / RUN_DURATION as f64;
    let progress = cycles.fract();
    let start = -RUN_HEAD;
    let end = (width.saturating_sub(1)).max(1) as f64 + RUN_TAIL;
    let span = end - start;
    let second_progress = if cycles < 0.5 { 0.0 } else { (cycles + 0.5).fract() };
    (start + coast(progress) * span, start + coast(second_progress) * span)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCENT: Rgb = (157, 124, 216); // hue.accent[200] (v2-dark)
    const BASE: Rgb = (10, 10, 10); // background.base
    const RAISED: Rgb = (20, 20, 20); // background.raised.base
    const MUTED: Rgb = (128, 128, 128); // text.muted

    #[test]
    fn smootherstep_matches_the_reference_curve() {
        assert!((smootherstep(0.0) - 0.0).abs() < 1e-12);
        assert!((smootherstep(0.5) - 0.5).abs() < 1e-12);
        assert!((smootherstep(1.0) - 1.0).abs() < 1e-12);
        // Hand-derived: sm(0.8) = 0.8³·(6·0.64 − 12 + 10) = 0.512 · 1.84.
        assert!((smootherstep(0.8) - 0.942_080).abs() < 1e-9);
        assert!((smootherstep(0.6) - 0.682_560).abs() < 1e-9);
        // Clamped outside [0, 1].
        assert_eq!(smootherstep(-1.0), 0.0);
        assert_eq!(smootherstep(2.0), 1.0);
    }

    #[test]
    fn intensity_falls_to_zero_behind_the_tail() {
        // tail = 12: full at 0-1, first decay cell is 2, zero from 13.
        assert!((glow_intensity_at(0, 12.0) - 1.0).abs() < 1e-12);
        assert!((glow_intensity_at(1, 12.0) - 1.0).abs() < 1e-12);
        assert!((glow_intensity_at(2, 12.0) - 0.994_912).abs() < 1e-6);
        assert!((glow_intensity_at(12, 12.0) - 0.005_088).abs() < 1e-6);
        assert_eq!(glow_intensity_at(13, 12.0), 0.0);
        // Width clamping: a 10-wide strip uses tail 8 (min(12, max(1, 8))).
        assert!((unread_glow_intensity(8, 10, GLOW_TAIL) - 0.016_052).abs() < 1e-6);
        assert_eq!(unread_glow_intensity(9, 10, GLOW_TAIL), 0.0);
        // The separator's outer pulse clamps 5 → 5 over a 10-wide strip.
        assert!((unread_glow_intensity(5, 10, 5.0) - 0.057_920).abs() < 1e-6);
        assert_eq!(unread_glow_intensity(6, 10, 5.0), 0.0);
    }

    /// The horizontal-bar golden (`session-tabs-horizontal.w80.ansi`
    /// row 0): the unread tab's pulse background, cell for cell —
    /// `tint(base, accent, glowLevel=1)` as the glow color (the bar's
    /// `glowColor` memo passes `glowLevel()` itself as the alpha), tail
    /// 12 over the 32-wide tab.
    #[test]
    fn resting_glow_matches_the_bar_golden_cells() {
        let glow = tint(BASE, ACCENT, 1.0);
        assert_eq!(glow, ACCENT, "alpha 1 keeps the accent verbatim");
        let expected = [
            (34, 28, 43),
            (34, 28, 43),
            (33, 28, 43),
            (33, 28, 42),
            (31, 26, 40),
            (29, 24, 36),
            (25, 22, 32),
            (22, 19, 26),
            (18, 16, 21),
            (15, 14, 17),
            (12, 12, 13),
            (11, 11, 11),
            (10, 10, 10),
        ];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                &resting_glow_bg(BASE, ACCENT, index, 32, GLOW_TAIL, 1.0),
                want,
                "bar cell {index}"
            );
        }
        // The tail is flat base from cell 13 on.
        assert_eq!(resting_glow_bg(BASE, ACCENT, 20, 32, GLOW_TAIL, 1.0), BASE);
    }

    /// The vertical-rail golden (`session-tabs-vertical.w80.ansi` rows
    /// 1-3): title pulse `tint(raised, accent, 0.45)` tail 8 over the
    /// 10-wide indicator strip, detail pulse `tint(raised, accent,
    /// 0.25)` tail 8, separator outer `tint(raised, accent, 0.1)` tail
    /// 5.
    #[test]
    fn resting_glow_matches_the_rail_golden_cells() {
        let title_glow = tint(RAISED, ACCENT, 0.45);
        assert_eq!(title_glow, (82, 67, 108));
        let expected = [
            (30, 28, 34),
            (30, 28, 34),
            (30, 27, 34),
            (29, 27, 33),
            (27, 25, 30),
            (25, 24, 27),
            (23, 22, 24),
            (21, 21, 21),
            (20, 20, 20),
            (20, 20, 20),
        ];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                &resting_glow_bg(RAISED, title_glow, index, 10, GLOW_TAIL, 1.0),
                want,
                "rail title cell {index}"
            );
        }
        let detail_glow = tint(RAISED, ACCENT, 0.25);
        assert_eq!(detail_glow, (54, 46, 69));
        assert_eq!(
            resting_glow_bg(RAISED, detail_glow, 0, 10, 10.0, 1.0),
            (25, 24, 28),
            "detail cell 0 (glowTail prop 10 clamps to 8)"
        );
        let separator_outer = tint(RAISED, ACCENT, 0.1);
        assert_eq!(separator_outer, (34, 30, 40));
        let expected =
            [(22, 22, 23), (22, 22, 23), (22, 22, 23), (22, 21, 22), (21, 21, 21), (20, 20, 20)];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                &resting_glow_bg(RAISED, separator_outer, index, 10, 5.0, 1.0),
                want,
                "separator cell {index}"
            );
        }
    }

    /// Title characters riding the glow: the bar golden's "First" run
    /// (`glowTextColor(muted, accent, 1 + numberWidth + i, 32)`) and
    /// the rail's (`(muted, tint(raised, accent, 0.45), …, 42)`).
    #[test]
    fn glow_text_color_matches_the_golden_characters() {
        let expected =
            [(131, 128, 138), (131, 128, 137), (131, 128, 136), (130, 128, 135), (130, 128, 133)];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(
                &glow_text_color(MUTED, ACCENT, 1 + 2 + i, 32, 1.0),
                want,
                "bar title char {i}"
            );
        }
        let rail_glow = tint(RAISED, ACCENT, 0.45);
        let expected =
            [(123, 121, 126), (123, 121, 126), (124, 122, 126), (124, 123, 126), (125, 124, 127)];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(
                &glow_text_color(MUTED, rail_glow, 1 + 2 + i, 42, 1.0),
                want,
                "rail title char {i}"
            );
        }
        // Beyond the tail the text stays neutral.
        assert_eq!(glow_text_color(MUTED, ACCENT, 30, 32, 1.0), MUTED);
    }

    #[test]
    fn tint_rounds_each_channel_once() {
        assert_eq!(tint(BASE, ACCENT, 0.0), BASE);
        assert_eq!(tint(BASE, ACCENT, 1.0), ACCENT);
        // 20 + (157 − 20) · 0.45 = 81.65 → 82 (fromInts rounding).
        assert_eq!(tint(RAISED, ACCENT, 0.45).0, 82);
    }

    // -- lane TAIL: the animated phases ----------------------------------

    /// `coast` — the reference's own anchors (the quadratic feet).
    #[test]
    fn coast_matches_the_reference_anchors() {
        assert_eq!(coast(0.0), 0.0);
        // v=0.2: (0.2²)/(2·0.2·0.8) = 0.04/0.32 = 0.125
        assert!((coast(0.2) - 0.125).abs() < 1e-12);
        assert!((coast(0.5) - 0.5).abs() < 1e-12);
        assert_eq!(coast(1.0), 1.0);
        // v=0.8: 1 − (0.2²)/0.32 = 0.875
        assert!((coast(0.8) - 0.875).abs() < 1e-12);
    }

    /// `intensityAt` — the sweep band: full at the front, a 4-cell
    /// head ahead, an 18-cell tail behind.
    #[test]
    fn sweep_band_profile_matches_the_reference() {
        assert!((intensity_at(10, 10.0, RUN_HEAD, RUN_TAIL) - 1.0).abs() < 1e-12);
        // 4 cells AHEAD of the front (index > front): head branch → 0.
        assert_eq!(intensity_at(14, 10.0, RUN_HEAD, RUN_TAIL), 0.0);
        // 2 cells ahead: 1 − 2/4 = 0.5 → smootherstep(0.5) = 0.5.
        assert!((intensity_at(12, 10.0, RUN_HEAD, RUN_TAIL) - 0.5).abs() < 1e-12);
        assert_eq!(intensity_at(28, 10.0, RUN_HEAD, RUN_TAIL), 0.0);
    }

    /// The run envelope: attack over 450 ms to sustain 1, hold while
    /// busy, release over 500 ms after — all from the stamps.
    #[test]
    fn run_envelope_attacks_sustains_and_releases() {
        let mut track = TabPulseTrack::default();
        assert_eq!(track.running(0), 0.0, "silent before the first busy");
        track.sync(true, false, false, 0, false, false, 0);
        assert_eq!(track.running(0), 0.0, "attack origin at the stamp");
        assert!((track.running(225) - 0.5).abs() < 1e-9, "attack midpoint");
        assert!((track.running(450) - 1.0).abs() < 1e-9, "sustain after the attack");
        assert!((track.running(5_000) - 1.0).abs() < 1e-9, "sustains while busy");
        track.sync(false, false, false, 0, false, false, 5_000);
        assert!((track.running(5_250) - 0.5).abs() < 1e-9, "release midpoint");
        assert_eq!(track.running(5_500), 0.0, "silent after the release");
        // The sweep clock stops when the release completes.
        assert_eq!(track.sweep_clock(9_000), 5_500, "sweep clamped at idle+fade");
    }

    /// The sweep front crosses cells left-to-right (`fronts_at`): the
    /// front starts at −4, coasts across `width − 1 + 18`, and a
    /// second pass joins half a cycle in.
    #[test]
    fn sweep_fronts_cross_left_to_right() {
        let width = 32usize;
        let (f0, f1) = fronts_at(0, width);
        assert_eq!(f0, -4.0);
        assert_eq!(f1, -4.0, "no second pass in the first half-cycle");
        let (f0, f1) = fronts_at(1_400, width);
        // coast(0.5) = 0.5 → f0 = −4 + 0.5·(31 + 18 + 4) = 22.5
        assert!((f0 - 22.5).abs() < 1e-9, "mid-cycle front: {f0}");
        assert!((f1 - -4.0).abs() < 1e-9, "second pass starts at mid-cycle");
        // A mid-strip cell sees the band pass: rises to ~1 then falls.
        let cell = 20usize;
        let mut seen_peak = false;
        let mut values = Vec::new();
        for step in 0..=28 {
            let (f0, f1) = fronts_at(step * 100, width);
            values.push(
                intensity_at(cell, f0, RUN_HEAD, RUN_TAIL)
                    .max(intensity_at(cell, f1, RUN_HEAD, RUN_TAIL)),
            );
            if values.last().is_some_and(|v| *v > 0.99) {
                seen_peak = true;
            }
        }
        assert!(seen_peak, "the band crosses cell {cell}: {values:?}");
    }

    /// The glow envelope: ignition peaks at 1.5 then settles to 1;
    /// release drains over the leading 200 ms with a swelling spread.
    #[test]
    fn glow_envelope_ignites_and_releases() {
        // G-503② re-pin: a glow present at the FIRST sync (mount)
        // holds sustain directly (the GatedEnvelope constructor's
        // initial "sustain", tab-pulse.tsx:274-279) — the ignition
        // ramp belongs to a POST-mount ignition only.
        let mut mounted = TabPulseTrack::default();
        mounted.sync(false, true, false, 0, false, false, 0);
        assert_eq!(mounted.glow_level(0), 1.0, "mount-with-glow sustains at 1");
        assert_eq!(mounted.glow_level(180), 1.0, "no ignition ramp at mount");
        assert_eq!(mounted.glow_level(9_000), 1.0, "sustain holds");
        // A post-mount ignition ramps: peak 1.5 at 30% of 600 ms.
        let mut track = TabPulseTrack::default();
        track.sync(false, false, false, 0, false, false, 0);
        track.sync(false, true, false, 0, false, false, 100);
        assert_eq!(track.glow_level(100), 0.0, "origin at the ignition stamp");
        assert!((track.glow_level(280) - 1.5).abs() < 1e-9, "ignition peak at 30% of 600 ms");
        assert!((track.glow_level(701) - 1.0).abs() < 1e-6, "settled to sustain");
        track.sync(false, false, false, 0, false, false, 1_000);
        assert_eq!(track.glow_level(1_000), 1.0, "note-off held at the drain's origin");
        assert!((track.glow_level(1_100) - 0.5).abs() < 1e-9, "halfway through the drain");
        assert!(track.glow_release_swell(1_100) > 1.0, "the swell rides above rest");
        assert!(track.glow_release_spread(12.0, 32, 1_450) > 12.0, "the tail spreads");
        // The mount-sustained epoch releases from level 1 (its
        // note-off is the sustain's 1, not a ramped level).
        mounted.sync(false, false, false, 0, false, false, 2_000);
        assert_eq!(mounted.glow_level(2_000), 1.0, "the mount epoch's note-off is 1");
        assert!(
            (mounted.glow_level(2_100) - 0.5).abs() < 1e-9,
            "the mount epoch drains from the sustain level"
        );
    }

    /// The completion flash: the activity unread landing after idle
    /// fires the 0.18 wash (1200 ms attackDecay).
    #[test]
    fn completion_flash_fires_when_the_turn_resolves() {
        let mut track = TabPulseTrack::default();
        track.sync(true, false, false, 0, false, false, 0);
        track.sync(false, false, false, 0, false, false, 4_000);
        assert_eq!(track.completion(4_000), 0.0, "idle without a completion yet");
        track.sync(false, false, true, 0, false, false, 4_100);
        assert!(track.completion(4_244) > 0.17, "near the 0.18 peak");
        assert_eq!(track.completion(5_400), 0.0, "decays away");
    }

    /// G-503③ — the completion pulse arms ONLY through the busy→idle
    /// pending transition (tab-pulse.tsx:342-374, 397-403): a
    /// complete stamp at mount or mid-busy fires NOTHING; complete
    /// arriving after the run release's 500 ms window never fires;
    /// the pending+complete landing together at the flip fires at the
    /// flip; a re-busy clears the pending; complete falling stops a
    /// running pulse.
    #[test]
    fn completion_pulse_arms_only_through_the_idle_pending_gate() {
        // A complete stamp at mount (idle, never busy): no flip, no
        // pending — never fires.
        let mut mounted = TabPulseTrack::default();
        mounted.sync(false, false, true, 0, false, false, 0);
        assert_eq!(mounted.completion(5_000), 0.0, "mount complete: no pulse");
        // A complete stamp MID-BUSY: no pending — the old port fired
        // here (tab_pulse.rs:396-398 pre-G-503); the ref fires only
        // at the busy→idle transition.
        let mut track = TabPulseTrack::default();
        track.sync(true, false, false, 0, false, false, 0);
        track.sync(true, false, true, 0, false, false, 100);
        assert_eq!(track.completion(500), 0.0, "mid-busy complete: no pulse yet");
        // The busy→idle transition (complete still true) fires AT the
        // flip — the pending armed by setActive(false) and observed
        // complete on the same frame (advance :397-403).
        track.sync(false, false, true, 0, false, false, 1_000);
        assert!(track.completion(1_144) > 0.17, "fires at the idle flip");
        assert_eq!(track.completion(2_400), 0.0, "decays away");
        // A re-busy clears the pending (setActive(true): stop +
        // pending=false) — the flip to idle re-arms it and complete
        // fires again.
        track.sync(true, false, true, 0, false, false, 3_000);
        assert_eq!(track.completion(3_100), 0.0, "re-busy stopped the pulse");
        track.sync(false, false, true, 0, false, false, 4_000);
        assert!(track.completion(4_144) > 0.17, "re-armed at the next idle flip");
        // complete arriving AFTER the run release's 500 ms window:
        // the pending expired — no pulse.
        let mut late = TabPulseTrack::default();
        late.sync(true, false, false, 0, false, false, 0);
        late.sync(false, false, false, 0, false, false, 1_000);
        late.sync(false, false, true, 0, false, false, 2_000);
        assert_eq!(late.completion(2_100), 0.0, "complete past the release window: no pulse");
        // complete falling stops a running pulse mid-flight.
        let mut stopped = TabPulseTrack::default();
        stopped.sync(true, false, false, 0, false, false, 0);
        stopped.sync(false, false, true, 0, false, false, 1_000);
        assert!(stopped.completion(1_100) > 0.0, "running after the flip");
        stopped.sync(false, false, false, 0, false, false, 1_200);
        assert_eq!(stopped.completion(1_300), 0.0, "complete=false stopped it");
    }

    /// `blend_tab_pulse_color` — the staged pull.
    #[test]
    fn staged_blend_pulls_toward_each_stage() {
        let bg = (10, 10, 10);
        let glow = (157, 124, 216);
        let run = (100, 100, 100);
        let only_glow = blend_tab_pulse_color(bg, glow, run, run, run, 0.5, 0.0, 0.0, 0.0);
        assert_eq!(only_glow, tint(bg, glow, 0.5));
        let both = blend_tab_pulse_color(bg, glow, run, run, run, 0.5, 0.5, 0.0, 0.0);
        assert_eq!(both, tint(only_glow, run, 0.5));
        assert_eq!(blend_tab_pulse_color(bg, glow, run, run, run, 0.0, 0.0, 0.0, 0.0), bg);
    }

    // -- G-479: the three unported envelope consumers (va20) ------------

    /// The edge flash (tab-pulse.tsx:42-45 + `setActive`/
    /// `setPromptPulse`): a runs flip starts the 800 ms attackDecay at
    /// scale 1; a `pulse++` RESTARTS it at `PROMPT_FLASH_SCALE` = 2; a
    /// flip mid-flash does not re-stamp (`start()` no-ops); the first
    /// sync (mount) records state without firing. The attackDecay
    /// rises over the leading 10% (peak at +80 ms) then decays to 0
    /// at 800 ms.
    #[test]
    fn edge_flash_starts_on_flips_and_restarts_on_pulses() {
        let mut track = TabPulseTrack::default();
        // Mount while busy: NO flash (the ref constructs with the live
        // status — the run envelope attacks, the flash does not).
        track.sync(true, false, false, 0, false, false, 0);
        assert_eq!(track.edge_flash(80), 0.0, "no mount flash");
        // A pulse++ restarts at scale 2: peak 0.2 opacity at +80 ms.
        track.sync(true, false, false, 1, false, false, 100);
        assert_eq!(track.edge_flash(100), 0.0, "origin at the stamp");
        assert!((track.edge_flash(180) - 0.2).abs() < 1e-12, "scale-2 peak (0.1 × 2)");
        assert!((track.edge_flash(500) - 0.2 * (1.0 - smootherstep(4.0 / 9.0))).abs() < 1e-12);
        assert_eq!(track.edge_flash(100 + EDGE_FLASH_DURATION), 0.0, "decays away");
        // A runs flip mid-flash does not re-stamp (scale stays 2).
        track.sync(false, false, false, 1, false, false, 200);
        assert!(
            (track.edge_flash(300) - 0.2 * (1.0 - smootherstep((0.25 - 0.1) / 0.9))).abs() < 1e-12,
            "the running flash is not restarted by the flip"
        );
        // A fresh flip with no flash running starts scale 1 (0.1 peak).
        let mut idle = TabPulseTrack::default();
        idle.sync(false, false, false, 0, false, false, 0);
        idle.sync(true, false, false, 0, false, false, 1_000);
        assert_eq!(idle.edge_flash(1_000), 0.0, "origin at the stamp");
        assert!((idle.edge_flash(1_080) - 0.1).abs() < 1e-12, "scale-1 peak at a runs flip");
        // G-503① — the flash RE-ARMS on every idle-state flip once the
        // previous one decayed (`Envelope.start()` re-fires — only a
        // RUNNING clock no-ops, tab-pulse.tsx:136-140): the second and
        // third flips each start a fresh scale-1 flash. (The old
        // `flash_since.is_none()` gate armed it exactly once and runs
        // flips after the first NEVER flashed.)
        idle.sync(false, false, false, 0, false, false, 2_000);
        assert!((idle.edge_flash(2_080) - 0.1).abs() < 1e-12, "the second flip re-arms the flash");
        idle.sync(true, false, false, 0, false, false, 3_000);
        assert!((idle.edge_flash(3_080) - 0.1).abs() < 1e-12, "the third flip re-arms it too");
    }

    /// The number ignition (`createNumberIgnition`,
    /// session-tabs.tsx:215-230): 0.85 jump → tween to 0 over 700 ms
    /// with the ref tween's DEFAULT smoothstep ease (G-501 re-pin:
    /// the old test pinned the port's linear fall — the ref's
    /// `tween({duration: 0.7})` passes no ease, animation.ts:50, so
    /// the value is `0.85·(1 − smoothstep(p))`), retriggered by a
    /// runs flip OR a pulse++, never at mount.
    #[test]
    fn number_ignition_jumps_and_falls_smoothstep() {
        let mut track = TabPulseTrack::default();
        track.sync(true, false, false, 0, false, false, 0);
        assert_eq!(track.ignition_level(0), 0.0, "no mount ignition");
        track.sync(true, false, false, 1, false, false, 200);
        assert_eq!(track.ignition_level(200), IGNITION_JUMP, "jump at the trigger");
        assert!((track.ignition_level(200 + 350) - IGNITION_JUMP / 2.0).abs() < 1e-9, "midpoint");
        assert_eq!(track.ignition_level(200 + IGNITION_DURATION_MS), 0.0, "faded out");
        // The smoothstep shape: a quarter in, the ease has barely
        // moved (3t²−2t³ at 0.25 ≈ 0.156 — vs linear 0.25).
        assert!(
            (track.ignition_level(200 + 175) - IGNITION_JUMP * (1.0 - smoothstep(0.25))).abs()
                < 1e-9
        );
        // The runs flip retriggers too.
        track.sync(false, false, false, 1, false, false, 1_000);
        assert_eq!(track.ignition_level(1_000), IGNITION_JUMP, "retriggered by the runs flip");
        assert!(
            (track.ignition_level(1_000 + 50) - IGNITION_JUMP * (1.0 - smoothstep(50.0 / 700.0)))
                .abs()
                < 1e-9
        );
    }

    /// The indicator dissolve (`TabIndicator`'s fade,
    /// session-tabs.tsx:161-172): unread jumps to 1 (no animation),
    /// the falling edge animates 1 → 0 over 180 ms (G-501: with the
    /// ref tween's DEFAULT smoothstep ease — the midpoint is 0.5 on
    /// every symmetric curve, the shoulders differ from linear), and
    /// pending (busy or attention) clears instantly.
    #[test]
    fn indicator_dissolve_falls_over_180ms_and_skips_pending() {
        let mut track = TabPulseTrack::default();
        track.sync(false, false, false, 0, false, false, 0);
        track.sync(false, false, false, 0, true, false, 100);
        assert_eq!(track.dissolve_since, None, "unread jumps to 1 — no dissolve");
        // Busy (pending) while unread set: the memo falls, the fade
        // jumps to 0.
        track.sync(false, false, false, 0, false, true, 200);
        assert_eq!(track.dissolve_since, None, "pending clears instantly");
        // Unread returns, then clears while NOT pending: the dissolve.
        track.sync(false, false, false, 0, true, false, 300);
        track.sync(false, false, false, 0, false, false, 400);
        assert_eq!(track.dissolve_since, Some(400), "the falling edge stamps");
        assert_eq!(track.dissolve_opacity(400), 1.0, "starts at full");
        assert!((track.dissolve_opacity(400 + 90) - 0.5).abs() < 1e-9, "smoothstep midpoint");
        // G-501 — the ease's shoulder: a quarter in the smoothstep
        // has fallen only ≈15.6% (linear would be 25%).
        assert!((track.dissolve_opacity(445) - (1.0 - smoothstep(0.25))).abs() < 1e-9);
        assert_eq!(track.dissolve_opacity(400 + DISSOLVE_DURATION_MS), 0.0, "dissolved");
        // The consumer's mid-boost peaks at opacity 0.8 (float-noise
        // tolerant at the shoulders).
        let boost = |opacity: f64| (1.0 - (opacity - 0.8).abs() / 0.2).max(0.0);
        assert!(boost(1.0) < 1e-12);
        assert_eq!(boost(0.8), 1.0);
        assert!(boost(0.6) < 1e-12);
    }

    /// `tabFlashIntensity` (tab-pulse.tsx:91-92) is the glow's own
    /// spatial falloff — the flash tail clamps exactly like
    /// [`unread_glow_intensity`].
    #[test]
    fn tab_flash_intensity_is_the_glow_falloff() {
        assert_eq!(tab_flash_intensity(0, FLASH_TAIL), 1.0);
        assert_eq!(tab_flash_intensity(1, FLASH_TAIL), 1.0);
        assert_eq!(tab_flash_intensity(9, FLASH_TAIL), glow_intensity_at(9, FLASH_TAIL));
        // The width clamp the renderers apply (min(tail, max(1, w−2))).
        let clamped = pulse_tail(FLASH_TAIL, 8);
        assert_eq!(tab_flash_intensity(7, clamped), glow_intensity_at(7, clamped));
    }
}
