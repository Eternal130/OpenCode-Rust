//! The oc-syntax consumption helper for **path-shaped** sources — the
//! Rust counterpart of v2's `util/filetype.ts` (`filetype(path)` → the
//! `<code filetype={…}>` prop): a file path's extension resolves to a
//! registry filetype, and the payload highlights through the same
//! oc-syntax engine the markdown fences use ([`oc_syntax::highlight`],
//! content-keyed cache included).
//!
//! Consumers: the diff viewer's text column
//! ([`crate::components::diff_viewer`]) and the tool-row code payload
//! ([`crate::components::tool_rows`], v2's Write-tool `# Wrote` block).
//! Unknown extensions return `None` — callers render plain lines,
//! exactly today's behavior (the offline rule).

use crate::layout::StyledSpan;
use ratatui::style::{Color, Style};

use oc_syntax::{HighlightedBlock, SyntaxStyles};

/// Path extension → registry filetype (v2 `LANGUAGE_EXTENSIONS` subset
/// that hits the registered 19-grammar catalog, plus `.toml`/`.h` the
/// registry covers). v2's collapse rule (`filetype()`:
/// typescriptreact/javascriptreact/javascript → `typescript`) is kept
/// — `.js` highlights through the typescript grammar, as in v2.
#[must_use]
pub fn filetype_for_path(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "rs" => Some("rust"),
        "py" | "pyi" => Some("python"),
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Some("typescript"),
        "go" => Some("go"),
        "c" | "h" => Some("c"),
        "cpp" | "cxx" | "cc" | "c++" | "hpp" | "hh" | "hxx" => Some("cpp"),
        "cs" | "csx" => Some("csharp"),
        "java" => Some("java"),
        "json" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "diff" | "patch" => Some("diff"),
        "kt" | "kts" => Some("kotlin"),
        "rb" | "rake" | "gemspec" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        _ => None,
    }
}

/// Highlight one code payload by its file path's extension: the parsed
/// block when a grammar is registered, `None` for unknown extensions
/// (plain rendering — never blocks, never panics).
#[must_use]
pub fn highlight_for_path(content: &str, path: &str) -> Option<HighlightedBlock> {
    let filetype = filetype_for_path(path)?;
    oc_syntax::highlight(content, filetype)
}

/// Materialize one highlighted line as oc-tui spans, the shared shape
/// both consumers paint: `base` for uncaptured gaps, theme-matched
/// syntax styles elsewhere — and the run's background **pinned to
/// `base`'s** (syntax styles may only recolor the foreground; the diff
/// viewer's row backgrounds and the transcript's background own bg).
#[must_use]
pub fn line_spans(
    block: &HighlightedBlock,
    line: usize,
    base: Style,
    syntax: &SyntaxStyles,
) -> Vec<StyledSpan> {
    let background = base.bg;
    block
        .line_spans(line, base, syntax)
        .into_iter()
        .map(|span| {
            let mut style = span.style;
            style.bg = background;
            StyledSpan { text: span.text, style }
        })
        .collect()
}

/// Convenience for plain-render fallbacks: the payload's lines as
/// single base-styled spans (what unknown extensions render).
#[must_use]
pub fn plain_lines(content: &str, base: Style) -> Vec<StyledSpan> {
    content.lines().map(|line| StyledSpan { text: line.to_owned(), style: base }).collect()
}

/// Clip a line's styled spans at `width` display columns — the styled
/// counterpart of `wrap::take_width` (same cluster-width math), with
/// the budget shared across the line's spans.
#[must_use]
pub fn clip_spans(spans: Vec<StyledSpan>, width: usize) -> Vec<StyledSpan> {
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

/// The base paint style for one code line in the transcript surfaces:
/// `fg` over the given background.
#[must_use]
pub fn code_base(fg: Color, background: Color) -> Style {
    Style::new().fg(fg).bg(background)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v2's `filetype()` routing: the registered extensions resolve,
    /// the js-family collapses to typescript, unknowns return None.
    #[test]
    fn extensions_route_to_registry_filetypes() {
        assert_eq!(filetype_for_path("src/main.rs"), Some("rust"));
        assert_eq!(filetype_for_path("lib/util.py"), Some("python"));
        assert_eq!(filetype_for_path("app.ts"), Some("typescript"));
        assert_eq!(filetype_for_path("app.tsx"), Some("typescript"));
        assert_eq!(filetype_for_path("app.js"), Some("typescript"), "v2 collapses js → ts");
        assert_eq!(filetype_for_path("main.go"), Some("go"));
        assert_eq!(filetype_for_path("stdio.h"), Some("c"));
        assert_eq!(filetype_for_path("matrix.cpp"), Some("cpp"));
        assert_eq!(filetype_for_path("Point.cs"), Some("csharp"));
        assert_eq!(filetype_for_path("App.java"), Some("java"));
        assert_eq!(filetype_for_path("pkg.json"), Some("json"));
        assert_eq!(filetype_for_path("ci.yaml"), Some("yaml"));
        assert_eq!(filetype_for_path("Cargo.toml"), Some("toml"));
        assert_eq!(filetype_for_path("page.html"), Some("html"));
        assert_eq!(filetype_for_path("style.css"), Some("css"));
        assert_eq!(filetype_for_path("0001.patch"), Some("diff"));
        assert_eq!(filetype_for_path("Main.kt"), Some("kotlin"));
        assert_eq!(filetype_for_path("gem.rb"), Some("ruby"));
        assert_eq!(filetype_for_path("index.php"), Some("php"));
        assert_eq!(filetype_for_path("UI.swift"), Some("swift"));
        // Case-insensitive extensions; unknown → None (plain render).
        assert_eq!(filetype_for_path("README.MD"), None);
        assert_eq!(filetype_for_path("notes.txt"), None);
        assert_eq!(filetype_for_path("no-extension"), None);
        assert_eq!(filetype_for_path(""), None);
    }

    /// The full helper: a rust payload parses and materializes themed
    /// spans whose bg stays pinned; an unknown extension is None.
    #[test]
    fn highlight_for_path_parses_and_pins_background() {
        let theme = crate::theme::ThemeState::v2_dark().styles();
        let block =
            highlight_for_path("// note\nlet msg = \"hi\";\n", "src/main.rs").expect("rust");
        let base = code_base(theme.text.fg.unwrap_or_default(), theme.background);
        let spans = line_spans(&block, 0, base, &theme.syntax);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "// note");
        assert_ne!(spans[0].style.fg, base.fg, "comment resolves to a theme style");
        assert_eq!(spans[0].style.bg, Some(theme.background), "bg pinned to base");
        assert!(highlight_for_path("x = 1", "data.txt").is_none());
    }
}
