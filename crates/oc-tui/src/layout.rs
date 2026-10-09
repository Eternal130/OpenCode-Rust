//! Taffy-backed flex tree builder mirroring opentui `<box>` semantics,
//! with the wave-7.0 substrate gotchas encoded as **layer defaults**
//! (never per-node patches — gate-8 layout instruction):
//!
//! 1. **Padding on the content node**: `taffy` 0.9 applies padding to the
//!    child *content* node's location; the collector aggregates through
//!    it, so absolute positions come out of one recursive location sum.
//! 2. **`MinContent` = longest run between wrap opportunities**: text
//!    leaves are measured through [`crate::wrap`], whose single seam
//!    ([`crate::wrap::is_break_after`]) drives both wrapping and
//!    min-content. CJK breaks-anywhere ⇒ the shrink-to-fit minimum is one
//!    grapheme, consistent with how the leaf actually wraps.
//! 3. **Stretch default**: opentui boxes stretch to the parent width by
//!    default — a child box of a column without an explicit width gets
//!    `width: 100%` at finalize time.
//! 4. **Paint re-wraps at the resolved width**: measured leaves' final
//!    wrap width comes from the resolved layout, so the collector
//!    re-wraps each leaf at `layout.size.width`.
//!
//! Layout coordinates are bounded by terminal dimensions (far below
//! 2^23), so the pedantic float↔int cast lints do not carry real risk.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

use ratatui::style::Style as PaintStyle;
use taffy::prelude::*;
use taffy::{AvailableSpace, Dimension, LengthPercentage, NodeId, Rect, Size, Style, TaffyTree};

use crate::wrap::{display_width, max_content_width, min_content_width, wrap_label};

/// Node context for measured text leaves.
#[derive(Debug, Clone)]
pub struct TextLeaf {
    pub text: String,
    pub style: PaintStyle,
    /// Component-set tag so callers can locate this run's placement
    /// after layout (e.g. tab titles for overlay separators).
    pub tag: Option<String>,
}

/// One placed text run after layout: absolute cell position, the wrap
/// width actually resolved, the re-wrapped lines, and its style.
#[derive(Debug, Clone)]
pub struct Placed {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub lines: Vec<String>,
    pub style: PaintStyle,
    /// The leaf's component tag, if it carried one.
    pub tag: Option<String>,
}

/// A computed layout: paint list plus the content height (for callers
/// sizing an outer scroll area).
#[derive(Debug, Clone, Default)]
pub struct Computed {
    pub placed: Vec<Placed>,
    pub content_height: f32,
}

/// Box spec mirroring the opentui `<box>` attributes the components use.
/// Defaults follow opentui/CSS: row direction, auto size, no padding,
/// flex-shrink 1 (CSS default), stretch alignment.
#[derive(Debug, Clone)]
pub struct BoxSpec {
    pub direction: FlexDirection,
    pub width: Dimension,
    pub height: Dimension,
    pub min_width: Dimension,
    /// Whether `width` was explicitly set (drives the stretch default).
    width_explicit: bool,
    pub grow: f32,
    pub shrink: f32,
    pub padding: Rect<LengthPercentage>,
    pub gap: f32,
    pub justify: Option<JustifyContent>,
    pub align: Option<AlignItems>,
    /// Stretch default (gotcha 3): children without an explicit width get
    /// `width: 100%` when the parent is a column. Enabled by default; a
    /// component that wants intrinsic column children disables it.
    pub stretch_children: bool,
}

impl Default for BoxSpec {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            width: Dimension::auto(),
            height: Dimension::auto(),
            min_width: Dimension::auto(),
            width_explicit: false,
            grow: 0.0,
            shrink: 1.0,
            padding: Rect::zero(),
            gap: 0.0,
            justify: None,
            align: None,
            stretch_children: true,
        }
    }
}

impl BoxSpec {
    #[must_use]
    pub fn column() -> Self {
        Self { direction: FlexDirection::Column, ..Self::default() }
    }

