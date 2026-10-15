//! Session tabs — the horizontal bar and vertical rail, ported from
//! `component/session-tabs.tsx` + `context/session-tabs-model.ts` for the
//! golden-matched subset (animations disabled, status indicators,
//! adaptive windowing, the add slot, the unread-glow separator rows).
//!
//! Golden scope (`session-tabs-{horizontal,vertical}` @80/160): two
//! tabs, first carries an unread-activity indicator, second is active,
//! detail line "project", the add affordance present. Since 7.2 wave 2
//! the frames are **text + canonical ANSI byte-exact**: the resting
//! unread glow (per-cell pulse backgrounds, per-character title tints,
//! the separator's outer falloff) ports the deterministic math in
//! [`crate::components::tab_pulse`] — sustain state only; the
//! time-varying envelopes (ignition/release/sweep/flash, marquee,
//! motion springs) stay C2.

// Tab widths/gaps are terminal dimensions (far below 2^16/2^23), so
// the pedantic usize→u16/f32 cast lints carry no real risk here, and
// the golden renderers read as one geometry pass per surface.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use crate::store::{SessionTab, Unread};

/// `SESSION_TAB_WIDTH` / `SESSION_TAB_MAX_WIDTH` / `SESSION_TAB_MIN_WIDTH`.
pub const SESSION_TAB_WIDTH: usize = 22;
pub const SESSION_TAB_MAX_WIDTH: usize = 32;
pub const SESSION_TAB_MIN_WIDTH: usize = 8;
/// `SESSION_SIDEBAR_WIDTH` (the default rail width).
pub const SESSION_SIDEBAR_WIDTH: usize = 42;
/// `SESSION_TABS_COMPACT_BREAKPOINT`.
pub const SESSION_TABS_COMPACT_BREAKPOINT: usize = 12;
/// The add affordance renders as `" + "` and reserves that width.
pub const ADD_TAB_WIDTH: usize = 3;
/// `INDICATOR_WIDTH` for the rail pulse separators.
pub const INDICATOR_WIDTH: usize = 10;
/// `TAB_UNREAD_MARKERS["small-dot"]`.
pub const UNREAD_MARKER: &str = "•";
/// `SPINNER_FRAMES[0]` (frozen frame — animations off).
pub const SPINNER_FRAME_0: &str = "⠋";
/// `NEW_SESSION_TAB_TITLE`.
pub const NEW_SESSION_TAB_TITLE: &str = "New session";
/// Lower-half block: the glyph the above-separator pulse paints.
pub const SEPARATOR_GLYPH: char = '▄';

/// Overflow markers reserve one gap cell beside the arrow and count
/// (`sessionTabOverflowWidth`).
#[must_use]
pub fn session_tab_overflow_width(count: usize) -> usize {
    count.to_string().len() + 2
}

/// Tab status subset the 7.1 renderer consumes
/// (`EMPTY_SESSION_TAB_STATUS` shape).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabStatus {
    pub unread: Option<Unread>,
    pub busy: bool,
    /// `attention`: permission ("!") or question ("?").
    pub attention: Option<Attention>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    Permission,
    Question,
}

impl TabStatus {
    pub const EMPTY: Self = Self { unread: None, busy: false, attention: None };

    /// `runs`: busy without attention.
    #[must_use]
    pub fn runs(self) -> bool {
        self.busy && self.attention.is_none()
    }

    /// `glows`: attention, or unread on a non-selected non-busy tab.
    #[must_use]
    pub fn glows(self, selected: bool) -> bool {
        self.attention.is_some() || (!selected && !self.busy && self.unread.is_some())
    }

    /// The indicator glyph (`TabIndicator.label`, status mode).
    #[must_use]
    pub fn indicator_label(self) -> &'static str {
        match self.attention {
            Some(Attention::Permission) => "!",
            Some(Attention::Question) => "?",
            None if self.runs() => SPINNER_FRAME_0,
            None if self.unread.is_some() => UNREAD_MARKER,
            None => "",
        }
    }
}

/// `adaptiveSessionTabLayout`: solve the visible-tab window and per-tab
/// widths for the horizontal strip.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveLayout {
    /// The visible window of tabs.
    pub tabs: Vec<SessionTab>,
    pub widths: Vec<usize>,
    pub before: usize,
    pub after: usize,
    pub start: usize,
    pub total: usize,
}

