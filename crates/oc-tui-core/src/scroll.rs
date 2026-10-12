//! The transcript scroll driver — v2's session scroll commands over
//! the follow/sticky state machine.
//!
//! * **Commands** (`routes/session/index.tsx:763-805`, the
//!   `globalCommands` table): a "page" is `viewport / 2`, a half page
//!   `viewport / 4`, a line `±1` (the scrollbox's `scrollBy` — the
//!   port's line granularity matches); `session.first` jumps to the
//!   top, `session.last` re-sticks to the bottom (`toBottom`,
//!   index.tsx:732-746: `stickyScroll = true; scrollTo(scrollHeight)`).
//! * **Follow semantics** (`FollowState`): sticky at the maximum
//!   scrollTop (vscroll-stick-bottom), free while scrolled away, and
//!   re-stick when a scroll lands at the maximum — the vscrollbox's
//!   `stickyScroll={!navigationMessage() && !navigationSlack()}`
//!   (index.tsx:1392-1393) with `stickyStart="bottom"`.
//!
//! `FollowState` moved here from oc-tui's `message/list.rs` in the
//! Wave A lane (oc-tui re-exports it — the goldens' imports are
//! unchanged).

// v2's scroll math is f64 (opentui's ScrollBar keeps fractional
// scroll positions and rounds at the setter); the casts back to the
// cell-grid's usize are exact under the round().
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

/// The follow/jump state machine (the sticky/free contract the
/// vscroll goldens pin).
///
/// Transitions (driven by scroll events + appends):
/// * `Sticky` — viewport pinned at the maximum scrollTop; an append
///   moves the viewport to keep the tail visible (vscroll-stick-bottom).
/// * `Free(offset)` — the user scrolled away; appending content does
///   NOT move the viewport; the "Jump to latest ↓" indicator shows
///   (`session-jump-to-latest`).
/// * Returning the offset to the maximum ⇒ `Sticky` again (indicator
///   cleared, sticky re-armed — the vscroll-bottom golden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FollowState {
    #[default]
    Sticky,
    Free {
        offset: usize,
        maximum: usize,
    },
}

impl FollowState {
    /// The indicator's visibility rule (`awayFromBottom`).
    #[must_use]
    pub fn jump_to_latest_visible(&self) -> bool {
        matches!(self, Self::Free { offset, maximum } if offset < maximum)
    }

    /// A scroll event's transition (returns the next state).
    #[must_use]
    pub fn scrolled_to(self, offset: usize, maximum: usize) -> Self {
        if offset >= maximum {
            Self::Sticky
        } else {
            Self::Free { offset, maximum }
        }
    }

    /// An append's transition: sticky follows, free stays.
    #[must_use]
    pub fn appended(self, new_maximum: usize) -> Self {
        match self {
            Self::Sticky => Self::Sticky,
            free @ Self::Free { .. } => {
                Self::Free { offset: free.offset_value(), maximum: new_maximum }
            }
        }
    }

    fn offset_value(self) -> usize {
        match self {
            Self::Sticky => 0,
            Self::Free { offset, .. } => offset,
        }
    }
}

/// One of v2's six scroll commands + the first/last jumps
/// (keybind.ts:177-184).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollCommand {
    /// `session.page.up` — `scrollBy(-height/2)`.
    PageUp,
    /// `session.page.down`.
    PageDown,
    /// `session.line.up` — `scrollBy(-1)`.
    LineUp,
    /// `session.line.down`.
    LineDown,
    /// `session.half.page.up` — `scrollBy(-height/4)`.
    HalfPageUp,
    /// `session.half.page.down`.
    HalfPageDown,
    /// `session.first` — `scrollTo(0)` (sticky off).
    First,
    /// `session.last` — `toBottom()` (sticky re-armed).
    Last,
}

