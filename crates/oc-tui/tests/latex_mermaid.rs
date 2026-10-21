//! SURF-LATEX-MERMAID: the built-in LaTeX + Mermaid code-block renderers.
//!
//! v2 registers both unconditionally as markdown code-block renderers
//! (`packages/tui/src/plugin/builtins.ts`): `latex`/`math` fences render
//! through the unicode-math layout (`packages/latex`), `mermaid` fences
//! through the ASCII diagram renderer (`packages/merman`). This test
//! pins the END-TO-END seam: a fenced block inside a model text part
//! renders as the diagram/math rows, not the raw source.

use oc_tui::components::message::markdown::{render_markdown, MarkdownLine, MarkdownStyles};
use oc_tui::theme::ThemeState;

fn rendered(source: &str, width: usize) -> Vec<String> {
    let styles = MarkdownStyles::from_theme(&ThemeState::v2_dark().styles());
    render_markdown(source, width, &styles).iter().map(MarkdownLine::text).collect()
}

#[test]
fn renders_latex_code_block_as_unicode_math() {
    let rows = rendered("```math\n\\frac{x+1}{y-1}\n```", 40);
    assert!(rows.contains(&" x + 1".to_string()), "numerator row missing: {rows:?}");
    assert!(rows.contains(&"───────".to_string()), "fraction bar missing: {rows:?}");
    assert!(rows.contains(&" y - 1".to_string()), "denominator row missing: {rows:?}");
    // The raw source must NOT render (the renderer replaced the fence).
    assert!(!rows.iter().any(|row| row.contains(r"\frac")), "raw LaTeX leaked through: {rows:?}");
}

#[test]
fn renders_latex_fence_alias_too() {
    let rows = rendered("```latex\nE = mc^2\n```", 40);
    assert!(rows.contains(&"E = mc²".to_string()), "compacted scripts: {rows:?}");
}

#[test]
fn invalid_latex_falls_back_to_the_plain_code_block() {
    // `\unknown` is unsupported in strict mode → upstream keeps the
    // default code-block render for the exact source.
    let rows = rendered("```math\n\\unknown{x} + tail\n```", 40);
    assert!(
        rows.iter().any(|row| row.contains(r"\unknown{x} + tail")),
        "fallback code block missing: {rows:?}"
    );
}
