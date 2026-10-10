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
    RowRender { placed, fills: Vec::new(), height: 1 }
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
    RowRender { placed, fills: Vec::new(), height: 1 }
}

/// The reasoning-group header in hide mode: `+ Thought: <title> · <d>`
/// (open) or `Thinking: <title>` while running. Completed headers tint
/// `text.feedback.warning` at 0.6 alpha (premultiplied like
/// `RGBA.fromValues(r, g, b, 0.6)`); running ones use `text.base`.
#[must_use]
pub fn reasoning_header_row(
    title: Option<&str>,
    duration_ms: Option<u64>,
    completed: bool,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    if completed {
        let warning = theme.warning;
        let tint = tint_0_6_over(warning, theme.background);
        let mut label = String::from("Thought");
        if let Some(title) = title {
            label.push_str(": ");
            label.push_str(title);
        }
        if let Some(duration) = duration_ms.filter(|duration| *duration > 0) {
            label.push_str(" · ");
            label.push_str(&locale_duration(duration));
        }
        completed_tool_row('+', &label, theme, width).recolor(tint)
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
        RowRender { placed, fills: Vec::new(), height: 1 }
    }
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
        self.height += offset;
        self
    }
}

/// The user-message block (`UserMessage`): a raised-background box with
/// a `SplitBorder` `┃` left border in the session agent's color, one
/// blank padding row above and below, and the text at
/// `border(1) + paddingLeft(2)`.
#[must_use]
pub fn user_message_row(
    text: &str,
    agent_color: Color,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let raised = theme.background_raised;
    let fill_style = Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(raised);
    let border_style = Style::new().fg(agent_color).bg(raised);
    let text_style = Style::new().fg(theme.text.fg.unwrap_or_default()).bg(raised);
    let inner_width = width.saturating_sub(3);
    let lines = wrap_atoms(&crate::wrap::flatten(text, text_style), inner_width);
    let height = lines.len() + 2; // paddingTop 1 + text + paddingBottom 1
    let mut placed = Vec::new();
    for row in 0..height {
        placed.push(text_at(0, row, "┃", border_style));
    }
    for (index, atoms) in lines.into_iter().enumerate() {
        placed.push(Placed {
            x: 3,
            y: index + 1,
            width: inner_width,
            lines: vec![PlacedLine { runs: crate::wrap::styled_runs(&atoms) }],
            tag: None,
        });
    }
    let fills =
        vec![Fill { x: 0, y: 0, width: width as u16, height: height as u16, style: fill_style }];
    RowRender { placed, fills, height }
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
    let footer = RowRender { placed, fills: Vec::new(), height: 1 };
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
    RowRender { placed, fills: Vec::new(), height: 1 }
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
        let row = reasoning_header_row(Some("Failure modes"), Some(1), true, 0, &theme, 76);
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

    #[test]
    fn user_message_box_pads_wraps_and_fills() {
        let theme = theme();
        let agent = theme.agent_color(0);
        let row = user_message_row("What does the parser do?", agent, &theme, 38);
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
}
