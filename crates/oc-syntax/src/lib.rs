//! oc-syntax — the tree-sitter syntax highlighting engine for fenced
//! code blocks (R-01 spike; the native-grammar counterpart of v2's
//! wasm-parser pipeline in `packages/tui/src/parsers-config.ts`).
//!
//! ## Shape
//!
//! * [`highlight`] — parse one code block once for a filetype (unknown
//!   filetypes return `None` instantly: no grammar, no blocking, no
//!   panics — the caller renders plain lines, exactly today's behavior).
//! * [`HighlightedBlock::line_spans`] — materialize one pre-parsed line
//!   as styled spans against the theme's flattened syntax styles
//!   ([`SyntaxStyles`], built from oc-theme's
//!   `generate_syntax`/`convert_theme_to_styles` pair). Pure lookup —
//!   never parses, never touches tree-sitter.
//! * tree-sitter types never leak: the public surface is spans, styles
//!   and one opaque block handle.
//!
//! ## Spike status (R-01)
//!
//! Three languages wired (rust, python, typescript — queries vendored at
//! the exact crate-version tags, see `queries/*.scm` headers); the v2
//! catalog shape (aliases like `udiff`/`patch`, `make`/`makefile`) is
//! carried by the registry design in [`queries`]. Go/no-go findings live
//! in the spike report; binary-size and perf measurements accompany it.

mod engine;
mod queries;
mod scopes;

pub use engine::HighlightedBlock;
pub use scopes::SyntaxStyles;

/// One styled run of a highlighted line (oc-tui's span shape; the TUI
/// converts trivially since both fields are public).
#[derive(Debug, Clone, PartialEq)]
pub struct StyledSpan {
    pub text: String,
    pub style: ratatui::style::Style,
}

