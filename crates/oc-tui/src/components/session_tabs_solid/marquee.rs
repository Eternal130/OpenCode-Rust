//! The hover marquee (`util/marquee.ts` + `createTabMarquee`,
//! session-tabs.tsx:279-369) — lane TAIL item 1c: a long tab title
//! auto-scrolls while hovered.
//!
//! **Ownership (lane D3-6).** Pure compute for the tab strip: this
//! module writes no [`crate::render::Frame`] and holds no paint
//! state beyond the deterministic [`Marquee`] clock. It lives inside
//! the family whose surface it serves ([`super`], the tabs strip
//! tree); the strip's cells are painted by the shared opentui tree,
//! not here. D0b listed it as a "hand-drawn" tail file — that label
//! predates the solid migration: evidence (grep) shows a single live
//! consumer, so D3-6 absorbed it into the tabs family instead of
//! keeping a top-level component module.
//!
//! v2 drives the scroll with timers (`MARQUEE_DELAY` 600 ms, then +1
//! cell per `MARQUEE_INTERVAL` 80 ms, one full cycle then reset). The
//! port derives the offset deterministically from the injected store
//! clock: `offset(now) = 0` inside the delay window, then
//! `1 + (now − delay) / interval` until the cycle width is covered —
//! a frozen clock pins offset 0 (golden frames byte-stable).
//!
//! The text math (`marqueeTextParts`) is the reference verbatim: the
//! title plus a `" · "` separator loops, the visible window starts at
//! `offset % cycleWidth` graphemes and takes `width` cells, splitting
//! only on grapheme boundaries. (G-476②, va20: the loop cells were
//! built with `chars()` — multi-codepoint clusters (ZWJ families,
//! skin tones, flags, combining marks) could be cut at the window
//! seam; the cells are now whole grapheme clusters via
//! [`crate::wrap::flatten`], matching `Locale.graphemes`,
//! util/marquee.ts:30-33.) The one-pass scroll ends the way the ref
//! ends it (G-476①): when the next step would reach the cycle width
//! the interval stops, the offset resets to window 0, and the
//! leading fade tweens back out (`clear() + leading.animate({opacity:
//! 0}) + return 0`, session-tabs.tsx:296-302).

use crate::wrap::display_width;

/// The loop separator (`GAP`, marquee.ts:3).
pub const GAP: &str = " · ";
/// `MARQUEE_DELAY` — the still frame before the scroll starts (ms).
pub const MARQUEE_DELAY: u64 = 600;
/// `MARQUEE_INTERVAL` — one cell per tick (ms).
pub const MARQUEE_INTERVAL: u64 = 80;
/// The leading fade-in (`tween({ duration: 0.25 })`, session-tabs.tsx:283).
pub const LEADING_FADE_MS: u64 = 250;

/// `marqueeCycleWidth(value)` — the title + separator's cell width.
#[must_use]
pub fn marquee_cycle_width(value: &str) -> usize {
    display_width(&format!("{value}{GAP}"))
}

/// `marqueeOverflows(value, width)` — the hover-scroll gate.
#[must_use]
pub fn marquee_overflows(value: &str, width: usize) -> bool {
    display_width(value) > width
}

/// `marqueeTextParts(value, width, offset)` — the visible window as a
/// (text, leading-separator) run list. Offsets ≤ 0 (and fitting
/// titles) render the truncated head, no loop.
#[must_use]
pub fn marquee_text(value: &str, width: usize, offset: usize) -> String {
    marquee_text_parts(value, width, offset).join("")
}

