//! G-272⑩ — the styled scrollback entry writer: mini's transcript
//! bodies rendered with theme colors + syntax highlighting instead of
//! the mono plain-text rows, the port of the ref's `createEntry`
//! renderable family (`mini/scrollback.surface.ts:86-527`) over
//! `entryLook`/`entryColor` (`mini/scrollback.shared.ts:17-86`).
//!
//! The rows this module produces are `Vec<Vec<StyledSpan>>` — the
//! same span shape the exit splash's styled path uses — and the
//! lifecycle flush converts them to opentui `CapturedLine` span rows
//! (the G-272⑩ styled commit payload) so the tail-column math counts
//! span text, not SGR bytes.
//!
//! Theme basis: `fallback_dark` (`ThemeState::v2_dark()`) resolved
//! once — live OSC-probe theme switching is OUT OF SCOPE v1 (the ref
//! re-resolves per render; mini's quantizer basis is the v2 default
//! theme, matching `mini/theme.rs`'s pinned corpus). Every Rgb fg
//! passes the mini oklab quantizer (`scrollback_color`), the ref's
//! `theme.scrollback()` guard (`mini/theme.ts:123-126`).
//!
//! Out of scope v1 (documented divergences): image previews (the
//! ref's `writeImage` — `imagePreview` gated there too), OSC-probe
//! theme switching. The mono flag keeps the ENTIRE plain path (the
//! ref's `mono.ts` gate): `Lifecycle::append` dispatches on it.

use super::scrollback::{
    Commit, CommitKind, Phase, entry_body_with, entry_flags, separator_rows,
};
use super::theme::scrollback_color;
use crate::layout::StyledSpan;
use crate::render::CellColor;
use crate::style::{Color, Modifier, Style};
use crate::wrap::{flatten_spans, styled_runs, wrap_atoms};

/// `toolFiletype` (tool.ts:1340-1355): the file's extension resolved
/// through the G-302 extension table (the port of v2's
/// `LANGUAGE_EXTENSIONS`, util/filetype.ts — ~70 entries, js →
/// typescript normalization included, ref tool.ts:1353-1355), falling
/// back to a plain fence (`""`) for unknown extensions — the ref's
/// graceful degrade. Delegated rather than duplicated: one table, two
/// consumers.
#[must_use]
pub fn tool_filetype(file: &str) -> &'static str {
    crate::components::syntax::filetype_for_path(file).unwrap_or("")
}

/// The per-kind scrollback entry look (`entryLook` /
/// `entryColor`, scrollback.shared.ts:17-86): system + compaction
/// muted, user default, error BOLD in the error color, tool captions
/// muted (the ref paints captions through the group's dim look).
#[derive(Debug, Clone)]
pub struct EntryLook {
    /// `text.base` — user + default rows.
    pub base: Style,
    /// `text.muted` — system notes, tool captions, shell prompts,
    /// the compaction divider + summary rows.
    pub muted: Style,
    /// `text.feedback.error.base` + BOLD — error rows
    /// (`error: { color: "error", bold: true }`).
    pub error: Style,
}

impl EntryLook {
    /// Resolve from the v2 dark fallback theme (the v1 basis — see
    /// the module header).
    #[must_use]
    pub fn fallback_dark() -> Self {
        let styles = crate::theme::ThemeState::v2_dark().styles();
        Self {
            base: Style::new().fg(styles.text.fg.unwrap_or(Color::Reset)),
            muted: styles.text_muted,
            error: Style::new().fg(styles.error).add_modifier(Modifier::BOLD),
        }
    }

    fn for_kind(&self, kind: CommitKind) -> Style {
        match kind {
            CommitKind::System => self.muted,
            CommitKind::User => self.base,
            CommitKind::Error => self.error,
            CommitKind::Tool => self.muted,
            CommitKind::Assistant | CommitKind::Reasoning => self.base,
        }
    }
}

/// The markdown renderer + styles for assistant/reasoning/tool-code
/// bodies — `MarkdownStyles::from_theme` over the same fallback_dark
/// basis (theme.rs:114-192 quantizer corpus is the v2 default theme).
#[must_use]
pub fn markdown_styles() -> crate::components::message::markdown::MarkdownStyles {
    crate::components::message::markdown::MarkdownStyles::from_theme(
        &crate::theme::ThemeState::v2_dark().styles(),
    )
}

