//! The session sidebar (`routes/session/sidebar.tsx` + the
//! `sidebar.footer` slot's getting-started card,
//! `feature-plugins/sidebar/footer.tsx`) — the right-hand pane of the
//! session route (7.2 wave 2).
//!
//! ## Geometry (pinned by the `sidebar-expanded` golden @160)
//!
//! * The pane is [`SESSION_SIDEBAR_WIDTH`] (42) columns at the frame's
//!   right edge, full height, background `background.raised.base`.
//! * `paddingTop 1` → the bold session title renders on row 1 (the
//!   reference's `title_shimmer` with BOLD when animations are off),
//!   `paddingLeft/Right 2` → its text starts 2 columns in.
//! * The `sidebar.content` scroll slot (context/MCP plugin widgets)
//!   follows; the golden fixture carries no widget content, so the
//!   slot renders blank ([`SidebarSections`] is the seam).
//! * The `sidebar.footer` slot stacks from the bottom: the
//!   getting-started card (when shown) then the `{dir}:{branch}`
//!   location line on row `height − 2` (the box's `paddingBottom 1`
//!   keeps the last row blank).
//!
//! ## Visibility (`session-frame.tsx`)
//!
//! `sidebar = auto` (default) shows the pane only when the session
//! pane is wider than 120 columns (`wide()`); `hide` never shows it.

// Sidebar geometry is terminal-bounded (heights/widths far below 2^16).
#![allow(clippy::cast_possible_truncation)]

use crate::components::session_tabs::SESSION_SIDEBAR_WIDTH;
use crate::layout::{Placed, PlacedLine};
use crate::render::Fill;
use crate::theme::ThemeStyles;
use ratatui::style::{Modifier, Style};

/// `wide()` in `session-frame.tsx`: the pane is `auto`-shown only
/// above this session-pane width.
pub const SIDEBAR_AUTO_BREAKPOINT: usize = 120;

/// The card's inner text column (card width 38 − padding 2·2 − glyph
/// 1 − gap 1): both paragraphs wrap at exactly 32 columns.
const CARD_TEXT_WIDTH: usize = 32;

/// `session.sidebar` config values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SidebarMode {
    /// `auto` — show above [`SIDEBAR_AUTO_BREAKPOINT`].
    #[default]
    Auto,
    /// `hide` — never show.
    Hide,
}

/// The reference's visibility rule: `auto` ⇒ wide enough; `hide` ⇒
/// never (a subagent session or open panel suppresses it too — not
/// modeled here).
#[must_use]
pub fn sidebar_visible(mode: SidebarMode, session_pane_width: usize) -> bool {
    matches!(mode, SidebarMode::Auto) && session_pane_width > SIDEBAR_AUTO_BREAKPOINT
}

/// One plugin-provided sidebar section (the `sidebar.content` slot's
/// wave-3 fill): a bold title row plus muted rows, materialized at
/// registration/refresh through the plugin host's UI-request queue —
/// native rendering afterwards, no per-frame IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarSection {
    /// Owning plugin id (one section per plugin, latest wins).
    pub plugin_id: String,
    pub title: String,
    pub rows: Vec<String>,
}

impl SidebarSection {
    /// The section's rendered height: title + rows.
    #[must_use]
    pub fn height(&self) -> usize {
        1 + self.rows.len()
    }
}

/// The sidebar's plugin content slot (`sidebar.content`): the context
/// and MCP widgets plus the wave-3 plugin sections. The golden fixture
/// has no widget data, so empty sections render nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarSections {
    /// The context widget's entries (directory/recent files).
    pub context: Vec<String>,
    /// The MCP widget's server entries.
    pub mcp: Vec<String>,
    /// Plugin-provided sections (registration/refresh-time).
    pub plugins: Vec<SidebarSection>,
}

impl SidebarSections {
    #[must_use]
    pub const fn empty() -> Self {
        Self { context: Vec::new(), mcp: Vec::new(), plugins: Vec::new() }
    }