#[must_use]
pub fn adaptive_session_tab_layout(
    tabs: &[SessionTab],
    active: Option<&str>,
    available: usize,
    previous_start: usize,
) -> AdaptiveLayout {
    if tabs.is_empty() {
        return AdaptiveLayout {
            tabs: Vec::new(),
            widths: Vec::new(),
            before: 0,
            after: 0,
            start: 0,
            total: 0,
        };
    }
    let active_index = tabs.iter().position(|tab| Some(tab.session_id.as_str()) == active);
    let len = tabs.len();
    let (count, start) = solve(
        len,
        active_index,
        available,
        fit_for(available, len, active_index),
        previous_start,
        3,
    );
    let visible: Vec<SessionTab> = tabs.iter().skip(start).take(count).cloned().collect();
    let before = start;
    let after = len - start - count;
    let content_width = available
        .saturating_sub(if before > 0 { session_tab_overflow_width(before) } else { 0 })
        .saturating_sub(if after > 0 { session_tab_overflow_width(after) } else { 0 })
        .max(1);
    let roomy = content_width >= SESSION_TAB_WIDTH * visible.len();
    let total = if roomy {
        content_width.min(SESSION_TAB_MAX_WIDTH * visible.len())
    } else {
        content_width
    };
    let widths = distribute_widths(&visible, active, active_index.is_some(), total, roomy);
    AdaptiveLayout { widths, tabs: visible, before, after, start, total }
}

/// The final per-tab widths: roomy layouts split `total` evenly (the
/// remainder to the earliest tabs); cramped layouts give the active tab
/// the wider slot and shrink the rest evenly (the reference
/// `adaptiveSessionTabLayout` tail).
fn distribute_widths(
    visible: &[SessionTab],
    active: Option<&str>,
    has_active: bool,
    total: usize,
    roomy: bool,
) -> Vec<usize> {
    if roomy || !has_active {
        let width = total / visible.len();
        let remainder = total - width * visible.len();
        return visible
            .iter()
            .enumerate()
            .map(|(index, _)| width + usize::from(index < remainder))
            .collect();
    }
    let inactive_width = if visible.len() == 1 {
        0
    } else {
        SESSION_TAB_WIDTH.min(
            SESSION_TAB_MIN_WIDTH.max((total - SESSION_TAB_WIDTH.min(total)) / (visible.len() - 1)),
        )
    };
    let active_width =
        if visible.len() == 1 { total } else { total - inactive_width * (visible.len() - 1) };
    visible
        .iter()
        .map(
            |tab| {
                if Some(tab.session_id.as_str()) == active {
                    active_width
                } else {
                    inactive_width
                }
            },
        )
        .collect()
}

/// The windowing solver (`solve` in the reference): clamp the start so
/// the active tab stays visible, re-fit against the overflow markers,
/// up to `attempts` rounds.
#[allow(clippy::too_many_arguments)]
fn solve(
    len: usize,
    active_index: Option<usize>,
    available: usize,
    count: usize,
    start: usize,
    attempts: u32,
) -> (usize, usize) {
    let bounded_start = start.min(len - count);
    let next_start = {
        let mut next = bounded_start;
        if let Some(active) = active_index {
            if active < bounded_start {
                next = active;
            } else if active >= bounded_start + count {
                next = active + 1 - count;
            }
        }
        next.min(len - count)
    };
    let markers = (if next_start > 0 { session_tab_overflow_width(next_start) } else { 0 })
        + (if next_start + count < len {
            session_tab_overflow_width(len - next_start - count)
        } else {
            0
        });
    let next_count = fit_for(available.saturating_sub(markers), len, active_index);
    if next_count == count || attempts == 0 {
        return (count, next_start);
    }
    solve(len, active_index, available, next_count, next_start, attempts - 1)
}

fn fit_for(width: usize, len: usize, active_index: Option<usize>) -> usize {
    let base = match active_index {
        None => width / SESSION_TAB_MIN_WIDTH,
        Some(_) => 1 + width.saturating_sub(SESSION_TAB_WIDTH) / SESSION_TAB_MIN_WIDTH,
    };
    len.min(base.max(1))
}

