//! The complete session route (`component/session-frame.tsx` +
//! `routes/session/index.tsx`) — 7.2 wave 2's assembly surface:
//!
//! ```text
//! [ horizontal tabs bar (mode "on") ]
//! [ transcript column | sidebar pane ]
//! [ composer chrome · status line   ]
//! ```
//!
//! * The **transcript column** paints through [`crate::components::
//!   message::list`] (the wave-1 renderer with the wave-2 window/follow
//!   seam) at `width − sidebar_width`.
//! * The **sidebar pane** ([`crate::components::sidebar`]) renders when
//!   `session.sidebar = auto` and the pane is wide (>120 cols).
//! * The **composer slot** delegates to the sibling composer lane's
//!   component when it lands; until then [`ComposerSlot::Placeholder`]
//!   paints the exact chrome bytes the goldens pin (the seam fixture
//!   wave 1 labeled `composer_bottom_chrome` — moved here so the
//!   golden frames compare whole either way).
//! * The **tabs bar** row renders the active-tab pill + ` + `
//!   affordance (`component/session-tabs.tsx` horizontal bar,
//!   animations off; the windowing math is [`crate::components::
//!   session_tabs::adaptive_session_tab_layout`]).

#![allow(clippy::cast_possible_truncation)]

use crate::components::message::list::{render_transcript, FollowState, TranscriptOptions};
use crate::components::message::model::Message;
use crate::components::session_tabs::{
    adaptive_session_tab_layout, ADD_TAB_WIDTH, SESSION_SIDEBAR_WIDTH,
};
use crate::components::sidebar::{render_sidebar, sidebar_visible, SidebarMode, SidebarView};
use crate::components::tool_rows::{fill_style, DEFAULT_FOREGROUND};
use crate::layout::{Placed, PlacedLine};
use crate::render::{paint_placed, paint_placed_with_fills, Fill, Frame};
use crate::theme::ThemeState;
use ratatui::style::{Color, Modifier, Style};

/// One tabs-bar entry (the assembly's slice of the store's tab list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenTab {
    pub session_id: String,
    pub title: String,
}

impl ScreenTab {
    #[must_use]
    pub fn new(id: &str, title: &str) -> Self {
        Self { session_id: id.to_owned(), title: title.to_owned() }
    }
}

/// The composer slot: the sibling composer lane's frame (7 rows at the
/// transcript column width) or the placeholder chrome the goldens pin.
#[derive(Debug, Clone, Copy)]
pub enum ComposerSlot<'a> {
    /// The wave-2 composer stub: 4 `┃` border rows over the prompt
    /// background, the `╹▀▀…` transition, the `{dir}:{branch}` status
    /// line with right-aligned keybind hints, the bottom pad row.
    Placeholder,
    /// The real composer component's frame (swapped in 1:1 when the
    /// sibling lane lands — the golden bytes are identical by
    /// contract).
    Sibling(&'a Frame),
}

/// The session screen's chrome inputs.
#[derive(Debug, Clone)]
pub struct SessionScreen<'a> {
    /// Tabs mode "on": the bar's tabs (first = active in the fixture).
    pub tabs: Option<Vec<ScreenTab>>,
    /// `session.sidebar` (auto/hide).
    pub sidebar: SidebarMode,
    /// The sidebar pane's content (title, onboarding card, location).
    pub sidebar_view: SidebarView<'a>,
    /// The transcript's follow state (sticky/free offset).
    pub follow: FollowState,
    /// The status line's `{directory}:{branch}` label.
    pub location: &'a str,
}

impl<'a> SessionScreen<'a> {
    /// The `session-screen-40` fixture: tabs mode on with one
    /// "Narrow session" tab, sidebar hidden at 40 cols.
    #[must_use]
    pub fn narrow(location: &'a str) -> Self {
        Self {
            tabs: Some(vec![ScreenTab::new("ses_narrow", "Narrow session")]),
            sidebar: SidebarMode::Auto,
            sidebar_view: SidebarView::fixture("Narrow session", location),
            follow: FollowState::Sticky,
            location,
        }
    }

    /// The `sidebar-*` fixture: tabs mode off, sidebar auto/hide.
    #[must_use]
    pub fn wide(title: &'a str, location: &'a str) -> Self {
        Self {
            tabs: None,
            sidebar: SidebarMode::Auto,
            sidebar_view: SidebarView::fixture(title, location),
            follow: FollowState::Sticky,
            location,
        }
    }
}