    /// The slot's content height (rows the scroll area consumes).
    /// Empty sections render nothing, like the golden fixture.
    #[must_use]
    pub fn content_height(&self) -> usize {
        // Per non-empty section: title + rows + the 1-row gap that
        // follows.
        self.plugins
            .iter()
            .filter(|section| !section.title.is_empty() || !section.rows.is_empty())
            .map(|section| section.height() + 1)
            .sum()
    }
}

/// The getting-started onboarding card's fixed copy
/// (`feature-plugins/sidebar/footer.tsx`).
pub const GETTING_STARTED_TITLE: &str = "Getting started";
pub const GETTING_STARTED_PARAGRAPH_1: &str =
    "OpenCode includes free models so you can start immediately.";
pub const GETTING_STARTED_PARAGRAPH_2: &str =
    "Connect from 75+ providers to use other models, including Claude, GPT, Gemini etc";
pub const GETTING_STARTED_ACTION: &str = "Connect provider";
pub const GETTING_STARTED_ACTION_HINT: &str = "/connect";

/// One sidebar view: what the pane renders. The strings are OWNED —
/// the live route builds the title from store state per frame (the
/// golden fixtures keep their `&str` constructors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarView {
    /// The session title (bold, row 1).
    pub title: String,
    /// The content-slot sections (blank in the golden fixture).
    pub sections: SidebarSections,
    /// Show the getting-started card (hidden when dismissed or a
    /// provider is connected; `dimensions().height >= 22`).
    pub onboarding: bool,
    /// The footer location line `{directory}:{branch}` (row height−2).
    pub location: Option<String>,
}

impl SidebarView {
    /// The golden fixture's view: title "Sidebar fixture", no widget
    /// content, the onboarding card, the fixture location.
    #[must_use]
    pub fn fixture(title: &str, location: &str) -> Self {
        Self {
            title: title.to_owned(),
            sections: SidebarSections::empty(),
            onboarding: true,
            location: Some(location.to_owned()),
        }
    }
}

/// Wrap one paragraph at [`CARD_TEXT_WIDTH`] through the shared wrap
/// seam (breaking spaces that fit stay as trailing cells — pinned by
/// the card's `providers to ` / `including ` lines).
fn wrap_card_line(
    text: &str,
    fg: ratatui::style::Color,
    bg: ratatui::style::Color,
) -> Vec<PlacedLine> {
    let style = Style::new().fg(fg).bg(bg);
    crate::wrap::wrap_atoms(&crate::wrap::flatten(text, style), CARD_TEXT_WIDTH)
        .iter()
        .map(|atoms| PlacedLine { runs: crate::wrap::styled_runs(atoms) })
        .collect()
}

/// The onboarding card's total height: padding 1·2 + header + gap +
/// paragraph 1 + gap + paragraph 2 + gap + action row.
fn card_height(theme: &ThemeStyles) -> usize {
    let p1 = wrap_card_line(GETTING_STARTED_PARAGRAPH_1, card_fg(theme), card_bg(theme)).len();
    let p2 = wrap_card_line(GETTING_STARTED_PARAGRAPH_2, card_fg(theme), card_bg(theme)).len();
    // padTop + header + gap + p1 + gap + p2 + gap + action + padBottom
    1 + 1 + 1 + p1 + 1 + p2 + 1 + 1 + 1
}

fn card_bg(theme: &ThemeStyles) -> ratatui::style::Color {
    theme.action_hovered // `background.raised.high`
}

fn card_fg(theme: &ThemeStyles) -> ratatui::style::Color {
    theme.text_muted.fg.unwrap_or(theme.background)
}

/// The painted sidebar: placed runs + fills at column-local
/// coordinates (x = 0 is the pane's left edge); the caller offsets by
/// `frame_width − SESSION_SIDEBAR_WIDTH`.
#[derive(Debug, Clone, Default)]
pub struct SidebarPaint {
    pub placed: Vec<Placed>,
    pub fills: Vec<Fill>,
    /// The onboarding card's pointer targets at the SAME column-local
    /// coordinates (v2's two `onMouseUp` boxes, footer.tsx:45-70);
    /// `None` when the card didn't paint.
    pub onboarding_hits: Option<crate::store::SidebarOnboardingHits>,
}

