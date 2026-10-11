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

use crate::components::tab_pulse::{glow_text_color, resting_glow_bg, tint, Rgb, GLOW_TAIL};
use crate::layout::{BoxSpec, LayoutTree, Placed, PlacedLine, StyledSpan};
use crate::render::{paint_placed_with_fills, Fill, Frame};
use crate::store::{SessionTab, Unread};
use crate::theme::ThemeStyles;
use crate::wrap::take_width;
use ratatui::style::{Color, Style};
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

/// A renderer's output: placed text runs plus the background fills
/// they sit on (pulse gradients arrive as fills; the frame base fill
/// is applied by the painter).
#[derive(Debug, Default)]
pub struct TabRender {
    pub placed: Vec<Placed>,
    pub fills: Vec<Fill>,
}

/// Merge consecutive 1-row cells with equal style into rect fills.
fn fill_runs(y: u16, cells: &[Style], x0: u16) -> Vec<Fill> {
    let mut fills = Vec::new();
    let mut start = 0usize;
    for index in 1..=cells.len() {
        if index == cells.len() || cells[index] != cells[start] {
            fills.push(Fill {
                x: x0 + start as u16,
                y,
                width: (index - start) as u16,
                height: 1,
                style: cells[start],
            });
            start = index;
        }
    }
    fills
}