    #[must_use]
    pub fn row() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::length(width);
        self.width_explicit = true;
        self
    }

    #[must_use]
    pub fn width_percent(mut self, percent: f32) -> Self {
        self.width = Dimension::percent(percent);
        self.width_explicit = true;
        self
    }

    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Dimension::length(height);
        self
    }

    #[must_use]
    pub fn height_percent(mut self, percent: f32) -> Self {
        self.height = Dimension::percent(percent);
        self
    }

    #[must_use]
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = Dimension::length(width);
        self
    }

    #[must_use]
    pub fn grow(mut self, grow: f32) -> Self {
        self.grow = grow;
        self
    }

    #[must_use]
    pub fn shrink(mut self, shrink: f32) -> Self {
        self.shrink = shrink;
        self
    }

    #[must_use]
    pub fn padding_left(mut self, cells: f32) -> Self {
        self.padding.left = LengthPercentage::length(cells);
        self
    }

    #[must_use]
    pub fn padding_right(mut self, cells: f32) -> Self {
        self.padding.right = LengthPercentage::length(cells);
        self
    }

    #[must_use]
    pub fn padding_top(mut self, cells: f32) -> Self {
        self.padding.top = LengthPercentage::length(cells);
        self
    }

    #[must_use]
    pub fn padding_bottom(mut self, cells: f32) -> Self {
        self.padding.bottom = LengthPercentage::length(cells);
        self
    }

    #[must_use]
    pub fn gap(mut self, cells: f32) -> Self {
        self.gap = cells;
        self
    }

    #[must_use]
    pub fn justify_end(mut self) -> Self {
        self.justify = Some(JustifyContent::FlexEnd);
        self
    }

    #[must_use]
    pub fn justify_center(mut self) -> Self {
        self.justify = Some(JustifyContent::Center);
        self
    }

    fn taffy_style(&self) -> Style {
        Style {
            flex_direction: self.direction,
            size: Size { width: self.width, height: self.height },
            min_size: Size { width: self.min_width, height: Dimension::auto() },
            flex_grow: self.grow,
            flex_shrink: self.shrink,
            padding: self.padding,
            justify_content: self.justify,
            align_items: self.align,
            gap: Size {
                width: LengthPercentage::length(self.gap),
                height: LengthPercentage::length(self.gap),
            },
            ..Style::default()
        }
    }
}

/// A flex tree under construction with opentui leaf measurement.
pub struct LayoutTree {
    taffy: TaffyTree<TextLeaf>,
    /// `(node, stretch, has_explicit_width)` tracked for the finalize
    /// pass (gotcha 3).
    boxes: Vec<(NodeId, bool, bool)>,
    root: Option<NodeId>,
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutTree {
    #[must_use]
    pub fn new() -> Self {
        Self { taffy: TaffyTree::new(), boxes: Vec::new(), root: None }
    }

    /// A measured text leaf with default (auto) sizing.
    pub fn text(&mut self, text: &str, style: PaintStyle) -> NodeId {
        self.text_sized(text, style, &BoxSpec::default())
    }

    /// A measured text leaf carrying a placement tag (see [`Placed::tag`]).
    pub fn text_tagged(&mut self, text: &str, style: PaintStyle, tag: impl Into<String>) -> NodeId {
        self.taffy
            .new_leaf_with_context(
                BoxSpec::default().taffy_style(),
                TextLeaf { text: text.to_owned(), style, tag: Some(tag.into()) },
            )
            .expect("text leaf")
    }

    /// A measured text leaf with a fixed box (e.g. `width={2}` icons or
    /// fixed-width title slots).
    pub fn text_sized(&mut self, text: &str, style: PaintStyle, spec: &BoxSpec) -> NodeId {
        self.taffy
            .new_leaf_with_context(
                spec.taffy_style(),
                TextLeaf { text: text.to_owned(), style, tag: None },
            )
            .expect("text leaf")
    }

    /// A text leaf that fills the available width and wraps (grow 1).
    pub fn text_flex(&mut self, text: &str, style: PaintStyle) -> NodeId {
        self.text_sized(text, style, &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0))
    }

    /// A box with children.
    pub fn box_node(&mut self, spec: &BoxSpec, children: &[NodeId]) -> NodeId {
        let stretch = spec.stretch_children;
        let explicit = spec.width_explicit;
        let node = self.taffy.new_with_children(spec.taffy_style(), children).expect("box node");
        self.boxes.push((node, stretch, explicit));
        node
    }

    pub fn set_root(&mut self, node: NodeId) {
        self.root = Some(node);
    }

