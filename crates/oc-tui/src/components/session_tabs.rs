//! Session tabs — the horizontal bar and vertical rail, ported from
//! `component/session-tabs.tsx` + `context/session-tabs-model.ts` for the
//! golden-matched subset (animations disabled, status indicators,
//! adaptive windowing, the add slot, the unread-glow separator rows).
//!
//! Golden scope (`session-tabs-{horizontal,vertical}` @80/160): two
//! tabs, first carries an unread-activity indicator, second is active,
//! detail line "project", the add affordance present. The full component
//! surface — marquee scrolling, context menus, drag reordering, motion
//! (width springs), and per-cell glow/gradient backgrounds — lands with
//! later waves; text-grid parity is what this wave pins.

// Tab widths/gaps are terminal dimensions (far below 2^23), so the
// pedantic usize→f32 cast lints carry no real risk here.
#![allow(clippy::cast_precision_loss)]

use crate::layout::{BoxSpec, LayoutTree, Placed, PlacedLine, StyledSpan};
use crate::render::{paint_placed, Frame};
use crate::store::{SessionTab, Unread};
use crate::theme::ThemeStyles;
use crate::wrap::take_width;
use ratatui::style::Style;
use taffy::AvailableSpace;

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

/// The horizontal tab strip (height 1). Renders `[overflow-left] tabs…
/// [overflow-right] " + "` like `HorizontalSessionTabs` with animations
/// off; returns the placed runs plus the strip width consumed.
#[must_use]
pub fn render_bar(
    tabs: &[TabView],
    active: Option<&str>,
    show_plus: bool,
    available: usize,
) -> Vec<Placed> {
    let owned: Vec<SessionTab> = tabs.iter().map(|view| view.tab.clone()).collect();
    let layout = adaptive_session_tab_layout(
        &owned,
        active,
        available.saturating_sub(if show_plus { ADD_TAB_WIDTH } else { 0 }),
        0,
    );
    let number_width = tabs.len().to_string().len().max(2);
    let style = Style::new();
    let mut tree = LayoutTree::new();
    let mut children = Vec::new();
    if layout.before > 0 {
        children.push(tree.text_sized(
            &format!("‹{}", layout.before),
            style,
            &BoxSpec::default().width(session_tab_overflow_width(layout.before) as f32).shrink(0.0),
        ));
    }
    // The visible window of the adaptive layout parallels `tabs` in
    // order (`layout.tabs` is its clone) — iterate the source views.
    let window: &[TabView] = &tabs[layout.start..layout.start + layout.tabs.len()];
    for (index, view) in window.iter().enumerate() {
        let width = layout.widths[index];
        let glyph = view.status.indicator_label();
        // The indicator glyph renders right-aligned in a (numberWidth+1)
        // box with one trailing pad; an empty label leaves the reserved
        // slot blank (numbers mode paints ordinals — 7.2).
        let indicator = if glyph.is_empty() {
            tree.box_node(
                &BoxSpec::default().width((number_width + 1) as f32).shrink(0.0).padding_right(1.0),
                &[],
            )
        } else {
            let leaf = tree.text_sized(glyph, style, &BoxSpec::default().shrink(0.0));
            tree.box_node(
                &BoxSpec::default()
                    .width((number_width + 1) as f32)
                    .shrink(0.0)
                    .padding_right(1.0)
                    .justify_end(),
                &[leaf],
            )
        };
        let title = view.tab.title.clone().unwrap_or_else(|| "Untitled session".into());
        let title_width = width.saturating_sub(1 + number_width).max(1);
        // Styled-segment leaf (single span): the trivially-equivalent
        // migration proving the styled path subsumes plain leaves.
        let title_leaf = tree.text_styled(
            vec![StyledSpan { text: take_width(&title, title_width), style }],
            &BoxSpec::default().width(title_width as f32).shrink(0.0),
        );
        children.push(tree.box_node(
            &BoxSpec::default().width(width as f32).shrink(0.0),
            &[indicator, title_leaf],
        ));
    }
    if layout.after > 0 {
        children.push(tree.text_sized(
            &format!(" {}›", layout.after),
            style,
            &BoxSpec::default().width(session_tab_overflow_width(layout.after) as f32).shrink(0.0),
        ));
    }
    if show_plus {
        children.push(tree.text_sized(
            " + ",
            style,
            &BoxSpec::default().width(ADD_TAB_WIDTH as f32).shrink(0.0),
        ));
    }
    let strip = tree.box_node(&BoxSpec::row().height(1.0), &children);
    tree.set_root(strip);
    tree.compute((available as f32, AvailableSpace::Definite(1.0))).placed
}