/// The fixture status-line label every full-app golden carries
/// (`test/fixture/tui-client`'s directory + branch `main`).
pub const FIXTURE_LOCATION: &str = "/tmp/opencode/packages/tui:main";

/// **The composer chrome seam** (wave 1's labeled `composer_bottom_
/// chrome` stub, now the assembly's placeholder composer): 7 rows —
///
/// * blank spacing row,
/// * 4 composer rows (`┃` left border in `border.base` over the
///   `decrease(background.raised)` prompt background),
/// * the `╹▀▀…` transition row (▀ painted `promptBg` over base),
/// * the status line (`{directory}:{branch}` muted, the keybind hints
///   right-aligned — hidden below ~60 cols),
/// * the container's bottom padding row.
///
/// The sibling composer lane's component replaces this byte-for-byte
/// ([`ComposerSlot::Sibling`]).
#[must_use]
pub fn composer_bottom_chrome(width: u16, theme: &ThemeState, location: &str) -> Frame {
    // 7 rows: the composer body (4), the ╹▀ transition, the status
    // line, the container's bottom padding.
    let styles = theme.styles();
    let inset: u16 = if width < 44 { 1 } else { 2 };
    let content_width = width - inset * 2;
    let prompt_bg = {
        let raised = theme.resolved.tokens.background.raised.base;
        let decreased = theme.resolved.tokens.decrease(raised, 1.0);
        Color::Rgb(decreased.r, decreased.g, decreased.b)
    };
    let border = Style::new().fg(styles.border).bg(styles.background);
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    // Rows 0-3: the composer body (empty textarea + metadata padding).
    for row in 0..=3u16 {
        placed.push(Placed {
            x: inset as usize,
            y: row as usize,
            width: 1,
            lines: vec![PlacedLine::plain("┃", border)],
            tag: None,
        });
        fills.push(Fill {
            x: inset + 1,
            y: row,
            width: content_width - 1,
            height: 1,
            style: Style::new().fg(DEFAULT_FOREGROUND).bg(prompt_bg),
        });
    }
    // Row 5: the ╹▀ transition (bottom border drawn with upper-half
    // blocks in the prompt background color).
    placed.push(Placed {
        x: inset as usize,
        y: 4,
        width: 1,
        lines: vec![PlacedLine::plain("╹", border)],
        tag: None,
    });
    placed.push(Placed {
        x: (inset + 1) as usize,
        y: 4,
        width: usize::from(content_width - 1),
        lines: vec![PlacedLine::plain(
            "▀".repeat(usize::from(content_width - 1)),
            Style::new().fg(prompt_bg).bg(styles.background),
        )],
        tag: None,
    });
    // Row 6: the status line — location muted; the hints right-aligned
    // ("shift+tab agents" + 2 gap + "ctrl+p commands", keys in
    // `text.base`, labels muted). Hidden at narrow widths.
    placed.push(Placed {
        x: inset as usize,
        y: 5,
        width: crate::wrap::display_width(location),
        lines: vec![PlacedLine::plain(
            location,
            Style::new()
                .fg(styles.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND))
                .bg(styles.background),
        )],
        tag: None,
    });
    if width >= 60 {
        let base =
            Style::new().fg(styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)).bg(styles.background);
        let muted = Style::new()
            .fg(styles.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND))
            .bg(styles.background);
        // The 2-cell gap between the hint groups stays at the
        // frame default foreground (the golden pins fg 255 there,
        // not text.base).
        let gap = Style::new().fg(DEFAULT_FOREGROUND).bg(styles.background);
        let end = inset + content_width; // exclusive
        let right_width = 10 + 6 + 2 + 7 + 8; // "shift+tab agents" + gap + "ctrl+p commands"
        let mut x = (end - right_width) as usize;
        for (text, style) in [
            ("shift+tab ", base),
            ("agents", muted),
            ("  ", gap),
            ("ctrl+p ", base),
            ("commands", muted),
        ] {
            placed.push(Placed {
                x,
                y: 5,
                width: crate::wrap::display_width(text),
                lines: vec![PlacedLine::plain(text, style)],
                tag: None,
            });
            x += crate::wrap::display_width(text);
        }
    }
    paint_placed_with_fills(&placed, &fills, (width, 7), fill_style(&styles))
}

