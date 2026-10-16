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

use crate::components::message::rows::locale_number;
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
    /// The context widget (`SidebarContext`, feature-plugins/sidebar/
    /// context.tsx): the token/percent/cost rows, gated by
    /// `state() || cost > 0`.
    pub context: SidebarContextWidget,
    /// The MCP widget (`SidebarMcp`, feature-plugins/sidebar/mcp.tsx):
    /// per-server rows with status dots, collapsible past two.
    pub mcp: SidebarMcpWidget,
    /// Plugin-provided sections (registration/refresh-time).
    pub plugins: Vec<SidebarSection>,
}

/// The context widget's data (`contextUsage` + `session.cost`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarContextWidget {
    /// `contextUsage().tokens` → the `{n} tokens` row.
    pub tokens: Option<u64>,
    /// `percent` → the `{p}% used` row (only when the model's context
    /// limit resolved).
    pub percent: Option<u8>,
    /// `data.session.cost()` pre-formatted (`$X.XX`) → the `{cost}
    /// spent` row.
    pub cost: Option<String>,
}

impl SidebarContextWidget {
    /// `state() || cost() > 0` — the `<Show>` gate (context.tsx:24).
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.tokens.is_some() || self.cost.is_some()
    }

    /// The widget's row count: the bold `Context` title + the data rows.
    #[must_use]
    pub fn height(&self) -> usize {
        1 + usize::from(self.tokens.is_some())
            + usize::from(self.percent.is_some())
            + usize::from(self.cost.is_some())
    }

    /// The widget's rendered rows (plain text forms — the paint adds
    /// the bold title + muted styling).
    #[must_use]
    pub fn rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        if let Some(tokens) = self.tokens {
            rows.push(format!("{} tokens", locale_number(tokens)));
        }
        if let Some(percent) = self.percent {
            rows.push(format!("{percent}% used"));
        }
        if let Some(cost) = &self.cost {
            rows.push(format!("{cost} spent"));
        }
        rows
    }
}

/// The MCP widget's data + collapse state (mcp.tsx:19-27: `view.open`
/// persists per storage; rows hide past two servers while closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarMcpWidget {
    pub servers: Vec<crate::store::McpServerRow>,
    pub open: bool,
}

impl SidebarMcpWidget {
    /// `list.length > 0` — the section's `<Show>` gate.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        !self.servers.is_empty()
    }

    /// Rows render when `list.length <= 2 || view.open` (mcp.tsx:73).
    #[must_use]
    pub fn rows_visible(&self) -> bool {
        self.servers.len() <= 2 || self.open
    }

    /// The connected/errored counts for the collapsed header label
    /// (`({n} active[, {m} errors])`, mcp.tsx:37-42).
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (
            self.servers.iter().filter(|server| server.status == "connected").count(),
            self.servers
                .iter()
                .filter(|server| server.status == "failed" || server.status == "needs_auth")
                .count(),
        )
    }

    /// The widget's row count: the header + the visible server rows.
    #[must_use]
    pub fn height(&self) -> usize {
        1 + if self.rows_visible() { self.servers.len() } else { 0 }
    }

    /// One server row's label + status (the paint adds the dot).
    #[must_use]
    pub fn status_label(status: &str) -> &'static str {
        match status {
            "connected" => "Connected",
            "pending" => "Connecting",
            "failed" => "Error",
            "disabled" => "Disabled",
            "needs_auth" => "Sign in",
            _ => "status",
        }
    }
}

impl SidebarSections {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            context: SidebarContextWidget::default(),
            mcp: SidebarMcpWidget::default(),
            plugins: Vec::new(),
        }
    }

    /// The slot's content height (rows the scroll area consumes).
    /// Empty sections render nothing, like the golden fixture.
    #[must_use]
    pub fn content_height(&self) -> usize {
        // Per non-empty section: title + rows + the 1-row gap that
        // follows.
        let mut total = 0usize;
        if self.context.is_shown() {
            total += self.context.height() + 1;
        }
        if self.mcp.is_shown() {
            total += self.mcp.height() + 1;
        }
        total
            + self
                .plugins
                .iter()
                .filter(|section| !section.title.is_empty() || !section.rows.is_empty())
                .map(|section| section.height() + 1)
                .sum::<usize>()
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
    /// The MCP section's pointer targets (mcp.tsx:26-33/76-84) at the
    /// same column-local coordinates — the header's collapse toggle
    /// and the per-server rows.
    pub mcp_hits: crate::store::SidebarMcpHits,
}

/// The content slot's viewport height (rows between the title and
/// the footer stack) — the scroll clamp's input, the same bound
/// [`render_sidebar_scrolled`] clips against.
#[must_use]
pub fn sidebar_content_viewport(view: &SidebarView, theme: &ThemeStyles, height: usize) -> usize {
    let footer_top = if view.onboarding {
        let card_h = card_height(theme);
        height.saturating_sub(4).saturating_sub(card_h).saturating_add(1)
    } else {
        height.saturating_sub(2)
    };
    footer_top.saturating_sub(2)
}

