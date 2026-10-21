//! TUI-17 — the mini footer's subagent inspector
//! (`mini/footer.subagent.tsx`): a 14-row body over the composer that
//! shows ONE child session per view — the status glyph (spinner while
//! running; `●`/`○`/`◍` settled, ASCII `*`/`-`/`!` in mono), the
//! title + label subtitle, the detail transcript (the child's folded
//! commits, bottom-sticky scroll), and the hint row (`esc back`,
//! `ctrl+c interrupt`, `pgup/pgdn scroll`, `tab next i/n`). The
//! compact gate (`width < 56 || height < 12`) drops the pads and the
//! scroll hints.

use super::transport::SubagentChild;
use super::transport::SubagentStatus;
use crate::components::footer::canvas::{Canvas, Span};
use crate::components::footer::FooterTheme;
use crate::render::Frame;

/// `SUBAGENT_INSPECTOR_ROWS` (footer.subagent.tsx:16): the body's
/// dedicated dock rows.
pub const SUBAGENT_INSPECTOR_ROWS: usize = 14;

/// `statusIcon` (footer.subagent.tsx:41-55): the settled glyph, ASCII
/// under `mini.mono`.
#[must_use]
pub fn status_icon(status: SubagentStatus, mono: bool) -> &'static str {
    match (status, mono) {
        (SubagentStatus::Completed, true) => "*",
        (SubagentStatus::Completed, false) => "●",
        (SubagentStatus::Cancelled, true) => "-",
        (SubagentStatus::Cancelled, false) => "○",
        (SubagentStatus::Error, true) => "!",
        (SubagentStatus::Error, false) => "◍",
        (SubagentStatus::Running, true) => ".",
        (SubagentStatus::Running, false) => "◔",
    }
}

/// `statusColor` (footer.subagent.tsx:29-39).
#[must_use]
fn status_color(theme: &FooterTheme, status: SubagentStatus) -> crate::render::CellColor {
    match status {
        SubagentStatus::Completed => theme.success,
        SubagentStatus::Cancelled => theme.muted,
        SubagentStatus::Error => theme.error,
        SubagentStatus::Running => theme.running,
    }
}

/// One child's view inputs (the tracker's entry + the scroll state).
pub struct SubagentView<'a> {
    pub child: &'a SubagentChild,
    /// The selected child's 1-based position (0 = none selected yet).
    pub index: usize,
    /// The tracked children count.
    pub total: usize,
    /// The detail scroll offset (rows from the BOTTOM; sticky-bottom).
    pub scroll: usize,
    /// `mini.mono` — ASCII glyphs + the compact spinner.
    pub mono: bool,
}

