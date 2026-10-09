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

/// The renderer's default foreground (opentui `RGBA.defaultForeground()`)
/// — what unstyled cells carry; the golden frame fill pairs it with the
/// theme background.
pub const DEFAULT_FOREGROUND: Color = Color::Rgb(255, 255, 255);

/// One tool row of the reference fixture.
#[derive(Debug, Clone)]
pub struct ToolRowFixture {
    pub icon: char,
    pub label: String,
    /// `failed={true}`: the row paints the error color even while the
    /// error text itself stays hidden (`errorExpanded` off).
    pub failed: bool,
}

/// The five tool rows of `inline-tool-wrap-snapshot.test.tsx`
/// (byte-for-byte labels; the `Read index.ts` row is `failed` — its
/// error stays hidden without `errorExpanded` but its color shows).
#[must_use]
pub fn tool_wrap_fixture() -> Vec<ToolRowFixture> {
    vec![
        ToolRowFixture {
            icon: '✱',
            label: r#"Grep "OPENCODE.*DB|database|sqlite|drizzle|dev.*db|data.*dir|xdg|APPDATA" in packages/opencode/src (151 matches)"#.into(),
            failed: false,
        },
        ToolRowFixture {
            icon: '✱',
            label: r#"Glob "**/*db*" in packages/opencode (6 matches)"#.into(),
            failed: false,
        },
        ToolRowFixture {
            icon: '→',
            label: "Read packages/opencode/src/storage/db.ts [offset=1, limit=130]".into(),
            failed: false,
        },
        ToolRowFixture {
            icon: '→',
            label: "Read packages/opencode/src/index.ts [offset=1, limit=100]".into(),
            failed: true,
        },
        ToolRowFixture {
            icon: '✱',
            label: r#"Grep "export const OPENCODE_DB|OPENCODE_DB|OPENCODE_DEV|Global\.Path\.data|data =" in packages/opencode/src (115 matches)"#.into(),
            failed: false,
        },
    ]
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
    let mut tree = LayoutTree::new();
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
            tree.box_node(&BoxSpec::default().padding_left(3.0), &[content])
        })
        .collect();
    let column = tree.box_node(&BoxSpec::column(), &wrappers);
    let root = tree.box_node(&BoxSpec::column().width(BOX_WIDTH), &[column]);
    tree.set_root(root);
    tree.compute((BOX_WIDTH, AvailableSpace::MaxContent)).placed
}

/// Render the `tool-wrap-rows` scenario at a terminal size under the
/// v2-dark corpus theme.
#[must_use]
pub fn render_tool_wrap(theme: &ThemeStyles, width: u16, height: u16) -> Frame {
    let placed = layout_tool_wrap(&tool_wrap_fixture(), theme);
    paint_placed(&placed, (width, height), fill_style(theme))
}
