//! Markdown TEXT renderer (C1) — the port of `@opentui/core`'s
//! `MarkdownRenderable` **text behavior** as `routes/session/
//! message-parts.tsx`'s `TextPart` drives it (`conceal=true`,
//! `internalBlockMode="top-level"`, `fg=theme.markdown.text`,
//! inside `box paddingLeft=3`).
//!
//! ## What the reference actually paints
//!
//! Read off the compiled `index.bun.js` of @opentui/core 0.5.14 plus
//! the golden ANSI:
//!
//! **Blocks** (`buildTopLevelRenderBlocks`): a heading/list/code/
//! table/blockquote/hr block is "separated" — any adjacent pair
//! involving one takes a 1-row margin; paragraph→paragraph only when
//! the source had a blank line (`(?:\r?\n){2,}$` on prev raw + gap).
//!
//! **Paragraphs / headings** render through the tree-sitter markdown
//! pass: body text `markdown.text`, headings `markdown.heading` +
//! BOLD, `**strong**` `markdown.strong` + BOLD, `*em*`
//! `markdown.emphasis` + ITALIC, `` `code` `` `markdown.code` — the
//! markers concealed in rendered mode. Links keep the label
//! (`markup.link.label` → `markdown.link_text`) and append ` (url)`
//! when the terminal has no hyperlink support.
//!
//! **Lists** (`createListRenderable`): a fixed-width marker run
//! (`"-"`/`"N."` padded to the widest marker + one space) followed by
//! the item's **raw** text through a `CodeRenderable` with
//! `drawUnstyledText` — plain default-fg text with the inline markers
//! *visible* (the golden pins ``- item one with `inline.code` `` in
//! the renderer default foreground). Nested lists indent under the
//! parent's content column.
//!
//! **Fenced code**: highlighted lines via oc-syntax for registered
//! grammars (R-01 spike: rust/python/typescript), plain
//! `markdown.code_block` lines otherwise; spans clip at the content
//! width like `wrapMode=none`. **Blockquotes**: a left border + one
//! padding column around the raw content. **hr**: a full-width
//! top-border row.
//!
//! Everything flows through the ONE wrap seam ([`crate::wrap`]) as
//! styled atom streams — a bold run crossing a wrap boundary keeps its
//! style on both fragments (the gate-9 instrument case this golden
//! pins at both widths).

#![allow(clippy::cast_possible_truncation)]

use ratatui::style::{Modifier, Style};

/// The hard-break sentinel `inline_spans` emits for [`Inline::Break`]
/// (`push_wrapped` splits segments on it; never a real glyph — source
/// lines are split before inline parsing).
const HARD_BREAK: &str = "\u{0}";

use crate::layout::StyledSpan;
use crate::theme::ThemeStyles;
use crate::wrap::{flatten_spans, styled_runs, wrap_atoms};

/// One rendered markdown line: styled runs over one terminal row.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownLine(pub Vec<StyledSpan>);

impl MarkdownLine {
    /// The line's concatenated glyph content (test/breadcrumb use).
    #[must_use]
    pub fn text(&self) -> String {
        self.0.iter().map(|run| run.text.as_str()).collect()
    }
}

/// The markdown text renderer's theme-derived style set (plain
/// `Style`s built from [`ThemeStyles::markdown`], plus the flattened
/// syntax scope map the fenced-code highlight path consumes).
#[derive(Debug, Clone)]
pub struct MarkdownStyles {
    body: Style,
    heading: Style,
    strong: Style,
    emphasis: Style,
    code: Style,
    code_block: Style,
    link_text: Style,
    link: Style,
    quote_border: Style,
    rule: Style,
    /// The renderer default foreground — list-item raw text and markers.
    default_fg: Style,
    background: ratatui::style::Color,
    /// tree-sitter capture → style resolution (`convertThemeToStyles`
    /// output; R-01 spike — fenced code blocks).
    pub syntax: oc_syntax::SyntaxStyles,
}

