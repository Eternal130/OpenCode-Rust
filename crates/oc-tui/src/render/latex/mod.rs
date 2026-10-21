//! Unicode math typography for terminal LaTeX blocks — the port of
//! opencode v2 `packages/latex/src` (parser/layout/symbols/limits).
//!
//! Upstream contract (v2.0.24, `packages/latex`): a fenced
//! `latex`/`math` code block parses through a strict-mode TeX subset
//! parser into a [`MathNode`] tree, then lays out onto a cell grid
//! (fractions with rules, radicals with vinculum, stretching
//! delimiters, matrices, scripts via Unicode super/subscripts, …).
//! The TUI registers the result as the code-block renderer
//! (`packages/tui/src/plugin/builtins.ts` registers Latex and Merman
//! unconditionally); see `components/message/markdown.rs`.
//!
//! Port notes:
//! * The parser indexes `Vec<char>` (upstream indexes UTF-16 units —
//!   equivalent for every command the grammar recognizes).
//! * `limits` and `types` are folded into this module
//!   (upstream `limits.ts`, ~40 lines).
//! * Cell widths use the upstream `cellWidth` port (not unicode-width)
//!   so combining marks stay width-0 inside glyphs.

pub mod layout;
pub mod parser;
pub mod symbols;
pub mod types;

#[cfg(test)]
mod tests;

pub use layout::{cell_width as glyph_width, MathLayout, ParseDefaults, RenderLatexOptions};
pub use parser::parse_latex;
pub use types::{LatexError, MathNode, ParseOptions};

/// `renderLatex` (render.ts): parse + layout.
pub fn render_latex(source: &str, options: &RenderLatexOptions) -> Result<MathLayout, LatexError> {
    Ok(layout::layout_math(&parse_latex(source, &options.parse_options())?, options))
}

/// `renderLatexToString` (render.ts): the grid as trimmed rows joined
/// by newlines (the golden-test surface).
pub fn render_latex_to_string(
    source: &str,
    options: &RenderLatexOptions,
) -> Result<String, LatexError> {
    Ok(render_latex(source, options)?.to_string())
}