/// The parts (joined with the separator trailing each chunk that ends
/// on the loop's `·`) — the painter only needs the joined string and
/// where the separator sits, so the port returns the joined text plus
/// the separator's cell index (None while the head shows).
#[must_use]
pub fn marquee_text_parts(value: &str, width: usize, offset: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    if display_width(value) <= width || offset == 0 {
        return vec![crate::wrap::take_width(value, width)];
    }
    // The loop: title graphemes, then the separator's (space · space)
    // — whole clusters (G-476②: `Locale.graphemes`, util/marquee.ts:30-33;
    // a ZWJ family or skin-toned emoji is ONE loop cell, never cut).
    let mut loop_cells: Vec<(String, usize)> = Vec::new();
    for atom in crate::wrap::flatten(value, crate::style::Style::new()) {
        loop_cells.push((atom.text, atom.width));
    }
    for atom in crate::wrap::flatten(GAP, crate::style::Style::new()) {
        loop_cells.push((atom.text, atom.width));
    }
    let cycle = loop_cells.iter().map(|(_, w)| *w).sum::<usize>().max(1);
    let cursor = offset % cycle;
    // The start index: the first grapheme at or past the cursor.
    let mut acc = 0usize;
    let mut start = 0usize;
    for (index, (_, w)) in loop_cells.iter().enumerate() {
        if acc >= cursor {
            start = index;
            break;
        }
        acc += w;
        start = index + 1;
    }
    // One doubled loop covers any window start.
    let parts: Vec<&(String, usize)> = loop_cells.iter().chain(loop_cells.iter()).collect();
    let mut out = String::new();
    let mut taken = 0usize;
    for (text, w) in parts.iter().skip(start) {
        if taken + w > width {
            break;
        }
        out.push_str(text);
        taken += w;
    }
    vec![out]
}

/// The deterministic scroll state for one hovered tab (the port of
/// `createMarquee`'s timers, keyed off the store clock).
#[derive(Debug, Default, Clone)]
pub struct Marquee {
    /// The hovered session (the scroll's owner).
    active: Option<String>,
    /// When the hover landed (clock ms).
    entered_ms: u64,
    /// The cycle width at enter time.
    cycle_width: usize,
}

impl Marquee {
    /// `enter(sessionID, title, width)` — a fitting title resets; a
    /// re-enter of the same session keeps its position.
    pub fn enter(&mut self, session_id: &str, title: &str, width: usize, now_ms: u64) {
        if !marquee_overflows(title, width) {
            self.reset();
            return;
        }
        if self.active.as_deref() == Some(session_id) {
            return;
        }
        self.active = Some(session_id.to_owned());
        self.entered_ms = now_ms;
        self.cycle_width = marquee_cycle_width(title);
    }

    /// `leave(sessionID)` — hover off resets (v2's clear timer is
    /// pointer-event bookkeeping; the visual state is the reset).
    pub fn leave(&mut self, session_id: &str) {
        if self.active.as_deref() == Some(session_id) {
            self.reset();
        }
    }

    pub fn reset(&mut self) {
        self.active = None;
        self.entered_ms = 0;
        self.cycle_width = 0;
    }

    /// The scroll offset at `now_ms`: 0 through the delay, then +1 per
    /// interval — and when the next step would reach the cycle width
    /// the pass ENDS the reference's way (G-476①): the offset resets
    /// to 0 and stays there (`clear()` + `return 0`,
    /// session-tabs.tsx:296-302). The old port clamped at
    /// `cycle_width − 1` and froze on the last window forever.
    #[must_use]
    pub fn offset(&self, now_ms: u64) -> usize {
        let Some(_) = self.active else { return 0 };
        if self.cycle_width == 0 {
            return 0;
        }
        let elapsed = now_ms.saturating_sub(self.entered_ms);
        if elapsed < MARQUEE_DELAY {
            return 0;
        }
        let steps = 1 + (elapsed - MARQUEE_DELAY) / MARQUEE_INTERVAL;
        let steps = usize::try_from(steps).unwrap_or(0);
        if steps >= self.cycle_width {
            // The reset tick fired (offset stepped cycle−1 → 0 and the
            // interval cleared); window 0 shows from here on.
            0
        } else {
            steps
        }
    }

    /// The clock of the cycle's end (the reset tick) — delay + (cycle−1)
    /// intervals, the k = cycle−1 tick of `setInterval`.
    fn reset_ms(&self) -> u64 {
        self.entered_ms
            .saturating_add(MARQUEE_DELAY)
            .saturating_add(
                u64::try_from(self.cycle_width.saturating_sub(1)).unwrap_or(0) * MARQUEE_INTERVAL,
            )
    }

