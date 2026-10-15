//! Markdown rendering for transcript/model content — RE-HOMED onto
//! opentui-core's `MarkdownRenderable` block pipeline (the retirement
//! lane; the C1 hand-rolled port it replaces stayed byte-faithful to
//! the same upstream, but the pipeline is the ONE proven line: the
//! marked@17 lexer port, the settled block tree, and the tree-sitter
//! markdown/code highlighter are all differential-oracle-zero-diff
//! against @opentui 0.5.14 — see `/opt/opentui-rust`).
//!
//! The v2 consumption shape (`routes/session/message-parts.tsx`
//! `TextPart`): `<markdown content={text.trim()} conceal
//! internalBlockMode="top-level" fg={theme.markdown.text}
//! bg={theme.background.base} …>` inside `box paddingLeft=3` — this
//! module renders THAT renderable's settled output to styled span
//! rows through the ONE wrap seam ([`crate::wrap`]):
//!
//! * `build_markdown_blocks` builds the settled block tree — the
//!   top-level coalescing (`internalBlockMode="top-level"`), the
//!   inter-block margins, the list marker/column geometry, the
//!   blockquote border, the hr.
//! * Every text-bearing spec (paragraphs/headings AND fenced code)
//!   renders through `markdown_code_styled` — the markdown-text
//!   `CodeRenderable` path (conceal, `markup.*` capture styling,
//!   `detectLinks`) or the code highlighter (registered grammars;
//!   unknown filetypes stay plain).
//! * The resulting chunks convert to [`StyledSpan`] rows: hard break
//!   per source line, then the shared word-wrap — a bold run crossing
//!   a wrap boundary keeps its style on both fragments.
//!
//! The module's byte contract moved to the pipeline's output with
//! this lane (the goldens were re-pinned to it — they now pin
//! upstream 0.5.14's semantics, not the C1 reading).

use opentui_core::renderables::code::CodeOptions;
use opentui_core::renderables::markdown::{
    build_markdown_blocks, markdown_code_styled, BlockSpec, ListItemSpec, MarkdownOptions,
};
use opentui_core::styled_text::StyledText;
use opentui_core::syntax_style::{StyleDefinitionInput, SyntaxStyle};
use ratatui::style::{Color, Modifier, Style};

use crate::layout::StyledSpan;
use crate::theme::ThemeStyles;
use crate::wrap::{flatten_spans, styled_runs, wrap_atoms};

/// One rendered markdown line: styled runs over one terminal row.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownLine(pub Vec<StyledSpan>);

impl MarkdownLine {
    /// The row's plain text (grid assertions).
    #[must_use]
    pub fn text(&self) -> String {
        self.0.iter().map(|span| span.text.as_str()).collect()
    }
}

/// The theme inputs the pipeline needs (pre-resolved once per
/// transcript walk): the fallback colors for unstyled chunks and the
/// `SyntaxStyle` built from the SAME generated syntax rules the old
/// oc-syntax path consumed (`generate_syntax` → one map, two
/// consumers).
pub struct MarkdownStyles {
    /// `fg={theme.markdown.text}` — the renderable's base fg.
    markdown_text: Style,
    /// `bg={theme.background.base}` — the renderable's base bg.
    background: Style,
    /// The blockquote's border run style (fallback when the spec
    /// carries no border color).
    quote_border: Style,
    /// The hr row style (fallback when the spec carries no border
    /// color).
    rule: Style,
    /// Fenced-code fallback base (filetypes with an oc-syntax grammar
    /// but no opentui parser — the R-01 registration set).
    code_block: Style,
    /// The capture → style resolver (upstream `SyntaxStyle`).
    pub syntax: SyntaxStyle,
    /// The oc-syntax resolver (the R-01 fenced-code fallback line).
    pub syntax_defs: oc_syntax::SyntaxStyles,
    /// Fingerprint of the syntax map (the styled cache's key salt —
    /// a theme switch invalidates every cached pipeline render).
    syntax_hash: u64,
}

