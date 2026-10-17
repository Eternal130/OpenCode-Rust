//! The hover marquee (`util/marquee.ts` + `createTabMarquee`,
//! session-tabs.tsx:279-369) — lane TAIL item 1c: a long tab title
//! auto-scrolls while hovered.
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
//! only on grapheme boundaries.

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
    // The loop: title graphemes, then the separator's (space · space).
    let mut loop_cells: Vec<(char, usize)> = Vec::new();
    for ch in value.chars() {
        loop_cells.push((ch, display_width(&ch.to_string())));
    }
    for ch in GAP.chars() {
        loop_cells.push((ch, display_width(&ch.to_string())));
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
    let parts: Vec<&(char, usize)> = loop_cells.iter().chain(loop_cells.iter()).collect();
    let mut out = String::new();
    let mut taken = 0usize;
    for (ch, w) in parts.iter().skip(start) {
        if taken + w > width {
            break;
        }
        out.push(*ch);
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

    #[must_use]
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// The scroll offset at `now_ms`: 0 through the delay, then +1 per
    /// interval until the cycle width is covered — v2 stops the
    /// interval and resets (`value + 1 < cycleWidth` else 0).
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
        usize::try_from(steps).unwrap_or(0).min(self.cycle_width.saturating_sub(1))
    }

    /// The leading fade (`leading.animate({opacity: 1})` while the
    /// scroll runs): smootherstep over the fade window after the delay.
    #[must_use]
    pub fn leading(&self, now_ms: u64) -> f64 {
        let Some(_) = self.active else { return 0.0 };
        let elapsed = now_ms.saturating_sub(self.entered_ms);
        if elapsed <= MARQUEE_DELAY {
            return 0.0;
        }
        crate::components::tab_pulse::smootherstep(
            f64::from(u32::try_from((elapsed - MARQUEE_DELAY).min(LEADING_FADE_MS)).unwrap_or(0))
                / f64::from(u32::try_from(LEADING_FADE_MS).unwrap_or(1)),
        )
    }

    /// The next instant the scroll offset changes — the interval's
    /// next fire (`setInterval(…, MARQUEE_INTERVAL)` in
    /// `createMarquee`, session-tabs.tsx:279-369; the still delay's
    /// end counts as the first boundary). `None` while idle or after
    /// the one-pass clamp (v2 stops the timer at the cycle end). The
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
        // `offset` steps at start + k·interval, k = 1.. — clamped at
        // cycle−1, so the last CHANGING boundary is k = cycle−2 (the
        // clamp already held at k−1).
        let k = (now_ms - start) / MARQUEE_INTERVAL + 1;
        let max_k = u64::try_from(self.cycle_width.saturating_sub(2)).unwrap_or(0);
        if k > max_k {
            return None;
        }
        Some(start + k * MARQUEE_INTERVAL)
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

    /// The deterministic timers: 600 ms still, then +1 per 80 ms,
    /// clamped at the cycle width (v2 stops after one full pass).
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
        assert_eq!(
            marquee.offset(600 + 80 * (cycle as u64 - 1)),
            cycle - 1,
            "clamped at the cycle"
        );
        // The leading fade rides the same clock.
        assert_eq!(marquee.leading(600), 0.0);
        assert!((marquee.leading(600 + 125) - 0.5).abs() < 1e-9, "tween midpoint");
        assert_eq!(marquee.leading(600 + 250), 1.0);
        // Leave + re-enter restarts from 0.
        marquee.leave("s1");
        assert_eq!(marquee.offset(10_000), 0, "reset on leave");
    }

    /// The steady ticker (beta.13 bug 3): while a scroll runs,
    /// `next_step_ms` names every offset-change instant — the delay's
    /// end, then each 80 ms boundary, `None` after the one-pass clamp.
    /// A loop that wakes on each and pins the clock to it advances the
    /// offset by EXACTLY one cell per tick (v2's interval semantics —
    /// `setInterval(…, 80)` in createMarquee, session-tabs.tsx:279-369;
    /// sampling an absolute clock only at event-driven redraws is what
    /// stuttered +0/+2 cells in the real env).
    #[test]
    fn next_step_names_every_boundary_for_a_steady_ticker() {
        let mut marquee = Marquee::default();
        assert_eq!(marquee.next_step_ms(0), None, "idle: no ticker");
        marquee.enter("s1", "A very long session title that overflows", 10, 0);
        assert_eq!(marquee.next_step_ms(0), Some(MARQUEE_DELAY), "the still delay's end");
        assert_eq!(marquee.next_step_ms(599), Some(MARQUEE_DELAY));
        assert_eq!(marquee.next_step_ms(600), Some(680), "k=1 strictly after now");
        // Walking the boundaries one tick at a time: the offset steps
        // by exactly 1 per wake, through the clamp.
        let mut now = 0u64;
        let mut offset = marquee.offset(now);
        let mut ticks = 0u64;
        while let Some(deadline) = marquee.next_step_ms(now) {
            now = deadline;
            let next = marquee.offset(now);
            assert_eq!(next, offset + 1, "tick {ticks}: exactly one cell");
            offset = next;
            ticks += 1;
        }
        let cycle = marquee_cycle_width("A very long session title that overflows");
        // The first change lands AT the delay's end (offset(600) = 1),
        // then one per interval through the clamp.
        assert_eq!(ticks, (cycle - 1) as u64, "every offset-change boundary");
        assert_eq!(offset, cycle - 1, "the clamped final offset");
        // A hover-off kills the ticker.
        marquee.leave("s1");
        assert_eq!(marquee.next_step_ms(now), None);
    }
}