impl MarkdownStyles {
    #[must_use]
    pub fn from_theme(theme: &ThemeStyles) -> Self {
        let md = theme.markdown;
        let base = Style::new().fg(md.text).bg(theme.background);
        Self {
            body: base,
            heading: base.fg(md.heading).add_modifier(Modifier::BOLD),
            strong: base.fg(md.strong).add_modifier(Modifier::BOLD),
            emphasis: base.fg(md.emphasis).add_modifier(Modifier::ITALIC),
            code: base.fg(md.code),
            code_block: base.fg(md.code_block),
            link_text: base.fg(md.link_text),
            link: base.fg(md.link),
            quote_border: base.fg(md.block_quote),
            rule: base.fg(md.horizontal_rule),
            default_fg: Style::new()
                .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                .bg(theme.background),
            background: theme.background,
            syntax: theme.syntax.clone(),
        }
    }
}

// -- block model ---------------------------------------------------------------

/// One parsed block-level markdown unit (the `marked` subset the
/// assistant output uses).
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        inline: Vec<Inline>,
    },
    Paragraph {
        inline: Vec<Inline>,
        /// `trailing_breaks`: the source had ≥2 newlines after this
        /// paragraph (para→para margin).
        trailing_breaks: bool,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Code {
        lang: String,
        lines: Vec<String>,
    },
    Blockquote {
        lines: Vec<String>,
    },
    Rule,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// Raw item text (inline markers NOT parsed — the reference's list
    /// children draw unstyled raw).
    pub text: Vec<String>,
    /// Blank line after the item in the source (`marginBottom 1`).
    pub spaced: bool,
    pub children: Vec<Block>,
}

/// One inline span.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    /// A source line boundary inside a paragraph. opentui's Markdown
    /// renderable parses with the raw `Lexer` tokens — `token.text`
    /// keeps the `\n` characters — and its text layer treats them as
    /// line breaks (`Markdown.ts` pushes the chunk text verbatim;
    /// `TextRenderable` breaks rows on `\n`, the same seam as the
    /// user-message block's F6). A soft `\n` therefore renders as a
    /// HARD row break, each source line wrapping independently.
    Break,
    Strong(Vec<Inline>),
    Em(Vec<Inline>),
    Code(String),
    /// `[label](url)` / autolink — the label renders, the URL appends in
    /// parentheses (no hyperlink support headless).
    Link {
        label: Vec<Inline>,
        url: String,
    },
    Strikethrough(Vec<Inline>),
}