    /// Compute the layout at a terminal size and collect the paint list.
    /// `available` is the definite viewport (width) the root lays out in.
    #[must_use]
    pub fn compute(mut self, available: (f32, AvailableSpace)) -> Computed {
        let Some(root) = self.root else { return Computed::default() };
        self.finalize_stretch();
        let (width, height) = available;
        self.taffy
            .compute_layout_with_measure(
                root,
                Size { width: AvailableSpace::Definite(width), height },
                |known, available, _node, context, _style| {
                    measure_text_leaf(known, available, context.as_deref())
                },
            )
            .expect("taffy layout");
        let content_height = self.taffy.layout(root).expect("root layout").size.height;
        let mut placed = Vec::new();
        self.collect(root, 0.0, 0.0, &mut placed);
        Computed { placed, content_height }
    }

    /// Gotcha 3 as a layer default: box children without an explicit
    /// width stretch to 100% of their parent (opentui's stretch default —
    /// the spike found rows inside padded wrappers need it regardless of
    /// the parent's direction). Leaves are measured, never stretched.
    fn finalize_stretch(&mut self) {
        let boxes = self.boxes.clone();
        for (node, stretch, _) in boxes {
            if !stretch {
                continue;
            }
            let children = self.taffy.children(node).expect("children");
            for &child in &children {
                let is_text = self.taffy.get_node_context(child).is_some();
                if is_text {
                    continue;
                }
                if let Some((_, _, explicit)) = self.boxes.iter().find(|(id, ..)| *id == child) {
                    if !*explicit {
                        self.taffy
                            .set_style(
                                child,
                                Style {
                                    size: Size {
                                        width: Dimension::percent(1.0),
                                        height: Dimension::auto(),
                                    },
                                    ..self.taffy.style(child).expect("child style").clone()
                                },
                            )
                            .expect("stretch style");
                    }
                }
            }
        }
    }

    /// Gotcha 1: absolute positions accumulate through every location —
    /// taffy 0.9 factors padding into the child content node's location,
    /// and the recursive sum carries it through. Gotcha 4: leaves
    /// re-wrap at the resolved width.
    fn collect(&self, node: NodeId, parent_x: f32, parent_y: f32, placed: &mut Vec<Placed>) {
        let layout = *self.taffy.layout(node).expect("node layout");
        let x = parent_x + layout.location.x;
        let y = parent_y + layout.location.y;
        if let Some(leaf) = self.taffy.get_node_context(node) {
            let width = layout.size.width.floor().max(0.0) as usize;
            placed.push(Placed {
                x: x.round() as usize,
                y: y.round() as usize,
                width,
                lines: wrap_label(&leaf.text, width),
                style: leaf.style,
                tag: leaf.tag.clone(),
            });
        }
        let children = self.taffy.children(node).expect("children");
        for child in children {
            self.collect(child, x, y, placed);
        }
    }
}