fn ot_rgba(color: Color) -> Option<opentui_core::Rgba> {
    match color {
        Color::Rgb(r, g, b) => Some(opentui_core::Rgba::rgb(r, g, b)),
        _ => None,
    }
}

fn hex_of(color: Color) -> Option<String> {
    match color {
        Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        _ => None,
    }
}

impl MarkdownStyles {
    #[must_use]
    pub fn from_theme(theme: &ThemeStyles) -> Self {
        let md = theme.markdown;
        let base = Style::new().fg(md.text).bg(theme.background);
        // The opentui SyntaxStyle from the SAME generated rules the
        // oc-syntax highlighter consumed (one theme, one rule table).
        let styles = theme
            .syntax
            .iter()
            .map(|(scope, def)| {
                (
                    scope.as_str(),
                    StyleDefinitionInput {
                        fg: def.fg.and_then(|c| hex_of(Color::Rgb(c.r, c.g, c.b))),
                        bg: def.bg.and_then(|c| hex_of(Color::Rgb(c.r, c.g, c.b))),
                        bold: def.bold,
                        italic: def.italic,
                        underline: def.underline,
                        dim: def.dim,
                    },
                )
            })
            .collect::<Vec<_>>();
        Self {
            markdown_text: base,
            background: Style::new().bg(theme.background),
            quote_border: base.fg(md.block_quote),
            rule: base.fg(md.horizontal_rule),
            code_block: base.fg(md.code_block),
            syntax: SyntaxStyle::from_styles(styles.clone()),
            syntax_defs: theme.syntax.clone(),
            syntax_hash: {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                for (scope, def) in &styles {
                    scope.hash(&mut hash);
                    def.fg.hash(&mut hash);
                    def.bg.hash(&mut hash);
                    def.bold.hash(&mut hash);
                    def.italic.hash(&mut hash);
                    def.underline.hash(&mut hash);
                    def.dim.hash(&mut hash);
                }
                hash.finish()
            },
        }
    }

    /// The `MarkdownOptions` v2's `TextPart` passes.
    fn options(&self) -> MarkdownOptions {
        MarkdownOptions {
            content: String::new(),
            fg: ot_rgba(self.markdown_text.fg.unwrap_or(Color::Reset)),
            bg: ot_rgba(self.background.bg.unwrap_or(Color::Reset)),
            conceal: Some(true),
            conceal_code: None,
            hyperlinks_supported: false,
        }
    }
}

/// Render `source` (a model text part, already trimmed by the walk)
/// to styled rows at `width` — the settled `MarkdownRenderable` tree.
#[must_use]
pub fn render_markdown(source: &str, width: usize, styles: &MarkdownStyles) -> Vec<MarkdownLine> {
    let mut options = styles.options();
    source.clone_into(&mut options.content);
    let blocks = build_markdown_blocks(&options, &styles.syntax);
    let mut lines = Vec::new();
    for spec in &blocks {
        render_spec(spec, width, 0, styles, &mut lines);
    }
    lines
}

