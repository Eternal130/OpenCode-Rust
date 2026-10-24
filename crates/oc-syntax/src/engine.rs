//! The highlighting engine: parse a fenced code block **once** with its
//! grammar, run the highlight query, and collect per-line capture ranges
//! into an immutable [`HighlightedBlock`]. Span materialization
//! ([`HighlightedBlock::line_spans`]) is then a pure lookup — it never
//! touches tree-sitter, so the render path only ever re-styles text.
//!
//! ## Parse-time discipline
//!
//! v2 parses when the markdown renderable mounts a completed block; the
//! spike enforces the same economics with a content-keyed cache: the
//! first [`crate::highlight`] call for a `(filetype, content)` pair does
//! the parse+query work (message-build time — the block's text is final
//! by then), and every later render of the same block hits the cache.
//! Streaming messages produce new content keys, so partial parses are
//! never reused. The cache is bounded; when full it drops the oldest
//! generation wholesale (message content is immutable, so a dropped
//! entry only costs one re-parse the next time it scrolls into view).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use ratatui::style::Style;
use streaming_iterator::StreamingIterator;

use crate::queries;
use crate::scopes::{self, SyntaxStyles};
use crate::StyledSpan;

/// Cache capacity: blocks kept parsed (a long session shows far fewer
/// distinct code blocks than this on screen at once).
const CACHE_CAPACITY: usize = 128;

/// The wall-clock ceiling on one block's parse (the never-block guard —
/// sized for debug-build test runs, where tree-sitter runs several times
/// slower than release; a release parse of a 2000-line block sits ~40ms
/// under it, and the ceiling only ever bites on genuinely pathological
/// input, which degrades to plain rendering).
const PARSE_CEILING: std::time::Duration = std::time::Duration::from_millis(500);

/// One capture range on one line, in **char** offsets within that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaptureSpan {
    start: usize,
    end: usize,
    /// Index into the block's capture-name arena.
    name: u32,
}

/// A parsed-and-queried code block: the block's lines plus per-line
/// capture ranges. Cheap to clone (an [`Arc`] over the immutable
/// payload); tree-sitter types never escape this crate.
#[derive(Debug, Clone)]
pub struct HighlightedBlock(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    lines: Vec<String>,
    /// Per line: capture ranges in query-emission order (later ranges
    /// overlay earlier ones where they overlap — the overlay convention
    /// the vendored queries are written against: general patterns
    /// first, specific ones later).
    captures: Vec<Vec<CaptureSpan>>,
    /// Interned capture names (`@`-less, dotted).
    names: Vec<Box<str>>,
}

impl HighlightedBlock {
    /// The block's line count (a `line` argument >= this renders empty).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.0.lines.len()
    }

    /// Test observability: the capture names active on one line with
    /// their char ranges (asserts on the engine's raw captures, not
    /// styles).
    #[cfg(test)]
    pub(crate) fn captures_at(&self, line: usize) -> Vec<(usize, usize, String)> {
        self.0.captures[line]
            .iter()
            .map(|span| (span.start, span.end, self.0.names[span.name as usize].to_string()))
            .collect()
    }

    /// Materialize one line as styled spans: `base` for uncaptured gaps,
    /// the theme-matched capture style overlaid everywhere else.
    /// Adjacent equal styles merge; the concatenated text always equals
    /// the source line — styling never changes glyphs.
    #[must_use]
    pub fn line_spans(&self, line: usize, base: Style, styles: &SyntaxStyles) -> Vec<StyledSpan> {
        let Some(text) = self.0.lines.get(line) else {
            return Vec::new();
        };
        let ranges = &self.0.captures[line];
        if ranges.is_empty() {
            return vec![StyledSpan { text: text.clone(), style: base }];
        }
        // Resolve every interned capture name once (a block uses a
        // handful of distinct captures), then overlay per char: later
        // captures win, mirroring editor overlay order.
        let resolved: Vec<Style> =
            self.0.names.iter().map(|name| scopes::resolve(styles, name, base)).collect();
        let chars: Vec<char> = text.chars().collect();
        let mut cells: Vec<Option<u32>> = vec![None; chars.len()];
        for range in ranges {
            for cell in &mut cells[range.start..range.end] {
                *cell = Some(range.name);
            }
        }
        // Cut the per-char table into contiguous same-style runs.
        let mut spans: Vec<StyledSpan> = Vec::new();
        let mut run = String::with_capacity(text.len());
        let mut current = base;
        for (ch, cell) in chars.iter().zip(&cells) {
            let style = cell.map_or(base, |name| resolved[name as usize]);
            if style != current {
                if !run.is_empty() {
                    spans.push(StyledSpan { text: std::mem::take(&mut run), style: current });
                }
                current = style;
            }
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(StyledSpan { text: run, style: current });
        }
        spans
    }
}