/// The vertical rail: padding-top 1, one 2-row tab block per session
/// (title row + detail row, gap 1), the `+ New session` slot, and the
/// unread-glow separator rows (▄ × `INDICATOR_WIDTH`) above a tab whose
/// predecessor glows (`TabPulse` static rendering with animations off).
#[must_use]
pub fn render_rail(
    tabs: &[TabView],
    active: Option<&str>,
    show_add: bool,
    width: usize,
) -> Vec<Placed> {
    let compact = width < SESSION_TABS_COMPACT_BREAKPOINT;
    let style = Style::new();
    let number_width = tabs.len().to_string().len().max(2);
    let prefix_width = number_width + 1;
    let title_width = (width.saturating_sub(prefix_width + 1)).max(1);
    let mut tree = LayoutTree::new();
    let mut blocks = Vec::new();
    for (index, view) in tabs.iter().enumerate() {
        let glyph = view.status.indicator_label();
        let indicator = if glyph.is_empty() {
            tree.box_node(
                &BoxSpec::default().width((number_width + 1) as f32).shrink(0.0).padding_right(1.0),
                &[],
            )
        } else {
            let leaf = tree.text_sized(glyph, style, &BoxSpec::default().shrink(0.0));
            tree.box_node(
                &BoxSpec::default()
                    .width((number_width + 1) as f32)
                    .shrink(0.0)
                    .padding_right(1.0)
                    .justify_end(),
                &[leaf],
            )
        };
        let title = view.tab.title.clone().unwrap_or_else(|| "Untitled session".into());
        // Styled-segment leaf carrying the placement tag (single span —
        // trivially-equivalent styled migration).
        let title_leaf = tree.text_styled_tagged(
            vec![StyledSpan { text: take_width(&title, title_width), style }],
            format!("tab-title:{index}"),
        );
        let title_box =
            tree.box_node(&BoxSpec::default().width(title_width as f32).shrink(0.0), &[title_leaf]);
        let title_row =
            tree.box_node(&BoxSpec::row().height(1.0).padding_right(1.0), &[indicator, title_box]);
        let detail = view.detail.clone().unwrap_or_default();
        let detail_leaf = tree.text_styled(
            vec![StyledSpan { text: take_width(&detail, title_width), style }],
            &BoxSpec::default().shrink(0.0),
        );
        let detail_row = tree.box_node(
            &BoxSpec::row().height(1.0).padding_left(prefix_width as f32).padding_right(2.0),
            &[detail_leaf],
        );
        blocks.push(tree.box_node(
            &BoxSpec::column().height(if compact { 1.0 } else { 2.0 }),
            &[title_row, detail_row],
        ));
    }
    if show_add {
        let plus = tree.text_sized("+", style, &BoxSpec::default().width(2.0).shrink(0.0));
        let label = tree.text_sized(NEW_SESSION_TAB_TITLE, style, &BoxSpec::default().shrink(0.0));
        blocks.push(tree.box_node(
            &BoxSpec::row().height(1.0).padding_left(if compact { 0.0 } else { 1.0 }),
            &[plus, label],
        ));
    }
    let rail =
        tree.box_node(&BoxSpec::column().width(width as f32).padding_top(1.0).gap(1.0), &blocks);
    tree.set_root(rail);
    let mut placed = tree.compute((width as f32, AvailableSpace::MaxContent)).placed;
    // Separator rows: above tab i when tab i-1 glows (the golden's ▄ row
    // between First and Second).
    for index in 1..tabs.len() {
        let previous = &tabs[index - 1];
        let previous_selected = Some(previous.tab.session_id.as_str()) == active;
        if previous.status.glows(previous_selected) {
            if let Some(title) =
                placed.iter().find(|p| p.tag.as_deref() == Some(&format!("tab-title:{index}")))
            {
                let y = title.y.saturating_sub(1);
                placed.push(Placed {
                    x: 0,
                    y,
                    width: INDICATOR_WIDTH,
                    lines: vec![PlacedLine::plain(
                        SEPARATOR_GLYPH.to_string().repeat(INDICATOR_WIDTH),
                        style,
                    )],
                    tag: Some(format!("separator:{index}")),
                });
            }
        }
    }
    placed
}

/// Paint the horizontal strip frame (golden scenario shape).
#[must_use]
pub fn paint_bar(
    tabs: &[TabView],
    active: Option<&str>,
    show_plus: bool,
    size: (u16, u16),
    theme: &ThemeStyles,
) -> Frame {
    let placed = render_bar(tabs, active, show_plus, usize::from(size.0));
    paint_placed(&placed, size, Style::new().bg(theme.background))
}

/// Paint the vertical rail frame (golden scenario shape).
#[must_use]
pub fn paint_rail(
    tabs: &[TabView],
    active: Option<&str>,
    show_add: bool,
    size: (u16, u16),
    theme: &ThemeStyles,
) -> Frame {
    let width = SESSION_SIDEBAR_WIDTH.min(usize::from(size.0));
    let placed = render_rail(tabs, active, show_add, width);
    paint_placed(&placed, size, Style::new().bg(theme.background_raised))
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
}