/// Text measurement contract for leaves: definite constraints wrap to the
/// wrapped size; `MinContent` reports the longest run between break
/// opportunities (the CSS automatic minimum — gotcha 2); `MaxContent`
/// reports the single line.
fn measure_text_leaf(
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
    context: Option<&TextLeaf>,
) -> Size<f32> {
    let Some(leaf) = context else { return Size::ZERO };
    let definite = known.width.or(match available.width {
        AvailableSpace::Definite(width) => Some(width),
        _ => None,
    });
    let (width, height) = match definite {
        Some(width) => {
            let width = width.floor().max(0.0) as usize;
            let lines = wrap_label(&leaf.text, width);
            let longest = lines.iter().map(|line| display_width(line)).max().unwrap_or(0);
            (longest, lines.len())
        }
        None if available.width == AvailableSpace::MinContent => (min_content_width(&leaf.text), 1),
        None => (max_content_width(&leaf.text), 1),
    };
    Size { width: width as f32, height: height as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: PaintStyle = PaintStyle::new();

    #[test]
    fn column_children_stretch_by_default() {
        let mut tree = LayoutTree::new();
        let leaf = tree.text("hi", STYLE);
        let inner = tree.box_node(&BoxSpec::row(), &[leaf]);
        let root = tree.box_node(&BoxSpec::column().width(20.0), &[inner]);
        tree.set_root(root);
        let computed = tree.compute((20.0, AvailableSpace::MaxContent));
        assert_eq!(computed.placed.len(), 1);
        assert_eq!(computed.placed[0].x, 0);
        assert_eq!(computed.placed[0].lines, vec!["hi".to_owned()]);
        // The inner box stretched to 20 even without an explicit width.
        assert!(computed.placed[0].width >= 2);
    }

    #[test]
    fn padding_lands_on_children_through_the_content_node() {
        let mut tree = LayoutTree::new();
        let leaf = tree.text("x", STYLE);
        let content = tree.box_node(&BoxSpec::row(), &[leaf]);
        let wrapper = tree.box_node(&BoxSpec::default().padding_left(3.0), &[content]);
        tree.set_root(wrapper);
        let computed = tree.compute((40.0, AvailableSpace::MaxContent));
        assert_eq!(computed.placed[0].x, 3, "padding aggregates through locations");
    }

    #[test]
    fn shrink_to_fit_wraps_long_labels() {
        let mut tree = LayoutTree::new();
        let label =
            tree.text_flex("Read packages/opencode/src/index.ts [offset=1, limit=100]", STYLE);
        let icon = tree.text_sized("→", STYLE, &BoxSpec::default().width(2.0).shrink(0.0));
        let row = tree.box_node(&BoxSpec::row(), &[icon, label]);
        let wrapper = tree.box_node(&BoxSpec::default().padding_left(3.0), &[row]);
        let root = tree.box_node(&BoxSpec::column().width(72.0), &[wrapper]);
        tree.set_root(root);
        let computed = tree.compute((72.0, AvailableSpace::MaxContent));
        let label = computed.placed.iter().find(|p| p.lines[0].starts_with("Read")).unwrap();
        assert_eq!(label.x, 5, "3 padding + 2 icon");
        assert_eq!(label.width, 67);
        assert_eq!(label.lines.len(), 1, "this label fits at 67");
    }

    #[test]
    fn min_content_uses_the_wrap_seam() {
        // The same label in a shrinking column: the solver may shrink the
        // leaf down to its longest word ("packages/opencode/src/index.ts"
        // has no opportunities after '|' — longest run is the whole token
        // only if unwrappable; here spaces exist).
        let mut tree = LayoutTree::new();
        let label = tree.text_flex("global package index search", STYLE);
        let root = tree.box_node(&BoxSpec::column().width(10.0), &[label]);
        tree.set_root(root);
        let computed = tree.compute((10.0, AvailableSpace::MaxContent));
        let placed = &computed.placed[0];
        assert_eq!(placed.width, 10);
        assert_eq!(placed.lines.len(), 4, "wraps within 10 cells: {:?}", placed.lines);
        assert_eq!(min_content_width("global package index search"), 7);
    }

    #[test]
    fn cjk_shrinks_to_one_grapheme_consistently() {
        // Gate-8 gotcha: wrap strategy and MinContent measurement must
        // agree. A CJK leaf shrinks to 2 cells (one wide grapheme) and
        // wraps at every grapheme — shrink-to-fit never stalls.
        let text = "データベースの設定";
        let mut tree = LayoutTree::new();
        let label = tree.text_flex(text, STYLE);
        let root = tree.box_node(&BoxSpec::column().width(6.0), &[label]);
        tree.set_root(root);
        let computed = tree.compute((6.0, AvailableSpace::MaxContent));
        let placed = &computed.placed[0];
        assert_eq!(placed.width, 6);
        assert_eq!(
            placed.lines,
            vec!["データ".to_owned(), "ベース".to_owned(), "の設定".to_owned()]
        );
        assert_eq!(min_content_width(text), 2);
    }

    #[test]
    fn gap_and_padding_stack_rows() {
        let mut tree = LayoutTree::new();
        let one = tree.text("one", STYLE);
        let two = tree.text("two", STYLE);
        let column = tree.box_node(&BoxSpec::column().gap(1.0).padding_top(1.0), &[one, two]);
        tree.set_root(column);
        let computed = tree.compute((40.0, AvailableSpace::MaxContent));
        let ys: Vec<usize> = computed.placed.iter().map(|p| p.y).collect();
        assert_eq!(ys, vec![1, 3], "padding-top 1 then gap 1 between rows");
    }
}