/// The styled twin of [`super::scrollback::format_commit_with`]:
/// same separator/flags/trailing-blank shape (the G-68/G-221 folds
/// ride the shared helpers), but the body rows carry per-kind styles
/// and the markdown-eligible kinds render through the styled markdown
/// pipeline (per-commit re-render, like the ref re-sets entry
/// content).
#[must_use]
pub fn format_commit_styled_with(
    prev: Option<&Commit>,
    commit: &Commit,
    width: usize,
    shell_output: bool,
    look: &EntryLook,
) -> Vec<Vec<StyledSpan>> {
    let width = width.max(1);
    let (_, trailing_newline) = entry_flags(commit);
    let mut rows: Vec<Vec<StyledSpan>> = Vec::new();
    if prev.is_some() && separator_rows(prev, commit) > 0 {
        rows.push(Vec::new());
    }
    if commit.compaction {
        rows.push(styled_row(
            &format!("─── {} ───", commit.text.trim()),
            look.muted,
            width,
        ));
        return rows;
    }
    if commit.summary {
        rows.push(styled_row(commit.text.trim(), look.muted, width));
        return rows;
    }
    for row in entry_body_styled(commit, width, shell_output, look) {
        if row.is_empty() {
            rows.push(Vec::new());
        } else {
            rows.extend(wrap_styled(&row, width));
        }
    }
    if trailing_newline && !rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

/// One text row as a single-span styled row, pre-wrapped (the plain
/// path's `wrap_label` shape).
fn styled_row(text: &str, style: Style, width: usize) -> Vec<StyledSpan> {
    crate::wrap::wrap_label(text, width.max(1))
        .into_iter()
        .map(|line| StyledSpan { text: line, style, link: None })
        .collect()
}

/// Wrap an already-spanned row at `width` (the markdown pipeline's
/// flatten → wrap → runs shape).
fn wrap_styled(spans: &[StyledSpan], width: usize) -> Vec<Vec<StyledSpan>> {
    let atoms = flatten_spans(spans);
    wrap_atoms(&atoms, width.max(1))
        .into_iter()
        .map(|line| styled_runs(&line))
        .collect()
}

/// `entryBody` styled arm (`entry.body.ts:189-234`): the markdown /
/// code / text-run mapping per kind. Non-markdown kinds reuse the
/// mono writer's row logic (`entry_body_with` — the G-272 view gates,
/// structured snapshots, fallbacks) and paint the rows with the
/// kind's look.
fn entry_body_styled(
    commit: &Commit,
    width: usize,
    shell_output: bool,
    look: &EntryLook,
) -> Vec<Vec<StyledSpan>> {
    let raw = commit.text.replace("\r\n", "\n").replace('\r', "\n");
    let md = markdown_styles();

    // Tool structured Code snapshot (G-134): the card's title row
    // muted, the body a fenced code block in the snapshot's language
    // (`toolFiletype`, tool.ts:1340-1349) — syntax-highlighted through
    // the markdown pipeline's code path.
    if commit.kind == CommitKind::Tool
        && commit.phase == Phase::Final
        && commit.tool_state == Some("completed")
        && matches!(commit.snapshot, crate::mini::tool_snapshot::ToolSnapshot::Code { .. })
    {
        if let crate::mini::tool_snapshot::ToolSnapshot::Code { title, content, file } =
            &commit.snapshot
        {
            let lang = tool_filetype(file);
            let source = format!("```{lang}\n{content}\n```");
            let mut rows = vec![vec![StyledSpan {
                text: title.clone(),
                style: look.muted,
                link: None,
            }]];
            rows.extend(
                crate::components::message::markdown::render_markdown(&source, width, &md)
                    .into_iter()
                    .map(|line| line.0),
            );
            return rows;
        }
    }

    // Assistant + reasoning bodies render as MARKDOWN (the ref mounts
    // a Markdown renderable per entry and re-sets content per commit;
    // the port re-renders each commit's chunk). DIVERGENCE (bounded,
    // documented): the ref renders markdown over the ACCUMULATED entry
    // content with a committedRows/committedBlocks high-water
    // (scrollback.surface.ts:287-328), while the port's transcript is
    // append-only and each chunk delta renders independently — so
    // constructs split across chunk boundaries (an unclosed fence, a
    // list growing across deltas) render per-chunk. The mono writer
    // has the same per-chunk granularity; the styled path keeps it.
    if !commit.compaction && !commit.summary {
        match (commit.kind, commit.phase) {
            (CommitKind::Assistant, Phase::Progress) if !raw.trim().is_empty() => {
                return crate::components::message::markdown::render_markdown(&raw, width, &md)
                    .into_iter()
                    .map(|line| line.0)
                    .collect();
            }
            (CommitKind::Reasoning, Phase::Progress) if !raw.trim().is_empty() => {
                return crate::components::message::markdown::render_markdown(&raw, width, &md)
                    .into_iter()
                    .map(|line| line.0)
                    .collect();
            }
            _ => {}
        }
    }

    // Everything else: the mono writer's rows (gates + snapshots +
    // fallbacks identical), painted with the kind look (the shell `$`
    // start caption muted).
    let kind_style = look.for_kind(commit.kind);
    let plain = entry_body_with(commit, shell_output);
    let mut rows = Vec::with_capacity(plain.len());
    for row in &plain {
        let style =
            if commit.shell.is_some() && commit.phase == Phase::Start { look.muted } else { kind_style };
        rows.push(vec![StyledSpan { text: row.clone(), style, link: None }]);
    }
    rows
}

/// The styled commit bytes' span payload: convert styled rows to
/// opentui `CapturedLine` span sets — fg quantized through the mini
/// oklab quantizer (`scrollback_color`), bg terminal-default, the
/// ratatui modifier word mapped to opentui attribute bits, and the
/// raw-mode CR appended to each row's final span (the row payload /
/// newline-intent split the plain styled path established — the
/// session is in raw mode, so the engine's LF separator needs the CR
/// half to return the cursor).
#[must_use]
pub fn styled_rows_to_captured(rows: &[Vec<StyledSpan>]) -> Vec<opentui_core::buffer::CapturedLine> {
    rows.iter().map(|spans| {
        // Coalesce same-style spans (get_span_lines' run shape).
        let mut runs: Vec<(Style, String)> = Vec::new();
        for span in spans {
            match runs.last_mut() {
                Some((style, text)) if *style == span.style => text.push_str(&span.text),
                _ => runs.push((span.style, span.text.clone())),
            }
        }
        let mut spans: Vec<opentui_core::buffer::CapturedSpan> = runs
            .into_iter()
            .map(|(style, text)| {
                let width = crate::wrap::display_width(&text) as u32;
                opentui_core::buffer::CapturedSpan {
                    text,
                    fg: captured_fg(style.fg),
                    bg: opentui_core::Rgba::default_color(0, 0, 0, 255),
                    attributes: captured_attributes(style.add_modifier),
                    width,
                }
            })
            .collect();
        // The raw-mode CR rides the final span (cluster width 0 —
        // the tail-column math is unaffected).
        if let Some(last) = spans.last_mut() {
            last.text.push('\r');
        }
        opentui_core::buffer::CapturedLine { spans }
    })
    .collect()
}

/// fg → opentui `Rgba`: Rgb quantizes through the mini oklab
/// nearest-neighbor (`theme.scrollback()`), Indexed passes, default /
/// reset ride the terminal-default intent (`39`).
fn captured_fg(color: Option<Color>) -> opentui_core::Rgba {
    match color {
        Some(Color::Rgb(r, g, b)) => match scrollback_color(CellColor::Rgb(r, g, b)) {
            CellColor::Indexed(index) => opentui_core::Rgba::from_index(index),
            CellColor::Rgb(r, g, b) => opentui_core::Rgba::rgb(r, g, b),
            _ => opentui_core::Rgba::default_color(255, 255, 255, 255),
        },
        Some(Color::Indexed(index)) => opentui_core::Rgba::from_index(index),
        _ => opentui_core::Rgba::default_color(255, 255, 255, 255),
    }
}

/// ratatui `Modifier` word → opentui text-attribute bits (the bit
/// positions differ: ratatui BOLD=bit0 … CROSSED_OUT=bit8; opentui
/// BOLD=1 … STRIKETHROUGH=128).
fn captured_attributes(modifier: Modifier) -> u8 {
    use opentui_core::ansi::text_attributes as ot;
    let mut bits = 0u8;
    if modifier.contains(Modifier::BOLD) {
        bits |= ot::BOLD;
    }
    if modifier.contains(Modifier::DIM) {
        bits |= ot::DIM;
    }
    if modifier.contains(Modifier::ITALIC) {
        bits |= ot::ITALIC;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        bits |= ot::UNDERLINE;
    }
    if modifier.contains(Modifier::SLOW_BLINK) {
        bits |= ot::BLINK;
    }
    if modifier.contains(Modifier::REVERSED) {
        bits |= ot::INVERSE;
    }
    if modifier.contains(Modifier::HIDDEN) {
        bits |= ot::HIDDEN;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        bits |= ot::STRIKETHROUGH;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mini::scrollback::Commit;

    fn assistant(text: &str) -> Commit {
        Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Progress,
            text: text.to_owned(),
            ..Commit::default()
        }
    }

    /// Assistant markdown bodies render through the styled markdown
    /// pipeline: headings carry the heading color (quantized), and
    /// the row set is the markdown render, not split_rows.
    #[test]
    fn assistant_bodies_render_as_markdown() {
        let look = EntryLook::fallback_dark();
        let rows = format_commit_styled_with(None, &assistant("# Title\n\nbody"), 40, true, &look);
        let flat: Vec<String> =
            rows.iter().map(|row| row.iter().map(|s| s.text.as_str()).collect()).collect();
        assert!(flat.iter().any(|r| r.contains("Title")), "heading row present: {flat:?}");
        assert!(flat.iter().any(|r| r.contains("body")));
        // the heading row carries a non-default style (theme heading
        // color), unlike the mono base
        let heading = rows.iter().find(|row| {
            row.iter().any(|s| s.text.contains("Title") && s.style.fg.is_some())
        });
        assert!(heading.is_some(), "styled heading span");
    }

    /// Kind look mapping: system muted, error BOLD error color, user
    /// base, tool captions muted.
    #[test]
    fn kind_looks_map_entry_colors() {
        let look = EntryLook::fallback_dark();
        let styles = format_commit_styled_with(None, &Commit::system("note"), 40, true, &look);
        assert_eq!(styles[0][0].style, look.muted, "system muted");
        let err = format_commit_styled_with(None, &Commit::error("boom"), 40, true, &look);
        assert_eq!(err[0][0].style, look.error, "error bold error color");
        assert!(err[0][0].style.add_modifier.contains(Modifier::BOLD));
        let user = format_commit_styled_with(None, &Commit::user("hi"), 40, true, &look);
        assert_eq!(user[0][0].style, look.base, "user default");
    }

    /// Rgb fgs quantize through the oklab nearest-neighbor before
    /// landing in the CapturedSpan payload (the pinned corpus colors:
    /// 238-gray → 255, muted 128-gray → 8).
    #[test]
    fn captured_payload_quantizes_fg() {
        let rows = vec![vec![StyledSpan {
            text: "hi".to_owned(),
            style: Style::new().fg(Color::Rgb(238, 238, 238)).add_modifier(Modifier::BOLD),
            link: None,
        }]];
        let captured = styled_rows_to_captured(&rows);
        let span = &captured[0].spans[0];
        // Rgba::from_index(255) — gray ramp tail (matches the corpus)
        assert_eq!(span.fg, opentui_core::Rgba::from_index(255));
        assert_eq!(span.attributes, opentui_core::ansi::text_attributes::BOLD);
        assert!(span.text.ends_with('\r'), "raw-mode CR rides the final span");
        // default fg (no color) → terminal-default intent
        let plain = styled_rows_to_captured(&[vec![StyledSpan {
            text: "x".to_owned(),
            style: Style::new(),
            link: None,
        }]]);
        assert_eq!(plain[0].spans[0].fg, opentui_core::Rgba::default_color(255, 255, 255, 255));
    }

    /// The opentui side consumes the payload correctly: row widths
    /// count span text (the CR is zero-width), transcript bytes carry
    /// the SGR run + CR + row separator.
    #[test]
    fn opentui_styled_commit_consumes_the_payload() {
        let rows = vec![vec![StyledSpan {
            text: "hi".to_owned(),
            style: Style::new().fg(Color::Rgb(0, 0, 0)),
            link: None,
        }]];
        let commit = opentui_core::scrollback::ExternalOutputCommit {
            lines: Vec::new(),
            styled: Some(styled_rows_to_captured(&rows)),
            row_columns: 40,
            start_on_new_line: true,
            trailing_newline: true,
        };
        assert_eq!(commit.row_widths(), vec![2], "CR does not count");
        let bytes = commit.transcript_bytes(0);
        assert!(bytes.contains("\x1b[38;5;"), "indexed SGR emitted: {bytes:?}");
        assert!(bytes.contains("hi\r"), "the CR rides the payload: {bytes:?}");
    }

    /// toolFiletype: the G-302 extension table (js → typescript per
    /// the ref's tool.ts:1353-1355 normalization) + graceful degrade.
    #[test]
    fn tool_filetype_maps_extensions() {
        assert_eq!(tool_filetype("src/main.rs"), "rust");
        assert_eq!(tool_filetype("x.tsx"), "typescript");
        assert_eq!(tool_filetype("app.js"), "typescript", "v2 collapses js → ts");
        assert_eq!(tool_filetype("run.sh"), "bash");
        assert_eq!(tool_filetype("README.MD"), "markdown", "case-insensitive extension");
        assert_eq!(tool_filetype("noext"), "");
        assert_eq!(tool_filetype("x.weirdext"), "", "unknown → plain fence");
    }

    /// Plain-shape parity: the styled writer keeps the mono writer's
    /// row SHAPE (separator, trailing blank, compaction divider) for
    /// the plain families.
    #[test]
    fn styled_writer_keeps_the_commit_shape() {
        let look = EntryLook::fallback_dark();
        let user = Commit::user("one");
        let next = Commit::user("two");
        let rows = format_commit_styled_with(Some(&user), &next, 40, true, &look);
        assert!(rows.first().is_some_and(|r| r.is_empty()), "separator blank row");
        let compaction = crate::mini::scrollback::compaction_note("done");
        let rows = format_commit_styled_with(None, &compaction, 40, true, &look);
        let text: String = rows[0].iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "─── done ───");
        // the compaction divider returns without a trailing blank —
        // the plain writer's exact early-return shape
        assert_eq!(rows.len(), 1);
    }
}
