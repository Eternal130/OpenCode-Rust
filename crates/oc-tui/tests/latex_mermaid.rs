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

#[test]
fn renders_mermaid_flowchart_code_block_as_ascii_diagram() {
    let rows = rendered("```mermaid\nflowchart TD\n  Start --> Stop\n```", 40);
    let text = rows.join("\n");
    assert!(text.contains("Start"), "node label missing: {text}");
    assert!(text.contains("Stop"), "node label missing: {text}");
    assert!(text.contains('▼'), "vertical arrowhead missing: {text}");
    assert!(text.contains('╭'), "rounded node frame missing: {text}");
    // The raw source must NOT render.
    assert!(!text.contains("flowchart TD"), "raw source leaked: {text}");
}

#[test]
fn renders_mermaid_sequence_code_block_as_ascii_diagram() {
    let rows = rendered(
        "```mermaid\nsequenceDiagram\n  participant B as Browser\n  participant S as Server\n  B->>S: GET /\n  S-->>B: 401\n```",
        40,
    );
    let text = rows.join("\n");
    assert!(text.contains("Browser"), "participant missing: {text}");
    assert!(text.contains("GET /"), "message label missing: {text}");
    assert!(text.contains('►'), "arrow missing: {text}");
    assert!(text.contains('│'), "lifeline missing: {text}");
}

#[test]
fn renders_mermaid_state_code_block_as_ascii_diagram() {
    let rows = rendered(
        "```mermaid\nstateDiagram-v2\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n  Loading --> Success: done\n  Success --> [*]\n```",
        40,
    );
    let text = rows.join("\n");
    assert!(text.contains("Idle"), "state label missing: {text}");
    assert!(text.contains("submit"), "transition label missing: {text}");
    assert!(text.contains('●'), "start marker missing: {text}");
    assert!(text.contains('◎'), "end marker missing: {text}");
    assert!(text.contains('▶'), "arrowhead missing: {text}");
    // The raw source must NOT render.
    assert!(!text.contains("stateDiagram-v2"), "raw source leaked: {text}");
}

#[test]
fn unknown_mermaid_kind_falls_back_to_the_plain_code_block() {
    // gitGraph is detected by upstream but not ported in this lane —
    // the fence renders through the default code path.
    let rows = rendered("```mermaid\ngitGraph\n  commit id: \"one\"\n```", 40);
    assert!(
        rows.iter().any(|row| row.contains("gitGraph")),
        "fallback code block missing: {rows:?}"
    );
}

#[test]
fn broken_mermaid_syntax_falls_back_to_the_plain_code_block() {
    let rows = rendered("```mermaid\nflowchart TD\n  A ==> totally unknown !!\n```", 40);
    assert!(
        rows.iter().any(|row| row.contains("totally unknown !!")),
        "fallback code block missing: {rows:?}"
    );
}