/// Render the inspector body at `width × height`
/// (`RunFooterSubagentBody`): header (glyph + title + subtitle +
/// controls) → the detail rows (bottom-sticky) → the hint row when
/// the controls did not fit.
#[allow(clippy::too_many_lines)] // the header/detail/hint regions in one walk
#[must_use]
pub fn render_subagent_body(
    view: &SubagentView<'_>,
    theme: &FooterTheme,
    width: usize,
    height: usize,
) -> Frame {
    let compact = width < 56 || height < 12;
    let pad = usize::from(!compact);
    let mut canvas = Canvas::new(width, height);
    let child = view.child;
    // The title: description → title → label; the subtitle is the
    // label whenever it is not the title itself
    // (footer.subagent.tsx:71-88).
    let title = if child.description.is_empty() {
        if child.title.is_empty() {
            child.label.clone()
        } else {
            child.title.clone()
        }
    } else {
        child.description.clone()
    };
    let subtitle = if title == child.label { String::new() } else { child.label.clone() };
    // The header controls: `{interrupt} interrupt` + `i of n` — only
    // when the title leaves room (headerControls, :112-114).
    let interrupt_hint = if child.status == SubagentStatus::Running { "ctrl+c" } else { "" };
    let count = if view.total > 1 && view.index > 0 {
        format!("{} of {}", view.index, view.total)
    } else {
        String::new()
    };
    let controls_width = usize::from(!interrupt_hint.is_empty()) * (interrupt_hint.len() + 1 + 9)
        + usize::from(!count.is_empty()) * (count.chars().count() + 1);
    let header_controls =
        !compact && title.chars().count() + 2 + controls_width <= width.saturating_sub(4).max(1);
    let title_width = width
        .saturating_sub(if compact { 2 } else { 6 })
        .saturating_sub(if header_controls { controls_width } else { 0 })
        .max(1);

    // The header row: the glyph cell + the title run (the RUNNING
    // glyph is the spinner's resting frame — the animation clock
    // belongs to the caller's redraw).
    let glyph = status_icon(child.status, view.mono);
    canvas.spans(pad, pad, &[Span::new(glyph, status_color(theme, child.status), theme.surface)]);
    let visible = crate::wrap::take_width(&title, title_width.saturating_sub(1).max(1));
    canvas.spans(pad, pad + 2, &[Span::new(visible.clone(), theme.text, theme.surface)]);
    if !subtitle.is_empty() && title_width >= title.chars().count() + subtitle.chars().count() + 2 {
        let at = pad + 2 + visible.chars().count() + 2;
        canvas.spans(pad, at, &[Span::new(format!("  {subtitle}"), theme.muted, theme.surface)]);
    }
    if header_controls {
        let right = width.saturating_sub(3);
        let mut spans: Vec<Span> = Vec::new();
        if !count.is_empty() {
            spans.push(Span::new(count.clone(), theme.muted, theme.surface));
        }
        if !interrupt_hint.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::new(" ", theme.muted, theme.surface));
            }
            spans.push(Span::new(
                format!("{interrupt_hint} interrupt"),
                theme.muted,
                theme.surface,
            ));
        }
        canvas.spans_right(pad, right, &spans);
    }

    // The detail transcript (sticky bottom): the visible window over
    // the child's commits, newest at the bottom, `scroll` rows up
    // from the end. Each commit renders through the scrollback
    // writer (mono off — the inspector keeps the styled rows).
    let detail_top = pad + if compact { 1 } else { 2 };
    let hints_height = usize::from(!header_controls);
    let detail_height = height.saturating_sub(detail_top + pad + hints_height).max(1);
    let commits = &child.commits;
    let total_rows = commits.len();
    let end = total_rows.saturating_sub(view.scroll.min(total_rows));
    let start = end.saturating_sub(detail_height);
    let mut y = detail_top;
    if commits.is_empty() {
        canvas.spans(y, pad, &[Span::new("No subagent activity yet", theme.muted, theme.surface)]);
    } else {
        let region = width.saturating_sub(pad + 3).max(1);
        for (index, commit) in commits.iter().enumerate().skip(start).take(end - start) {
            if y >= detail_top + detail_height {
                break;
            }
            let lines = super::scrollback::format_commit_with(
                commits.get(index.wrapping_sub(1)),
                commit,
                region,
                true,
            );
            for line in lines {
                if y >= detail_top + detail_height {
                    break;
                }
                let fg = match commit.kind {
                    super::scrollback::CommitKind::User => theme.text,
                    super::scrollback::CommitKind::Error => theme.error,
                    _ => theme.muted,
                };
                let visible = crate::wrap::take_width(&line, region);
                canvas.spans(y, pad, &[Span::new(visible, fg, theme.surface)]);
                y += 1;
            }
        }
    }

    // The hint row (when the controls did not fit the header):
    // `esc back`, `{interrupt} interrupt|stop`, `pgup/pgdn scroll`,
    // `tab next i/n` (footer.subagent.tsx:186-227).
    if !header_controls {
        let y = height.saturating_sub(1);
        let mut x = pad;
        canvas.spans(y, x, &[Span::new("esc back", theme.action_secondary_text, theme.surface)]);
        x += "esc back".len() + 1;
        if !interrupt_hint.is_empty() {
            let label = if width >= interrupt_hint.len() + 10 { "interrupt" } else { "stop" };
            let text = format!("{interrupt_hint} {label}");
            canvas.spans(
                y,
                x,
                &[Span::new(text.clone(), theme.action_secondary_text, theme.surface)],
            );
            x += text.chars().count() + 1;
        }
        if width >= 56 {
            canvas.spans(y, x, &[Span::new("pgup/pgdn scroll", theme.muted, theme.surface)]);
            x += "pgup/pgdn scroll".len() + 1;
            if view.total > 1 && view.index > 0 {
                canvas.spans(
                    y,
                    x,
                    &[Span::new(
                        format!("tab next {}/{}", view.index, view.total),
                        theme.action_secondary_text,
                        theme.surface,
                    )],
                );
            }
        }
    }
    canvas.into_frame()
}

#[cfg(test)]
mod tests {
    use super::super::scrollback::Commit;
    use super::*;

    fn child(status: SubagentStatus, commits: Vec<Commit>) -> SubagentChild {
        SubagentChild {
            session_id: "ses_child_1".to_owned(),
            label: "Subagent".to_owned(),
            title: "Refactor the parser".to_owned(),
            description: String::new(),
            status,
            commits,
        }
    }

    fn rows(frame: &Frame) -> Vec<String> {
        frame.text_grid()
    }

    /// The settled glyph table (footer.subagent.tsx:41-55) — the mono
    /// ASCII fallbacks included.
    #[test]
    fn status_glyphs_match_the_reference() {
        assert_eq!(status_icon(SubagentStatus::Completed, false), "●");
        assert_eq!(status_icon(SubagentStatus::Completed, true), "*");
        assert_eq!(status_icon(SubagentStatus::Cancelled, false), "○");
        assert_eq!(status_icon(SubagentStatus::Error, true), "!");
        assert_eq!(status_icon(SubagentStatus::Running, false), "◔");
        assert_eq!(status_icon(SubagentStatus::Running, true), ".");
    }

