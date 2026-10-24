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
//! ## Catalog status (R-01b + G-08)
//!
//! The full catalog: the spike's 3 languages (rust, python, typescript)
//! plus 17 more filetypes (go, c, cpp, java, json, yaml, toml, html,
//! css, diff, javascript, ruby, php — R-01b — and lua, vim, scala,
//! make — G-08, v2's tail languages now registered at their exact ref
//! pins: lua v0.5.0, vim v0.8.1, scala v0.24.0, make v1.1.1; 20
//! registry entries, queries vendored at the exact crate-version tags,
//! see `queries/*.scm` headers and [`queries`] for the v2 routing
//! notes). toml and vim grammars ship as vendored path crates under
//! `third_party/`. csharp/kotlin/swift were dropped for release-binary
//! size (ROADMAP R-01b); the registry design carries the alias
//! convention (`udiff`/`patch` for diff, `makefile`/`gnumake` for
//! make, `vi` for vim).
//! Binary-size and perf measurements accompany the R-01b report.

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
        let js = highlight(source, "js").expect("js alias");
        assert_captures(&js, &["keyword"]);
    }

    // -- R-01b catalog: per-language capture coverage -------------------

    #[test]
    fn go_snippet_captures_the_core_scopes() {
        let block = highlight(
            "// note\nfunc greet(name string) string {\n\tmsg := \"hello\"\n\treturn msg\n}\n",
            "go",
        )
        .expect("go parses");
        assert_captures(&block, &["comment", "keyword", "function", "type", "string"]);
    }

    #[test]
    fn c_snippet_captures_the_core_scopes() {
        // The pinned parser-repo query types primitives as plain @type
        // (no type.builtin split — that's nvim's).
        let block = highlight("/* note */\nint add(int a, int b) {\n  return a + b;\n}\n", "c")
            .expect("c parses");
        assert_captures(&block, &["comment", "keyword", "function", "type", "operator"]);
    }

    #[test]
    fn cpp_snippet_captures_the_core_scopes() {
        // The pinned parser-repo cpp query is the minimal upstream one:
        // no comment pattern (comments render base-styled), functions
        // only through template/qualified declarators, strings via raw
        // literals — the trade of exact-tag ABI coherence over nvim's
        // fuller cross-repo query. The core scopes still hold.
        let block = highlight(
            "namespace App {\nclass Point {};\ntemplate <typename T> T id(T v) { return v; }\nint best = std::max(1, 2);\nconst char* msg = R\"(hi)\";\n}\n",
            "cpp",
        )
        .expect("cpp parses");
        assert_captures(&block, &["keyword", "type", "function", "string"]);
    }

    #[test]
    fn java_snippet_captures_the_core_scopes() {
        let block = highlight(
            "// note\nclass Point {\n  String greet(String name) {\n    return \"hi\";\n  }\n}\n",
            "java",
        )
        .expect("java parses");
        assert_captures(&block, &["comment", "keyword", "type", "string"]);
    }

    #[test]
    fn json_snippet_captures_the_core_scopes() {
        let block =
            highlight("{\n  \"key\": \"value\",\n  \"n\": 1\n}\n", "json").expect("json parses");
        assert_captures(&block, &["string.special.key", "string", "number"]);
    }

    #[test]
    fn yaml_snippet_captures_the_core_scopes() {
        let block = highlight("# note\nname: value\nlist:\n  - 1\n  - \"two\"\n", "yaml")
            .expect("yaml parses");
        assert_captures(&block, &["comment", "property", "number", "string"]);
    }

    #[test]
    fn toml_snippet_captures_the_core_scopes() {
        let block = highlight("# note\nkey = \"value\"\nnum = 1\n", "toml").expect("toml parses");
        assert_captures(&block, &["comment", "property", "string", "number"]);
    }

    #[test]
    fn html_snippet_captures_the_core_scopes() {
        let block =
            highlight("<!-- note -->\n<p class=\"x\">hi</p>\n", "html").expect("html parses");
        assert_captures(&block, &["comment", "tag", "attribute", "string"]);
    }

    #[test]
    fn css_snippet_captures_the_core_scopes() {
        let block =
            highlight("/* note */\np.cls {\n  color: red;\n}\n", "css").expect("css parses");
        assert_captures(&block, &["comment", "tag", "property"]);
    }

    #[test]
    fn diff_snippet_captures_the_core_scopes() {
        // The unified-patch shape the diff viewer and ```diff fences
        // carry; the udiff/patch aliases route to the same grammar.
        let source = "--- a/file.rs\n+++ b/file.rs\n@@ -1,2 +1,2 @@\n const\n-old\n+new\n";
        let block = highlight(source, "diff").expect("diff parses");
        assert_captures(&block, &["diff.plus", "diff.minus"]);
        let alias = highlight(source, "patch").expect("patch alias");
        assert_captures(&alias, &["diff.plus", "diff.minus"]);
    }

    #[test]
    fn javascript_snippet_captures_the_core_scopes() {
        // The js entry runs the same vendored query the typescript
        // composition uses — against the javascript grammar itself.
        let block = highlight(
            "// note\nconst msg = \"hi\";\nfunction greet(p) {\n  return msg;\n}\n",
            "javascript",
        )
        .expect("javascript parses");
        assert_captures(&block, &["comment", "keyword", "string", "function"]);
    }

    #[test]
    fn ruby_snippet_captures_the_core_scopes() {
        let block = highlight("# note\ndef greet(name)\n  msg = \"hi\"\n  msg\nend\n", "ruby")
            .expect("ruby parses");
        assert_captures(&block, &["comment", "keyword", "function.method", "string"]);
    }

    #[test]
    fn php_snippet_captures_the_core_scopes() {
        let block =
            highlight("<?php\n// note\nfunction greet($name) {\n  return \"hi\";\n}\n", "php")
                .expect("php parses");
        assert_captures(&block, &["comment", "keyword", "function", "string"]);
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