/// Parse + query one block for one grammar (no caching here — see
/// [`crate::highlight`]).
fn build(
    source: &str,
    lines: &[String],
    entry: &queries::GrammarEntry,
) -> Option<HighlightedBlock> {
    let mut parser = queries::parser(entry)?;
    // Never-block guard: a hard wall-clock ceiling on the parse so a
    // pathological block can never wedge a caller (v2's wasm parsers
    // carry the same protection host-side). On cancel, degrade to plain
    // rendering.
    let deadline = std::time::Instant::now() + PARSE_CEILING;
    let mut cancelled = false;
    let mut progress = |_: &tree_sitter::ParseState| -> bool {
        cancelled = std::time::Instant::now() > deadline;
        cancelled
    };
    // Chunked-input parse: hand tree-sitter the block from whatever
    // offset it asks (a mid-parse re-request past an external scanner
    // or recovery point is `source[offset..]`, NOT a hard EOF — scala
    // re-requests mid-source and an `""` there silently truncates the
    // parse); past the end it gets an empty slice.
    let mut chunks =
        |offset: usize, _: tree_sitter::Point| -> &str { source.get(offset..).unwrap_or_default() };
    let tree = parser.parse_with_options(
        &mut chunks,
        None,
        Some(tree_sitter::ParseOptions::new().progress_callback(&mut progress)),
    )?;
    if cancelled {
        return None;
    }

    // Byte → line mapping, built once: each line's start offset in the
    // joined source (single '\n' separators, trailing newline omitted —
    // exactly `lines.join("\n")`).
    let mut line_starts: Vec<usize> = Vec::with_capacity(lines.len() + 1);
    let mut offset = 0usize;
    for line in lines {
        line_starts.push(offset);
        offset += line.len() + 1;
    }
    line_starts.push(offset);

    let query = queries::highlight_query(entry);
    let mut cursor = tree_sitter::QueryCursor::new();
    // v2 registers (and opentui executes) the locals query alongside
    // highlights wherever one exists; its captures (`@local.*`) carry no
    // styling scopes, so the pass runs and is discarded — running it
    // keeps the registry entry honest and matches reference behavior.
    if let Some(locals) = queries::locals_query(entry) {
        let mut locals_matches = cursor.matches(&locals, tree.root_node(), source.as_bytes());
        while locals_matches.next().is_some() {}
        cursor.set_byte_range(0..source.len());
    }
    let mut names: Vec<Box<str>> = Vec::new();
    let mut name_index: HashMap<Box<str>, u32> = HashMap::new();
    let mut captures: Vec<Vec<CaptureSpan>> = vec![Vec::new(); lines.len()];

    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(match_) = matches.next() {
        for capture in match_.captures {
            let node = capture.node;
            let name = query.capture_names()[usize::try_from(capture.index).ok()?];
            let start_byte = node.start_byte().min(source.len());
            let end_byte = node.end_byte().min(source.len()).max(start_byte);
            if start_byte == end_byte {
                continue; // zero-width (e.g. missing nodes in recovery)
            }
            let index = *name_index.entry(name.into()).or_insert_with(|| {
                names.push(name.into());
                u32::try_from(names.len() - 1).expect("capture name arena fits u32")
            });
            // Split the range across the lines it intersects; store char
            // offsets within each line.
            let first_line = line_starts.partition_point(|start| *start <= start_byte) - 1;
            for (line, line_start) in line_starts[first_line..].iter().enumerate() {
                let line = first_line + line;
                if line >= lines.len() {
                    break; // range ended inside the trailing separator
                }
                if *line_start >= end_byte {
                    break;
                }
                let line_end = line_start + lines[line].len();
                let from = start_byte.max(*line_start);
                let to = end_byte.min(line_end);
                if from >= to {
                    continue;
                }
                let line_text = &lines[line];
                let char_start = line_text[..from - line_start].chars().count();
                let char_end =
                    char_start + line_text[from - line_start..to - line_start].chars().count();
                captures[line].push(CaptureSpan { start: char_start, end: char_end, name: index });
            }
        }
    }
    Some(HighlightedBlock(Arc::new(Inner { lines: lines.to_vec(), captures, names })))
}