/// Parse markdown source into blocks (`marked.lexer` subset).
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn parse_blocks(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines: Vec<&str> = source.lines().collect();
    // Trailing newline handling: `lines` drops it; paragraphs track
    // their source trailing breaks via the consumed line count.
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        // Fenced code.
        if let Some(fence) = trimmed.strip_prefix("```") {
            let lang = fence.trim().to_owned();
            let mut body = Vec::new();
            index += 1;
            while index < lines.len() && !lines[index].trim_start().starts_with("```") {
                body.push(lines[index].to_owned());
                index += 1;
            }
            index += 1; // closing fence
            blocks.push(Block::Code { lang, lines: body });
            continue;
        }
        // Heading.
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count() as u8;
            let rest = &trimmed[usize::from(level)..];
            if level <= 6 && (rest.is_empty() || rest.starts_with(' ')) {
                let text = rest.trim();
                blocks.push(Block::Heading { level, inline: parse_inline(text) });
                index += 1;
                continue;
            }
        }
        // Horizontal rule: 3+ of -, * or _ alone on the line.
        if is_rule(trimmed) {
            blocks.push(Block::Rule);
            index += 1;
            continue;
        }
        // Blockquote: consecutive `>` lines.
        if trimmed.starts_with('>') {
            let mut body = Vec::new();
            while index < lines.len() {
                let line = lines[index].trim_start();
                if let Some(rest) = line.strip_prefix('>') {
                    body.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
                    index += 1;
                } else if !line.is_empty() && body.last().is_some_and(String::is_empty) {
                    // Blank `>` line inside the quote, then a lazy
                    // continuation line.
                    body.push(line.to_owned());
                    index += 1;
                } else {
                    break;
                }
            }
            blocks.push(Block::Blockquote { lines: body });
            continue;
        }
        // List: consecutive list items at this indent.
        if let Some((ordered, _marker_len)) = list_marker(trimmed) {
            let start = if ordered {
                trimmed.split(['.', ')']).next().and_then(|digits| digits.parse().ok()).unwrap_or(1)
            } else {
                1
            };
            let mut items = Vec::new();
            while index < lines.len() {
                let line = lines[index];
                let item_trimmed = line.trim_start();
                let item_indent = line.len() - item_trimmed.len();
                if item_indent != indent {
                    break;
                }
                let Some((item_ordered, marker_len)) = list_marker(item_trimmed) else {
                    break;
                };
                if item_ordered != ordered {
                    break;
                }
                let mut content = vec![item_trimmed[marker_len..].trim_start().to_owned()];
                index += 1;
                // Continuation and nested blocks: more-indented lines
                // belong to the item (nested lists parse as children;
                // other indented lines continue the text).
                let mut children = Vec::new();
                let mut spaced = false;
                while index < lines.len() {
                    let next = lines[index];
                    let next_trimmed = next.trim_start();
                    let next_indent = next.len() - next_trimmed.len();
                    if next_trimmed.is_empty() {
                        // A blank line belongs to the item when the next
                        // line is indented deeper (loose list); else the
                        // list ends.
                        let after = lines.get(index + 1).map(|after| {
                            let trimmed_after = after.trim_start();
                            (after.len() - trimmed_after.len(), trimmed_after)
                        });
                        match after {
                            Some((after_indent, text))
                                if !text.is_empty() && after_indent > item_indent =>
                            {
                                spaced = true;
                                index += 1;
                                continue;
                            }
                            // Loose list: the blank sits between two
                            // items of the SAME list and belongs to this
                            // item's raw; a different marker kind starts
                            // a new list (tight boundary).
                            Some((after_indent, text))
                                if !text.is_empty()
                                    && after_indent == item_indent
                                    && list_marker(text).is_some_and(|(next_ordered, _)| {
                                        next_ordered == ordered
                                    }) =>
                            {
                                spaced = true;
                            }
                            _ => {}
                        }
                        break;
                    }
                    if next_indent > item_indent {
                        if list_marker(next_trimmed).is_some() {
                            // Nested list: collect its run and recurse.
                            let nested_start = index;
                            let mut nested_end = index;
                            while nested_end < lines.len() {
                                let candidate = lines[nested_end];
                                let candidate_trimmed = candidate.trim_start();
                                let candidate_indent = candidate.len() - candidate_trimmed.len();
                                let same_item = candidate_indent == item_indent
                                    && list_marker(candidate_trimmed).is_some_and(
                                        |(marker_ordered, _)| marker_ordered == ordered,
                                    );
                                if candidate_trimmed.is_empty()
                                    || candidate_indent > item_indent
                                    || same_item
                                {
                                    nested_end += 1;
                                } else {
                                    break;
                                }
                            }
                            let child_lines: Vec<String> = lines[nested_start..nested_end]
                                .iter()
                                .map(|source| {
                                    let source_trimmed = source.trim_start();
                                    let source_indent = source.len() - source_trimmed.len();
                                    if source_indent > item_indent {
                                        // Strip one indent level (2 spaces
                                        // or the marker width).
                                        let strip =
                                            (source_indent - item_indent).min(source_indent);
                                        source[strip..].to_owned()
                                    } else {
                                        (*source).to_owned()
                                    }
                                })
                                .collect();
                            children.extend(parse_blocks(&child_lines.join("\n")));
                            index = nested_end;
                            continue;
                        }
                        content.push(next_trimmed.to_owned());
                        index += 1;
                        continue;
                    }
                    break;
                }
                if content.last().is_some_and(String::is_empty) {
                    content.pop();
                    spaced = true;
                }
                items.push(ListItem { text: content, spaced, children });
            }
            blocks.push(Block::List { ordered, start, items });
            continue;
        }
        // Paragraph: consecutive non-blank, non-structural lines.
        let mut text = Vec::new();
        let mut trailing_breaks = false;
        while index < lines.len() {
            let line = lines[index];
            let trimmed_line = line.trim_start();
            if trimmed_line.is_empty() {
                trailing_breaks = true;
                index += 1;
                break;
            }
            if trimmed_line.starts_with('#')
                || trimmed_line.starts_with("```")
                || trimmed_line.starts_with('>')
                || is_rule(trimmed_line)
                || list_marker(trimmed_line).is_some()
            {
                break;
            }
            text.push(trimmed_line.to_owned());
            index += 1;
        }
        // Hard line breaks: the paragraph keeps its source line
        // structure (opentui's lexer-level chunks carry `\n` through
        // to the text layer — see [`Inline::Break`]); each line's
        // inline content parses independently and the breaks join
        // them.
        let mut inline = Vec::new();
        for (position, line) in text.iter().enumerate() {
            if position > 0 {
                inline.push(Inline::Break);
            }
            inline.extend(parse_inline(line));
        }
        blocks.push(Block::Paragraph { inline, trailing_breaks });
    }
    lines.clear();
    blocks
}

