//! Message-flow row widgets — the `routes/session/message-parts.tsx`
//! ports: [`InlineToolRow`] (spinner + completed states), the running
//! [`Spinner`] with `ManualClock` semantics, the user-message block and
//! the assistant footer ("Build · model · 2ms").
//!
//! Every row renders as [`Placed`] runs + [`Fill`] rects at a given
//! origin; [`crate::components::message::list`] stacks them.

#![allow(clippy::cast_possible_truncation)]

use ratatui::style::{Color, Style};

use crate::components::message::model::{locale_duration, titlecase, turn_duration, Message};
use crate::layout::{Placed, PlacedLine, StyledSpan};
use crate::render::Fill;
use crate::theme::ThemeStyles;
use crate::wrap::wrap_atoms;

/// Wrap text WITH hard line breaks (F6): each `\n`-separated line
/// wraps independently (`\r\n` normalized); the concatenated streams
/// are the text block's rows. Empty lines wrap to one empty row —
/// opentui `<text>` renders them as blank lines.
fn wrap_lines(text: &str, style: Style, width: usize) -> Vec<Vec<crate::wrap::Atom>> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for line in normalized.split('\n') {
        lines.extend(wrap_atoms(&crate::wrap::flatten(line, style), width));
    }
    lines
}

/// `SPINNER_FRAMES` (`component/spinner-frames.ts`) — braille dots at
/// 80 ms/frame.
pub const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
/// `Spinner`'s interval (`interval={80}`).
pub const SPINNER_INTERVAL_MS: u64 = 80;
/// `InlineToolRow paddingLeft` (`paddingLeft={3}` on the row wrapper).
pub const ROW_PADDING_LEFT: usize = 3;
/// `INLINE_TOOL_ICON_WIDTH`.
pub const ICON_WIDTH: usize = 2;

/// The spinner glyph for an elapsed clock (`ManualClock` semantics: the
/// caller injects the clock; frame 0 at t=0 and the glyph only moves
/// when the clock advances — a frozen clock pins `⠋` forever).
#[must_use]
pub fn spinner_frame(elapsed_ms: u64) -> char {
    let index = (elapsed_ms / SPINNER_INTERVAL_MS) % SPINNER_FRAMES.len() as u64;
    SPINNER_FRAMES[usize::try_from(index).unwrap_or(0)]
}

/// One rendered row: placed runs plus background fills, all at
/// relative (0, 0) — the list offsets them.
#[derive(Debug, Clone, Default)]
pub struct RowRender {
    pub placed: Vec<Placed>,
    pub fills: Vec<Fill>,
    pub height: usize,
    /// In-TUI image placements (R-07): cell rects the frame reserves
    /// for graphics overlays (the transcript's user-message image
    /// strip). The cell grid stays authoritative for the placeholder
    /// boxes; these ride alongside for the post-draw escape emission.
    pub images: Vec<crate::image::ImagePlacement>,
}

fn text_at(x: usize, y: usize, text: impl Into<String>, style: Style) -> Placed {
    let text: String = text.into();
    let width = crate::wrap::display_width(&text);
    Placed { x, y, width, lines: vec![PlacedLine::plain(text, style)], tag: None }
}

/// Wrap a single-style label into placed lines starting at `(x, y)`.
fn wrapped_label(x: usize, y: usize, text: &str, width: usize, style: Style) -> Vec<Placed> {
    wrap_atoms(&crate::wrap::flatten(text, style), width)
        .into_iter()
        .enumerate()
        .map(|(line, atoms)| Placed {
            x,
            y: y + line,
            width,
            lines: vec![PlacedLine { runs: crate::wrap::styled_runs(&atoms) }],
            tag: None,
        })
        .collect()
}

/// The running-tool row (`InlineToolRow spinner complete={false}`):
/// braille glyph + one space + the pending label, all `text.muted`
/// (`color={theme.text.muted}` — the group passes the muted color).
///
/// `clock_ms` is the injected store clock — frozen ⇒ `⠋` (golden).
#[must_use]
pub fn running_tool_row(
    label: &str,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let muted = theme.text_muted;
    let glyph = spinner_frame(clock_ms);
    let label_width = width.saturating_sub(ROW_PADDING_LEFT + 2);
    let placed = vec![
        text_at(ROW_PADDING_LEFT, 0, glyph, muted),
        text_at(ROW_PADDING_LEFT + 2, 0, crate::wrap::take_width(label, label_width), muted),
    ];
    RowRender { placed, fills: Vec::new(), height: 1, images: Vec::new() }
}