/// Render the sidebar pane over `height` rows.
#[must_use]
pub fn render_sidebar(view: &SidebarView, theme: &ThemeStyles, height: usize) -> SidebarPaint {
    let raised = theme.background_raised;
    let fg_base = theme.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
    let fg_muted = theme.text_muted.fg.unwrap_or(fg_base);
    let mut paint = SidebarPaint::default();

    // The title: bold `text.base` over the raised background, row 1
    // (paddingTop 1), 2 columns in (paddingLeft 2).
    paint.placed.push(Placed {
        x: 2,
        y: 1,
        width: crate::wrap::display_width(&view.title),
        lines: vec![PlacedLine::plain(
            view.title.as_str(),
            Style::new().fg(fg_base).bg(raised).add_modifier(Modifier::BOLD),
        )],
        tag: Some("sidebar.title".into()),
    });

    // The content slot (`sidebar.content`): one blank row after the
    // title, then every plugin section — a bold header row plus
    // muted rows, 2 columns in, 1-row gap between sections. Empty
    // sections render nothing (the golden fixture's shape).
    let mut y = 2;
    for section in &view.sections.plugins {
        if section.title.is_empty() && section.rows.is_empty() {
            continue;
        }
        if !section.title.is_empty() {
            let title = crate::wrap::take_width(&section.title, SESSION_SIDEBAR_WIDTH - 4);
            paint.placed.push(Placed {
                x: 2,
                y,
                width: crate::wrap::display_width(&title),
                lines: vec![PlacedLine::plain(
                    title,
                    Style::new().fg(fg_base).bg(raised).add_modifier(Modifier::BOLD),
                )],
                tag: Some("sidebar.plugin-section".into()),
            });
            y += 1;
        }
        for row in &section.rows {
            let text = crate::wrap::take_width(row, SESSION_SIDEBAR_WIDTH - 4);
            paint.placed.push(Placed {
                x: 2,
                y,
                width: crate::wrap::display_width(&text),
                lines: vec![PlacedLine::plain(text, Style::new().fg(fg_muted).bg(raised))],
                tag: None,
            });
            y += 1;
        }
        y += 1; // the inter-section gap
    }

    // The footer slot stacks from the bottom: location row at
    // height−2 (the box's paddingBottom keeps the last row blank),
    // gap 1, then the onboarding card above it with its own 1-row
    // top pad (the footer box's paddingTop).
    if let Some(location) = &view.location {
        paint.placed.push(Placed {
            x: 2,
            y: height.saturating_sub(2),
            width: crate::wrap::display_width(location),
            lines: vec![PlacedLine::plain(location.as_str(), Style::new().fg(fg_muted).bg(raised))],
            tag: Some("sidebar.footer.location".into()),
        });
    }
    if view.onboarding {
        let card_h = card_height(theme);
        let card_bottom = height.saturating_sub(4); // location row − gap 1
        let card_top = card_bottom.saturating_sub(card_h).saturating_add(1);
        paint_card(&mut paint, theme, card_top);
    }
    paint
}