    /// The leading fade (`leading.animate({opacity: 1})` as the scroll
    /// starts; `leading.animate({opacity: 0})` at the cycle's end —
    /// session-tabs.tsx:296-302): smootherstep rise over the fade
    /// window after the delay, hold 1 while the pass runs, then from
    /// the reset tick a 250 ms fall from whatever value the rise had
    /// reached (the ref's tween starts from the current value).
    #[must_use]
    pub fn leading(&self, now_ms: u64) -> f64 {
        let Some(_) = self.active else { return 0.0 };
        let elapsed = now_ms.saturating_sub(self.entered_ms);
        if elapsed <= MARQUEE_DELAY {
            return 0.0;
        }
        let rise = |since_delay: u64| {
            crate::components::tab_pulse::smootherstep(
                f64::from(u32::try_from(since_delay.min(LEADING_FADE_MS)).unwrap_or(0))
                    / f64::from(u32::try_from(LEADING_FADE_MS).unwrap_or(1)),
            )
        };
        if self.cycle_width <= 1 {
            return rise(elapsed - MARQUEE_DELAY);
        }
        let reset = self.reset_ms();
        if elapsed >= reset {
            // The fall starts from the value the rise had reached at
            // the reset tick (opentui's animate tweens from current).
            let since_delay_at_reset =
                reset.saturating_sub(self.entered_ms.saturating_add(MARQUEE_DELAY));
            let at_reset = rise(since_delay_at_reset);
            at_reset * (1.0 - rise(elapsed.saturating_sub(reset)))
        } else {
            rise(elapsed - MARQUEE_DELAY)
        }
    }

    /// The next instant the scroll offset changes — the interval's
    /// next fire (`setInterval(…, MARQUEE_INTERVAL)` in
    /// `createMarquee`, session-tabs.tsx:279-369; the still delay's
    /// end counts as the first boundary). `None` while idle or after
    /// the cycle's reset tick (v2 clears the timer there — the offset
    /// step cycle−1 → 0 at k = cycle−1 is the LAST changing boundary). The
    /// app loop wakes on exactly these instants and pins the clock to
    /// them, so the offset advances by exactly one cell per tick —
    /// sampling an absolute clock at event-driven redraws instead is
    /// what produced the beta.13 stutter (+0/+2 cells per render).
    #[must_use]
    pub fn next_step_ms(&self, now_ms: u64) -> Option<u64> {
        self.active.as_ref()?;
        if self.cycle_width <= 1 {
            return None;
        }
        let start = self.entered_ms.saturating_add(MARQUEE_DELAY);
        if now_ms < start {
            return Some(start);
        }
        // `offset` steps at start + k·interval, k = 1.. — the pass
        // ends at the reset tick k = cycle−1 (offset cycle−1 → 0,
        // G-476①), so the last CHANGING boundary is k = cycle−1.
        let k = (now_ms - start) / MARQUEE_INTERVAL + 1;
        let max_k = u64::try_from(self.cycle_width.saturating_sub(1)).unwrap_or(0);
        if k > max_k {
            return None;
        }
        Some(start + k * MARQUEE_INTERVAL)
    }