/// The subagent tool card (`Subagent`, routes/session/index.tsx:3087-3120
/// over `InlineTool`/`InlineToolRow`, message-parts.tsx:198-275) — #40.
///
/// * completed: `✓ {Titlecase(agent)} Subagent — {description}` (the
///   icon slot's 2-wide shape, muted);
/// * running (a fresh, non-continuation call): the spinner + the SAME
///   composed label (`spinner` arm renders `children`, never
///   `pending`);
/// * streaming (args still arriving — `complete` falsy, no spinner):
///   the spinner + the `Delegating…` pending row;
/// * continuation (`input.sessionID` present): the `↳` icon + the
///   `Continue subagent — …` label, never a spinner.
///
/// `session_running` carries the child session's live status (the
/// `metadata.sessionID` lookup — `isRunning`'s second clause); the
/// `· {model}` suffix appends when the input names a model (the raw
/// reference — the model-list name resolution is the store's to add).
#[must_use]
pub fn subagent_tool_row(
    state: &crate::components::message::model::ToolState,
    session_running: Option<bool>,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    use crate::components::message::model::ToolStatus;
    let continuation = state.input.contains_key("sessionID");
    let running = state.status == ToolStatus::Running
        || (session_running.unwrap_or(false) && state.metadata.contains_key("sessionID"));
    let description = state.input.get("description").map(String::as_str).filter(|d| !d.is_empty());
    let agent = state
        .input
        .get("agent")
        .or_else(|| state.input.get("subagent_type"))
        .map_or("General", String::as_str);
    let mut label = if continuation {
        "Continue subagent".to_owned()
    } else {
        format!("{} Subagent", titlecase(agent))
    };
    label.push_str(" — ");
    label.push_str(description.unwrap_or("Subagent"));
    if let Some(model) = state.input.get("model") {
        label.push_str(" · ");
        label.push_str(model);
    }
    let spinner = !continuation && running;
    if spinner {
        running_tool_row(&label, clock_ms, theme, width)
    } else if description.is_some() || state.status == ToolStatus::Error {
        // `complete || failed` (InlineToolRow's Show gate): either a
        // truthy description or the error state renders the icon row.
        let icon = if continuation {
            '↳'
        } else if state.status == ToolStatus::Completed {
            '✓'
        } else {
            '│'
        };
        completed_tool_row(icon, &label, theme, width)
    } else {
        running_tool_row("Delegating…", clock_ms, theme, width)
    }
}

/// The completed exploration header (`→ Explored: 1 read`): icon in a
/// 2-wide slot, then the summary label — both `text.muted`
/// (`color={hover ? base : muted}` with hover off).
#[must_use]
pub fn completed_tool_row(icon: char, label: &str, theme: &ThemeStyles, width: usize) -> RowRender {
    let muted = theme.text_muted;
    let label_width = width.saturating_sub(ROW_PADDING_LEFT + ICON_WIDTH);
    let label_x = ROW_PADDING_LEFT + ICON_WIDTH;
    let mut placed = vec![text_at(ROW_PADDING_LEFT, 0, icon, muted)];
    placed.extend(wrapped_label(label_x, 0, label, label_width, muted));
    RowRender { placed, fills: Vec::new(), height: 1, images: Vec::new() }
}

/// The reasoning-group header in hide mode: collapsed `+ Thought` /
/// expanded `- Thought` (beta.8 bug 4 — the disclosure toggle flips
/// the icon and lifts the 0.6-alpha tint to the full
/// `text.feedback.warning`, `group-view.tsx:164-186`); `Thinking`
/// while running. Completed headers tint
/// `text.feedback.warning` at 0.6 alpha (premultiplied like
/// `RGBA.fromValues(r, g, b, 0.6)`) unless EXPANDED (the full
/// warning color, `hover() || expanded()`); running ones use
/// `text.base`.
#[must_use]
pub fn reasoning_header_row(
    title: Option<&str>,
    duration_ms: Option<u64>,
    completed: bool,
    expanded: bool,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    if completed {
        let warning = theme.warning;
        let tint = if expanded { warning } else { tint_0_6_over(warning, theme.background) };
        let glyph = if expanded { '-' } else { '+' };
        let mut label = String::from("Thought");
        if let Some(title) = title {
            label.push_str(": ");
            label.push_str(title);
        }
        if let Some(duration) = duration_ms.filter(|duration| *duration > 0) {
            label.push_str(" · ");
            label.push_str(&locale_duration(duration));
        }
        completed_tool_row(glyph, &label, theme, width).recolor(tint)
    } else {
        let label = match title {
            Some(title) => format!("Thinking: {title}"),
            None => "Thinking".to_owned(),
        };
        let style = theme.text;
        let glyph = spinner_frame(clock_ms);
        let label_width = width.saturating_sub(ROW_PADDING_LEFT + 2);
        let placed = vec![
            text_at(ROW_PADDING_LEFT, 0, glyph, style),
            text_at(ROW_PADDING_LEFT + 2, 0, crate::wrap::take_width(&label, label_width), style),
        ];
        RowRender { placed, fills: Vec::new(), height: 1, images: Vec::new() }
    }
}

/// One expanded reasoning child (`ThoughtEntry`,
/// group-view.tsx:351-399): the thought's markdown body inside a left
/// `┃` border (`SplitBorder` vertical, `ui/border.ts:26`),
/// `paddingLeft 1`, in `text.muted`. The entry's `marginTop 1` lives
/// on the LIST's per-row margin (the `EntryAnchor marginTop={1}` is
/// the same marginTop-1 every transcript row carries — rendered once,
/// not twice: no internal margin row here).
#[must_use]
pub fn thought_entry_row(text: &str, theme: &ThemeStyles, width: usize) -> RowRender {
    let muted = theme.text_muted;
    // The group's expanded children sit inside `paddingLeft 3`
    // (group-view.tsx:190-192); the border takes column 3 and the
    // body starts one cell right of it.
    let border_x = 3;
    let body_x = border_x + 2;
    let body_width = width.saturating_sub(body_x).max(1);
    let lines: Vec<String> = if text.trim().is_empty() {
        Vec::new()
    } else {
        crate::wrap::wrap_label(text.trim(), body_width)
    };
    let mut placed = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        placed.push(text_at(border_x, index, "┃", muted));
        placed.push(text_at(body_x, index, line.clone(), muted));
    }
    RowRender { placed, fills: Vec::new(), height: lines.len().max(1), images: Vec::new() }
}