fn rgb(color: Color) -> Rgb {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

fn color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// `tabFeedbackColor ?? unreadColor` — the hue a glow renders in.
fn glow_hue(status: TabStatus, theme: &ThemeStyles) -> Rgb {
    if status.attention.is_none() && status.unread == Some(Unread::Error) {
        rgb(theme.error)
    } else {
        rgb(theme.accent)
    }
}

/// The glow color each surface derives from its pulse background:
/// the bar tints with `glowLevel()` (1) as the alpha; the rail's title
/// pulse uses 0.45, detail 0.25, separator outer 0.1.
fn glow_over(background: Rgb, hue: Rgb, alpha: f64) -> Rgb {
    tint(background, hue, alpha)
}

/// The horizontal tab strip (height 1). Renders `[overflow-left] tabs…
/// [overflow-right] " + "` like `HorizontalSessionTabs` with animations
/// off; the unread tab paints its full-width resting pulse (per-cell
/// backgrounds) and per-character tinted title.
///
/// Pointer state (beta.8 bug 3): `hover` names the tab under the
/// pointer — v2's `hovered()` lifts a NON-selected tab to
/// `background.action.primary.hovered`, turns the title
/// `text.base`, shrinks the title two cells and reveals the `✕`
/// close mark one cell in from the right edge
/// (`session-tabs.tsx:1570-1575`, `1588-1591`, `1731-1757`);
/// `add_hovered` highlights the ` + ` affordance
/// (`session-tabs.tsx:1768-1796`).
#[must_use]
pub fn render_bar(
    tabs: &[TabView],
    active: Option<&str>,
    show_plus: bool,
    available: usize,
    theme: &ThemeStyles,
    hover: Option<&str>,
    add_hovered: bool,
) -> TabRender {
    let owned: Vec<SessionTab> = tabs.iter().map(|view| view.tab.clone()).collect();
    let layout = adaptive_session_tab_layout(
        &owned,
        active,
        available.saturating_sub(if show_plus { ADD_TAB_WIDTH } else { 0 }),
        0,
    );
    let number_width = tabs.len().to_string().len().max(2);
    // `background()`: tint(base, decrease(raised.base), selection) —
    // `decrease` of the gray raised surface resolves to raised.high
    // (30,30,30) in v2-dark; `ThemeStyles::background_prompt` carries
    // exactly that decrease for the composer, reuse it.
    let selected_bg = theme.background_prompt;
    let mut tree = LayoutTree::new();
    let mut children = Vec::new();
    let mut fills = Vec::new();
    if layout.before > 0 {
        children.push(tree.text_sized(
            &format!("‹{}", layout.before),
            Style::new().fg(theme.text_muted.fg.unwrap_or_default()).bg(theme.background),
            &BoxSpec::default().width(session_tab_overflow_width(layout.before) as f32).shrink(0.0),
        ));
    }
    // The visible window of the adaptive layout parallels `tabs` in
    // order (`layout.tabs` is its clone) — iterate the source views.
    let window: &[TabView] = &tabs[layout.start..layout.start + layout.tabs.len()];
    let mut tab_x = if layout.before > 0 { session_tab_overflow_width(layout.before) } else { 0 };
    let mut close_marks = Vec::new();
    for (index, view) in window.iter().enumerate() {
        let width = layout.widths[index];
        let hovered = hover == Some(view.tab.session_id.as_str());
        bar_tab_fills(&mut fills, view, active, theme, tab_x, width, selected_bg, hovered);
        children.push(bar_tab_node(
            &mut tree,
            view,
            active,
            theme,
            width,
            number_width,
            selected_bg,
            hovered,
        ));
        // The revealed `✕` mark: absolute `right={1}` — one cell in
        // from the tab's right edge (v2 renders it only on hover).
        if hovered && width >= 2 {
            close_marks.push(close_mark(tab_x + width - 2, theme, active, &view.tab.session_id));
        }
        tab_x += width;
    }
    let mut plus_x = tab_x;
    if layout.after > 0 {
        children.push(tree.text_sized(
            &format!(" {}›", layout.after),
            Style::new().fg(theme.text_muted.fg.unwrap_or_default()).bg(theme.background),
            &BoxSpec::default().width(session_tab_overflow_width(layout.after) as f32).shrink(0.0),
        ));
        plus_x += session_tab_overflow_width(layout.after);
    }
    let strip = tree.box_node(&BoxSpec::row().height(1.0), &children);
    tree.set_root(strip);
    let mut placed = tree.compute((available as f32, AvailableSpace::Definite(1.0))).placed;
    placed.extend(close_marks);
    if show_plus {
        placed.push(add_affordance(plus_x, theme, add_hovered));
    }
    TabRender { placed, fills }
}

/// The `✕` close glyph as a one-cell placed run (`closeColor =
/// tint(text.muted, text.base, 0.6)`, `session-tabs.tsx:1645`), over
/// the tab's own background (lifted when hovered, selected pill when
/// the hovered tab is the active one).
fn close_mark(x: usize, theme: &ThemeStyles, active: Option<&str>, session_id: &str) -> Placed {
    let bg =
        if Some(session_id) == active { theme.background_prompt } else { theme.action_hovered };
    let close_color = tint(
        rgb(theme.text_muted.fg.unwrap_or(Color::Rgb(128, 128, 128))),
        rgb(theme.text.fg.unwrap_or(Color::Rgb(255, 255, 255))),
        0.6,
    );
    Placed {
        x,
        y: 0,
        width: 1,
        lines: vec![PlacedLine {
            runs: vec![StyledSpan {
                text: "✕".to_owned(),
                style: Style::new()
                    .fg(Color::Rgb(close_color.0, close_color.1, close_color.2))
                    .bg(bg),
            }],
        }],
        tag: Some(format!("session-tabs.close:{session_id}")),
    }
}

/// One bar tab's background fills: the resting pulse per cell while
/// glowing, else the flat tab background. A hovered NON-selected tab
/// lifts to `background.action.primary.hovered`
/// (`session-tabs.tsx:1570-1572`).
#[allow(clippy::too_many_arguments)]
fn bar_tab_fills(
    fills: &mut Vec<Fill>,
    view: &TabView,
    active: Option<&str>,
    theme: &ThemeStyles,
    tab_x: usize,
    width: usize,
    selected_bg: Color,
    hovered: bool,
) {
    let selected = Some(view.tab.session_id.as_str()) == active;
    let tab_bg = if selected {
        selected_bg
    } else if hovered {
        theme.action_hovered
    } else {
        theme.background
    };
    if !view.status.glows(selected) {
        fills.push(Fill {
            x: tab_x as u16,
            y: 0,
            width: width as u16,
            height: 1,
            style: Style::new().bg(tab_bg),
        });
        return;
    }
    let glow = glow_over(rgb(tab_bg), glow_hue(view.status, theme), 1.0);
    let cells: Vec<Style> = (0..width)
        .map(|cell| {
            Style::new().fg(Color::Reset).bg(color(resting_glow_bg(
                rgb(tab_bg),
                glow,
                cell,
                width,
                GLOW_TAIL,
                1.0,
            )))
        })
        .collect();
    fills.extend(fill_runs(0, &cells, tab_x as u16));
}

/// One bar tab's layout node: the indicator glyph and the title runs
/// (per-character glow tints while glowing — `characterColor`). A
/// hovered tab's title renders `text.base` and shrinks two cells to
/// make room for the revealed `✕` (`session-tabs.tsx:1588-1591`,
/// `1611-1613`).
#[allow(clippy::too_many_arguments)]
fn bar_tab_node(
    tree: &mut LayoutTree,
    view: &TabView,
    active: Option<&str>,
    theme: &ThemeStyles,
    width: usize,
    number_width: usize,
    selected_bg: Color,
    hovered: bool,
) -> taffy::NodeId {
    let selected = Some(view.tab.session_id.as_str()) == active;
    let tab_bg = if selected {
        selected_bg
    } else if hovered {
        theme.action_hovered
    } else {
        theme.background
    };
    let glows = view.status.glows(selected);
    let hue = glow_hue(view.status, theme);
    let glow = glow_over(rgb(tab_bg), hue, 1.0);
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
        // The glyph right-aligns at column numberWidth − 1.
        let bg_at_glyph = if glows {
            color(resting_glow_bg(
                rgb(tab_bg),
                glow,
                number_width.saturating_sub(1),
                width,
                GLOW_TAIL,
                1.0,
            ))
        } else {
            tab_bg
        };
        let fg = if view.status.unread.is_some() || view.status.attention.is_some() {
            Color::Rgb(hue.0, hue.1, hue.2)
        } else {
            theme.text.fg.unwrap_or(Color::Rgb(255, 255, 255))
        };
        let leaf = tree.text_sized(
            glyph,
            Style::new().fg(fg).bg(bg_at_glyph),
            &BoxSpec::default().shrink(0.0),
        );
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
    // `restingTitleWidth` = width − 1 − numberWidth; hovering shifts
    // the right bound left of the close mark (− 2).
    let title_width = width.saturating_sub(1 + number_width + usize::from(hovered) * 2).max(1);
    let title_text = take_width(&title, title_width);
    let foreground = if selected || hovered {
        theme.text.fg.unwrap_or(Color::Rgb(255, 255, 255))
    } else {
        theme.text_muted.fg.unwrap_or(Color::Rgb(128, 128, 128))
    };
    let bold =
        if selected { ratatui::style::Modifier::BOLD } else { ratatui::style::Modifier::empty() };
    let spans: Vec<StyledSpan> = if glows {
        title_text
            .chars()
            .enumerate()
            .map(|(cell, ch)| {
                let column = 1 + number_width + cell;
                StyledSpan {
                    text: ch.to_string(),
                    style: Style::new()
                        .fg(color(glow_text_color(rgb(foreground), glow, column, width, 1.0)))
                        .bg(color(resting_glow_bg(
                            rgb(tab_bg),
                            glow,
                            column,
                            width,
                            GLOW_TAIL,
                            1.0,
                        )))
                        .add_modifier(bold),
                }
            })
            .collect()
    } else {
        vec![StyledSpan {
            text: title_text,
            style: Style::new().fg(foreground).bg(tab_bg).add_modifier(bold),
        }]
    };
    let title_leaf =
        tree.text_styled(spans, &BoxSpec::default().width(title_width as f32).shrink(0.0));
    tree.box_node(&BoxSpec::default().width(width as f32).shrink(0.0), &[indicator, title_leaf])
}

/// The `" + "` affordance, placed directly (the wrap seam eats the
/// trailing pad space of a tree leaf at some widths — 160 — and the
/// golden bytes pin it verbatim). Hovered: `text.base` over
/// `background.action.primary.hovered` (`session-tabs.tsx:1771-1772`).
fn add_affordance(x: usize, theme: &ThemeStyles, hovered: bool) -> Placed {
    let (fg, bg) = if hovered {
        (theme.text.fg.unwrap_or_default(), theme.action_hovered)
    } else {
        (theme.text_muted.fg.unwrap_or_default(), theme.background)
    };
    Placed {
        x,
        y: 0,
        width: ADD_TAB_WIDTH,
        lines: vec![PlacedLine {
            runs: vec![StyledSpan { text: " + ".to_owned(), style: Style::new().fg(fg).bg(bg) }],
        }],
        tag: Some("session-tabs.add".to_owned()),
    }
}

/// The vertical rail: padding-top 1, one 2-row tab block per session
/// (title row + detail row, gap 1), the `+ New session` slot, and the
/// unread-glow separator rows (▄ × `INDICATOR_WIDTH`) above a tab whose
/// predecessor glows — each surface with its own resting pulse
/// (title 0.45/tail 8, detail 0.25/tail 8, separator outer 0.1/tail 5).
#[must_use]
pub fn render_rail(
    tabs: &[TabView],
    active: Option<&str>,
    show_add: bool,
    width: usize,
    theme: &ThemeStyles,
) -> TabRender {
    let compact = width < SESSION_TABS_COMPACT_BREAKPOINT;
    let number_width = tabs.len().to_string().len().max(2);
    let prefix_width = number_width + 1;
    let title_width = (width.saturating_sub(prefix_width + 1)).max(1);
    let rail_bg = theme.background_raised;
    let muted = theme.text_muted.fg.unwrap_or(Color::Rgb(128, 128, 128));
    let mut tree = LayoutTree::new();
    let mut blocks = Vec::new();
    for (index, view) in tabs.iter().enumerate() {
        blocks.push(rail_tab_block(
            &mut tree,
            view,
            index,
            active,
            theme,
            width,
            number_width,
            prefix_width,
            title_width,
            compact,
        ));
    }
    if show_add {
        blocks.push(rail_add_row(&mut tree, theme, rail_bg, muted, compact));
    }
    let rail =
        tree.box_node(&BoxSpec::column().width(width as f32).padding_top(1.0).gap(1.0), &blocks);
    tree.set_root(rail);
    let mut placed = tree.compute((width as f32, AvailableSpace::MaxContent)).placed;
    let fills = rail_pulse_fills(tabs, active, theme, &placed);
    rail_separators(tabs, active, theme, &mut placed);
    TabRender { placed, fills }
}

/// One rail tab's 2-row block: the indicator + tinted title row, the
/// detail row over its pulse. One function per surface row keeps the
/// reference's own row-at-a-time shape readable.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn rail_tab_block(
    tree: &mut LayoutTree,
    view: &TabView,
    index: usize,
    active: Option<&str>,
    theme: &ThemeStyles,
    width: usize,
    number_width: usize,
    prefix_width: usize,
    title_width: usize,
    compact: bool,
) -> taffy::NodeId {
    let selected = Some(view.tab.session_id.as_str()) == active;
    let glows = view.status.glows(selected);
    let hue = glow_hue(view.status, theme);
    let rail_bg = theme.background_raised;
    let text_base = theme.text.fg.unwrap_or(Color::Rgb(255, 255, 255));
    let muted = theme.text_muted.fg.unwrap_or(Color::Rgb(128, 128, 128));
    // The pulse backgrounds sit on `pulseBackground` — tint(bg,
    // tabBackground, tabBackground.a); the selected rail tab's
    // action surface is transparent (a=0), so every golden state
    // resolves to the rail background.
    let pulse_bg = rgb(rail_bg);
    let title_glow = glow_over(pulse_bg, hue, 0.45);
    let detail_glow = glow_over(pulse_bg, hue, 0.25);
    let bold =
        if selected { ratatui::style::Modifier::BOLD } else { ratatui::style::Modifier::empty() };
    let glyph = view.status.indicator_label();
    let indicator = if glyph.is_empty() {
        tree.box_node(
            &BoxSpec::default().width((number_width + 1) as f32).shrink(0.0).padding_right(1.0),
            &[],
        )
    } else {
        let fg = if view.status.unread.is_some() || view.status.attention.is_some() {
            Color::Rgb(hue.0, hue.1, hue.2)
        } else {
            muted
        };
        // The glyph right-aligns at column numberWidth − 1.
        let bg = if glows {
            color(resting_glow_bg(
                pulse_bg,
                title_glow,
                number_width.saturating_sub(1),
                INDICATOR_WIDTH,
                GLOW_TAIL,
                1.0,
            ))
        } else {
            rail_bg
        };
        let leaf =
            tree.text_sized(glyph, Style::new().fg(fg).bg(bg), &BoxSpec::default().shrink(0.0));
        tree.box_node(
            &BoxSpec::default()
                .width((number_width + 1) as f32)
                .shrink(0.0)
                .padding_right(1.0)
                .justify_end(),
            &[leaf],
        )
    };
    // `titleColor`: per-character glow tints while glowing (level 1 —
    // the eased `titleGlow` rests at 1 with animations off), flat
    // otherwise.
    let title = view.tab.title.clone().unwrap_or_else(|| "Untitled session".into());
    let title_text = take_width(&title, title_width);
    let foreground = if selected { text_base } else { muted };
    let spans: Vec<StyledSpan> = if glows && !compact {
        title_text
            .chars()
            .enumerate()
            .map(|(cell, ch)| {
                let column = 1 + number_width + cell;
                StyledSpan {
                    text: ch.to_string(),
                    style: Style::new()
                        .fg(color(glow_text_color(rgb(foreground), title_glow, column, width, 1.0)))
                        .bg(color(resting_glow_bg(
                            pulse_bg,
                            title_glow,
                            column,
                            INDICATOR_WIDTH,
                            GLOW_TAIL,
                            1.0,
                        )))
                        .add_modifier(bold),
                }
            })
            .collect()
    } else {
        vec![StyledSpan {
            text: title_text,
            style: Style::new().fg(foreground).bg(rail_bg).add_modifier(bold),
        }]
    };
    // Styled-segment leaf carrying the placement tag.
    let title_leaf = tree.text_styled_tagged(spans, format!("tab-title:{index}"));
    let title_box =
        tree.box_node(&BoxSpec::default().width(title_width as f32).shrink(0.0), &[title_leaf]);
    let title_row =
        tree.box_node(&BoxSpec::row().height(1.0).padding_right(1.0), &[indicator, title_box]);
    // `detailColor` = tint(muted, pulseBackground, 0.35); the
    // per-character glow applies only while detail-fading (marquee) —
    // flat here, over the detail pulse cells.
    let detail = view.detail.clone().unwrap_or_default();
    let detail_fg = color(tint(rgb(muted), pulse_bg, 0.35));
    let detail_text = take_width(&detail, title_width);
    let detail_spans: Vec<StyledSpan> = if glows && !compact {
        detail_text
            .chars()
            .enumerate()
            .map(|(cell, ch)| StyledSpan {
                text: ch.to_string(),
                style: Style::new().fg(detail_fg).bg(color(resting_glow_bg(
                    pulse_bg,
                    detail_glow,
                    prefix_width + cell,
                    INDICATOR_WIDTH,
                    10.0,
                    1.0,
                ))),
            })
            .collect()
    } else {
        vec![StyledSpan { text: detail_text, style: Style::new().fg(detail_fg).bg(rail_bg) }]
    };
    let detail_leaf = tree.text_styled(detail_spans, &BoxSpec::default().shrink(0.0));
    let detail_row = tree.box_node(
        &BoxSpec::row().height(1.0).padding_left(prefix_width as f32).padding_right(2.0),
        &[detail_leaf],
    );
    tree.box_node(
        &BoxSpec::column().height(if compact { 1.0 } else { 2.0 }),
        &[title_row, detail_row],
    )
}