impl ScrollCommand {
    /// The dot command id (`session.page.up`).
    #[must_use]
    pub const fn command(self) -> &'static str {
        match self {
            Self::PageUp => "session.page.up",
            Self::PageDown => "session.page.down",
            Self::LineUp => "session.line.up",
            Self::LineDown => "session.line.down",
            Self::HalfPageUp => "session.half.page.up",
            Self::HalfPageDown => "session.half.page.down",
            Self::First => "session.first",
            Self::Last => "session.last",
        }
    }

    /// Parse a dot command id.
    #[must_use]
    pub fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "session.page.up" => Self::PageUp,
            "session.page.down" => Self::PageDown,
            "session.line.up" => Self::LineUp,
            "session.line.down" => Self::LineDown,
            "session.half.page.up" => Self::HalfPageUp,
            "session.half.page.down" => Self::HalfPageDown,
            "session.first" => Self::First,
            "session.last" => Self::Last,
            _ => return None,
        })
    }

    /// All eight commands (dispatch wiring).
    #[must_use]
    pub const fn all() -> [ScrollCommand; 8] {
        [
            Self::PageUp,
            Self::PageDown,
            Self::LineUp,
            Self::LineDown,
            Self::HalfPageUp,
            Self::HalfPageDown,
            Self::First,
            Self::Last,
        ]
    }
}

/// JS `Math.round` — halves round toward +∞ (the [`ScrollBar`]
/// rounds the landed scroll position; `round(-7.5) = -7`,
/// `round(7.5) = 8`).
fn js_round(value: f64) -> usize {
    let rounded = (value + 0.5).floor();
    if rounded.is_finite() && rounded >= 0.0 {
        rounded as usize
    } else {
        0
    }
}

/// The scroll driver: one [`FollowState`] per session, stepped by the
/// eight commands. The host supplies the CURRENT viewport +
/// content-height metrics each step (the render pass records them —
/// `render_session`'s transcript budget).
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollDriver {
    follow: FollowState,
}

impl ScrollDriver {
    #[must_use]
    pub const fn new() -> Self {
        Self { follow: FollowState::Sticky }
    }

    #[must_use]
    pub const fn follow(&self) -> FollowState {
        self.follow
    }

    /// The paint offset for a follow state: sticky sits at the
    /// maximum scrollTop, free keeps its offset
    /// (`Transcript::paint`'s offset math).
    #[must_use]
    pub fn offset(follow: FollowState, viewport: usize, content_height: usize) -> usize {
        let maximum = content_height.saturating_sub(viewport);
        match follow {
            FollowState::Sticky => maximum,
            FollowState::Free { offset, .. } => offset.min(maximum),
        }
    }

    /// The paint offset under the driver's live state.
    #[must_use]
    pub fn paint_offset(&self, viewport: usize, content_height: usize) -> usize {
        Self::offset(self.follow, viewport, content_height)
    }

    /// Apply one scroll command (v2's `moveTranscript` /
    /// `toBottom`, index.tsx:732-761): the deltas are
    /// `viewport/2` (page), `viewport/4` (half), `1` (line) as
    /// FLOATS, and the scrollbar ROUNDS the landed position —
    /// opentui's `ScrollBar.scrollPosition` setter is
    /// `Math.round(clamp(value, 0, scrollSize - viewportSize))`
    /// (`@opentui/core` `src/renderables/ScrollBar.ts:72-74`), so a
    /// half-viewport delta of N.5 lands at `N` going up and `N+1`
    /// coming down (`Math.round` halves go toward +∞: pageup from
    /// an integer offset moves 7 at a 15-row viewport, pagedown 8
    /// — pinned by the long-reply-scroll corpus: 20→13→6, then
    /// 6→14); the offset clamps to `[0, maximum]`; landing at the
    /// maximum re-sticks (`updateAwayFromBottom`), `Last`
    /// re-sticks unconditionally, `First` jumps to the top (sticky
    /// off).
    pub fn scroll(&mut self, command: ScrollCommand, viewport: usize, content_height: usize) {
        let maximum = content_height.saturating_sub(viewport);
        let current = Self::offset(self.follow, viewport, content_height);
        match command {
            ScrollCommand::First => {
                self.follow = FollowState::Free { offset: 0, maximum };
            }
            ScrollCommand::Last => {
                self.follow = FollowState::Sticky;
            }
            other => {
                let delta: f64 = match other {
                    ScrollCommand::PageUp => -(f64::from(viewport as u32) / 2.0),
                    ScrollCommand::PageDown => f64::from(viewport as u32) / 2.0,
                    ScrollCommand::HalfPageUp => -(f64::from(viewport as u32) / 4.0),
                    ScrollCommand::HalfPageDown => f64::from(viewport as u32) / 4.0,
                    ScrollCommand::LineUp => -1.0,
                    ScrollCommand::LineDown => 1.0,
                    ScrollCommand::First | ScrollCommand::Last => 0.0,
                };
                let next = js_round((f64::from(current as u32) + delta).clamp(0.0, maximum as f64));
                self.follow = self.follow.scrolled_to(next, maximum);
            }
        }
    }