/// A 0.6-alpha tint **composited over the background** (opentui blends
/// RGBA over the frame fill): `0.6·color + 0.4·background` per channel,
/// rounded — `(245,167,66)` over `(10,10,10)` → `(151,104,44)`.
fn tint_0_6_over(color: Color, background: Color) -> Color {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let blend = |channel: u8, base: u8| -> u8 {
        (f32::from(channel) * 0.6 + f32::from(base) * 0.4).round() as u8
    };
    match (color, background) {
        (Color::Rgb(r, g, b), Color::Rgb(br, bg_, bb)) => {
            Color::Rgb(blend(r, br), blend(g, bg_), blend(b, bb))
        }
        (other, _) => other,
    }
}

impl RowRender {
    /// Replace every run's foreground with `fg` (the reference's
    /// reasoning header paints icon and label the same tint).
    #[must_use]
    fn recolor(mut self, fg: Color) -> Self {
        for item in &mut self.placed {
            for line in &mut item.lines {
                for run in &mut line.runs {
                    run.style = run.style.fg(fg);
                }
            }
        }
        self
    }

    /// Shift the row's runs/fills down by `offset` rows (and record the
    /// new height).
    #[must_use]
    pub fn offset_y(mut self, offset: usize) -> Self {
        for item in &mut self.placed {
            item.y += offset;
        }
        for fill in &mut self.fills {
            fill.y += offset as u16;
        }
        for image in &mut self.images {
            image.y += offset as u16;
        }
        self.height += offset;
        self
    }
}

/// How the transcript's image strip renders (R-07): the resolved
/// graphics protocol and whether `session.image_preview` enabled the
/// strip at all (v2 hides it unless the config turns it on,
/// config/index.tsx:164 + session/index.tsx:2459).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageStripOptions {
    /// `session.image_preview` (default off — v2's falsy check).
    pub enabled: bool,
    /// The terminal's graphics protocol.
    pub protocol: crate::image::GraphicsProtocol,
    /// The terminal's row count (thumb height = `clamp(h/4, 4, 8)`).
    pub terminal_rows: u16,
}

impl ImageStripOptions {
    /// `SessionImages`'s thumb height: `max(4, min(8, floor(h / 4)))`
    /// (session/index.tsx:2460).
    #[must_use]
    pub fn thumb_height(&self) -> u16 {
        (self.terminal_rows / 4).clamp(4, 8)
    }

    /// Up to three thumbs show; the rest collapse into `+N more`.
    pub const MAX_VISIBLE: usize = 3;
}

/// The user-message image strip (`SessionImages`,
/// session/index.tsx:2456): a `paddingTop 1` row of thumb boxes
/// (`width = height * 2`, `gap 1`, `paddingLeft 2`/`paddingRight 2`,
/// `paddingBottom 1`) — at most [`ImageStripOptions::MAX_VISIBLE`]
/// visible plus a `+N more` cell box. With a drawable protocol the
/// thumb rects become image placements (the placeholder cells stay
/// underneath); with none (or sixel without raw sixel data) the
/// decoded PNG renders as Unicode block glyphs — v2's `blocks`
/// fallback (R-08) — and only undecodable payloads keep the
/// bordered placeholder (v2's onError → "No preview"). Empty when
/// disabled or image-less.
#[must_use]
pub fn image_strip_row(
    files: &[crate::components::message::model::UserFile],
    label_of: &dyn Fn(usize) -> String,
    options: &ImageStripOptions,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let images: Vec<&crate::components::message::model::UserFile> =
        files.iter().filter(|file| file.is_image()).collect();
    if !options.enabled || images.is_empty() {
        return RowRender::default();
    }
    let height = options.thumb_height() as usize;
    let thumb_width = height * 2;
    let visible = images.len().min(ImageStripOptions::MAX_VISIBLE);
    let overflow = images.len() - visible;

    let mut row = RowRender { height: height + 2, ..RowRender::default() };
    // paddingTop 1: the strip's boxes start on row 1.
    let mut x = 2usize; // paddingLeft 2
    for (index, image) in images.iter().enumerate().take(visible) {
        let bytes = decode_png_payload(image);
        let data = crate::image::ImageData::Png(bytes);
        if crate::image::drawable(options.protocol, &data) {
            row.images.push(crate::image::ImagePlacement {
                id: crate::image::image_id(&data),
                data,
                x: u16::try_from(x).unwrap_or(u16::MAX),
                y: 1,
                cols: thumb_width as u16,
                rows: height as u16,
            });
            // The placeholder box (border + centered label) — the cells
            // under the graphic and the no-protocol fallback.
            paint_placeholder_box(&mut row, x, 1, thumb_width, height, &label_of(index), theme);
        } else if let Some(cells) = blocks_thumbnail(&data, thumb_width, height) {
            // v2's fallback IS the block renderer (`resolveImage­
            // RenderProtocol` → "blocks" for terminals without a
            // graphics protocol — and for sixel without pixel
            // resolution): the decoded image quantizes to Unicode
            // block glyphs, aspect-fit and centered in the thumb
            // rect. No border, no label (the box is only the
            // load-error fallback, mini/scrollback.surface.ts:390).
            paint_blocks(&mut row, x, 1, thumb_width, height, &cells);
        } else {
            // Undecodable payload: v2's onError → "No preview".
            paint_placeholder_box(&mut row, x, 1, thumb_width, height, &label_of(index), theme);
        }
        x += thumb_width + 1; // gap 1
    }
    if overflow > 0 {
        let label = format!("+{overflow} more");
        // v2's overflow cell is an 8-wide box with the truncated count
        // label; size the bordered placeholder to fit the label.
        let box_width =
            (crate::wrap::display_width(&label) + 2).min(width.saturating_sub(x)).max(3);
        paint_placeholder_box(&mut row, x, 1, box_width, height, &label, theme);
    }
    row
}