/// The horizontal tabs bar row (height 1): the active tab's pill in
/// `background.action.primary.selected` with a bold `text.base` title
/// (the reference's `tabBackground()`/`foreground()` with animations
/// off), inactive pills on the base background with muted titles, and
/// the `" + "` affordance in `text.muted`.
#[must_use]
pub fn paint_tabs_bar(
    tabs: &[ScreenTab],
    active: Option<&str>,
    size: (u16, u16),
    theme: &crate::theme::ThemeStyles,
) -> Frame {
    let width = usize::from(size.0);
    let owned: Vec<crate::store::SessionTab> = tabs
        .iter()
        .map(|tab| crate::store::SessionTab {
            session_id: tab.session_id.clone(),
            title: Some(tab.title.clone()),
        })
        .collect();
    let layout =
        adaptive_session_tab_layout(&owned, active, width.saturating_sub(ADD_TAB_WIDTH), 0);
    let number_width = tabs.len().to_string().len().max(2);
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    let mut x = 0usize;
    let window_len = layout.tabs.len();
    let window: &[ScreenTab] =
        if window_len > 0 { &tabs[layout.start..layout.start + window_len] } else { &[] };
    for (index, tab) in window.iter().enumerate() {
        let pill = layout.widths[index];
        let selected = Some(tab.session_id.as_str()) == active;
        // The raw token (background.action.primary.selected) is
        // transparent in v2-dark; the reference renders the selected
        // pill at raised.high (30,30,30) — decrease(raised.base, 1)
        // lands on the same value.
        let pill_bg = if selected { theme.action_hovered } else { theme.background };
        fills.push(Fill {
            x: x as u16,
            y: 0,
            width: pill as u16,
            height: 1,
            style: Style::new().fg(DEFAULT_FOREGROUND).bg(pill_bg),
        });
        let title_width = pill.saturating_sub(1 + number_width).max(1);
        let title = crate::wrap::take_width(&tab.title, title_width);
        let style = if selected {
            Style::new()
                .fg(theme.text.fg.unwrap_or(DEFAULT_FOREGROUND))
                .bg(pill_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND)).bg(pill_bg)
        };
        placed.push(Placed {
            x: x + number_width + 1,
            y: 0,
            width: crate::wrap::display_width(&title),
            lines: vec![PlacedLine::plain(title, style)],
            tag: Some(format!("session-tab:{}", tab.session_id)),
        });
        x += pill;
    }
    // The add affordance (" + " in text.muted over the base bg).
    placed.push(Placed {
        x,
        y: 0,
        width: ADD_TAB_WIDTH,
        lines: vec![PlacedLine::plain(
            " + ",
            Style::new().fg(theme.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND)).bg(theme.background),
        )],
        tag: Some("session-tabs.add".into()),
    });
    paint_placed_with_fills(&placed, &fills, size, fill_style(theme))
}

/// Paste `region`'s cells at (`x`, `y`) — regions never overlap in the
/// assembly; anything left keeps the base fill.
fn paste(target: &mut Frame, region: &Frame, x: u16, y: u16) {
    for (row, cells) in region.cells.iter().enumerate() {
        let Some(slot) = target.cells.get_mut(y as usize + row) else { break };
        slot[x as usize..x as usize + cells.len()].clone_from_slice(cells);
    }
}