/// Render one block spec at `indent` content columns.
fn render_spec(
    spec: &BlockSpec,
    width: usize,
    indent: usize,
    styles: &MarkdownStyles,
    lines: &mut Vec<MarkdownLine>,
) {
    let content_width = width.saturating_sub(indent).max(1);
    match spec {
        BlockSpec::MarkdownText { options, margin_bottom } => {
            let text = styled_cached(options, styles);
            push_wrapped(&text, content_width, styles, lines);
            push_margins(*margin_bottom, lines);
        }
        BlockSpec::CodeBlock { options, margin_bottom } => {
            // The fence seam, by REGISTRATION SET: the R-01 oc-syntax
            // grammars (rust/python/typescript/diff/…, content-cached
            // engine) render through oc-syntax — byte-stable with the
            // pre-swap renderer; everything else (the opentui
            // markdown/ts/js/zig family, unknown filetypes) renders
            // through the pipeline's code document.
            let content = &options.content;
            let filetype = options.filetype.as_deref();
            let fallback = filetype.and_then(|ft| oc_syntax::highlight(content, ft));
            if let Some(block) = fallback {
                for (index, line) in content.split('\n').enumerate() {
                    let mut spans: Vec<StyledSpan> = block
                        .line_spans(index, styles.code_block, &styles.syntax_defs)
                        .into_iter()
                        .map(|span| StyledSpan { text: span.text, style: span.style })
                        .collect();
                    if spans.is_empty() {
                        spans.push(StyledSpan {
                            text: crate::wrap::take_width(line, content_width),
                            style: styles.code_block,
                        });
                    }
                    clip_spans(&mut spans, content_width);
                    lines.push(MarkdownLine(spans));
                }
            } else {
                let text = styled_cached(options, styles);
                push_clipped(&text, content_width, styles, lines);
            }
            push_margins(*margin_bottom, lines);
        }
        BlockSpec::Blockquote { content, border_color, margin_bottom } => {
            let mut body = Vec::new();
            render_spec(content, content_width.saturating_sub(2), 0, styles, &mut body);
            let border = border_color
                .and_then(ratatui_color)
                .map_or(styles.quote_border, |color| styles.quote_border.fg(color));
            for line in body {
                let mut runs = vec![
                    StyledSpan { text: "│".into(), style: border },
                    StyledSpan { text: " ".into(), style: styles.quote_border },
                ];
                runs.extend(line.0);
                lines.push(MarkdownLine(runs));
            }
            push_margins(*margin_bottom, lines);
        }
        BlockSpec::HorizontalRule { border_color, margin_bottom } => {
            let rule = border_color
                .and_then(ratatui_color)
                .map_or(styles.rule, |color| styles.rule.fg(color));
            lines.push(MarkdownLine(vec![StyledSpan {
                text: "─".repeat(content_width),
                style: rule,
            }]));
            push_margins(*margin_bottom, lines);
        }
        BlockSpec::List { items, margin_bottom } => {
            render_list(items, content_width, styles, lines);
            push_margins(*margin_bottom, lines);
        }
    }
}

/// `createListRenderable` rows: the marker column (padStart(width)
/// plus one space, styled `markup.list`), the item's children in the
/// content column, nested lists indenting under the parent's column.
fn render_list(
    items: &[ListItemSpec],
    width: usize,
    styles: &MarkdownStyles,
    lines: &mut Vec<MarkdownLine>,
) {
    for item in items {
        let marker = chunk_span(&item.marker_chunk, styles);
        let marker_width = item.marker_width;
        let mut body: Vec<MarkdownLine> = Vec::new();
        for (index, child) in item.children.iter().enumerate() {
            if index > 0 && item.child_margins_top.get(index).is_some_and(|m| *m > 0) {
                body.push(MarkdownLine(Vec::new()));
            }
            render_spec(child, width.saturating_sub(marker_width), 0, styles, &mut body);
        }
        for (row, line) in body.into_iter().enumerate() {
            let mut runs = Vec::new();
            if row == 0 {
                runs.push(marker.clone());
            } else {
                runs.push(StyledSpan {
                    text: " ".repeat(marker_width),
                    style: styles.markdown_text,
                });
            }
            runs.extend(line.0);
            lines.push(MarkdownLine(runs));
        }
        push_margins(item.margin_bottom, lines);
    }
}

fn push_margins(count: usize, lines: &mut Vec<MarkdownLine>) {
    for _ in 0..count {
        lines.push(MarkdownLine(Vec::new()));
    }
}