/// Base64-decode the attachment payload back into image bytes (the
/// display data URI's payload half). Non-decodable payloads render as
/// the placeholder only (v2's onError → "No preview").
fn decode_png_payload(file: &crate::components::message::model::UserFile) -> Vec<u8> {
    crate::clipboard::base64_decode(&file.data).unwrap_or_default()
}

/// One thumb box: a `┌─┐│└┘` border in the muted border color with
/// the label centered on the middle row (the deterministic fallback —
/// v2 centers "No preview" the same way).
fn paint_placeholder_box(
    row: &mut RowRender,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    label: &str,
    theme: &ThemeStyles,
) {
    if width < 2 || height < 2 {
        return;
    }
    let border = Style::new().fg(theme.border).bg(theme.background_raised);
    let label_style = theme.text_muted.bg(theme.background_raised);
    let top: String = format!("┌{}┐", "─".repeat(width - 2));
    let bottom: String = format!("└{}┘", "─".repeat(width - 2));
    row.placed.push(text_at(x, y, top, border));
    row.placed.push(text_at(x, y + height - 1, bottom, border));
    for row_index in 1..height - 1 {
        row.placed.push(text_at(x, y + row_index, "│", border));
        row.placed.push(text_at(x + width - 1, y + row_index, "│", border));
    }
    // Center the label (clamped to the inner width).
    let inner = width - 2;
    let label_width = crate::wrap::display_width(label).min(inner);
    let trimmed = crate::wrap::take_width(label, inner);
    let left = (inner.saturating_sub(label_width)) / 2;
    let mid = y + height / 2;
    row.placed.push(text_at(x + 1 + left, mid, trimmed, label_style));
}

/// Decode + quantize a thumbnail's PNG into block cells, aspect-
/// fit inside `max_cols × max_rows` (`None` when the payload does
/// not decode — the "No preview" fallback's condition).
fn blocks_thumbnail(
    data: &crate::image::ImageData,
    max_cols: usize,
    max_rows: usize,
) -> Option<Vec<Vec<crate::image_blocks::BlockCell>>> {
    let crate::image::ImageData::Png(bytes) = data else {
        return None;
    };
    let image = crate::image_blocks::decode_png(bytes)?;
    let (cols, rows) =
        crate::image_blocks::fit_cells(image.width, image.height, max_cols, max_rows);
    Some(crate::image_blocks::blocks_cells(&image, cols, rows))
}

/// Paint a blocks thumbnail centered in its thumb rect: one
/// [`Placed`] per cell row, one styled run per cell (truecolor fg
/// paints the glyph's filled quadrants, bg the rest).
fn paint_blocks(
    row: &mut RowRender,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    cells: &[Vec<crate::image_blocks::BlockCell>],
) {
    let block_rows = cells.len();
    if block_rows == 0 {
        return;
    }
    let block_cols = cells[0].len();
    let offset_x = x + width.saturating_sub(block_cols) / 2;
    let offset_y = y + height.saturating_sub(block_rows) / 2;
    for (line_index, line) in cells.iter().enumerate() {
        let runs = line
            .iter()
            .map(|cell| crate::layout::StyledSpan {
                text: cell.glyph.to_string(),
                style: Style::new()
                    .fg(Color::Rgb(cell.fg[0], cell.fg[1], cell.fg[2]))
                    .bg(Color::Rgb(cell.bg[0], cell.bg[1], cell.bg[2])),
            })
            .collect();
        row.placed.push(Placed {
            x: offset_x,
            y: offset_y + line_index,
            width: block_cols,
            lines: vec![PlacedLine { runs }],
            tag: None,
        });
    }
}

