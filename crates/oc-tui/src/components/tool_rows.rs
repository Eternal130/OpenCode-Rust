//! Inline tool rows — the production absorption of the wave-7.0 spike
//! fixture (`inline-tool-wrap-snapshot.test.tsx`): a column of
//! `InlineToolRow`s at width 72, each `paddingLeft=3` → row
//! `[icon (w2), label (grow)]`, labels word-wrapped by [`crate::wrap`].
//!
//! Painted under the corpus theme (v2-dark): the frame fill uses the
//! renderer's default foreground over `background.base`; icons and
//! labels paint `text.muted` over `background.base` — matching the
//! re-pinned golden ANSI streams byte-for-byte (P1).
//!
//! Wave 0: icons and labels are **styled-segment leaves** (one span
//! each) — the trivially-equivalent migration onto the styled-segment
//! path, proven byte-identical by the golden instrument in
//! `golden_tests`.

use ratatui::style::{Color, Style};

use crate::layout::{BoxSpec, LayoutTree, StyledSpan};
use crate::render::{paint_placed, Frame};
use crate::theme::ThemeStyles;
use taffy::AvailableSpace;

/// Icon column width (`INLINE_TOOL_ICON_WIDTH`).
pub const ICON_WIDTH: f32 = 2.0;
/// Fixture box width (`width={72}`).
pub const BOX_WIDTH: f32 = 72.0;
/// The code-body line-number gutter: `minWidth={3}` + `paddingRight={1}`
/// of v2's `<line_number>` (right-aligned number, one space pad).
pub const CODE_GUTTER_WIDTH: f32 = 4.0;

/// The renderer's default foreground (opentui `RGBA.defaultForeground()`)
/// — what unstyled cells carry; the golden frame fill pairs it with the
/// theme background.
pub const DEFAULT_FOREGROUND: Color = Color::Rgb(255, 255, 255);

/// A tool row's code payload (v2's block-tool body: the Write tool's
/// `# Wrote` code block, the Read tool's loaded content — R-01b wires
/// these through oc-syntax by file extension).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRowCode {
    /// The file the code belongs to — its extension picks the grammar
    /// (v2 `filetype(path)` routing).
    pub path: String,
    /// The code text (one rendered line per source line).
    pub content: String,
}

/// One tool row of the reference fixture.
#[derive(Debug, Clone)]
pub struct ToolRowFixture {
    pub icon: char,
    pub label: String,
    /// `failed={true}`: the row paints the error color even while the
    /// error text itself stays hidden (`errorExpanded` off).
    pub failed: bool,
    /// `error`: the row's error payload — rendered under the row when
    /// failed AND `errorExpanded` (`InlineToolRow`'s second box:
    /// `paddingLeft={INLINE_TOOL_ICON_WIDTH}`, `fg={errorColor}`).
    pub error: Option<String>,
    /// R-01b: the row's code payload — v2's completed block-tool body
    /// (Write's `# Wrote` block): a numbered, oc-syntax-highlighted
    /// code column under the row.
    pub code: Option<ToolRowCode>,
}

/// The five tool rows of `inline-tool-wrap-snapshot.test.tsx`
/// (byte-for-byte labels; the `Read index.ts` row is `failed` — its
/// error stays hidden without `errorExpanded` but its color shows).
#[must_use]
pub fn tool_wrap_fixture() -> Vec<ToolRowFixture> {
    tool_wrap_rows()
        .map(|(icon, label, failed, _)| ToolRowFixture {
            icon,
            label: label.into(),
            failed,
            error: None,
            code: None,
        })
        .collect()
}

/// The `tool-wrap-error-expanded` rows: the same five with the failed
/// read carrying the LSP error payload (rendered under the row).
#[must_use]
pub fn tool_wrap_error_fixture() -> Vec<ToolRowFixture> {
    tool_wrap_rows()
        .map(|(icon, label, failed, error)| ToolRowFixture {
            icon,
            label: label.into(),
            failed,
            error: error.map(str::to_owned),
            code: None,
        })
        .collect()
}

/// The block-tool write row (v2's completed `Write`): the `# Wrote`
/// label over a numbered, syntax-highlighted code body — the R-01b
/// oc-syntax wiring through the path-extension helper (a `.rs` payload
/// highlights; `.txt` renders plain).
#[must_use]
pub fn tool_write_fixture() -> Vec<ToolRowFixture> {
    vec![
        ToolRowFixture {
            icon: '←',
            label: "Write src/main.rs".into(),
            failed: false,
            error: None,
            code: Some(ToolRowCode {
                path: "src/main.rs".into(),
                content: "// written by the tool\nfn main() {\n    println!(\"hi\");\n}\n".into(),
            }),
        },
        ToolRowFixture {
            icon: '←',
            label: "Write notes.txt".into(),
            failed: false,
            error: None,
            code: Some(ToolRowCode {
                path: "notes.txt".into(),
                content: "plain text payload\nsecond line\n".into(),
            }),
        },
    ]
}

type ToolRowSource = (char, &'static str, bool, Option<&'static str>);