/// One styled text → wrapped rows: hard break per source line (the
/// chunk stream carries `\n`), then the shared word-wrap seam.
fn push_wrapped(
    text: &StyledText,
    width: usize,
    theme: &MarkdownStyles,
    lines: &mut Vec<MarkdownLine>,
) {
    for line_chunks in split_chunks_on_newlines(text) {
        let spans: Vec<StyledSpan> =
            line_chunks.iter().map(|chunk| chunk_span(chunk, theme)).collect();
        if spans.is_empty() {
            lines.push(MarkdownLine(Vec::new()));
            continue;
        }
        // The shared seam: flatten to styled atoms, word-wrap, split
        // back into styled runs per row (a styled run crossing the
        // boundary keeps its style on both fragments).
        let atoms = flatten_spans(&spans);
        for row in wrap_atoms(&atoms, width) {
            lines.push(MarkdownLine(styled_runs(&row)));
        }
    }
}

/// Code blocks do not word-wrap in the reference (horizontal scroll
/// in the tree); the row walk clips at the content width like
/// `wrapMode=none`.
fn push_clipped(
    text: &StyledText,
    width: usize,
    theme: &MarkdownStyles,
    lines: &mut Vec<MarkdownLine>,
) {
    for line_chunks in split_chunks_on_newlines(text) {
        let mut spans: Vec<StyledSpan> =
            line_chunks.iter().map(|chunk| chunk_span(chunk, theme)).collect();
        clip_spans(&mut spans, width);
        lines.push(MarkdownLine(spans));
    }
}

/// Split a chunk stream into per-source-line chunk slices (a chunk's
/// text may itself contain newlines — split those too).
fn split_chunks_on_newlines(styled: &StyledText) -> Vec<Vec<opentui_core::styled_text::TextChunk>> {
    let mut out: Vec<Vec<opentui_core::styled_text::TextChunk>> = vec![Vec::new()];
    for chunk in &styled.chunks {
        let mut first = true;
        for part in chunk.text.split('\n') {
            if !first {
                out.push(Vec::new());
            }
            if !part.is_empty() {
                let mut piece = chunk.clone();
                part.clone_into(&mut piece.text);
                out.last_mut().expect("a line buffer").push(piece);
            }
            first = false;
        }
    }
    out
}

/// Clip a line's styled spans at `width` display columns — the styled
/// counterpart of `wrap::take_width` (same cluster-width math), with
/// the budget shared across the line's spans.
fn clip_spans(spans: &mut Vec<StyledSpan>, width: usize) {
    let mut remaining = width;
    spans.retain_mut(|span| {
        if remaining == 0 {
            return false;
        }
        let text = crate::wrap::take_width(&span.text, remaining);
        if text.is_empty() {
            return false;
        }
        remaining -= crate::wrap::display_width(&text);
        span.text = text;
        true
    });
}

/// One pipeline chunk → a styled span. Unstyled sides fall back to
/// the renderable's base (`fg`/`bg` props); ECMA attribute bits map
/// onto ratatui modifiers (bold=1, dim=1<<1, italic=1<<2,
/// underline=1<<3, strikethrough=1<<7 — ansi.ts's packed order).
fn chunk_span(chunk: &opentui_core::styled_text::TextChunk, styles: &MarkdownStyles) -> StyledSpan {
    let fg = chunk.fg.and_then(ratatui_color).or(styles.markdown_text.fg);
    let bg = chunk.bg.and_then(ratatui_color).or(styles.background.bg);
    let mut style = Style::new();
    if let Some(fg) = fg {
        style = style.fg(fg);
    }
    if let Some(bg) = bg {
        style = style.bg(bg);
    }
    let mut modifier = Modifier::empty();
    if chunk.attributes & 0x1 != 0 {
        modifier |= Modifier::BOLD;
    }
    if chunk.attributes & 0x2 != 0 {
        modifier |= Modifier::DIM;
    }
    if chunk.attributes & 0x4 != 0 {
        modifier |= Modifier::ITALIC;
    }
    if chunk.attributes & 0x8 != 0 {
        modifier |= Modifier::UNDERLINED;
    }
    if chunk.attributes & 0x80 != 0 {
        modifier |= Modifier::CROSSED_OUT;
    }
    StyledSpan { text: chunk.text.clone(), style: style.add_modifier(modifier) }
}