/// Render the sidebar pane over `height` rows: the title, the
/// scrolling CONTENT SLOT (the plugin sections — the wheel surface,
/// auditA #1) and the bottom footer stack (location row + onboarding
/// card), which stay put. Sections sit at their natural positions
/// (the golden fixtures' view); the wheel path uses
/// [`render_sidebar_scrolled`].
#[must_use]
pub fn render_sidebar(view: &SidebarView, theme: &ThemeStyles, height: usize) -> SidebarPaint {
    render_sidebar_scrolled(view, theme, height, 0)
}

/// [`render_sidebar`] with the content slot's wheel offset — rows
/// scrolled out of the slot's viewport (bounded by the footer
/// stack's top) are dropped, like v2's `sidebar.content` scrollbox
/// clipping its children.
// The SURF-1 context/MCP sections pushed the walk past the budget.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn render_sidebar_scrolled(
    view: &SidebarView,
    theme: &ThemeStyles,
    height: usize,
    scroll: usize,
) -> SidebarPaint {
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

    // The content slot's viewport: rows [2, footer_top) — the footer
    // stack (location row at height−2, the onboarding card above it)
    // owns everything below `footer_top`.
    let footer_top = if view.onboarding {
        let card_h = card_height(theme);
        height.saturating_sub(4).saturating_sub(card_h).saturating_add(1)
    } else {
        height.saturating_sub(2)
    };

    // The content slot (`sidebar.content`): one blank row after the
    // title, then every section — 2 columns in, 1-row gap between
    // sections. Empty sections render nothing (the golden fixture's
    // shape). The wheel offset shifts the slot; rows outside the
    // viewport drop.
    let mut y = 0usize;
    let slot_row = |y: usize| y.checked_sub(scroll).map(|visible| visible + 2);

    // The context widget (context.tsx:24-45): the bold `Context`
    // title, then the muted token/percent/cost rows.
    if view.sections.context.is_shown() {
        if let Some(row) = slot_row(y).filter(|row| *row < footer_top) {
            paint.placed.push(Placed {
                x: 2,
                y: row,
                width: 7,
                lines: vec![PlacedLine::plain(
                    "Context",
                    Style::new().fg(fg_base).bg(raised).add_modifier(Modifier::BOLD),
                )],
                tag: Some("sidebar.context".into()),
            });
        }
        y += 1;
        for text in view.sections.context.rows() {
            if let Some(slot) = slot_row(y).filter(|slot| *slot < footer_top) {
                let text = crate::wrap::take_width(&text, SESSION_SIDEBAR_WIDTH - 4);
                paint.placed.push(Placed {
                    x: 2,
                    y: slot,
                    width: crate::wrap::display_width(&text),
                    lines: vec![PlacedLine::plain(text, Style::new().fg(fg_muted).bg(raised))],
                    tag: None,
                });
            }
            y += 1;
        }
        y += 1; // the inter-section gap
    }

    // The MCP widget (mcp.tsx:20-102): the bold `MCP` header (the
    // disclosure marker + collapsed counts past two servers), then the
    // per-server rows — a status dot, the bold name (truncated) and
    // the right-aligned status label.
    if view.sections.mcp.is_shown() {
        let (active, errors) = view.sections.mcp.counts();
        let collapsed = !view.sections.mcp.rows_visible();
        let header = if collapsed {
            if errors > 0 {
                format!(
                    "MCP ({active} active, {errors} error{})",
                    if errors > 1 { "s" } else { "" }
                )
            } else {
                format!("MCP ({active} active)")
            }
        } else {
            "MCP".to_owned()
        };
        let marker = if view.sections.mcp.servers.len() > 2 {
            if collapsed {
                "▶ "
            } else {
                "▼ "
            }
        } else {
            ""
        };
        if let Some(row) = slot_row(y).filter(|row| *row < footer_top) {
            paint.placed.push(Placed {
                x: 2,
                y: row,
                width: crate::wrap::display_width(&header) + 2,
                lines: vec![PlacedLine {
                    runs: vec![
                        crate::layout::StyledSpan {
                            link: None,
                            text: marker.to_owned(),
                            style: Style::new().fg(fg_base).bg(raised),
                        },
                        crate::layout::StyledSpan {
                            link: None,
                            text: header.clone(),
                            style: Style::new().fg(fg_base).bg(raised).add_modifier(Modifier::BOLD),
                        },
                    ],
                }],
                tag: Some("sidebar.mcp".into()),
            });
            // The header's toggle surface (mcp.tsx:26-33): only past
            // two servers does the release flip `view.open`.
            if view.sections.mcp.servers.len() > 2 {
                paint.mcp_hits.header = Some(crate::store::HitRect {
                    x: 2,
                    y: row,
                    width: crate::wrap::display_width(&header) + 2,
                    height: 1,
                });
            }
        }
        y += 1;
        if !collapsed {
            for server in &view.sections.mcp.servers {
                if let Some(slot) = slot_row(y).filter(|slot| *slot < footer_top) {
                    let status = SidebarMcpWidget::status_label(server.status);
                    let dot_color = match server.status {
                        "connected" => theme.success,
                        "failed" => theme.error,
                        "needs_auth" => theme.warning,
                        _ => fg_muted,
                    };
                    let status_width = crate::wrap::display_width(status);
                    // `• {name}` truncated to leave the right-aligned
                    // status its columns (gap 1).
                    let name_budget =
                        SESSION_SIDEBAR_WIDTH.saturating_sub(4 + 2 + 1 + status_width + 1).max(2);
                    let name = crate::wrap::take_width(&server.name, name_budget);
                    let name_width = crate::wrap::display_width(&name);
                    paint.placed.push(Placed {
                        x: 2,
                        y: slot,
                        width: 2 + name_width,
                        lines: vec![PlacedLine {
                            runs: vec![
                                crate::layout::StyledSpan {
                                    link: None,
                                    text: "• ".to_owned(),
                                    style: Style::new().fg(dot_color).bg(raised),
                                },
                                crate::layout::StyledSpan {
                                    link: None,
                                    text: name,
                                    style: Style::new()
                                        .fg(fg_base)
                                        .bg(raised)
                                        .add_modifier(Modifier::BOLD),
                                },
                            ],
                        }],
                        tag: Some(format!("sidebar.mcp.server:{}", server.name)),
                    });
                    paint.placed.push(Placed {
                        x: SESSION_SIDEBAR_WIDTH - 2 - status_width,
                        y: slot,
                        width: status_width,
                        lines: vec![PlacedLine::plain(
                            status,
                            Style::new()
                                .fg(if server.status == "failed" { theme.error } else { fg_muted })
                                .bg(raised),
                        )],
                        tag: None,
                    });
                    // The row's click surface (mcp.tsx:76-84):
                    // release opens the MCP dialog on this server.
                    paint.mcp_hits.rows.push((
                        server.name.clone(),
                        crate::store::HitRect {
                            x: 2,
                            y: slot,
                            width: SESSION_SIDEBAR_WIDTH - 4,
                            height: 1,
                        },
                    ));
                }
                y += 1;
            }
        }
        y += 1; // the inter-section gap
    }

    for section in &view.sections.plugins {
        if section.title.is_empty() && section.rows.is_empty() {
            continue;
        }
        let place_slot = |paint: &mut SidebarPaint, text: &str, y: usize| {
            let Some(row) = slot_row(y) else { return };
            if row >= footer_top {
                return;
            }
            let text = crate::wrap::take_width(text, SESSION_SIDEBAR_WIDTH - 4);
            paint.placed.push(Placed {
                x: 2,
                y: row,
                width: crate::wrap::display_width(&text),
                lines: vec![PlacedLine::plain(
                    text,
                    Style::new().fg(fg_base).bg(raised).add_modifier(Modifier::BOLD),
                )],
                tag: Some("sidebar.plugin-section".into()),
            });
        };
        if !section.title.is_empty() {
            place_slot(&mut paint, &section.title, y);
            y += 1;
        }
        for row in &section.rows {
            if let Some(slot) = slot_row(y).filter(|slot| *slot < footer_top) {
                let text = crate::wrap::take_width(row, SESSION_SIDEBAR_WIDTH - 4);
                paint.placed.push(Placed {
                    x: 2,
                    y: slot,
                    width: crate::wrap::display_width(&text),
                    lines: vec![PlacedLine::plain(text, Style::new().fg(fg_muted).bg(raised))],
                    tag: None,
                });
            }
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

    /// The wheel offset re-slices the content slot (auditA #1): the
    /// sections shift up by `scroll`, rows scrolled past the slot's
    /// viewport drop, and the title + footer chrome stay put — v2's
    /// `sidebar.content` scrollbox clipping.
    #[test]
    fn scrolled_content_slot_shifts_sections_and_keeps_chrome() {
        let theme = theme();
        let mut view = SidebarView::fixture("Sidebar fixture", "/tmp/opencode/x:main");
        view.onboarding = false;
        view.sections.plugins = vec![SidebarSection {
            plugin_id: "omos".into(),
            title: "Preset".into(),
            rows: (0..8).map(|index| format!("row {index}")).collect(),
        }];
        let paint = render_sidebar_scrolled(&view, &theme, 12, 2);
        let slot: Vec<(usize, String)> = paint
            .placed
            .iter()
            .filter(|p| p.y >= 2 && p.y < 10)
            .map(|p| (p.y, p.lines.iter().map(PlacedLine::text).collect::<String>()))
            .collect();
        assert_eq!(
            slot,
            vec![
                (2, "row 1".to_owned()),
                (3, "row 2".to_owned()),
                (4, "row 3".to_owned()),
                (5, "row 4".to_owned()),
                (6, "row 5".to_owned()),
                (7, "row 6".to_owned()),
                (8, "row 7".to_owned()),
            ],
            "scroll 2 hides the section title + first body row; the rest shift up (the slot ends above the location row at 10)"
        );
        // Chrome: the title never moves; the location row stays at
        // height − 2.
        let title =
            paint.placed.iter().find(|p| p.tag.as_deref() == Some("sidebar.title")).expect("title");
        assert_eq!(title.y, 1, "the title is chrome — it never scrolls");
        let location = paint
            .placed
            .iter()
            .find(|p| p.tag.as_deref() == Some("sidebar.footer.location"))
            .expect("location");
        assert_eq!(location.y, 10, "the footer stack stays pinned to the bottom");
    }
}

#[cfg(test)]
mod surf1_tests {
    use super::*;

    fn theme() -> ThemeStyles {
        crate::theme::ThemeState::v2_dark().styles()
    }

    #[test]
    fn context_widget_rows_match_the_ref_labels() {
        let widget = SidebarContextWidget {
            tokens: Some(60_400),
            percent: Some(12),
            cost: Some("$0.42".to_owned()),
        };
        assert!(widget.is_shown());
        assert_eq!(widget.height(), 4);
        assert_eq!(widget.rows(), vec!["60,400 tokens", "12% used", "$0.42 spent"]);
        assert!(!SidebarContextWidget::default().is_shown());
        // Cost alone still shows (`state() || cost > 0`).
        assert!(
            SidebarContextWidget { cost: Some("$0.01".into()), ..Default::default() }.is_shown()
        );
    }

    #[test]
    fn mcp_widget_collapses_past_two_servers() {
        let servers = |status: &'static str, count: usize| {
            (0..count)
                .map(|index| crate::store::McpServerRow {
                    name: format!("srv{index}"),
                    status,
                    error: None,
                })
                .collect::<Vec<_>>()
        };
        // ≤ 2 servers: rows always visible, no toggle.
        let two = SidebarMcpWidget { servers: servers("connected", 2), open: false };
        assert!(two.rows_visible());
        assert_eq!(two.height(), 3);
        // 3 servers closed: rows hide, the header carries the counts.
        let three = SidebarMcpWidget { servers: servers("connected", 3), open: false };
        assert!(!three.rows_visible());
        assert_eq!(three.height(), 1);
        assert_eq!(three.counts(), (3, 0));
        // failed + needs_auth count as errors.
        let mut bad = servers("failed", 1);
        bad.push(crate::store::McpServerRow {
            name: "auth".into(),
            status: "needs_auth",
            error: None,
        });
        let widget = SidebarMcpWidget { servers: bad, open: true };
        assert_eq!(widget.counts(), (0, 2));
        assert_eq!(SidebarMcpWidget::status_label("needs_auth"), "Sign in");
        assert_eq!(SidebarMcpWidget::status_label("pending"), "Connecting");
    }

    #[test]
    fn live_sections_paint_context_mcp_and_location() {
        let view = SidebarView {
            title: "S".to_owned(),
            sections: SidebarSections {
                context: SidebarContextWidget { tokens: Some(60_400), percent: None, cost: None },
                mcp: SidebarMcpWidget {
                    servers: vec![
                        crate::store::McpServerRow {
                            name: "github".into(),
                            status: "connected",
                            error: None,
                        },
                        crate::store::McpServerRow {
                            name: "broken".into(),
                            status: "failed",
                            error: Some("ECONNREFUSED".into()),
                        },
                    ],
                    open: true,
                },
                plugins: Vec::new(),
            },
            onboarding: false,
            location: Some("/tmp/opencode:main".to_owned()),
        };
        let paint = render_sidebar(&view, &theme(), 30);
        let frame = crate::render::paint_placed_with_fills(
            &paint.placed,
            &paint.fills,
            (42, 30),
            crate::components::tool_rows::fill_style(&theme()),
        );
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Context"), "{grid}");
        assert!(grid.contains("60,400 tokens"), "{grid}");
        assert!(grid.contains("MCP"), "{grid}");
        assert!(grid.contains("github"), "{grid}");
        assert!(grid.contains("Connected"), "{grid}");
        assert!(grid.contains("Error"), "the failed server's status: {grid}");
        assert!(grid.contains("/tmp/opencode:main"), "the location row: {grid}");
        assert_eq!(view.sections.content_height(), 7, "context(2+gap) + mcp(3+gap)");
    }
}