/// Paint the getting-started card with its top row at `y` (column
/// local): background `raised.high` cols [2, 40), glyph ⬖ then the
/// 32-column text stack, the ✕ dismiss at the column's right edge.
/// Also records the card's two pointer targets (v2's `onMouseUp`
/// boxes, footer.tsx:45-70): the ✕ cell and the full connect row.
fn paint_card(paint: &mut SidebarPaint, theme: &ThemeStyles, y: usize) {
    let bg = card_bg(theme);
    let fg_base = theme.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
    let fg_muted = theme.text_muted.fg.unwrap_or(fg_base);
    // Card surface: cols [2, 40) — paddingLeft/Right 2 inside the
    // pane; the fill carries the frame-default fg (255), not
    // text.base.
    paint.fills.push(Fill {
        x: 2,
        y: y as u16,
        width: (SESSION_SIDEBAR_WIDTH - 4) as u16,
        height: card_height(theme) as u16,
        style: Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(bg),
    });
    // Row y+1: ⬖ (text.base) after the card's 2-col pad (col 4),
    // gap 1, then the 32-column stack at col 6.
    let stack_x = 2 + 2 + 1 + 1; // padding 2 + glyph 1 + gap 1
    paint.placed.push(Placed {
        x: 4,
        y: y + 1,
        width: 1,
        lines: vec![PlacedLine::plain("⬖", Style::new().fg(fg_base).bg(bg))],
        tag: Some("sidebar.footer.getting-started".into()),
    });
    // Header: bold title left, ✕ (muted) right-aligned with 2-col pad.
    paint.placed.push(Placed {
        x: stack_x,
        y: y + 1,
        width: crate::wrap::display_width(GETTING_STARTED_TITLE),
        lines: vec![PlacedLine::plain(
            GETTING_STARTED_TITLE,
            Style::new().fg(fg_base).bg(bg).add_modifier(Modifier::BOLD),
        )],
        tag: None,
    });
    paint.placed.push(Placed {
        x: stack_x + CARD_TEXT_WIDTH - 1, // right edge of the text column
        y: y + 1,
        width: 1,
        lines: vec![PlacedLine::plain("✕", Style::new().fg(fg_muted).bg(bg))],
        tag: Some("sidebar.footer.getting-started.dismiss".into()),
    });
    // The two paragraphs, wrapped at 32 (muted), gap 1 between blocks.
    let mut row = y + 3; // padTop + header + gap
    for paragraph in [GETTING_STARTED_PARAGRAPH_1, GETTING_STARTED_PARAGRAPH_2] {
        for line in wrap_card_line(paragraph, fg_muted, bg) {
            paint.placed.push(Placed {
                x: stack_x,
                y: row,
                width: CARD_TEXT_WIDTH,
                lines: vec![line],
                tag: None,
            });
            row += 1;
        }
        row += 1; // gap 1
    }
    // The action row: label (base) left, hint (muted) right-aligned.
    paint.placed.push(Placed {
        x: stack_x,
        y: row,
        width: crate::wrap::display_width(GETTING_STARTED_ACTION),
        lines: vec![PlacedLine::plain(GETTING_STARTED_ACTION, Style::new().fg(fg_base).bg(bg))],
        tag: Some("sidebar.footer.getting-started.connect".into()),
    });
    let hint_x =
        stack_x + CARD_TEXT_WIDTH - crate::wrap::display_width(GETTING_STARTED_ACTION_HINT);
    paint.placed.push(Placed {
        x: hint_x,
        y: row,
        width: crate::wrap::display_width(GETTING_STARTED_ACTION_HINT),
        lines: vec![PlacedLine::plain(
            GETTING_STARTED_ACTION_HINT,
            Style::new().fg(fg_muted).bg(bg),
        )],
        tag: None,
    });
    // The card's pointer targets, column-local: the ✕ cell (the
    // dismiss `text`'s own box, footer.tsx:45-55) and the connect row
    // (the `space-between` box spanning the text column,
    // footer.tsx:61-70). The session render translates by the pane's
    // paste origin.
    paint.onboarding_hits = Some(crate::store::SidebarOnboardingHits {
        dismiss: crate::store::HitRect {
            x: stack_x + CARD_TEXT_WIDTH - 1,
            y: y + 1,
            width: 1,
            height: 1,
        },
        connect: crate::store::HitRect { x: stack_x, y: row, width: CARD_TEXT_WIDTH, height: 1 },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// The golden's visibility rule: auto shows only above 120.
    #[test]
    fn auto_show_rule_matches_the_reference() {
        assert!(sidebar_visible(SidebarMode::Auto, 121));
        assert!(!sidebar_visible(SidebarMode::Auto, 120), "strictly greater than 120");
        assert!(sidebar_visible(SidebarMode::Auto, 160));
        assert!(!sidebar_visible(SidebarMode::Hide, 160));
        assert!(!sidebar_visible(SidebarMode::Auto, 80));
    }

    /// The card wraps its paragraphs at 32 exactly like the golden:
    /// line 1 fits whole, line 2 is the remainder; paragraph 2 keeps
    /// its fitting breaking spaces (`to `, `including `).
    #[test]
    fn card_paragraph_wrap_is_the_golden_shape() {
        let theme = theme();
        let p1 = wrap_card_line(GETTING_STARTED_PARAGRAPH_1, card_fg(&theme), card_bg(&theme));
        let p2 = wrap_card_line(GETTING_STARTED_PARAGRAPH_2, card_fg(&theme), card_bg(&theme));
        let texts = |lines: &[PlacedLine]| lines.iter().map(PlacedLine::text).collect::<Vec<_>>();
        assert_eq!(
            texts(&p1),
            vec!["OpenCode includes free models so", "you can start immediately."]
        );
        assert_eq!(
            texts(&p2),
            vec![
                "Connect from 75+ providers to ",
                "use other models, including ",
                "Claude, GPT, Gemini etc",
            ],
            "a fitting breaking space stays as a trailing cell"
        );
    }

    /// The footer stack lands at the golden rows: card rows 15–26,
    /// gap 27, location 28, pad 29 at height 30.
    #[test]
    fn footer_stack_geometry_at_height_30() {
        let theme = theme();
        let view = SidebarView::fixture("Sidebar fixture", "/tmp/opencode/packages/tui:main");
        let paint = render_sidebar(&view, &theme, 30);
        let card = paint
            .placed
            .iter()
            .find(|p| p.tag.as_deref() == Some("sidebar.footer.getting-started"))
            .expect("card glyph");
        assert_eq!(card.y, 16, "header row (card padTop 15 + 1)");
        assert_eq!(card_height(&theme), 12, "card spans rows 15..=26");
        let location = paint
            .placed
            .iter()
            .find(|p| p.tag.as_deref() == Some("sidebar.footer.location"))
            .expect("location");
        assert_eq!(location.y, 28);
        let title =
            paint.placed.iter().find(|p| p.tag.as_deref() == Some("sidebar.title")).expect("title");
        assert_eq!((title.x, title.y), (2, 1));
    }

    /// The wave-3 plugin section renders under the title (the blank
    /// seam filled): bold header at row 2, muted rows under it, a gap
    /// between sections — and empty sections stay invisible (the
    /// golden fixture's shape). Text-grid pinned locally (plugins are
    /// not in the reference corpus).
    #[test]
    fn plugin_section_renders_under_the_title() {
        let theme = theme();
        let mut view = SidebarView::fixture("Sidebar fixture", "/tmp/opencode/x:main");
        view.sections.plugins = vec![
            SidebarSection {
                plugin_id: "omos".into(),
                title: "Preset".into(),
                rows: vec!["zhipuai · own/glm-5.3".into(), "openai · gpt-5.6".into()],
            },
            SidebarSection { plugin_id: "empty".into(), title: String::new(), rows: vec![] },
            SidebarSection {
                plugin_id: "mcp".into(),
                title: "MCP".into(),
                rows: vec!["context7".into()],
            },
        ];
        assert_eq!(view.sections.content_height(), 7, "(1+2)+gap + (1+1)+gap; empty skipped");
        let paint = render_sidebar(&view, &theme, 30);
        let texts: Vec<(usize, String)> = paint
            .placed
            .iter()
            .filter(|p| p.y < 10)
            .map(|p| (p.y, p.lines.iter().map(PlacedLine::text).collect::<String>()))
            .collect();
        assert_eq!(
            texts,
            vec![
                (1, "Sidebar fixture".to_owned()),
                (2, "Preset".to_owned()),
                (3, "zhipuai · own/glm-5.3".to_owned()),
                (4, "openai · gpt-5.6".to_owned()),
                (6, "MCP".to_owned()), // gap row 5, next section at 6
                (7, "context7".to_owned()),
            ],
            "the plugin sections stack under the title"
        );
        // The empty section painted nothing at row 5.
        assert!(paint.placed.iter().all(|p| p.y != 5), "the gap row stays blank");
    }
}