fn tool_wrap_rows() -> impl Iterator<Item = ToolRowSource> {
    [
        (
            '✱',
            r#"Grep "OPENCODE.*DB|database|sqlite|drizzle|dev.*db|data.*dir|xdg|APPDATA" in packages/opencode/src (151 matches)"#,
            false,
            None,
        ),
        ('✱', r#"Glob "**/*db*" in packages/opencode (6 matches)"#, false, None),
        ('→', "Read packages/opencode/src/storage/db.ts [offset=1, limit=130]", false, None),
        ('→', "Read packages/opencode/src/index.ts [offset=1, limit=100]", true, Some("No LSP server available for this file type.")),
        (
            '✱',
            r#"Grep "export const OPENCODE_DB|OPENCODE_DB|OPENCODE_DEV|Global\.Path\.data|data =" in packages/opencode/src (115 matches)"#,
            false,
            None,
        ),
    ]
    .into_iter()
}

/// The frame fill style: renderer default foreground over the theme
/// background (the reference fills the frame with the theme style).
#[must_use]
pub fn fill_style(theme: &ThemeStyles) -> Style {
    Style::new().fg(DEFAULT_FOREGROUND).bg(theme.background)
}

/// Icon + label style: `text.muted` over the theme background; a failed
/// row paints the error color (`text.feedback.error.base`).
#[must_use]
pub fn row_style(theme: &ThemeStyles, failed: bool) -> Style {
    let fg = if failed { theme.error } else { theme.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND) };
    Style::new().fg(fg).bg(theme.background)
}

/// Build the tool-wrap box tree through the production layout layer and
/// collect the placed runs (the spike's bespoke taffy tree, now one
/// [`LayoutTree`] consumer among many).
///
/// Wave 0: icons and labels are **styled-segment leaves** (one span
/// each — trivially equivalent to the old plain leaves; the golden
/// instrument in `golden_tests` proves the outputs byte-identical),
/// exercising the styled path in production.
#[must_use]
pub fn layout_tool_wrap(
    fixture: &[ToolRowFixture],
    theme: &ThemeStyles,
) -> Vec<crate::layout::Placed> {
    layout_tool_rows(fixture, theme, false)
}

/// Build the tool-wrap box tree; `error_expanded` renders each failed
/// row's error payload under the row (`InlineToolRow`'s
/// `<Show when={failed && errorExpanded}>` box: one more paddingLeft
/// `INLINE_TOOL_ICON_WIDTH` inside the row's paddingLeft 3, the text
/// in the error color).
///
/// R-01b: a row's code payload (v2's completed block-tool body, e.g.
/// Write's `# Wrote` block) renders under the row whenever present —
/// a numbered column (v2 `<line_number minWidth={3} paddingRight={1}>`
/// semantics: number right-aligned in 3 + one space) with each line
/// oc-syntax-highlighted via the path-extension helper; unknown
/// extensions render plain base-styled lines.
#[must_use]
pub fn layout_tool_rows(
    fixture: &[ToolRowFixture],
    theme: &ThemeStyles,
    error_expanded: bool,
) -> Vec<crate::layout::Placed> {
    let mut tree = LayoutTree::new();
    let code_base = crate::components::syntax::code_base(
        theme.text.fg.unwrap_or(DEFAULT_FOREGROUND),
        theme.background,
    );
    let wrappers: Vec<_> = fixture
        .iter()
        .map(|row| {
            let style = row_style(theme, row.failed);
            let icon = tree.text_styled(
                vec![StyledSpan { text: row.icon.to_string(), style }],
                &BoxSpec::default().width(ICON_WIDTH).shrink(0.0),
            );
            let label = tree.text_styled(
                vec![StyledSpan { text: row.label.clone(), style }],
                &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0),
            );
            let content = tree.box_node(&BoxSpec::row(), &[icon, label]);
            let wrapper_spec = BoxSpec::column().padding_left(3.0);
            let mut children = vec![content];
            if error_expanded && row.failed {
                if let Some(error) = &row.error {
                    let error_text = tree.text_styled(
                        vec![StyledSpan { text: error.clone(), style }],
                        &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0),
                    );
                    let error_box =
                        tree.box_node(&BoxSpec::row().padding_left(ICON_WIDTH), &[error_text]);
                    children.push(error_box);
                }
            }
            if let Some(code) = &row.code {
                children.push(code_body_node(&mut tree, code, code_base, theme));
            }
            tree.box_node(&wrapper_spec, &children)
        })
        .collect();
    let column = tree.box_node(&BoxSpec::column(), &wrappers);
    let root = tree.box_node(&BoxSpec::column().width(BOX_WIDTH), &[column]);
    tree.set_root(root);
    tree.compute((BOX_WIDTH, AvailableSpace::MaxContent)).placed
}