/// Highlight one fenced code block for a filetype.
///
/// `block` is the block's content **without** trailing newline(s);
/// `filetype` is the fence info string (`"rust"`, `"rs"`, `"ts"`, … —
/// aliases and `,extra` fields are normalized by the registry lookup).
///
/// Returns `None` for unknown filetypes (instantly — the offline rule),
/// for grammars that fail to initialize, and for parses that hit the
/// per-parser timeout ceiling; callers fall back to plain rendering.
#[must_use]
pub fn highlight(block: &str, filetype: &str) -> Option<HighlightedBlock> {
    let lines: Vec<String> = block.lines().map(str::to_owned).collect();
    engine::highlight_cached(block, &lines, filetype)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_filetype_returns_none_instantly() {
        assert!(highlight("let x = 1;", "brainfuck").is_none());
        assert!(highlight("let x = 1;", "").is_none());
        assert!(highlight("", "rust").is_some(), "empty block still highlights");
    }

    #[test]
    fn block_without_trailing_newline_matches_lines() {
        // `block.lines()` and the engine's join must agree for every
        // call shape the markdown renderer produces.
        let block = highlight("fn main() {}\n// tail", "rust").expect("rust");
        assert_eq!(block.line_count(), 2);
    }

    // -- per-language capture coverage (spike acceptance) ----------------

    /// Assert every listed capture name appears somewhere in the block.
    #[track_caller]
    fn assert_captures(block: &HighlightedBlock, expected: &[&str]) {
        let all: Vec<String> = (0..block.line_count())
            .flat_map(|line| block.captures_at(line).into_iter().map(|(_, _, name)| name))
            .collect();
        for wanted in expected {
            assert!(
                all.iter().any(|name| name == wanted),
                "capture @{wanted} missing (got: {all:?})"
            );
        }
    }

    #[test]
    fn rust_snippet_captures_the_core_scopes() {
        let block = highlight(
            "// doc note\nstruct Point { x: u32 }\nfn greet(name: &str) -> &str {\n    let msg = \"hello\\n\";\n    msg\n}\n",
            "rust",
        )
        .expect("rust parses");
        assert_captures(
            &block,
            &[
                "comment",
                "function",
                "keyword",
                "string",
                "escape",
                "variable.parameter",
                "type",
                "type.builtin",
            ],
        );
    }

    #[test]
    fn python_snippet_captures_the_core_scopes() {
        // Note: the pinned parser-repo python query has no parameter
        // pattern (nvim's does) — parameters fall under @variable.
        let block = highlight(
            "# top note\ndef greet(name):\n    msg = f\"hello {name}\"\n    return msg\n",
            "python",
        )
        .expect("python parses");
        assert_captures(&block, &["comment", "function", "keyword", "string", "variable"]);
    }

    #[test]
    fn typescript_snippet_captures_the_core_scopes() {
        // Exercises the composed javascript base (const/string/comment)
        // AND the TS-specific tail (interface/type) of the query.
        let block = highlight(
            "// note\ninterface Point {\n  x: number;\n}\nconst msg: string = \"hello\";\nexport function greet(p: Point) {\n  return msg;\n}\n",
            "typescript",
        )
        .expect("typescript parses");
        assert_captures(
            &block,
            &["comment", "keyword", "type", "type.builtin", "string", "function"],
        );
    }

    #[test]
    fn fence_aliases_route_to_the_same_grammar() {
        let source = "let x = 1;";
        let rust = highlight(source, "rust").expect("rust");
        let rs = highlight(source, "rs").expect("rs alias");
        assert_captures(&rust, &["keyword"]);
        assert_captures(&rs, &["keyword"]);
        let ts = highlight(source, "ts").expect("ts alias");
        assert_captures(&ts, &["keyword"]);
    }

    // -- measurement (spike report: parse-once + span materialization) --

    fn v2_dark_syntax_styles() -> SyntaxStyles {
        let source = oc_theme::builtins::builtin_v1_theme("opencode").expect("opencode builtin");
        let value: serde_json::Value = serde_json::from_str(source).expect("builtin JSON parses");
        let document = oc_theme::migrate_v1(&value).expect("v1 migrates");
        let theme = oc_theme::resolve_theme_document(&document, Some(oc_theme::Mode::Dark))
            .expect("resolves");
        SyntaxStyles::from_rules(&oc_theme::generate_syntax(&theme.tokens))
    }

    /// Perf micro-bench for the spike report: highlight a ~2000-line
    /// rust block once (parse + query), then materialize the spans of
    /// 30 visible lines (the render path). Run with
    /// `NOCAPTURE=1 cargo nextest run --release perf` for the numbers;
    /// the wall assert is a generous regression ceiling (10× the target).
    #[test]
    fn perf_2000_line_block_once_then_30_visible_lines() {
        let lines: Vec<String> = (0..2000)
            .map(|index| {
                format!(
                    "fn f{index}(x: u32) -> u32 {{ let s = \"text{index}\"; x + {index} }} // note {index}"
                )
            })
            .collect();
        let source = lines.join("\n");
        let styles = v2_dark_syntax_styles();
        let base = ratatui::style::Style::new();

        let started = std::time::Instant::now();
        let block = highlight(&source, "rust").expect("rust parses");
        let parse_ms = started.elapsed().as_secs_f64() * 1000.0;

        let started = std::time::Instant::now();
        let mut span_count = 0usize;
        for line in 0..30 {
            span_count += block.line_spans(line, base, &styles).len();
        }
        let spans_ms = started.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "perf: parse+query 2000-line rust block = {parse_ms:.2} ms; \
             30-line span materialization = {spans_ms:.3} ms ({span_count} spans)"
        );
        assert!(spans_ms < 100.0, "span materialization regressed: {spans_ms:.2} ms");
    }

    #[test]
    fn line_spans_preserve_text_and_restyle_captures() {
        // Style resolution through the REAL generated theme rules (the
        // same path the TUI drives): comment must differ from base, and
        // the concatenated span text must equal the source line.
        let styles = v2_dark_syntax_styles();
        let base = ratatui::style::Style::new().fg(ratatui::style::Color::Rgb(1, 2, 3));

        let block = highlight("// a comment\nlet x = \"s\";", "rust").expect("rust");
        let comment = block.line_spans(0, base, &styles);
        assert_eq!(comment.len(), 1, "one styled run: {comment:?}");
        assert_eq!(comment[0].text, "// a comment");
        assert_ne!(comment[0].style, base, "comment resolves to a theme style");

        let code = block.line_spans(1, base, &styles);
        let text: String = code.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "let x = \"s\";");
        let styles_seen: Vec<_> = code.iter().map(|span| span.style).collect();
        assert!(styles_seen.iter().any(|style| *style != base), "keyword/string styled");
        // Adjacent equal styles merged: `let x = ` is one base run, the
        // string one themed run, `;` rejoins base.
        assert!(code.len() <= 4, "merged runs: {code:?}");
    }
}