fn is_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else { return false };
    (first == '-' || first == '*' || first == '_')
        && trimmed.chars().count() >= 3
        && trimmed.chars().all(|c| c == first)
}

/// `(ordered, marker length)` when the line opens a list item.
fn list_marker(trimmed: &str) -> Option<(bool, usize)> {
    let bytes = trimmed.as_bytes();
    if (bytes.first() == Some(&b'-')
        || bytes.first() == Some(&b'*')
        || bytes.first() == Some(&b'+'))
        && bytes.get(1) == Some(&b' ')
    {
        return Some((false, 2));
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if rest.starts_with(". ") || rest.starts_with(") ") {
            return Some((true, digits + 2));
        }
        if rest == "." || rest == ")" {
            return Some((true, digits + 1));
        }
    }
    None
}

/// Parse inline markdown (`marked.parseInline` subset).
#[must_use]
pub fn parse_inline(text: &str) -> Vec<Inline> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '\\' if index + 1 < chars.len() => {
                plain.push(chars[index + 1]);
                index += 2;
            }
            '`' => {
                if let Some(close) = chars[index + 1..].iter().position(|c| *c == '`') {
                    flush(&mut spans, &mut plain);
                    let code: String = chars[index + 1..index + 1 + close].iter().collect();
                    spans.push(Inline::Code(code));
                    index += close + 2;
                    continue;
                }
                plain.push(ch);
                index += 1;
            }
            // `~~del~~` strikethrough; a single `~` is a literal glyph
            // (marked only pairs doubled tildes).
            '*' | '~' => {
                let double = chars.get(index + 1) == Some(&ch);
                if ch == '~' && !double {
                    plain.push(ch);
                    index += 1;
                    continue;
                }
                let width = if double { 2 } else { 1 };
                // Left-flanking (marked): an opener must be followed by
                // non-whitespace, else it's a literal glyph.
                let opens = chars.get(index + width).is_some_and(|next| !next.is_whitespace());
                if opens {
                    if let Some(end) = find_close(&chars, index + width, ch, double) {
                        flush(&mut spans, &mut plain);
                        let inner: String = chars[index + width..end].iter().collect();
                        let parsed = parse_inline(&inner);
                        spans.push(match (ch, double) {
                            ('*', true) => Inline::Strong(parsed),
                            ('*', false) => Inline::Em(parsed),
                            ('~', true) => Inline::Strikethrough(parsed),
                            _ => Inline::Text(inner),
                        });
                        index = end + width;
                        continue;
                    }
                }
                plain.push(ch);
                index += 1;
            }
            '[' => {
                if let Some(close) = chars[index + 1..].iter().position(|c| *c == ']') {
                    let label: String = chars[index + 1..index + 1 + close].iter().collect();
                    let after = index + 1 + close;
                    if chars.get(after + 1) == Some(&'(') {
                        if let Some(paren) = chars[after + 2..].iter().position(|c| *c == ')') {
                            let url: String = chars[after + 2..after + 2 + paren].iter().collect();
                            flush(&mut spans, &mut plain);
                            spans.push(Inline::Link { label: parse_inline(&label), url });
                            index = after + 2 + paren + 1;
                            continue;
                        }
                    }
                }
                plain.push(ch);
                index += 1;
            }
            _ => {
                plain.push(ch);
                index += 1;
            }
        }
    }
    flush(&mut spans, &mut plain);
    spans
}