/// Inputs the renderers need per tab.
#[derive(Debug, Clone)]
pub struct TabView {
    pub tab: SessionTab,
    pub status: TabStatus,
    /// Detail line under the title (rail) — `project ⎇ branch` shape.
    pub detail: Option<String>,
}

/// What a pointer column over the strip addresses — the hit-test
/// mirror of the strip renderer's geometry (beta.8 bug 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripHit {
    /// The tab body (click = select; `release()` → `tabs.select`,
    /// `session-tabs.tsx:1505-1514`).
    Tab(String),
    /// The revealed `✕` affordance cell on a HOVERED tab ("the close
    /// glyph sits one cell in from the right edge: x = tab start +
    /// width − 2", `session-tabs.tsx:264`; the click path is
    /// `tabs.close`, `session-tabs.tsx:1745-1754`). Glyph-tight like
    /// the ref — the `✕` is a 1-cell text renderable.
    Close(String),
    /// The `+` glyph cell of the `" + "` affordance (`tabs.add?.()` —
    /// navigates home, `session-tabs.tsx:1786-1792` +
    /// `session-tabs.tsx:406-416`). GLYPH-TIGHT: opentui hit-tests the
    /// 3-cell text renderable per glyph, so the surrounding padding
    /// cells are NOT part of the zone (corpus2 #28).
    Add,
    /// Outside every control.
    Outside,
}