/// The content-keyed block cache (see the module docs for the policy).
fn cache() -> &'static Mutex<HashMap<(String, String), HighlightedBlock>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), HighlightedBlock>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Parse (or fetch from cache) one fenced code block for a filetype.
/// `lines` are the block's lines (no trailing newline); `source` their
/// `'\n'`-join — kept separate so callers never rebuild the join twice.
pub(crate) fn highlight_cached(
    source: &str,
    lines: &[String],
    filetype: &str,
) -> Option<HighlightedBlock> {
    // Offline rule first: unknown filetype → None before any locking.
    let entry = queries::lookup(filetype)?;

    let key = (filetype.to_owned(), source.to_owned());
    let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(block) = cache.get(&key) {
        return Some(block.clone());
    }
    let block = build(source, lines, entry)?;
    if cache.len() >= CACHE_CAPACITY {
        cache.clear();
    }
    cache.insert(key, block.clone());
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capture names active on one line (test observability into the
    /// engine's raw captures — unit tests assert on these, not styles).
    fn captures_on(block: &HighlightedBlock, line: usize) -> Vec<(usize, usize, String)> {
        block.0.captures[line]
            .iter()
            .map(|span| (span.start, span.end, block.0.names[span.name as usize].to_string()))
            .collect()
    }

    #[test]
    fn multiline_captures_split_across_lines() {
        let lines: Vec<String> = vec!["/* a".into(), "   b */".into(), "let x = 1;".into()];
        let source = lines.join("\n");
        let block = highlight_cached(&source, &lines, "rust").expect("rust parses");
        let first = captures_on(&block, 0);
        assert!(
            first.iter().any(|(start, end, name)| name == "comment" && *start == 0 && *end == 4),
            "comment opens on line 0: {first:?}"
        );
        let second = captures_on(&block, 1);
        assert!(
            second.iter().any(|(start, end, name)| name == "comment" && *start == 0 && *end == 7),
            "comment closes on line 1: {second:?}"
        );
    }

    #[test]
    fn cache_returns_the_same_block() {
        let lines: Vec<String> = vec!["let x = 1;".into()];
        let source = lines.join("\n");
        let first = highlight_cached(&source, &lines, "rs").expect("rs parses");
        let second = highlight_cached(&source, &lines, "rs").expect("cached");
        assert!(Arc::ptr_eq(&first.0, &second.0), "same content hits the cache");
    }

    #[test]
    fn error_recovered_input_still_highlights() {
        // Error-recovered trees carry captures for the valid parts.
        let lines: Vec<String> = vec!["let x = ; unclosed".into()];
        let source = lines.join("\n");
        let block = highlight_cached(&source, &lines, "rust").expect("recovered tree");
        let names = captures_on(&block, 0);
        assert!(names.iter().any(|(_, _, name)| name == "keyword"), "{names:?}");
    }
}