fn find_close(chars: &[char], from: usize, marker: char, double: bool) -> Option<usize> {
    let width = if double { 2 } else { 1 };
    let mut index = from;
    while index + width <= chars.len() {
        if chars[index] == '\\' {
            // An escaped char can never close (or open) a span.
            index += 2;
            continue;
        }
        if chars[index] == marker {
            let matches = if double { chars.get(index + 1) == Some(&marker) } else { true };
            // Right-flanking (marked): a closer must follow non-space
            // content.
            let closes = index > from && !chars[index - 1].is_whitespace();
            if matches && closes {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn flush(spans: &mut Vec<Inline>, plain: &mut String) {
    if !plain.is_empty() {
        spans.push(Inline::Text(std::mem::take(plain)));
    }
}

// -- rendering -----------------------------------------------------------------

/// Render markdown source to styled terminal lines at `width` (the
/// content column — the caller has already applied `paddingLeft 3`).
#[must_use]
pub fn render_markdown(source: &str, width: usize, styles: &MarkdownStyles) -> Vec<MarkdownLine> {
    let blocks = parse_blocks(source);
    let mut lines = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 && should_margin(&blocks[index - 1], block) {
            lines.push(MarkdownLine(Vec::new()));
        }
        render_block(block, width, styles, &mut lines);
    }
    lines
}

/// `shouldAddTopLevelMargin`: separated blocks (heading/list/code/
/// table/blockquote/hr) force the margin on either side;
/// paragraph→paragraph only with a source blank line.
fn should_margin(previous: &Block, current: &Block) -> bool {
    if is_separated(previous) || is_separated(current) {
        return true;
    }
    matches!(
        (previous, current),
        (Block::Paragraph { trailing_breaks: true, .. }, Block::Paragraph { .. })
    )
}

fn is_separated(block: &Block) -> bool {
    matches!(
        block,
        Block::Heading { .. }
            | Block::List { .. }
            | Block::Code { .. }
            | Block::Blockquote { .. }
            | Block::Rule
    )
}

fn render_block(
    block: &Block,
    width: usize,
    styles: &MarkdownStyles,
    lines: &mut Vec<MarkdownLine>,
) {
    match block {
        Block::Heading { inline, .. } => {
            let spans = inline_spans(inline, styles.heading, styles);
            push_wrapped(&spans, width, lines);
        }
        Block::Paragraph { inline, .. } => {
            let spans = inline_spans(inline, styles.body, styles);
            push_wrapped(&spans, width, lines);
        }
        Block::Code { lang, lines: code } => {
            // Fenced code (R-01 spike): the fence info string becomes the
            // filetype; a registered grammar renders the block through
            // oc-syntax (parse once per block — content-keyed cache in
            // the engine; this path only materializes spans). Unknown
            // filetypes render today's plain `markdown.code_block` lines.
            let highlighted = oc_syntax::highlight(&code.join("\n"), lang);
            for (index, code_line) in code.iter().enumerate() {
                let spans: Vec<StyledSpan> = match &highlighted {
                    Some(block) => block
                        .line_spans(index, styles.code_block, &styles.syntax)
                        .into_iter()
                        .map(|span| StyledSpan { text: span.text, style: span.style })
                        .collect(),
                    // Code does not word-wrap in the reference (horizontal
                    // scroll); clip at the content width like wrapMode=none.
                    None => vec![StyledSpan {
                        text: crate::wrap::take_width(code_line, width),
                        style: styles.code_block,
                    }],
                };
                lines.push(MarkdownLine(clip_spans(spans, width)));
            }
        }
        Block::Rule => {
            lines.push(MarkdownLine(vec![StyledSpan {
                text: "─".repeat(width),
                style: styles.rule,
            }]));
        }
        Block::Blockquote { lines: quote } => {
            let inner = width.saturating_sub(2);
            let mut body = Vec::new();
            for quote_line in quote {
                let spans = vec![StyledSpan {
                    text: crate::wrap::take_width(quote_line, inner),
                    style: styles.body,
                }];
                push_wrapped(&spans, inner, &mut body);
            }
            for line in body {
                let mut runs = vec![
                    StyledSpan { text: "│".into(), style: styles.quote_border },
                    StyledSpan { text: " ".into(), style: styles.body },
                ];
                runs.extend(line.0);
                lines.push(MarkdownLine(runs));
            }
        }
        Block::List { ordered, start, items } => {
            render_list(*ordered, *start, items, width, styles, lines);
        }
    }
}

/// Clip a line's styled spans at `width` display columns — the styled
/// counterpart of `wrap::take_width` (same cluster-width math), with the
/// budget shared across the line's spans so multi-span lines clip
/// exactly where the plain single-span line always did.
fn clip_spans(spans: Vec<StyledSpan>, width: usize) -> Vec<StyledSpan> {
    let mut remaining = width;
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if remaining == 0 {
            break;
        }
        let text = crate::wrap::take_width(&span.text, remaining);
        if text.is_empty() {
            break;
        }
        remaining -= crate::wrap::display_width(&text);
        out.push(StyledSpan { text, style: span.style });
    }
    out
}

fn render_list(
    ordered: bool,
    start: u64,
    items: &[ListItem],
    width: usize,
    styles: &MarkdownStyles,
    lines: &mut Vec<MarkdownLine>,
) {
    let markers: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if ordered {
                format!("{}.", u64::try_from(index).unwrap_or(0) + start)
            } else {
                "-".to_owned()
            }
        })
        .collect();
    let marker_width = markers.iter().map(String::len).max().unwrap_or(1);
    let content_width = width.saturating_sub(marker_width + 1);
    for (index, item) in items.iter().enumerate() {
        let marker = format!("{:>width$} ", markers[index], width = marker_width);
        // The item's raw text lines, unwrapped at the content width.
        let mut first = true;
        for raw in &item.text {
            let wrapped = wrap_raw(raw, content_width);
            for line in wrapped {
                if first {
                    lines.push(MarkdownLine(vec![
                        StyledSpan { text: marker.clone(), style: styles.default_fg },
                        StyledSpan { text: line, style: styles.default_fg },
                    ]));
                    first = false;
                } else {
                    lines.push(MarkdownLine(vec![
                        StyledSpan { text: " ".repeat(marker_width + 1), style: styles.default_fg },
                        StyledSpan { text: line, style: styles.default_fg },
                    ]));
                }
            }
            if first {
                // Empty item text: marker alone.
                lines.push(MarkdownLine(vec![StyledSpan {
                    text: marker.clone(),
                    style: styles.default_fg,
                }]));
                first = false;
            }
        }
        for child in &item.children {
            if let Block::List { ordered, start, items } = child {
                let indent = " ".repeat(marker_width + 1);
                let mut nested = Vec::new();
                render_list(*ordered, *start, items, content_width, styles, &mut nested);
                for line in nested {
                    let mut runs =
                        vec![StyledSpan { text: indent.clone(), style: styles.default_fg }];
                    runs.extend(line.0);
                    lines.push(MarkdownLine(runs));
                }
            } else {
                let mut nested = Vec::new();
                render_block(child, content_width, styles, &mut nested);
                for line in nested {
                    let mut runs = vec![StyledSpan {
                        text: " ".repeat(marker_width + 1),
                        style: styles.default_fg,
                    }];
                    runs.extend(line.0);
                    lines.push(MarkdownLine(runs));
                }
            }
        }
        if item.spaced {
            lines.push(MarkdownLine(Vec::new()));
        }
    }
}