/// The `+ New session` slot: `idleNumber` = `tint(formfield.base,
/// background, 0.55)` for the `+`; the label stays muted.
fn rail_add_row(
    tree: &mut LayoutTree,
    theme: &ThemeStyles,
    rail_bg: Color,
    muted: Color,
    compact: bool,
) -> taffy::NodeId {
    let idle = tint(rgb(theme.formfield_base), rgb(rail_bg), 0.55);
    let plus = tree.text_sized(
        "+",
        Style::new().fg(color(idle)).bg(rail_bg),
        &BoxSpec::default().width(2.0).shrink(0.0),
    );
    let label = tree.text_sized(
        NEW_SESSION_TAB_TITLE,
        Style::new().fg(muted).bg(rail_bg),
        &BoxSpec::default().shrink(0.0),
    );
    tree.box_node(
        &BoxSpec::row().height(1.0).padding_left(if compact { 0.0 } else { 1.0 }),
        &[plus, label],
    )
}

/// The rail's pulse fills: per glowing tab, the title row's strip
/// (glow α 0.45, tail clamped to 8) and the detail row's (α 0.25,
/// glowTail prop 10 → still 8).
fn rail_pulse_fills(
    tabs: &[TabView],
    active: Option<&str>,
    theme: &ThemeStyles,
    placed: &[Placed],
) -> Vec<Fill> {
    let rail_bg = theme.background_raised;
    let mut fills = Vec::new();
    for (index, view) in tabs.iter().enumerate() {
        let selected = Some(view.tab.session_id.as_str()) == active;
        if !view.status.glows(selected) {
            continue;
        }
        let Some(title) =
            placed.iter().find(|p| p.tag.as_deref() == Some(&format!("tab-title:{index}")))
        else {
            continue;
        };
        let pulse_bg = rgb(rail_bg);
        let hue = glow_hue(view.status, theme);
        for (y, alpha, tail) in [(title.y, 0.45, GLOW_TAIL), (title.y + 1, 0.25, 10.0)] {
            let glow = glow_over(pulse_bg, hue, alpha);
            let cells: Vec<Style> = (0..INDICATOR_WIDTH)
                .map(|cell| {
                    Style::new().fg(Color::Reset).bg(color(resting_glow_bg(
                        pulse_bg,
                        glow,
                        cell,
                        INDICATOR_WIDTH,
                        tail,
                        1.0,
                    )))
                })
                .collect();
            fills.extend(fill_runs(y as u16, &cells, 0));
        }
    }
    fills
}