/// The user-message block (`UserMessage`): a raised-background box with
/// a `SplitBorder` `┃` left border in the session agent's color, one
/// blank padding row above and below, and the text at
/// `border(1) + paddingLeft(2)`. The R-07 image strip
/// (`SessionImages`) renders inside the box above the text when
/// enabled and the message carries inline images.
#[must_use]
pub fn user_message_row(
    text: &str,
    files: &[crate::components::message::model::UserFile],
    agent_color: Color,
    theme: &ThemeStyles,
    width: usize,
    strip: &ImageStripOptions,
) -> RowRender {
    let raised = theme.background_raised;
    let fill_style = Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(raised);
    let border_style = Style::new().fg(agent_color).bg(raised);
    let text_style = Style::new().fg(theme.text.fg.unwrap_or_default()).bg(raised);
    let inner_width = width.saturating_sub(3);
    // F6 — hard line breaks: the user text block renders every `\n`
    // line (v2's `<text>{message.text}</text>` — opentui text treats
    // newlines as breaks). The wrap stream is per-LINE; the old
    // single-stream wrap ran `\n` through as a zero-width cluster,
    // collapsing "a\nb\nc" onto one row.
    let lines = wrap_lines(text, text_style, inner_width);
    // The image strip (its own paddingTop/paddingBottom rows) stacks
    // above the text block; labels reuse each file's mention text
    // (falling back to `[Image N]` by position).
    let strip_row = image_strip_row(
        files,
        &|index: usize| {
            files
                .iter()
                .filter(|file| file.is_image())
                .nth(index)
                .and_then(|file| file.name.clone())
                .unwrap_or_else(|| format!("[Image {}]", index + 1))
        },
        strip,
        theme,
        inner_width,
    );
    let strip_height = strip_row.height;
    let height = lines.len() + 2 + strip_height;
    let mut placed = Vec::new();
    for row in 0..height {
        placed.push(text_at(0, row, "┃", border_style));
    }
    for item in strip_row.placed {
        let Placed { x, y, width, lines, tag } = item;
        // The strip's own paddingLeft 2 is relative to the bordered
        // box's content area (col 1): the border(1) + 2 = col 3 the
        // text also uses.
        placed.push(Placed { x: x + 1, y, width, lines, tag });
    }
    let mut images = strip_row.images;
    for image in &mut images {
        image.x += 1;
    }
    let text_top = 1 + strip_height;
    for (index, atoms) in lines.into_iter().enumerate() {
        placed.push(Placed {
            x: 3,
            y: text_top + index,
            width: inner_width,
            lines: vec![PlacedLine { runs: crate::wrap::styled_runs(&atoms) }],
            tag: None,
        });
    }
    let box_fills =
        vec![Fill { x: 0, y: 0, width: width as u16, height: height as u16, style: fill_style }];
    RowRender { placed, fills: box_fills, height, images }
}

/// The assistant footer (`AssistantFooter`): `Build · model · 2ms` —
/// titlecased agent in the agent color, the model and duration muted
/// (` · ` separators), at `paddingLeft 3`.
///
/// A step error (`message.error`) prepends the `Error: …` row in
/// `text.feedback.error.base` plus the footer's marginTop 1 blank
/// row, and the agent label drops to muted (ref
/// session/index.tsx:1879-1896: `props.message.error ?
/// theme.text.muted : local.agent.color(...)`).
#[must_use]
pub fn assistant_footer_row(
    agent: &str,
    model: &str,
    duration_ms: u64,
    error: Option<&str>,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let agent_color = theme.agent_color(0);
    let muted = theme.text_muted;
    let agent = titlecase(agent);
    let duration = if duration_ms > 0 { locale_duration(duration_ms) } else { String::new() };
    let mut spans = vec![StyledSpan {
        text: agent,
        style: if error.is_some() {
            muted
        } else {
            Style::new().fg(agent_color).bg(theme.background)
        },
    }];
    if !model.is_empty() {
        spans.push(StyledSpan { text: format!(" · {model}"), style: muted });
    }
    if !duration.is_empty() {
        spans.push(StyledSpan { text: format!(" · {duration}"), style: muted });
    }
    let label_width = width.saturating_sub(ROW_PADDING_LEFT);
    let placed = wrap_atoms(&crate::wrap::flatten_spans(&spans), label_width)
        .into_iter()
        .enumerate()
        .map(|(line, atoms)| Placed {
            x: ROW_PADDING_LEFT,
            y: line,
            width: label_width,
            lines: vec![PlacedLine { runs: crate::wrap::styled_runs(&atoms) }],
            tag: None,
        })
        .collect();
    let footer = RowRender { placed, fills: Vec::new(), height: 1, images: Vec::new() };
    match error {
        None => footer,
        Some(message) => {
            // error row + the marginTop-1 blank + the (muted) footer.
            let error_row = assistant_error_row(message, theme, width);
            stack_rows(error_row, footer.offset_y(1))
        }
    }
}

/// The `Error: {message}` row (`AssistantFooter`'s error block):
/// `paddingLeft 3`, `text.feedback.error.base` foreground.
#[must_use]
pub fn assistant_error_row(message: &str, theme: &ThemeStyles, width: usize) -> RowRender {
    let error_style = Style::new().fg(theme.error).bg(theme.background);
    let label_width = width.saturating_sub(ROW_PADDING_LEFT);
    let text = format!("Error: {message}");
    let placed = wrap_atoms(&crate::wrap::flatten(&text, error_style), label_width)
        .into_iter()
        .enumerate()
        .map(|(line, atoms)| Placed {
            x: ROW_PADDING_LEFT,
            y: line,
            width: label_width,
            lines: vec![PlacedLine { runs: crate::wrap::styled_runs(&atoms) }],
            tag: None,
        })
        .collect();
    RowRender { placed, fills: Vec::new(), height: 1, images: Vec::new() }
}