/// Wrap raw (unparsed) item text: plain single-style atoms.
fn wrap_raw(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    wrap_atoms(&flatten_spans(&[StyledSpan { text: text.to_owned(), style: Style::new() }]), width)
        .into_iter()
        .map(|line| styled_runs(&line).into_iter().map(|run| run.text).collect())
        .collect()
}

/// Inline spans → styled runs, wrapping as ONE flattened stream.
fn push_wrapped(spans: &[StyledSpan], width: usize, lines: &mut Vec<MarkdownLine>) {
    if width == 0 {
        return;
    }
    // Hard breaks split the stream: each source line wraps alone
    // (the `\u{0}` sentinel never reaches `wrap_atoms`).
    let mut segment: Vec<StyledSpan> = Vec::new();
    for span in spans {
        if span.text == HARD_BREAK {
            push_wrapped_segment(&segment, width, lines);
            segment.clear();
        } else {
            segment.push(span.clone());
        }
    }
    push_wrapped_segment(&segment, width, lines);
}

fn push_wrapped_segment(spans: &[StyledSpan], width: usize, lines: &mut Vec<MarkdownLine>) {
    for line in wrap_atoms(&flatten_spans(spans), width) {
        lines.push(MarkdownLine(styled_runs(&line)));
    }
}

/// Flatten inline nodes to styled spans (conceal=true: markers dropped).
fn inline_spans(nodes: &[Inline], style: Style, styles: &MarkdownStyles) -> Vec<StyledSpan> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Inline::Text(text) => push_span(&mut out, text.clone(), style),
            Inline::Break => out.push(StyledSpan { text: HARD_BREAK.to_owned(), style }),
            Inline::Strong(children) => {
                extend_spans(&mut out, inline_spans(children, styles.strong, styles));
            }
            Inline::Em(children) => {
                extend_spans(&mut out, inline_spans(children, styles.emphasis, styles));
            }
            Inline::Strikethrough(children) => {
                let mut struck = style.add_modifier(Modifier::CROSSED_OUT);
                struck = struck.bg(styles.background);
                extend_spans(&mut out, inline_spans(children, struck, styles));
            }
            Inline::Code(text) => push_span(&mut out, text.clone(), styles.code),
            Inline::Link { label, url } => {
                extend_spans(&mut out, inline_spans(label, styles.link_text, styles));
                push_span(&mut out, format!(" ({url})"), styles.link);
            }
        }
    }
    out
}