    /// The body paints the header (glyph + title + label subtitle +
    /// the controls) and the empty-detail placeholder; the hint row
    /// only exists when the controls did NOT fit (compact width).
    #[test]
    fn body_paints_header_detail_and_hints() {
        let theme = FooterTheme::fallback_dark();
        let owned = child(SubagentStatus::Running, Vec::new());
        let view = SubagentView { child: &owned, index: 2, total: 3, scroll: 0, mono: false };
        let frame = render_subagent_body(&view, &theme, 80, SUBAGENT_INSPECTOR_ROWS);
        let text = rows(&frame).join("\n");
        assert!(text.contains('◔'), "the running glyph");
        assert!(text.contains("Refactor the parser"), "the title");
        assert!(text.contains("Subagent"), "the label subtitle");
        assert!(text.contains("No subagent activity yet"), "the empty detail");
        assert!(text.contains("2 of 3"), "the count control");
        assert!(text.contains("ctrl+c interrupt"), "the interrupt control");
        // Compact: the controls drop, the hint row appears.
        let frame = render_subagent_body(&view, &theme, 40, SUBAGENT_INSPECTOR_ROWS);
        let text = rows(&frame).join("\n");
        assert!(text.contains("esc back"), "the close hint");
        assert!(text.contains("interrupt"), "the interrupt hint");
        assert!(!text.contains("pgup/pgdn scroll"), "no scroll hints under 56 cols");
    }

    /// The detail transcript shows the child's commits newest-last
    /// and the scroll offset walks up from the bottom.
    #[test]
    fn detail_is_bottom_sticky_with_scroll() {
        let theme = FooterTheme::fallback_dark();
        let commits: Vec<Commit> =
            (0..20).map(|index| Commit::user(&format!("row {index}"))).collect();
        let owned = child(SubagentStatus::Completed, commits);
        // Scroll 0: the window's tail — the writer renders each user
        // commit over two rows (the `>` echo + the text), so the last
        // ~5 commits fill the ~10 detail rows.
        let view = SubagentView { child: &owned, index: 0, total: 1, scroll: 0, mono: false };
        let frame = render_subagent_body(&view, &theme, 80, SUBAGENT_INSPECTOR_ROWS);
        let grid = rows(&frame);
        let bottom = grid[frame.cells.len() - 2].trim_end();
        assert!(bottom.contains("row 14"), "the newest window pins to the bottom: {bottom}");
        // Scrolled 4: the window walks up four commits.
        let view = SubagentView { child: &owned, index: 0, total: 1, scroll: 4, mono: false };
        let frame = render_subagent_body(&view, &theme, 80, SUBAGENT_INSPECTOR_ROWS);
        let grid = rows(&frame);
        let bottom = grid[frame.cells.len() - 2].trim_end();
        assert!(bottom.contains("row 10"), "scroll moves the window: {bottom}");
    }

    /// Probe: the `kid_a` shape from the PTY failure (title + folded
    /// text) renders the header and detail on DISTINCT rows.
    #[test]
    fn kid_shape_renders_header_and_detail_rows() {
        let theme = FooterTheme::fallback_dark();
        let commits = vec![Commit {
            kind: super::super::scrollback::CommitKind::Assistant,
            text: "Working on it".to_owned(),
            phase: super::super::scrollback::Phase::Progress,
            ..Commit::default()
        }];
        let owned = SubagentChild {
            session_id: "ses_kid_a".to_owned(),
            label: "Subagent".to_owned(),
            title: "Check the parsers".to_owned(),
            description: String::new(),
            status: SubagentStatus::Running,
            commits,
        };
        let view = SubagentView { child: &owned, index: 1, total: 2, scroll: 0, mono: false };
        let frame = render_subagent_body(&view, &theme, 80, 17);
        let grid = frame.text_grid();
        for (y, row) in grid.iter().enumerate() {
            eprintln!("ROW {y:2}: {row}");
        }
        let header = grid[1].trim_end();
        assert!(header.contains("Check the parsers"), "header: {header}");
        assert!(!header.contains("Working on it"), "detail leaked into header: {header}");
    }

    /// The compact gate (<56 wide or <12 tall) drops the pads and the
    /// scroll hints (footer.subagent.tsx:65-66,194-227).
    #[test]
    fn compact_width_drops_the_scroll_hints() {
        let theme = FooterTheme::fallback_dark();
        let owned = child(SubagentStatus::Running, Vec::new());
        let view = SubagentView { child: &owned, index: 0, total: 1, scroll: 0, mono: false };
        let frame = render_subagent_body(&view, &theme, 40, SUBAGENT_INSPECTOR_ROWS);
        let text = rows(&frame).join("\n");
        assert!(!text.contains("pgup/pgdn scroll"), "no scroll hints under 56 cols");
        assert!(text.contains("esc back"), "the close hint stays");
    }
}