/// Stack two rows vertically (the upper row keeps its height; the
/// lower one shifts down by it).
#[must_use]
pub fn stack_rows(upper: RowRender, mut lower: RowRender) -> RowRender {
    let height = upper.height;
    let mut combined = upper;
    for item in &mut lower.placed {
        item.y += height;
    }
    for fill in &mut lower.fills {
        fill.y += height as u16;
    }
    combined.placed.extend(lower.placed);
    combined.fills.extend(lower.fills);
    combined.height += lower.height;
    combined
}

/// Convenience: the footer's duration from the message list
/// (`turnDuration` over the transcript messages).
#[must_use]
pub fn footer_duration(message: &Message, messages: &[Message]) -> u64 {
    turn_duration(message.id(), messages)
}

/// The `spinner-frozen` golden fixture: the running `InlineToolRow`
/// exactly as `spinner_frozen.tsx` mounts it (paddingTop 1 wrapper,
/// muted color, `Exploring: 2 searches, 1 read`).
#[must_use]
pub fn spinner_frozen_frame(theme: &ThemeStyles, width: u16, height: u16) -> crate::render::Frame {
    let label = "Exploring: 2 searches, 1 read";
    let row = running_tool_row(label, 0, theme, usize::from(width)).offset_y(1); // paddingTop 1
    crate::render::paint_placed(
        &row.placed,
        (width, height),
        crate::components::tool_rows::fill_style(theme),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    #[test]
    fn spinner_frames_advance_with_the_clock_and_freeze_at_zero() {
        assert_eq!(spinner_frame(0), '⠋');
        assert_eq!(spinner_frame(79), '⠋');
        assert_eq!(spinner_frame(80), '⠙');
        assert_eq!(spinner_frame(160), '⠹');
        assert_eq!(spinner_frame(800), '⠋', "full cycle");
        // ManualClock: the same injected clock yields the same glyph —
        // no wall time participates.
        assert_eq!(spinner_frame(0), spinner_frame(0));
    }

    #[test]
    fn frozen_running_row_paints_the_golden_line() {
        let theme = theme();
        let frame = spinner_frozen_frame(&theme, 80, 6);
        let grid = frame.text_grid();
        assert_eq!(grid[0], " ".repeat(80));
        assert_eq!(
            grid[1],
            format!("   ⠋ Exploring: 2 searches, 1 read{}", " ".repeat(80 - 5 - 29))
        );
        // Styles: glyph and label muted over base.
        let glyph = frame.cell(1, 3);
        assert_eq!(glyph.ch, '⠋');
        assert_eq!(glyph.fg, crate::render::CellColor::Rgb(128, 128, 128));
        let label = frame.cell(1, 5);
        assert_eq!(label.ch, 'E');
        assert_eq!(label.fg, crate::render::CellColor::Rgb(128, 128, 128));
        // The gap cell between glyph and label is unpainted default.
        assert_eq!(frame.cell(1, 4).fg, crate::render::CellColor::Rgb(255, 255, 255));
    }

    #[test]
    fn completed_row_icon_slot_is_two_wide_with_unpainted_pad() {
        let theme = theme();
        let row = completed_tool_row('→', "Explored: 1 read", &theme, 76);
        let frame = crate::render::paint_placed(
            &row.placed,
            (76, 1),
            crate::components::tool_rows::fill_style(&theme),
        );
        let line = &frame.text_grid()[0];
        assert!(line.starts_with("   → Ex"), "{line}");
        // Icon col 3 muted, the width-2 slot's pad col 4 unpainted
        // default, label col 5 muted.
        assert_eq!(frame.cell(0, 3).fg, crate::render::CellColor::Rgb(128, 128, 128));
        assert_eq!(frame.cell(0, 4).fg, crate::render::CellColor::Rgb(255, 255, 255));
        assert_eq!(frame.cell(0, 5).fg, crate::render::CellColor::Rgb(128, 128, 128));
    }

    #[test]
    fn reasoning_header_tints_warning_at_sixty_percent() {
        let theme = theme();
        let row = reasoning_header_row(Some("Failure modes"), Some(1), true, false, 0, &theme, 76);
        let frame = crate::render::paint_placed(
            &row.placed,
            (76, 1),
            crate::components::tool_rows::fill_style(&theme),
        );
        let line = &frame.text_grid()[0];
        assert!(line.starts_with("   + Thought: Failure modes · 1ms"), "{line}");
        // (151,104,44) = warning (245? no — 252-class orange) × 0.6.
        assert_eq!(frame.cell(0, 3).fg, crate::render::CellColor::Rgb(151, 104, 44));
    }

    /// beta.8 bug 4: the EXPANDED header flips the icon to `-` and
    /// lifts the tint to the full warning color
    /// (`hover() || expanded()`, group-view.tsx:167-176).
    #[test]
    fn reasoning_header_expanded_flips_the_icon_and_lifts_the_tint() {
        let theme = theme();
        let row = reasoning_header_row(Some("Failure modes"), Some(1), true, true, 0, &theme, 76);
        let frame = crate::render::paint_placed(
            &row.placed,
            (76, 1),
            crate::components::tool_rows::fill_style(&theme),
        );
        let line = &frame.text_grid()[0];
        assert!(line.starts_with("   - Thought: Failure modes · 1ms"), "{line}");
        let expected = match theme.warning {
            ratatui::style::Color::Rgb(r, g, b) => crate::render::CellColor::Rgb(r, g, b),
            other => panic!("warning token must be RGB, got {other:?}"),
        };
        assert_eq!(frame.cell(0, 3).fg, expected, "expanded header uses the full warning color");
    }

    /// The expanded thought body: a `┃` left border, `text.muted`,
    /// body — the leading blank margin comes from the LIST's
    /// per-row marginTop (rendered once, not inside the row).
    #[test]
    fn thought_entry_row_borders_and_mutes_the_body() {
        let theme = theme();
        let row = thought_entry_row("Inspecting the parser edges", &theme, 40);
        assert_eq!(row.height, 1, "one body line, no internal margin");
        let frame = crate::render::paint_placed(
            &row.placed,
            (40, 1),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert!(grid[0].starts_with("   ┃ Inspecting the parser edges"), "{grid:?}");
        assert_eq!(frame.cell(0, 3).fg, crate::render::CellColor::Rgb(128, 128, 128));
    }

    #[test]
    fn user_message_box_pads_wraps_and_fills() {
        let theme = theme();
        let agent = theme.agent_color(0);
        let row = user_message_row(
            "What does the parser do?",
            &[],
            agent,
            &theme,
            38,
            &ImageStripOptions::default(),
        );
        assert_eq!(row.height, 3);
        let frame = crate::render::paint_placed_with_fills(
            &row.placed,
            &row.fills,
            (38, 3),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert_eq!(grid[0], "┃".to_owned() + &" ".repeat(37));
        assert_eq!(grid[1], "┃  What does the parser do?".to_owned() + &" ".repeat(11));
        assert_eq!(grid[2], "┃".to_owned() + &" ".repeat(37));
        // The fill covers every row in the raised background; the border
        // paints the agent color over it.
        assert_eq!(frame.cell(0, 0).fg, crate::render::CellColor::Rgb(92, 156, 245));
        assert_eq!(frame.cell(1, 5).bg, crate::render::CellColor::Rgb(20, 20, 20));
        assert_eq!(frame.cell(1, 37).bg, crate::render::CellColor::Rgb(20, 20, 20));
    }

    #[test]
    fn assistant_footer_matches_the_golden_line() {
        let theme = theme();
        let row = assistant_footer_row("build", "fixture/fixture", 2, None, &theme, 76);
        let frame = crate::render::paint_placed(
            &row.placed,
            (76, 1),
            crate::components::tool_rows::fill_style(&theme),
        );
        let line = &frame.text_grid()[0];
        assert_eq!(line.trim(), "Build · fixture/fixture · 2ms");
        assert_eq!(frame.cell(0, 3).fg, crate::render::CellColor::Rgb(92, 156, 245));
        assert_eq!(frame.cell(0, 8).fg, crate::render::CellColor::Rgb(128, 128, 128));
    }

    /// The error footer (`AssistantFooter` with `message.error`): the
    /// `Error: …` row in `text.feedback.error.base`, a marginTop-1
    /// blank, then the footer with its agent label MUTED (ref
    /// session/index.tsx:1879-1896).
    #[test]
    fn assistant_footer_with_error_renders_the_error_row_and_muted_label() {
        let theme = theme();
        let row = assistant_footer_row(
            "build",
            "fixture/fixture",
            0,
            Some("unexpected status 500"),
            &theme,
            76,
        );
        assert_eq!(row.height, 3, "error row + blank + footer");
        let frame = crate::render::paint_placed(
            &row.placed,
            (76, 3),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert_eq!(grid[0].trim(), "Error: unexpected status 500");
        assert_eq!(grid[1].trim(), "", "the marginTop-1 blank row");
        assert_eq!(grid[2].trim(), "Build · fixture/fixture");
        // The error row paints the feedback error color…
        assert_eq!(frame.cell(0, 3).fg, crate::render::CellColor::Rgb(224, 108, 117));
        // …and the agent label dropped to muted (not the categorical
        // blue the healthy footer carries).
        assert_eq!(frame.cell(2, 3).fg, crate::render::CellColor::Rgb(128, 128, 128));
    }

    // -- R-07: the user-message image strip ---------------------------------

    use crate::components::message::model::UserFile;
    use crate::image::{GraphicsProtocol, ImageData};

    fn png_file(data: &str) -> UserFile {
        UserFile { mime: "image/png".into(), data: data.into(), name: Some("shot.png".into()) }
    }

    fn strip_options(protocol: GraphicsProtocol) -> ImageStripOptions {
        ImageStripOptions { enabled: true, protocol, terminal_rows: 24 }
    }

    #[test]
    fn image_strip_sizes_thumbs_per_v2() {
        // thumb height = clamp(h/4, 4..8); width = height * 2.
        assert_eq!(strip_options(GraphicsProtocol::None).thumb_height(), 6);
        assert_eq!(
            ImageStripOptions { enabled: true, protocol: GraphicsProtocol::None, terminal_rows: 8 }
                .thumb_height(),
            4
        );
        assert_eq!(
            ImageStripOptions {
                enabled: true,
                protocol: GraphicsProtocol::None,
                terminal_rows: 200
            }
            .thumb_height(),
            8
        );
    }

    #[test]
    fn image_strip_disabled_or_imageless_renders_nothing() {
        let theme = theme();
        let disabled = image_strip_row(
            &[png_file("AAECAw==")],
            &|index: usize| format!("[Image {}]", index + 1),
            &ImageStripOptions::default(),
            &theme,
            60,
        );
        assert_eq!(disabled.height, 0);
        assert!(disabled.placed.is_empty());
        assert_eq!(disabled.images, Vec::new());

        let imageless = image_strip_row(
            &[],
            &|index: usize| format!("[Image {}]", index + 1),
            &strip_options(GraphicsProtocol::Kitty),
            &theme,
            60,
        );
        assert_eq!(imageless.height, 0);
    }

    #[test]
    fn image_strip_kitty_collects_placements_and_placeholder_cells() {
        let theme = theme();
        let payload = crate::clipboard::base64_encode::encode(b"\x89PNG-tiny");
        let row = image_strip_row(
            &[png_file(&payload)],
            &|index: usize| format!("[Image {}]", index + 1),
            &strip_options(GraphicsProtocol::Kitty),
            &theme,
            60,
        );
        // paddingTop 1 + 6 thumb rows + paddingBottom 1.
        assert_eq!(row.height, 8);
        assert_eq!(row.images.len(), 1);
        let image = &row.images[0];
        assert_eq!((image.x, image.y, image.cols, image.rows), (2, 1, 12, 6));
        assert_eq!(image.data, ImageData::Png(b"\x89PNG-tiny".to_vec()));
        assert_eq!(image.id, crate::image::image_id(&image.data));
        // The placeholder box still paints under the graphic (the
        // bordered thumb with its centered label).
        let frame = crate::render::paint_placed(
            &row.placed,
            (60, 8),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert!(
            grid[1].starts_with("  ┌──────────┐"),
            "thumb border at paddingLeft 2: {}",
            grid[1]
        );
        assert!(grid[4].contains("[Image 1]"), "centered label: {}", grid[4]);
    }

    #[test]
    fn image_strip_fallback_renders_placeholders_without_placements() {
        let theme = theme();
        let payload = crate::clipboard::base64_encode::encode(b"\x89PNG-tiny");
        let row = image_strip_row(
            &[png_file(&payload), png_file(&payload), png_file("AAAA"), png_file("BBBB")],
            &|index: usize| format!("[Image {}]", index + 1),
            &strip_options(GraphicsProtocol::None),
            &theme,
            60,
        );
        // No protocol ⇒ no overlays; the placeholder boxes carry the
        // strip; 4 images ⇒ 3 thumbs + "+1 more".
        assert_eq!(row.images, Vec::new());
        let frame = crate::render::paint_placed(
            &row.placed,
            (60, 8),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert!(grid[4].contains("+1 more"), "overflow box: {}", grid[4]);
        let thumb_count = grid[1].matches('┌').count();
        assert_eq!(thumb_count, 4, "3 thumbs + the overflow box");
    }

    #[test]
    fn user_message_row_stacks_the_strip_above_the_text() {
        let theme = theme();
        let agent = theme.agent_color(0);
        let payload = crate::clipboard::base64_encode::encode(b"\x89PNG-tiny");
        let row = user_message_row(
            "describe this",
            &[png_file(&payload)],
            agent,
            &theme,
            38,
            &strip_options(GraphicsProtocol::Kitty),
        );
        // strip (1 + 6 + 1) + the message box's own 1 + text + 1.
        assert_eq!(row.height, 11);
        // The border spans the whole block; the strip lands at the
        // content column (3), the text below the strip.
        let frame = crate::render::paint_placed_with_fills(
            &row.placed,
            &row.fills,
            (38, 11),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert!(grid[1].starts_with("┃  ┌"), "border + thumb box top: {}", grid[1]);
        assert_eq!(grid[9].trim(), "┃  describe this");
        assert_eq!(grid.len(), 11);
        // The placement shifted with the strip (+1 past the border).
        assert_eq!(row.images[0].x, 3);
    }

    /// F6 — multi-line user messages render EVERY line: `\n` is a hard
    /// break (v2's `<text>{message.text}</text>`); the old single-wrap
    /// stream collapsed "a\nb\nc" onto row one.
    #[test]
    fn user_message_row_renders_every_line() {
        let theme = theme();
        let row = user_message_row(
            "alpha\nbeta\nc",
            &[],
            theme.agent_color(0),
            &theme,
            40,
            &ImageStripOptions::default(),
        );
        // 3 text lines + the box's padding rows.
        assert_eq!(row.height, 5, "one row per line + 2 box rows");
        let frame = crate::render::paint_placed_with_fills(
            &row.placed,
            &row.fills,
            (40, 5),
            crate::components::tool_rows::fill_style(&theme),
        );
        let grid = frame.text_grid();
        assert_eq!(grid[1].trim_end(), "┃  alpha", "line 1: {}", grid[1]);
        assert_eq!(grid[2].trim_end(), "┃  beta", "line 2: {}", grid[2]);
        assert_eq!(grid[3].trim_end(), "┃  c", "line 3: {}", grid[3]);
        // A long line still WRAPS inside its own hard-break segment.
        let long = "word ".repeat(20);
        let wrapped = user_message_row(
            &format!("{long}\nsecond"),
            &[],
            theme.agent_color(0),
            &theme,
            40,
            &ImageStripOptions::default(),
        );
        assert!(wrapped.height > 4, "the long segment wraps ({} rows)", wrapped.height);
    }
}