/// The pipeline's styled output, memoized by (content, options,
/// syntax map): the transcript walk re-renders the same parts every
/// frame while the content is settled — the tree-sitter markdown pass
/// runs once per CONTENT, not once per frame. Bounded; overflow clears
/// (streaming chats churn keys, stale entries never outlive a clear).
fn styled_cached(options: &CodeOptions, styles: &MarkdownStyles) -> StyledText {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::{Mutex, OnceLock};

    fn options_bits(options: &CodeOptions) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        options.filetype.hash(&mut hash);
        options.conceal.hash(&mut hash);
        options.detect_links.hash(&mut hash);
        options.markdown_link_highlights.hash(&mut hash);
        options.hyperlinks_supported.hash(&mut hash);
        options.base_highlight.hash(&mut hash);
        options.content.hash(&mut hash);
        hash.finish()
    }

    static CACHE: OnceLock<Mutex<HashMap<(u64, u64), StyledText>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (options_bits(options), styles.syntax_hash);
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let rendered = markdown_code_styled(options, &styles.syntax);
    if let Ok(mut guard) = cache.lock() {
        if guard.len() >= 512 {
            guard.clear();
        }
        guard.insert(key, rendered.clone());
    }
    rendered
}

fn ratatui_color(rgba: opentui_core::Rgba) -> Option<Color> {
    (rgba.alpha() != 0).then_some(Color::Rgb(rgba.red(), rgba.green(), rgba.blue()))
}

/// The golden markdown fixture (the `markdown-frame` scenario's
/// source): headings, paragraphs with long bold runs, inline marks,
/// unordered + ordered lists, a fenced `ts` block.
#[must_use]
pub fn markdown_frame_fixture() -> String {
    [
        "## Parser rewrite",
        "",
        "The parser now handles **bold-spanning-the-wrap-boundary-abcdefghij** as one styled run.",
        "",
        "Second paragraph opens with filler words and then **bold-run-deliberately-longer-than-the-wrap-boundary-at-eighty** before the tail.",
        "",
        "Ordinary paragraph with `inline.code` and *emphasis* mixed into prose.",
        "",
        "- item one with `inline.code`",
        "- item two follows the first",
        "- item three closes the list",
        "",
        "1. ordered entry",
        "2. second ordered entry",
        "",
        "```ts",
        "const x = parse(input)",
        "return x.map(transform)",
        "```",
    ]
    .join("\n")
}