/// Resolve pointer column `col` (strip-local) against the strip's
/// layout. `hover` gates the close cell exactly like v2: the mark
/// only renders on the hovered tab, and a release elsewhere selects.
#[must_use]
pub fn strip_hit(
    tabs: &[SessionTab],
    active: Option<&str>,
    show_plus: bool,
    width: usize,
    col: usize,
    hover: Option<&str>,
) -> StripHit {
    let layout = adaptive_session_tab_layout(
        tabs,
        active,
        width.saturating_sub(if show_plus { ADD_TAB_WIDTH } else { 0 }),
        0,
    );
    let mut x = if layout.before > 0 { session_tab_overflow_width(layout.before) } else { 0 };
    for (index, tab) in layout.tabs.iter().enumerate() {
        let tab_width = layout.widths[index];
        if col < x + tab_width {
            // The close cell: one in from the right edge ("x = tab
            // start + width − 2"), only while this tab is hovered.
            if hover == Some(tab.session_id.as_str()) && col + 2 == x + tab_width {
                return StripHit::Close(tab.session_id.clone());
            }
            return StripHit::Tab(tab.session_id.clone());
        }
        x += tab_width;
    }
    if layout.after > 0 {
        let after_width = session_tab_overflow_width(layout.after);
        if col < x + after_width {
            return StripHit::Outside;
        }
        x += after_width;
    }
    if show_plus && col >= x && col < x + ADD_TAB_WIDTH {
        // Glyph-tight: the affordance is the ` + ` TEXT renderable
        // (`session-tabs.tsx:1786-1801`) and opentui hit-tests text
        // elements per GLYPH cell — the `" + "` padding cells show no
        // affordance and take no click (corpus2 #28: padding clicks
        // navigated home on our side only, ref reacted nowhere). Only
        // the center `+` cell is the zone.
        if col == x + 1 {
            return StripHit::Add;
        }
    }
    StripHit::Outside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: &str, title: &str, status: TabStatus) -> TabView {
        TabView {
            tab: SessionTab { session_id: id.to_owned(), title: Some(title.to_owned()) },
            status,
            detail: Some("project".to_owned()),
        }
    }

    fn golden_tabs() -> Vec<TabView> {
        vec![
            view(
                "first",
                "First",
                TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY },
            ),
            view("second", "Second", TabStatus::EMPTY),
        ]
    }

    #[test]
    fn adaptive_layout_matches_the_reference_solver() {
        // Golden scenario at 80: two 32-wide tabs after the 3-wide plus.
        let tabs: Vec<SessionTab> = golden_tabs().iter().map(|v| v.tab.clone()).collect();
        let layout = adaptive_session_tab_layout(&tabs, Some("second"), 77, 0);
        assert_eq!(layout.widths, vec![32, 32]);
        assert_eq!(layout.before, 0);
        assert_eq!(layout.after, 0);
        // Active-clamped windowing: many tabs, narrow strip keeps the
        // active tab visible and reports overflow markers.
        let many: Vec<SessionTab> = (0..12)
            .map(|i| SessionTab { session_id: format!("t{i}"), title: Some(format!("T{i}")) })
            .collect();
        let layout = adaptive_session_tab_layout(&many, Some("t10"), 40, 0);
        assert!(layout.before + layout.after > 0, "overflow markers appear");
        let visible_active =
            layout.tabs.iter().position(|tab| tab.session_id == "t10").expect("active tab visible");
        assert!(visible_active < layout.tabs.len());
        // Roomy case distributes the remainder to the earliest tabs.
        let layout = adaptive_session_tab_layout(&tabs[..1], Some("first"), 70, 0);
        assert_eq!(layout.widths, vec![32], "capped at SESSION_TAB_MAX_WIDTH");
    }

    #[test]
    fn indicator_labels_follow_the_reference_order() {
        assert_eq!(
            TabStatus { attention: Some(Attention::Permission), busy: true, ..TabStatus::EMPTY }
                .indicator_label(),
            "!"
        );
        assert_eq!(
            TabStatus { attention: Some(Attention::Question), busy: true, ..TabStatus::EMPTY }
                .indicator_label(),
            "?"
        );
        assert_eq!(TabStatus { busy: true, ..TabStatus::EMPTY }.indicator_label(), SPINNER_FRAME_0);
        assert_eq!(
            TabStatus { unread: Some(Unread::Error), ..TabStatus::EMPTY }.indicator_label(),
            UNREAD_MARKER
        );
        assert_eq!(TabStatus::EMPTY.indicator_label(), "");
        assert!(
            !TabStatus { busy: true, attention: Some(Attention::Question), ..TabStatus::EMPTY }
                .runs(),
            "attention stops the spinner"
        );
        assert!(TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY }.glows(false));
        assert!(!TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY }.glows(true));
    }

    /// beta.8 bug 3 — the pointer geometry: the body selects, the
    /// `✕` cell (one in from the hovered tab's right edge) closes,
    /// the ` + ` adds. Mirrors the strip layout solver exactly.
    #[test]
    fn strip_hit_maps_pointer_columns_to_controls() {
        let tabs: Vec<SessionTab> = vec![
            SessionTab { session_id: "s1".into(), title: Some("Alpha".into()) },
            SessionTab { session_id: "s2".into(), title: Some("Beta".into()) },
        ];
        // Two 32-wide tabs + " + " at 80: tab1 0..32, tab2 32..64,
        // plus 64..67.
        assert_eq!(strip_hit(&tabs, Some("s1"), true, 80, 5, None), StripHit::Tab("s1".into()));
        assert_eq!(strip_hit(&tabs, Some("s1"), true, 80, 40, None), StripHit::Tab("s2".into()));
        assert_eq!(strip_hit(&tabs, Some("s1"), true, 80, 65, None), StripHit::Add);
        assert_eq!(strip_hit(&tabs, Some("s1"), true, 80, 70, None), StripHit::Outside);
        // Corpus2 #28 — the Add zone is GLYPH-TIGHT: the `" + "`
        // padding cells (64 and 66) take no click on the ref; our old
        // full-box zone navigated home on invisible affordances.
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 64, None),
            StripHit::Outside,
            "the left ` + ` padding cell is not a control"
        );
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 66, None),
            StripHit::Outside,
            "the right ` + ` padding cell is not a control"
        );
        // The close cell: x = tab start + width − 2, ONLY while that
        // tab is hovered.
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 62, Some("s2")),
            StripHit::Close("s2".into())
        );
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 62, Some("s1")),
            StripHit::Tab("s2".into()),
            "no close mark on an unhovered tab — the release selects"
        );
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 30, Some("s1")),
            StripHit::Close("s1".into()),
            "the active tab's own mark closes it too"
        );
        // The ✕ zone is exactly ONE cell (the glyph): its neighbors
        // select the tab even while hovered (session-tabs.tsx:264 —
        // the mark is a 1-cell text renderable; no padding disease).
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 61, Some("s2")),
            StripHit::Tab("s2".into()),
            "the cell left of the ✕ selects"
        );
        assert_eq!(
            strip_hit(&tabs, Some("s1"), true, 80, 63, Some("s2")),
            StripHit::Tab("s2".into()),
            "the cell right of the ✕ selects"
        );
    }
}
