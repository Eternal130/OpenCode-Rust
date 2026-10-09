//! Taffy flex-tree layout mirroring the opentui box tree of the golden
//! `tool-wrap-rows` scenario.
//!
//! Reference fixture (`test/cli/tui/inline-tool-wrap-snapshot.test.tsx`):
//!
//! ```text
//! <box flexDirection="column" width={72}>        // root
//!   <box flexDirection="column">                 // rows container
//!     <InlineToolRow …/> × 5                      // paddingLeft=3
//!       <box flexDirection="row">
//!         <text width={2}>{icon}</text>           // INLINE_TOOL_ICON_WIDTH
//!         <text flexGrow={1}>{label}</text>       // wraps, width 72-3-2=67
//!       </box>
//!   </box>
//! </box>
//! ```
//!
//! Text leaves carry their content as a taffy node context and are measured
//! through [`super::wrap::wrap_label`] during layout — the same division of
//! labor the opentui renderer uses (yoga measures text leaves, the painter
//! walks the computed layout).
//!
//! Layout coordinates and measured text extents are bounded by terminal
//! dimensions (far below 2^23), so the pedantic float↔int cast lints do not
//! carry real risk here.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

use taffy::prelude::*;
use taffy::{AvailableSpace, Dimension, LengthPercentage, NodeId, Rect, Size, Style, TaffyTree};

use super::wrap::{display_width, wrap_label};

/// Icon column width (`INLINE_TOOL_ICON_WIDTH` in the reference source).
pub const ICON_WIDTH: f32 = 2.0;
/// Fixture box width (`width={72}` in the reference test).
pub const BOX_WIDTH: f32 = 72.0;

/// One tool row of the reference fixture.
#[derive(Debug, Clone)]
pub struct ToolRowFixture {
    pub icon: char,
    pub label: String,
}

/// The five tool rows of `inline-tool-wrap-snapshot.test.tsx` (byte-for-byte
/// labels; row 4 carries an error that stays hidden without `errorExpanded`).
#[must_use]
pub fn tool_wrap_fixture() -> Vec<ToolRowFixture> {
    vec![
        ToolRowFixture {
            icon: '✱',
            label: r#"Grep "OPENCODE.*DB|database|sqlite|drizzle|dev.*db|data.*dir|xdg|APPDATA" in packages/opencode/src (151 matches)"#.into(),
        },
        ToolRowFixture {
            icon: '✱',
            label: r#"Glob "**/*db*" in packages/opencode (6 matches)"#.into(),
        },
        ToolRowFixture {
            icon: '→',
            label: "Read packages/opencode/src/storage/db.ts [offset=1, limit=130]".into(),
        },
        ToolRowFixture {
            icon: '→',
            label: "Read packages/opencode/src/index.ts [offset=1, limit=100]".into(),
        },
        ToolRowFixture {
            icon: '✱',
            label: r#"Grep "export const OPENCODE_DB|OPENCODE_DB|OPENCODE_DEV|Global\.Path\.data|data =" in packages/opencode/src (115 matches)"#.into(),
        },
    ]
}

/// Node context for measured text leaves.
#[derive(Debug, Clone)]
pub enum TextLeaf {
    Icon { glyph: char },
    Label { text: String },
}

/// A computed tool row: where to paint the icon and the wrapped label lines.
#[derive(Debug, Clone)]
pub struct LaidOutRow {
    pub icon: char,
    pub icon_x: usize,
    pub y: usize,
    pub label_x: usize,
    pub lines: Vec<String>,
}

/// Laid-out fixture: rows in paint order plus the total content height.
#[derive(Debug, Clone, Default)]
pub struct LaidOutFixture {
    pub rows: Vec<LaidOutRow>,
    pub content_height: f32,
}

struct RowNodes {
    wrapper: NodeId,
    content: NodeId,
    icon: NodeId,
    label: NodeId,
}

/// Compute the layout of the fixture box via taffy (integral coordinates —
/// rounding is enabled by default).
#[must_use]
pub fn layout_tool_wrap(fixture: &[ToolRowFixture]) -> LaidOutFixture {
    let mut tree: TaffyTree<TextLeaf> = TaffyTree::new();
    let nodes: Vec<RowNodes> = fixture.iter().map(|row| build_row(&mut tree, row)).collect();
    let root = build_root(&mut tree, &nodes);
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(BOX_WIDTH), height: AvailableSpace::MaxContent },
        |known, available, _node, context, _style| {
            measure_text_leaf(known, available, context.as_deref())
        },
    )
    .expect("taffy layout");
    let content_height = tree.layout(root).expect("root layout").size.height;
    LaidOutFixture {
        rows: nodes.iter().map(|node| collect_row(&tree, node)).collect(),
        content_height,
    }
}