// -- tests: the pipeline's pinned shape (absolute, post-re-home) ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn styles() -> MarkdownStyles {
        MarkdownStyles::from_theme(&ThemeState::v2_dark().styles())
    }

    fn rendered(source: &str, width: usize) -> Vec<String> {
        render_markdown(source, width, &styles()).iter().map(MarkdownLine::text).collect()
    }

    /// The pipeline pins the fixture's structure: heading row, blank
    /// margins between separated blocks, wrapped paragraphs, list
    /// rows with CONCEALED inline markers (the markdown-text
    /// renderable), the highlighted fence.
    #[test]
    fn golden_fixture_renders_the_reference_layout() {
        let rows = rendered(&markdown_frame_fixture(), 76);
        assert_eq!(rows[0], "Parser rewrite", "the heading");
        assert_eq!(rows[1], "", "heading → paragraph margin");
        assert!(rows[2].starts_with("The parser now handles "), "{:?}", rows[2]);
        let para2 = rows.iter().position(|row| row.starts_with("Second paragraph")).expect("para2");
        assert_eq!(rows[para2 - 1], "", "paragraph → paragraph margin (source blank line)");
        // The list rows conceal the inline markers (upstream renders
        // item children through the markdown-text renderable).
        let item = rows.iter().position(|row| row.starts_with("- item one")).expect("list");
        assert_eq!(rows[item], "- item one with inline.code", "{:?}", rows[item]);
        assert_eq!(rows[item + 1], "- item two follows the first");
        // Ordered markers pad to the widest marker.
        let ordered = rows.iter().position(|row| row == "1. ordered entry").expect("ordered");
        assert_eq!(rows[ordered + 1], "2. second ordered entry");
        // The fence renders its code lines.
        assert!(rows.contains(&"const x = parse(input)".to_owned()), "{rows:?}");
    }

    /// The wrap boundary inside a bold run keeps the strong style on
    /// BOTH fragments (the shared wrap seam over pipeline chunks).
    #[test]
    fn bold_span_crossing_the_wrap_keeps_style_on_both_fragments() {
        let source =
            "words and then **bold-run-deliberately-longer-than-the-wrap-boundary-at-eighty** tail";
        let lines = render_markdown(source, 37, &styles());
        let bold = Style::new()
            .fg(Color::Rgb(245, 167, 66))
            .bg(Color::Rgb(10, 10, 10))
            .add_modifier(Modifier::BOLD);
        let bold_rows: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.0.iter().any(|span| span.style == bold))
            .map(|(index, _)| index)
            .collect();
        assert!(bold_rows.len() >= 2, "the run crosses a wrap boundary: {bold_rows:?}");
        for index in bold_rows {
            assert!(
                lines[index]
                    .0
                    .iter()
                    .any(|span| span.style == bold && !span.text.trim().is_empty()),
                "row {index} keeps the strong style on its fragment"
            );
        }
    }

    /// Separated blocks (heading/list/code/blockquote/hr) margin on
    /// both sides; paragraph → paragraph only with a source blank
    /// line (`buildRenderableTokens` + `getInterBlockMargin`).
    #[test]
    fn paragraph_margin_needs_a_source_blank_line() {
        let with_break = rendered("first\n\nsecond", 40);
        assert_eq!(with_break, vec!["first", "", "second"]);
        let without_break = rendered("first\nsecond", 40);
        // marked coalesces the hard break into one paragraph block;
        // the source line break renders as a row boundary with NO
        // margin row.
        assert_eq!(without_break.len(), 2, "{without_break:?}");
        assert!(!without_break.contains(&String::new()));
    }

    /// Inline marks parse through the marked lexer and conceal:
    /// `**bold**` bold, `*em*` italic, `` `code` `` code-colored,
    /// headings bold.
    #[test]
    fn inline_marks_conceal_and_style() {
        let lines = render_markdown("## Head\n\n**b** *i* `c`", 40, &styles());
        assert_eq!(lines[0].text(), "Head");
        let head = &lines[0].0[0];
        assert!(head.style.add_modifier.contains(Modifier::BOLD));
        let row = &lines[2];
        assert_eq!(row.text(), "b i c");
        let styles_of: Vec<String> = row
            .0
            .iter()
            .map(|span| {
                let m = span.style.add_modifier;
                let mut tag = String::new();
                if m.contains(Modifier::BOLD) {
                    tag.push('B');
                }
                if m.contains(Modifier::ITALIC) {
                    tag.push('I');
                }
                tag
            })
            .collect();
        assert_eq!(
            styles_of,
            vec!["B", "", "I", "", ""],
            "bold/sep/italic/sep/code spans: {:?}",
            row.0
        );
        assert_ne!(row.0[2].style.fg, row.0[0].style.fg, "the code span carries its token color");
    }

    /// A blockquote renders the border column + one padding column
    /// around its content (the spec's border box).
    #[test]
    fn blockquote_renders_the_border_column() {
        let rows = rendered("> quoted line", 40);
        assert_eq!(rows, vec!["│ quoted line"]);
    }

    /// An hr renders the full-width rule row.
    #[test]
    fn hr_renders_the_rule_row() {
        let rows = rendered("a\n\n---\n\nb", 40);
        assert_eq!(rows[2], "─".repeat(40));
    }
}