    /// The most recent boundary AT OR BEFORE `now_ms` while the scroll
    /// is live — the pin the run loop advances the store clock to
    /// (SA-F3: the old loop compared `next_step_ms` — a STRICTLY
    /// FUTURE instant, `k = q+1 ⇒ start + k·interval > now` — against
    /// `now` and the pin branch never fired; the clock only advanced
    /// on the plain 100 ms ticks, which (with wakes landing on every
    /// 80 ms boundary) fired every OTHER boundary → the offset jumped
    /// +2 cells per ~160 ms — the measured ~166 ms/step vs the 80 ms
    /// pin). `None` while idle, inside the delay, or past the
    /// cycle's reset tick (mirrors `next_step_ms`'s liveness exactly).
    #[must_use]
    pub fn passed_step_ms(&self, now_ms: u64) -> Option<u64> {
        self.active.as_ref()?;
        if self.cycle_width <= 1 {
            return None;
        }
        let start = self.entered_ms.saturating_add(MARQUEE_DELAY);
        if now_ms < start {
            return None;
        }
        let q = (now_ms - start) / MARQUEE_INTERVAL;
        let max_k = u64::try_from(self.cycle_width.saturating_sub(1)).unwrap_or(0);
        if q + 1 > max_k {
            return None;
        }
        Some(start + q * MARQUEE_INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference's own shape: a fitting title renders truncated;
    /// an overflowing one loops title → `·` → title at the cursor.
    #[test]
    fn marquee_windows_match_the_reference_shape() {
        let title = "A very long session title that overflows";
        let width = 10;
        assert!(marquee_overflows(title, width));
        assert!(!marquee_overflows("Short", width));
        // Offset 0: the head, truncated.
        assert_eq!(marquee_text(title, width, 0), "A very lon");
        // Offset 2: the window starting two cells in.
        assert_eq!(marquee_text(title, width, 2), "very long ");
        // The loop: past the title's end the separator and the title's
        // head follow (the title is 40 cells).
        assert_eq!(display_width(title), 40);
        assert!(marquee_text(title, width, 38).contains('·'), "the separator loops in");
        // The cycle wraps: offset = cycleWidth + 3 ≡ 3.
        let cycle = marquee_cycle_width(title);
        assert_eq!(cycle, 40 + 3);
        assert_eq!(marquee_text(title, width, cycle + 3), marquee_text(title, width, 3));
    }

    /// G-476② — the loop windows move whole grapheme clusters
    /// (`Locale.graphemes`, util/marquee.ts:30-33): the flag's second
    /// regional indicator, an NFD combining mark and a ZWJ family's
    /// joiner are each parts of ONE loop cell — a window seam can
    /// never cut inside them, and every window that starts mid-cluster
    /// snaps forward to the next cluster boundary (the ref's
    /// start-index scan runs on clusters). The detectors are clusters
    /// whose ref emission (G-452, wrap.rs `ref_emission_shape`) keeps
    /// the full bytes: flags, NFD marks, and the family's base+ZWJ
    /// head (a cut between the base and the ZWJ emits a bare `🧑`,
    /// which no legal window ever contains).
    #[test]
    fn marquee_windows_never_split_grapheme_clusters() {
        // NFD ü (u + U+0308), the flag (two regional indicators) and
        // the ZWJ family written with explicit escapes — the cluster
        // seams under test are exactly the joining codepoints.
        let title = "\u{1F1FA}\u{1F1F8}u\u{0308}\u{1F9D1}\u{200D}\u{1F4BB} title that overflows the width";
        let width = 10;
        assert!(marquee_overflows(title, width));
        assert_eq!(display_width("\u{1F1FA}\u{1F1F8}"), 2, "the flag is one 2-cell cluster");
        assert_eq!(
            display_width("u\u{0308}"),
            1,
            "NFD ü (u + U+0308) is one 1-cell cluster"
        );
        assert_eq!(
            display_width("\u{1F9D1}\u{200D}\u{1F4BB}"),
            2,
            "the ZWJ family is one 2-cell cluster"
        );
        for offset in 1..(marquee_cycle_width(title) * 2) {
            let window = marquee_text(title, width, offset);
            // Whole flag: a lone first indicator never shows unpaired.
            if let Some(at) = window.find('\u{1F1FA}') {
                assert!(
                    window[at..].starts_with("\u{1F1FA}\u{1F1F8}"),
                    "offset {offset}: {window:?} split the flag"
                );
            }
            // Whole NFD cluster: every base keeps its combining mark.
            for (at, ch) in window.char_indices() {
                if ch == 'u' {
                    assert!(
                        window[at + 1..].starts_with('\u{0308}'),
                        "offset {offset}: {window:?} dropped the combining mark"
                    );
                }
            }
            // Whole family head: the base is never emitted without its
            // ZWJ (the emission shape is `🧑‍`).
            if let Some(at) = window.find('\u{1F9D1}') {
                assert!(
                    window[at + '\u{1F9D1}'.len_utf8()..].starts_with('\u{200D}'),
                    "offset {offset}: {window:?} cut between the base and the ZWJ"
                );
            }
            // Windows start at cluster boundaries — never on a joining
            // codepoint (the mark, the ZWJ, the flag's second
            // indicator; the mark/ZWJ are zero-width, the second
            // indicator only exists inside the flag).
            let first = window.chars().next().expect("nonempty while scrolling");
            assert!(
                first != '\u{0308}' && first != '\u{200D}' && first != '\u{1F1F8}',
                "offset {offset}: {window:?} starts mid-cluster"
            );
            assert!(
                display_width(&window) <= width,
                "offset {offset}: {window:?} over the window"
            );
        }
        // The window starting mid-cluster snaps forward to the NEXT
        // cluster boundary (cursor 1 = inside the flag → starts at ü).
        assert!(
            marquee_text(title, width, 1).starts_with('u'),
            "cursor 1 snaps past the flag to the NFD cluster"
        );
        // Cursor 2 = the flag's end cell → same snap (starts at ü);
        // cursor 5 = the family's end cell → the space.
        assert!(marquee_text(title, width, 2).starts_with('u'));
        assert!(marquee_text(title, width, 5).starts_with(' '));
    }

    /// The deterministic timers: 600 ms still, then +1 per 80 ms — and
    /// the pass ends the ref's way (G-476①): at the k = cycle−1 tick
    /// the offset resets to 0 (v2 clears the interval there,
    /// session-tabs.tsx:296-302). (Re-pin, va20: the old assertion
    /// pinned the pre-fix clamp at cycle−1 — the registered G-476
    /// divergence, not ref behavior.)
    #[test]
    fn offsets_follow_the_delay_and_interval() {
        let mut marquee = Marquee::default();
        marquee.enter("s1", "A very long session title that overflows", 10, 0);
        assert_eq!(marquee.offset(0), 0, "still at enter");
        assert_eq!(marquee.offset(599), 0, "still inside the delay");
        assert_eq!(marquee.offset(600), 1, "first step as the delay ends");
        assert_eq!(marquee.offset(680), 2, "one cell per 80 ms");
        assert_eq!(marquee.offset(1_000), 6);
        let cycle = marquee_cycle_width("A very long session title that overflows");
        // The last scrolling tick shows cycle−1 …
        assert_eq!(
            marquee.offset(600 + 80 * (cycle as u64 - 2)),
            cycle - 1,
            "k = cycle−2: the last scrolling window (offset cycle−1)"
        );
        // … and the NEXT tick (k = cycle−1) is the reset: window 0,
        // interval cleared, leading fading out.
        assert_eq!(
            marquee.offset(600 + 80 * (cycle as u64 - 1)),
            0,
            "the reset tick returns the pass to window 0"
        );
        assert_eq!(
            marquee.offset(600 + 80 * (cycle as u64 - 1) + 500),
            0,
            "stays at window 0 (the interval is cleared)"
        );
        // The leading fade rides the same clock.
        assert_eq!(marquee.leading(600), 0.0);
        assert!((marquee.leading(600 + 125) - 0.5).abs() < 1e-9, "tween midpoint");
        assert_eq!(marquee.leading(600 + 250), 1.0);
        // G-476①: at the reset tick the fade falls back out over
        // LEADING_FADE_MS (the rise had already completed: the reset
        // lands ≥ 250 ms after the delay for any real title).
        let reset = 600 + 80 * (cycle as u64 - 1);
        assert_eq!(marquee.leading(reset), 1.0, "full at the reset tick");
        assert!(
            (marquee.leading(reset + LEADING_FADE_MS / 2) - 0.5).abs() < 1e-9,
            "fade-out midpoint"
        );
        assert_eq!(marquee.leading(reset + LEADING_FADE_MS), 0.0, "faded out");
        // Leave + re-enter restarts from 0.
        marquee.leave("s1");
        assert_eq!(marquee.offset(10_000), 0, "reset on leave");
    }

    /// The steady ticker (beta.13 bug 3): while a scroll runs,
    /// `next_step_ms` names every offset-change instant — the delay's
    /// end, then each 80 ms boundary, `None` after the reset tick ends
    /// the pass. A loop that wakes on each and pins the clock to it
    /// advances the offset by EXACTLY one cell per tick, and the final
    /// tick is the G-476① reset (cycle−1 → 0, the interval cleared
    /// there — session-tabs.tsx:296-302). (Re-pin, va20: the old test
    /// ended at the pre-fix clamp, one boundary short.)
    #[test]
    fn next_step_names_every_boundary_for_a_steady_ticker() {
        let mut marquee = Marquee::default();
        assert_eq!(marquee.next_step_ms(0), None, "idle: no ticker");
        marquee.enter("s1", "A very long session title that overflows", 10, 0);
        assert_eq!(marquee.next_step_ms(0), Some(MARQUEE_DELAY), "the still delay's end");
        assert_eq!(marquee.next_step_ms(599), Some(MARQUEE_DELAY));
        assert_eq!(marquee.next_step_ms(600), Some(680), "k=1 strictly after now");
        // Walking the boundaries one tick at a time: the offset steps
        // by exactly 1 per wake — except the LAST wake, the reset tick
        // (cycle−1 → 0, G-476①).
        let cycle = marquee_cycle_width("A very long session title that overflows");
        let mut now = 0u64;
        let mut offset = marquee.offset(now);
        let mut ticks = 0u64;
        while let Some(deadline) = marquee.next_step_ms(now) {
            now = deadline;
            let next = marquee.offset(now);
            if next == 0 {
                assert_eq!(
                    offset, cycle - 1,
                    "the reset tick steps from the LAST window back to 0"
                );
            } else {
                assert_eq!(next, offset + 1, "tick {ticks}: exactly one cell");
            }
            offset = next;
            ticks += 1;
        }
        // The first change lands AT the delay's end (offset(600) = 1),
        // then one per interval, and the k = cycle−1 tick is the reset
        // (cycle−1 → 0) — the last changing boundary.
        assert_eq!(ticks, cycle as u64, "every offset-change boundary incl. the reset");
        assert_eq!(offset, 0, "the pass ends on window 0 (reset)");
        // A hover-off kills the ticker.
        marquee.leave("s1");
        assert_eq!(marquee.next_step_ms(now), None);
    }

    /// SA-F3: `passed_step_ms` is the pin the run loop needs — the
    /// most recent boundary AT OR BEFORE `now` (so a wake landing on a
    /// boundary can pin the clock to exactly it), liveness mirroring
    /// `next_step_ms` exactly. The OLD loop's `deadline <= now`
    /// comparison against `next_step_ms` could never fire (the next
    /// boundary is strictly future by construction) — this test pins
    /// the invariant that makes the fixed loop's pin branch fire.
    #[test]
    fn passed_step_names_the_boundary_to_pin_each_wake() {
        let mut marquee = Marquee::default();
        assert_eq!(marquee.passed_step_ms(0), None, "idle: nothing to pin");
        marquee.enter("s1", "A very long session title that overflows", 10, 0);
        assert_eq!(marquee.passed_step_ms(599), None, "inside the delay: nothing yet");
        // AT the delay's end the first boundary has passed (offset 1).
        assert_eq!(marquee.passed_step_ms(600), Some(600));
        assert_eq!(marquee.passed_step_ms(679), Some(600), "pre-boundary: still 600");
        assert_eq!(marquee.passed_step_ms(680), Some(680));
        assert_eq!(marquee.passed_step_ms(1_000), Some(1_000), "the grid: 600+5·80");
        // The next deadline is ALWAYS strictly future — the invariant
        // the old pin branch violated (it compared this against now).
        let mut now = 600u64;
        while let Some(next) = marquee.next_step_ms(now) {
            assert!(next > now, "the next boundary is strictly future");
            let passed = marquee.passed_step_ms(now).expect("live while next exists");
            assert!(passed <= now, "the pin is at or before now");
            assert_eq!(passed + MARQUEE_INTERVAL, next, "pin and next are one interval apart");
            now = next;
        }
        // Past the cycle's reset tick BOTH go None (plain ticks resume
        // for the 250 ms fade-out — the ref clears the interval there).
        let cycle = marquee_cycle_width("A very long session title that overflows");
        let end = MARQUEE_DELAY + u64::try_from(cycle).unwrap_or(0) * MARQUEE_INTERVAL + 500;
        assert_eq!(marquee.next_step_ms(end), None);
        assert_eq!(marquee.passed_step_ms(end), None, "the reset ends the pin too");
    }
}