fn push_span(out: &mut Vec<StyledSpan>, text: String, style: Style) {
    if !text.is_empty() {
        out.push(StyledSpan { text, style });
    }
}

fn extend_spans(out: &mut Vec<StyledSpan>, spans: Vec<StyledSpan>) {
    out.extend(spans);
}

/// The `markdown-frame` golden fixture (`tools/parity/tui-dump/
/// scenarios/markdown_frame.tsx` `MARKDOWN`), byte-for-byte.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn styles() -> MarkdownStyles {
        MarkdownStyles::from_theme(&ThemeState::v2_dark().styles())
    }

    fn rendered(source: &str, width: usize) -> Vec<String> {
        render_markdown(source, width, &styles()).into_iter().map(|line| line.text()).collect()
    }

    /// The golden's structure at 77 (80-col frame − paddingLeft 3):
    /// every block pair separated by exactly one blank row, list items
    /// raw, fence plain.
    #[test]
    fn golden_fixture_renders_the_reference_layout() {
        let lines = rendered(&markdown_frame_fixture(), 77);
        let expected: Vec<&str> = vec![
            "Parser rewrite",
            "",
            "The parser now handles bold-spanning-the-wrap-boundary-abcdefghij as one ",
            "styled run.",
            "",
            "Second paragraph opens with filler words and then bold-run-deliberately-",
            "longer-than-the-wrap-boundary-at-eighty before the tail.",
            "",
            "Ordinary paragraph with inline.code and emphasis mixed into prose.",
            "",
            "- item one with `inline.code`",
            "- item two follows the first",
            "- item three closes the list",
            "",
            "1. ordered entry",
            "2. second ordered entry",
            "",
            "const x = parse(input)",
            "return x.map(transform)",
        ];
        assert_eq!(lines, expected);
    }

    /// 40-col frame → content width 37: the bold run crosses the wrap
    /// boundary at the last hyphen that fits (the instrument case).
    #[test]
    fn golden_fixture_wraps_at_37() {
        let lines = rendered(&markdown_frame_fixture(), 37);
        assert_eq!(lines[0], "Parser rewrite");
        assert_eq!(lines[2], "The parser now handles bold-spanning-");
        assert_eq!(lines[3], "the-wrap-boundary-abcdefghij as one ");
        assert_eq!(lines[6], "Second paragraph opens with filler ");
        assert_eq!(lines[7], "words and then bold-run-deliberately-");
        assert_eq!(lines[8], "longer-than-the-wrap-boundary-at-");
        assert_eq!(lines[11], "Ordinary paragraph with inline.code ");
        assert_eq!(lines[12], "and emphasis mixed into prose.");
        assert_eq!(lines[14], "- item one with `inline.code`");
        assert_eq!(lines[18], "1. ordered entry");
        assert_eq!(lines[21], "const x = parse(input)");
    }

    /// The bold span keeps its style on both fragments across the wrap
    /// boundary (styles, not glyphs — the C1 instrument).
    #[test]
    fn bold_span_crossing_the_wrap_keeps_style_on_both_fragments() {
        let styles = styles();
        let lines = render_markdown(
            "prefix **bold-run-deliberately-longer-than-the-boundary** suffix",
            37,
            &styles,
        );
        // Greedy: the LAST hyphen that fits keeps "longer-" on line 1.
        assert_eq!(lines[0].text(), "prefix bold-run-deliberately-longer-");
        assert_eq!(lines[1].text(), "than-the-boundary suffix");
        let bold = styles.strong;
        // Line 1 tail and line 2 head both carry the strong style.
        assert_eq!(lines[0].0.last().unwrap().style.fg, bold.fg);
        assert_eq!(lines[0].0.last().unwrap().style.add_modifier(Modifier::BOLD), bold);
        assert_eq!(lines[1].0.first().unwrap().style, bold);
    }

    #[test]
    fn headings_lists_fences_rules_and_quotes() {
        let styles = styles();
        let source = "# Title\n\n> quoted line\n\n---\n\n- a\n- b\n";
        let lines = render_markdown(source, 20, &styles);
        let text: Vec<String> = lines.iter().map(super::MarkdownLine::text).collect();
        assert_eq!(
            text,
            vec!["Title", "", "│ quoted line", "", "────────────────────", "", "- a", "- b"]
        );
        // Heading is bold + heading color.
        assert_eq!(lines[0].0[0].style, styles.heading);
        // Nested list indents under the parent marker column.
        let nested = render_markdown("- a\n  - deep\n- b\n", 20, &styles);
        assert_eq!(nested[1].text(), "  - deep");
        // Ordered markers align to the widest number.
        let ordered = render_markdown("9. nine\n10. ten\n", 20, &styles);
        assert_eq!(ordered[0].text(), " 9. nine");
        assert_eq!(ordered[1].text(), "10. ten");
    }

    #[test]
    fn inline_marks_parse_and_conceal() {
        let spans = parse_inline("a **b *c*** `d` [e](https://x) ~~f~~ \\*g\\*");
        assert_eq!(spans.len(), 9, "{spans:?}");
        assert!(matches!(spans[1], Inline::Strong(_)));
        // The leftover `*` after the strong close is a literal glyph.
        assert!(matches!(&spans[2], Inline::Text(text) if text == "* "));
        assert!(matches!(spans[3], Inline::Code(ref text) if text == "d"));
        assert!(matches!(&spans[5], Inline::Link { url, .. } if url == "https://x"));
        assert!(matches!(spans[7], Inline::Strikethrough(_)));
        assert!(matches!(&spans[8], Inline::Text(text) if text == " *g*"));
        // A single tilde is literal (marked pairs only `~~`).
        let singles = parse_inline("a ~b~ c");
        assert_eq!(singles.len(), 1);
        assert!(matches!(&singles[0], Inline::Text(text) if text == "a ~b~ c"));
        let rendered = render_markdown("a **b** c", 20, &styles());
        assert_eq!(rendered[0].text(), "a b c");
    }

    #[test]
    fn paragraph_margin_needs_a_source_blank_line() {
        // Hard breaks: source lines render one row each (opentui's
        // lexer keeps `\n` in the text chunks); a blank line splits
        // paragraphs → the margin row.
        let spaced = rendered("one\ntwo\n\nthree", 40);
        assert_eq!(spaced, vec!["one", "two", "", "three"]);
        // Long source lines still WRAP inside their own hard-break
        // segment.
        let wrapped_once = rendered("one two\nthree", 40);
        assert_eq!(wrapped_once, vec!["one two", "three"]);
    }

    /// The long-reply corpus pin: the stub's 32 `Long reply line N…`
    /// lines render ONE PER ROW (each SSE delta ends `\n`; the
    /// hard break survives the paragraph accumulator).
    #[test]
    fn multiline_paragraph_renders_one_row_per_source_line() {
        let styles = MarkdownStyles::from_theme(&crate::theme::ThemeState::v2_dark().styles());
        let src = "Long reply line 1 of the parity scroll fixture.\nLong reply line 2 of the parity scroll fixture.";
        let lines: Vec<String> =
            render_markdown(src, 75, &styles).into_iter().map(|line| line.text()).collect();
        assert_eq!(
            lines,
            vec![
                "Long reply line 1 of the parity scroll fixture.",
                "Long reply line 2 of the parity scroll fixture."
            ]
        );
    }
}