/// The unread-glow separator rows: `▄` × `INDICATOR_WIDTH` above the tab
/// following a glowing tab (outer α 0.1, tail 5; the inner voice is
/// idle for a non-glowing successor, so the glyph fg is the plain
/// background).
fn rail_separators(
    tabs: &[TabView],
    active: Option<&str>,
    theme: &ThemeStyles,
    placed: &mut Vec<Placed>,
) {
    for (index, view) in tabs.iter().enumerate() {
        let selected = Some(view.tab.session_id.as_str()) == active;
        if !view.status.glows(selected) || index + 1 >= tabs.len() {
            continue;
        }
        let Some(next) =
            placed.iter().find(|p| p.tag.as_deref() == Some(&format!("tab-title:{}", index + 1)))
        else {
            continue;
        };
        let y = next.y.saturating_sub(1);
        let pulse_bg = rgb(theme.background_raised);
        let outer_glow = glow_over(pulse_bg, glow_hue(view.status, theme), 0.1);
        let runs: Vec<StyledSpan> = (0..INDICATOR_WIDTH)
            .map(|cell| StyledSpan {
                text: SEPARATOR_GLYPH.to_string(),
                style: Style::new().fg(theme.background_raised).bg(color(resting_glow_bg(
                    pulse_bg,
                    outer_glow,
                    cell,
                    INDICATOR_WIDTH,
                    5.0,
                    1.0,
                ))),
            })
            .collect();
        placed.push(Placed {
            x: 0,
            y,
            width: INDICATOR_WIDTH,
            lines: vec![PlacedLine { runs }],
            tag: Some(format!("separator:{}", index + 1)),
        });
    }
}