    /// An append transition (a streaming part grew the transcript):
    /// sticky follows the tail, free keeps its offset.
    pub fn appended(&mut self, new_maximum: usize) {
        self.follow = self.follow.appended(new_maximum);
    }

    /// A wheel/mouse scroll lands through the same transition.
    pub fn scrolled_to(&mut self, offset: usize, maximum: usize) {
        self.follow = self.follow.scrolled_to(offset, maximum);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver_at(offset: usize, maximum: usize) -> ScrollDriver {
        let mut driver = ScrollDriver::new();
        driver.scrolled_to(offset, maximum);
        driver
    }

    /// v2's deltas (index.tsx:763-805): page = height/2, half = height/4,
    /// line = 1 — against a 40-row viewport over 200 content rows.
    #[test]
    fn command_deltas_match_v2() {
        // From the bottom (maximum 160): page up lands at 140.
        let mut driver = ScrollDriver::new();
        driver.scroll(ScrollCommand::PageUp, 40, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 140, maximum: 160 });
        // Half page up from there: 130.
        driver.scroll(ScrollCommand::HalfPageUp, 40, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 130, maximum: 160 });
        // Line down thrice: 133.
        for _ in 0..3 {
            driver.scroll(ScrollCommand::LineDown, 40, 200);
        }
        assert_eq!(driver.follow(), FollowState::Free { offset: 133, maximum: 160 });
        // Page down from 133 lands at 153 (still free); the next one
        // clamps to the maximum 160 — and RE-STICKS.
        driver.scroll(ScrollCommand::PageDown, 40, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 153, maximum: 160 });
        driver.scroll(ScrollCommand::PageDown, 40, 200);
        assert_eq!(driver.follow(), FollowState::Sticky);
        assert!(!driver.follow().jump_to_latest_visible());
    }

    /// The long-reply-scroll corpus pin (80x24: viewport 15, content
    /// 41): sticky shows reply rows 20..32; pageup lands at 13, the
    /// second at 6, pagedown from there at 14 — the HALF-ROW deltas
    /// round through opentui's `Math.round` (halves toward +∞), so
    /// pageup moves 7 (round(-7.5) = -7) and pagedown 8
    /// (round(7.5) = 8) at an ODD viewport.
    #[test]
    fn half_viewport_deltas_round_like_opentui() {
        // Sticky offset 26 (content 41 − viewport 15).
        let mut driver = ScrollDriver::new();
        driver.scrolled_to(26, 26);
        assert_eq!(driver.paint_offset(15, 41), 26);
        driver.scroll(ScrollCommand::PageUp, 15, 41);
        assert_eq!(driver.follow(), FollowState::Free { offset: 19, maximum: 26 });
        driver.scroll(ScrollCommand::PageUp, 15, 41);
        assert_eq!(driver.follow(), FollowState::Free { offset: 12, maximum: 26 });
        driver.scroll(ScrollCommand::PageDown, 15, 41);
        assert_eq!(driver.follow(), FollowState::Free { offset: 20, maximum: 26 });
        // Even viewport: no halves, exact steps.
        let mut driver = ScrollDriver::new();
        driver.scrolled_to(160, 184);
        driver.scroll(ScrollCommand::PageUp, 16, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 152, maximum: 184 });
        driver.scroll(ScrollCommand::PageDown, 16, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 160, maximum: 184 });
    }

    /// The end-anchor (`session.last`): sticky re-arms and the paint
    /// offset sits at the maximum — the post-`end` state the corpus
    /// captures (the last viewport-15 window).
    #[test]
    fn end_re_anchors_to_the_bottom() {
        let mut driver = driver_at(12, 26);
        driver.scroll(ScrollCommand::Last, 15, 41);
        assert_eq!(driver.follow(), FollowState::Sticky);
        assert_eq!(driver.paint_offset(15, 41), 26);
    }

    /// Clamping: the offset never leaves [0, maximum].
    #[test]
    fn offsets_clamp_at_both_edges() {
        let mut driver = ScrollDriver::new();
        driver.scroll(ScrollCommand::PageDown, 40, 200);
        assert_eq!(driver.follow(), FollowState::Sticky, "down from sticky stays stuck");
        let mut driver = driver_at(0, 160);
        driver.scroll(ScrollCommand::PageUp, 40, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 0, maximum: 160 });
    }

    /// first/last (index.tsx:808-887): first jumps to the top with
    /// sticky off; last re-sticks unconditionally.
    #[test]
    fn first_and_last_jump() {
        let mut driver = ScrollDriver::new();
        driver.scroll(ScrollCommand::First, 40, 200);
        assert_eq!(driver.follow(), FollowState::Free { offset: 0, maximum: 160 });
        assert!(driver.follow().jump_to_latest_visible(), "the indicator shows");
        driver.scroll(ScrollCommand::Last, 40, 200);
        assert_eq!(driver.follow(), FollowState::Sticky);
        // Last from anywhere (a mid-scroll free state too).
        let mut driver = driver_at(100, 160);
        driver.scroll(ScrollCommand::Last, 40, 200);
        assert_eq!(driver.follow(), FollowState::Sticky);
        assert_eq!(driver.paint_offset(40, 200), 160);
    }

    /// Append-follow: sticky keeps the tail (offset = new maximum);
    /// free keeps its offset under the grown content.
    #[test]
    fn append_follows_only_when_sticky() {
        let mut driver = ScrollDriver::new();
        driver.appended(220);
        assert_eq!(driver.follow(), FollowState::Sticky);
        assert_eq!(driver.paint_offset(40, 260), 220, "sticky sits at the new maximum");
        let mut driver = driver_at(100, 160);
        driver.appended(220);
        assert_eq!(driver.follow(), FollowState::Free { offset: 100, maximum: 220 });
        assert!(driver.follow().jump_to_latest_visible());
        // Scrolling back to the maximum re-sticks (vscroll-bottom).
        driver.scrolled_to(220, 220);
        assert_eq!(driver.follow(), FollowState::Sticky);
    }

    /// The paint offset: sticky = content − viewport; free = its own
    /// offset clamped to the maximum.
    #[test]
    fn paint_offset_math() {
        assert_eq!(ScrollDriver::offset(FollowState::Sticky, 24, 100), 76);
        assert_eq!(
            ScrollDriver::offset(FollowState::Free { offset: 10, maximum: 90 }, 24, 100),
            10
        );
        // Content shorter than the viewport: no scroll at all.
        assert_eq!(ScrollDriver::offset(FollowState::Sticky, 24, 10), 0);
        assert_eq!(ScrollDriver::offset(FollowState::Free { offset: 5, maximum: 0 }, 24, 10), 0);
    }

    /// Every dot command parses; the ids match the keybind table's.
    #[test]
    fn commands_parse() {
        for command in ScrollCommand::all() {
            assert_eq!(ScrollCommand::parse(command.command()), Some(command));
        }
        assert_eq!(ScrollCommand::parse("session.page.up"), Some(ScrollCommand::PageUp));
        assert!(ScrollCommand::parse("nope").is_none());
    }
}