/// Text measurement contract for label leaves: definite constraints wrap to
/// the wrapped size; `MinContent` reports the longest word (the CSS automatic
/// minimum); `MaxContent` reports the single line. Reporting the longest
/// word at min-content is what lets taffy shrink wrapped labels below their
/// intrinsic width.
fn measure_text_leaf(
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
    context: Option<&TextLeaf>,
) -> Size<f32> {
    match context {
        Some(TextLeaf::Icon { .. }) => Size { width: ICON_WIDTH, height: 1.0 },
        Some(TextLeaf::Label { text }) => {
            let definite = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let (width, height) = match definite {
                Some(width) => {
                    let width = width.floor().max(0.0) as usize;
                    let lines = wrap_label(text, width);
                    let longest = lines.iter().map(|line| display_width(line)).max().unwrap_or(0);
                    (longest, lines.len())
                }
                None if available.width == AvailableSpace::MinContent => {
                    (text.split(' ').map(display_width).max().unwrap_or(0), 1)
                }
                None => (display_width(text), 1),
            };
            Size { width: width as f32, height: height as f32 }
        }
        None => Size::ZERO,
    }
}

fn build_row(tree: &mut TaffyTree<TextLeaf>, row: &ToolRowFixture) -> RowNodes {
    let icon = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::length(ICON_WIDTH), height: Dimension::auto() },
                flex_shrink: 0.0,
                ..Style::default()
            },
            TextLeaf::Icon { glyph: row.icon },
        )
        .expect("icon leaf");
    let label = tree
        .new_leaf_with_context(
            Style {
                flex_grow: 1.0,
                flex_shrink: 1.0,
                min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
                ..Style::default()
            },
            TextLeaf::Label { text: row.label.clone() },
        )
        .expect("label leaf");
    let content = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Row,
                // opentui boxes stretch to the parent width by default.
                size: Size { width: Dimension::percent(1.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[icon, label],
        )
        .expect("row content");
    let wrapper = tree
        .new_with_children(
            Style {
                padding: Rect {
                    left: LengthPercentage::length(3.0),
                    right: LengthPercentage::ZERO,
                    top: LengthPercentage::ZERO,
                    bottom: LengthPercentage::ZERO,
                },
                ..Style::default()
            },
            &[content],
        )
        .expect("row wrapper");
    RowNodes { wrapper, content, icon, label }
}

fn build_root(tree: &mut TaffyTree<TextLeaf>, nodes: &[RowNodes]) -> NodeId {
    let row_ids: Vec<NodeId> = nodes.iter().map(|node| node.wrapper).collect();
    let column = tree
        .new_with_children(
            Style { flex_direction: FlexDirection::Column, ..Style::default() },
            &row_ids,
        )
        .expect("rows container");
    tree.new_with_children(
        Style {
            flex_direction: FlexDirection::Column,
            size: Size { width: Dimension::length(BOX_WIDTH), height: Dimension::auto() },
            ..Style::default()
        },
        &[column],
    )
    .expect("root")
}

fn collect_row(tree: &TaffyTree<TextLeaf>, node: &RowNodes) -> LaidOutRow {
    let wrapper = *tree.layout(node.wrapper).expect("wrapper layout");
    let content = *tree.layout(node.content).expect("content layout");
    let icon = *tree.layout(node.icon).expect("icon layout");
    let label = *tree.layout(node.label).expect("label layout");
    let TextLeaf::Label { text } =
        tree.get_node_context(node.label).cloned().expect("label context")
    else {
        unreachable!("label leaf context");
    };
    let base_x = wrapper.location.x + content.location.x;
    LaidOutRow {
        icon: match tree.get_node_context(node.icon) {
            Some(TextLeaf::Icon { glyph }) => *glyph,
            _ => ' ',
        },
        icon_x: (base_x + icon.location.x).round() as usize,
        y: (wrapper.location.y + content.location.y + icon.location.y).round() as usize,
        label_x: (base_x + label.location.x).round() as usize,
        lines: wrap_label(&text, label.size.width.floor().max(0.0) as usize),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taffy_mirrors_the_reference_box_tree() {
        let laid_out = layout_tool_wrap(&tool_wrap_fixture());
        println!("{laid_out:#?}");
        // Five rows at x=3 (padding), icon at x=3, label at x=5.
        assert_eq!(laid_out.rows.len(), 5);
        for (index, row) in laid_out.rows.iter().enumerate() {
            assert_eq!(row.icon_x, 3, "icon x for row {index}");
            assert_eq!(row.label_x, 5, "label x for row {index}");
        }
        // Stacked column: rows at y = 0, 2, 3, 4, 5 (labels 1 and 5 wrap to
        // 2 lines; row 4's error stays hidden without errorExpanded).
        let ys: Vec<usize> = laid_out.rows.iter().map(|row| row.y).collect();
        assert_eq!(ys, vec![0, 2, 3, 4, 5]);
        assert_eq!(laid_out.rows[0].lines.len(), 2);
        assert_eq!(laid_out.rows[4].lines.len(), 2);
        assert_eq!(laid_out.rows[2].lines.len(), 1);
        assert_eq!(laid_out.content_height, 7.0);
    }
}