/// Paint the complete session route.
#[must_use]
pub fn paint_session_screen(
    messages: &[Message],
    options: &TranscriptOptions,
    screen: &SessionScreen,
    theme: &ThemeState,
    width: u16,
    height: u16,
    composer: ComposerSlot,
) -> Frame {
    let styles = theme.styles();
    let tabs_height = u16::from(screen.tabs.is_some());
    let sidebar_visible = sidebar_visible(screen.sidebar, usize::from(width));
    let sidebar_width = if sidebar_visible { SESSION_SIDEBAR_WIDTH as u16 } else { 0 };
    let transcript_width = width - sidebar_width;
    let transcript_height = height - 7 - tabs_height;

    let mut frame = paint_placed(&[], (width, height), fill_style(&styles));

    if let Some(tabs) = &screen.tabs {
        let active = tabs.first().map(|tab| tab.session_id.as_str());
        let bar = paint_tabs_bar(tabs, active, (width, 1), &styles);
        paste(&mut frame, &bar, 0, 0);
    }

    let transcript = render_transcript(messages, options, &styles, usize::from(transcript_width));
    let transcript_frame =
        transcript.paint(screen.follow, &styles, (transcript_width, transcript_height), None);
    paste(&mut frame, &transcript_frame, 0, tabs_height);

    let chrome = match composer {
        ComposerSlot::Placeholder => {
            composer_bottom_chrome(transcript_width, theme, screen.location)
        }
        ComposerSlot::Sibling(frame) => frame.clone(),
    };
    paste(&mut frame, &chrome, 0, tabs_height + transcript_height);

    if sidebar_visible {
        let sidebar =
            render_sidebar(&screen.sidebar_view, &styles, usize::from(height - tabs_height));
        // The pane paints at column-local coordinates; `paste` places
        // it at the frame's right edge (offsetting first would push
        // every run outside the 42-wide pane).
        let pane = paint_placed_with_fills(
            &sidebar.placed,
            &sidebar.fills,
            (sidebar_width, height - tabs_height),
            Style::new().fg(DEFAULT_FOREGROUND).bg(styles.background_raised),
        );
        paste(&mut frame, &pane, transcript_width, tabs_height);
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::message::list::message_groups_fixture;
    use crate::theme::ThemeState;

    fn theme() -> ThemeState {
        ThemeState::v2_dark()
    }

    /// The 40-col assembly geometry: tabs row 0, transcript 1–22
    /// (21-row sticky viewport + jump slot), composer 23–26, ╹▀ 27,
    /// status 28, pad 29 — and the 23-row content scrolls 2 (marginTop
    /// + the box's top border hide).
    #[test]
    fn narrow_screen_assembly_geometry() {
        let theme = theme();
        let screen = SessionScreen::narrow(FIXTURE_LOCATION);
        let frame = paint_session_screen(
            &message_groups_fixture(),
            &TranscriptOptions::default(),
            &screen,
            &theme,
            40,
            30,
            ComposerSlot::Placeholder,
        );
        let grid = frame.text_grid();
        assert_eq!(frame.height, 30);
        // Tabs row: the pill (32 wide) + " + ".
        assert!(grid[0].starts_with("   Narrow session"), "row 0: {:?}", grid[0]);
        assert_eq!(&grid[0][32..35], " + ");
        // Transcript scrolled by 2: the box's text row leads.
        assert!(grid[1].starts_with(" ┃  What does the parser do?"), "row 1: {:?}", grid[1]);
        // Composer rows 23..=26, transition 27, status 28.
        assert!(grid[23].starts_with(" ┃"), "row 23: {:?}", grid[23]);
        assert!(grid[27].starts_with(" ╹▀"), "row 27: {:?}", grid[27]);
        assert!(grid[28].starts_with(" /tmp/opencode/packages/tui:main"), "row 28: {:?}", grid[28]);
    }

    /// The sidebar column takes 42 cols at 160 and the transcript
    /// renders at 118.
    #[test]
    fn wide_screen_splits_transcript_and_sidebar() {
        let theme = theme();
        let screen = SessionScreen::wide("Sidebar fixture", FIXTURE_LOCATION);
        let frame = paint_session_screen(
            &message_groups_fixture(),
            &TranscriptOptions::default(),
            &screen,
            &theme,
            160,
            30,
            ComposerSlot::Placeholder,
        );
        let grid = frame.text_grid();
        // Sidebar title at row 1, cols 120+.
        assert_eq!(&grid[1][118..120], "  ");
        // Byte 122 = column 120 (the leading ┃ is 3 bytes wide).
        assert!(grid[1][122..].starts_with("Sidebar fixture"), "row 1 tail: {:?}", grid[1]);
        // The transcript's composer transition spans only cols 2..116.
        assert!(grid[27].starts_with("  ╹"), "row 27: {:?}", grid[27]);
        assert_eq!(grid[27].chars().nth(116), Some(' '), "sidebar region blank under ╹▀");
        // The sidebar footer location on row 28.
        assert!(
            grid[28][120..].starts_with("/tmp/opencode/packages/tui:main"),
            "row 28 tail: {:?}",
            grid[28]
        );
    }
}