/// Paint the horizontal strip frame (golden scenario shape — no
/// pointer state; the goldens pin the resting bytes).
#[must_use]
pub fn paint_bar(
    tabs: &[TabView],
    active: Option<&str>,
    show_plus: bool,
    size: (u16, u16),
    theme: &ThemeStyles,
) -> Frame {
    let render = render_bar(tabs, active, show_plus, usize::from(size.0), theme, None, false);
    paint_placed_with_fills(
        &render.placed,
        &render.fills,
        size,
        Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(theme.background),
    )
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
    let render = render_rail(tabs, active, show_add, width, theme);
    let mut fills = vec![Fill {
        x: 0,
        y: 0,
        width: width as u16,
        height: size.1,
        style: Style::new().bg(theme.background_raised),
    }];
    fills.extend(render.fills);
    paint_placed_with_fills(
        &render.placed,
        &fills,
        size,
        Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(theme.background),
    )
}

/// What a pointer column over the strip addresses — the hit-test
/// mirror of [`render_bar`]'s geometry (beta.8 bug 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripHit {
    /// The tab body (click = select; `release()` → `tabs.select`,
    /// `session-tabs.tsx:1505-1514`).
    Tab(String),
    /// The revealed `✕` cell on a HOVERED tab ("the close glyph sits
    /// one cell in from the right edge: x = tab start + width − 2",
    /// `session-tabs.tsx:264`; the click path is `tabs.close`,
    /// `session-tabs.tsx:1745-1754`).
    Close(String),
    /// The ` + ` affordance (`tabs.add?.()` — navigates home,
    /// `session-tabs.tsx:1786-1792` + `session-tabs.tsx:406-416`).
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
        return StripHit::Add;
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
    /// the ` + ` adds. Mirrors `render_bar`'s layout exactly.
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
    }

    /// beta.8 bug 3 — the hover surface: a hovered NON-selected tab
    /// lifts its background, turns the title `text.base`, shrinks the
    /// title two cells and reveals the `✕`; the ` + ` affordance
    /// highlights on its own hover.
    #[test]
    fn hover_lifts_the_tab_and_reveals_the_close_mark() {
        let tabs = golden_tabs();
        let theme = crate::theme::ThemeState::v2_dark().styles();
        // Hover the SECOND (inactive) tab: "Second" occupies the
        // 32-wide slot at x=32; the mark sits at 32+32−2=62.
        let render = render_bar(&tabs, Some("first"), true, 80, &theme, Some("second"), false);
        let mark = render
            .placed
            .iter()
            .find(|p| p.tag.as_deref().is_some_and(|tag| tag.starts_with("session-tabs.close:")))
            .expect("the hovered tab reveals its close mark");
        assert_eq!(mark.x, 62);
        assert_eq!(mark.lines[0].runs[0].text, "✕");
        assert_eq!(mark.tag.as_deref(), Some("session-tabs.close:second"));
        // The lifted background fill covers the whole tab.
        let lift = render
            .fills
            .iter()
            .find(|fill| fill.width == 32 && fill.x == 32)
            .expect("the hovered tab's fill");
        assert_eq!(lift.style.bg, Some(theme.action_hovered));
        // No hover: no mark anywhere, the resting fill stays base.
        let resting = render_bar(&tabs, Some("first"), true, 80, &theme, None, false);
        assert!(
            !resting.placed.iter().any(|p| p
                .tag
                .as_deref()
                .is_some_and(|tag| tag.starts_with("session-tabs.close:"))),
            "no close mark without hover"
        );
        // The hovered ` + ` affordance swaps to text.base over the
        // hovered action background.
        let plus = render_bar(&tabs, Some("first"), true, 80, &theme, None, true);
        let affordance =
            plus.placed.iter().find(|p| p.tag.as_deref() == Some("session-tabs.add")).unwrap();
        assert_eq!(affordance.lines[0].runs[0].style.fg, theme.text.fg);
        assert_eq!(affordance.lines[0].runs[0].style.bg, Some(theme.action_hovered));
    }
}