/// The code body node (v2 Write's completed block): one row per source
/// line — a muted right-aligned line number (3 cols + 1 pad) then the
/// highlighted code spans (base = `text.base` over the background;
/// unknown extensions stay one plain run).
fn code_body_node(
    tree: &mut LayoutTree,
    code: &ToolRowCode,
    base: ratatui::style::Style,
    theme: &ThemeStyles,
) -> taffy::NodeId {
    let muted = theme.text_muted;
    let highlighted = crate::components::syntax::highlight_for_path(&code.content, &code.path);
    let lines: Vec<Vec<StyledSpan>> = code
        .content
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let gutter = StyledSpan { text: format!("{:>3} ", index + 1), style: muted };
            let code_spans: Vec<StyledSpan> = match &highlighted {
                Some(block) => {
                    crate::components::syntax::line_spans(block, index, base, &theme.syntax)
                }
                None => vec![StyledSpan { text: line.to_owned(), style: base }],
            };
            vec![gutter].into_iter().chain(code_spans).collect()
        })
        .collect();
    let rows: Vec<_> = lines
        .into_iter()
        .map(|spans| {
            let gutter = tree.text_styled(
                vec![StyledSpan { text: spans[0].text.clone(), style: spans[0].style }],
                &BoxSpec::default().width(CODE_GUTTER_WIDTH).shrink(0.0),
            );
            let body = tree.text_styled(
                spans[1..].to_vec(),
                &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0),
            );
            tree.box_node(&BoxSpec::row(), &[gutter, body])
        })
        .collect();
    tree.box_node(&BoxSpec::column().padding_left(ICON_WIDTH), &rows)
}

/// Render the `tool-wrap-rows` scenario at a terminal size under the
/// v2-dark corpus theme.
#[must_use]
pub fn render_tool_wrap(theme: &ThemeStyles, width: u16, height: u16) -> Frame {
    let placed = layout_tool_wrap(&tool_wrap_fixture(), theme);
    paint_placed(&placed, (width, height), fill_style(theme))
}

/// Render the `tool-wrap-error-expanded` scenario: the same rows with
/// the failed read's LSP error expanded under it.
#[must_use]
pub fn render_tool_wrap_error_expanded(theme: &ThemeStyles, width: u16, height: u16) -> Frame {
    let placed = layout_tool_rows(&tool_wrap_error_fixture(), theme, true);
    paint_placed(&placed, (width, height), fill_style(theme))
}

/// Render the block-tool write scenario (R-01b): the `# Wrote`-style
/// rows with their oc-syntax-highlighted code bodies.
#[must_use]
pub fn render_tool_wrap_code(theme: &ThemeStyles, width: u16, height: u16) -> Frame {
    let placed = layout_tool_rows(&tool_write_fixture(), theme, false);
    paint_placed(&placed, (width, height), fill_style(theme))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::CellColor;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// R-01b: the Write rows render their code bodies — numbered
    /// (right-aligned 3 + pad, muted) and, for `.rs`, oc-syntax
    /// themed (the comment ≠ base fg); `.txt` stays plain base-styled
    /// (unknown extension — the offline rule).
    #[test]
    fn write_rows_render_numbered_highlighted_code_bodies() {
        let theme = theme();
        let frame = render_tool_wrap_code(&theme, 72, 12);
        let grid = frame.text_grid();
        assert!(grid[0].starts_with("   ← Write src/main.rs"), "row 0: {:?}", grid[0]);
        // paddingLeft 3 (wrapper) + 2 (code box) → x=5; the gutter is
        // `minWidth 3 + 1 pad`, so the number lands at col 7, code at 9.
        assert!(grid[1].starts_with("       1 // written by the tool"), "row 1: {:?}", grid[1]);
        assert!(grid[2].starts_with("       2 fn main() {"), "row 2: {:?}", grid[2]);
        assert!(grid[3].starts_with("       3     println!(\"hi\");"), "row 3: {:?}", grid[3]);
        let base = CellColor::from(theme.text.fg.unwrap_or(DEFAULT_FOREGROUND));
        assert_eq!(
            frame.cell(1, 7).fg,
            CellColor::from(theme.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND)),
            "line number muted"
        );
        assert_ne!(frame.cell(1, 9).fg, base, "the comment is themed, not base");
        assert_eq!(frame.cell(1, 9).bg, CellColor::from(theme.background));
        // The plain payload keeps base fg on every code cell.
        assert!(grid[5].starts_with("   ← Write notes.txt"), "row 5: {:?}", grid[5]);
        assert!(grid[6].starts_with("       1 plain text payload"), "row 6: {:?}", grid[6]);
        assert_eq!(frame.cell(6, 9).fg, base, "unknown extension renders plain");
    }

    /// Rows without payloads ignore the expansion flag — the code/error
    /// body plumbing changed nothing for the plain inline fixtures
    /// (their goldens stay byte-exact).
    #[test]
    fn rows_without_payloads_ignore_the_expansion_flag() {
        let theme = theme();
        let collapsed = layout_tool_rows(&tool_wrap_fixture(), &theme, false);
        let expanded = layout_tool_rows(&tool_wrap_fixture(), &theme, true);
        let texts = |placed: &[crate::layout::Placed]| -> Vec<Vec<String>> {
            placed.iter().map(crate::layout::Placed::line_texts).collect()
        };
        assert_eq!(texts(&collapsed), texts(&expanded));
        let write = layout_tool_rows(&tool_write_fixture(), &theme, false);
        assert!(texts(&write).len() > texts(&collapsed).len(), "code bodies add placed lines");
    }
}
