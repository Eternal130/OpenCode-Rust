//! Taffy-backed flex tree builder mirroring opentui `<box>` semantics,
//! with the wave-7.0 substrate gotchas encoded as **layer defaults**
//! (never per-node patches — gate-8 layout instruction):
//!
//! 1. **Padding on the content node**: `taffy` 0.9 applies padding to
//!    the child *content* node's location; the collector aggregates through
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
//! Wave 0 (gate-9): leaves are either **plain** (`(text, style)`) or
//! **styled-segment** (`Vec<(text, style)>` spans, [`TextLeaf::Styled`])
//! — both flatten through [`crate::wrap::flatten_spans`] into ONE
//! (cluster, style) stream, so measurement, wrapping and paint treat a
//! styled leaf exactly like a plain one: span boundaries are not break
//! opportunities, wrap boundaries may land mid-span, and placed lines
//! carry per-run styles re-assembled at paint time.
//!
//! Layout coordinates are bounded by terminal dimensions (far below
//! 2^23), so the pedantic float↔int cast lints do not carry real risk.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

use ratatui::style::Style as PaintStyle;
use taffy::prelude::*;
use taffy::{AvailableSpace, Dimension, LengthPercentage, NodeId, Rect, Size, Style, TaffyTree};

use crate::wrap::{
    atoms_max_content_width, atoms_min_content_width, flatten, flatten_spans, styled_runs,
    wrap_atoms, Atom,
};

/// The styled-span type text leaves are built from (defined in
/// [`crate::wrap`], re-exported as the layout-facing name).
pub use crate::wrap::StyledSpan;
pub use crate::wrap::StyledSpan as TextSpan;

/// Node context for measured text leaves: a plain run, or a
/// styled-segment leaf whose spans flow through layout/wrap/paint as
/// ONE text stream.
#[derive(Debug, Clone)]
pub enum TextLeaf {
    /// One text run under one style.
    Plain { text: String, style: PaintStyle, tag: Option<String> },
    /// `Vec<(text, style)>` spans — the critical-path leaf type
    /// (markdown bold/inline-code, per-cell tab colors, diff colors).
    Styled { spans: Vec<StyledSpan>, tag: Option<String> },
}

impl TextLeaf {
    /// The leaf's flattened (cluster, style) stream — the ONE seam
    /// shared by measurement, wrapping and paint.
    fn atoms(&self) -> Vec<Atom> {
        match self {
            Self::Plain { text, style, .. } => flatten(text, *style),
            Self::Styled { spans, .. } => flatten_spans(spans),
        }
    }

    fn tag(&self) -> Option<&str> {
        match self {
            Self::Plain { tag, .. } | Self::Styled { tag, .. } => tag.as_deref(),
        }
    }
}

/// One placed text line after layout: runs of contiguous style,
/// re-assembled from the flattened stream ([`crate::wrap::styled_runs`]).
/// Plain leaves yield a single run per line.
#[derive(Debug, Clone)]
pub struct PlacedLine {
    pub runs: Vec<StyledSpan>,
}

impl PlacedLine {
    #[must_use]
    pub fn plain(text: impl Into<String>, style: PaintStyle) -> Self {
        Self { runs: vec![StyledSpan { text: text.into(), style, link: None }] }
    }

    /// The line's concatenated text (glyph content, styles aside).
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    /// The line's display width.
    #[must_use]
    pub fn display_width(&self) -> usize {
        self.runs.iter().map(|run| crate::wrap::display_width(&run.text)).sum()
    }
}

/// One placed text run after layout: absolute cell position, the wrap
/// width actually resolved, the re-wrapped lines (per-run styles), and
/// the leaf's component tag.
#[derive(Debug, Clone)]
pub struct Placed {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub lines: Vec<PlacedLine>,
    /// The leaf's component tag, if it carried one.
    pub tag: Option<String>,
}

impl Placed {
    /// The placed lines as plain strings (glyph content).
    #[must_use]
    pub fn line_texts(&self) -> Vec<String> {
        self.lines.iter().map(PlacedLine::text).collect()
    }
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
                TextLeaf::Plain { text: text.to_owned(), style, tag: Some(tag.into()) },
            )
            .expect("text leaf")
    }

    /// A measured text leaf with a fixed box (e.g. `width={2}` icons or
    /// fixed-width title slots).
    pub fn text_sized(&mut self, text: &str, style: PaintStyle, spec: &BoxSpec) -> NodeId {
        self.taffy
            .new_leaf_with_context(
                spec.taffy_style(),
                TextLeaf::Plain { text: text.to_owned(), style, tag: None },
            )
            .expect("text leaf")
    }

    /// A styled-segment text leaf (`Vec<(text, style)>` spans flowing as
    /// ONE text stream — see [`TextLeaf::Styled`]) under a box spec.
    pub fn text_styled(&mut self, spans: Vec<StyledSpan>, spec: &BoxSpec) -> NodeId {
        self.taffy
            .new_leaf_with_context(spec.taffy_style(), TextLeaf::Styled { spans, tag: None })
            .expect("text leaf")
    }

    /// A styled-segment text leaf carrying a placement tag.
    pub fn text_styled_tagged(&mut self, spans: Vec<StyledSpan>, tag: impl Into<String>) -> NodeId {
        self.taffy
            .new_leaf_with_context(
                BoxSpec::default().taffy_style(),
                TextLeaf::Styled { spans, tag: Some(tag.into()) },
            )
            .expect("text leaf")
    }

    /// A text leaf that fills the available width and wraps (grow 1).
    pub fn text_flex(&mut self, text: &str, style: PaintStyle) -> NodeId {
        self.text_sized(text, style, &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0))
    }

    /// A styled-segment leaf that fills the available width and wraps.
    pub fn text_styled_flex(&mut self, spans: Vec<StyledSpan>) -> NodeId {
        self.text_styled(spans, &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0))
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
    /// re-wrap at the resolved width; styled lines re-assemble per
    /// contiguous style ([`styled_runs`]).
    fn collect(&self, node: NodeId, parent_x: f32, parent_y: f32, placed: &mut Vec<Placed>) {
        let layout = *self.taffy.layout(node).expect("node layout");
        let x = parent_x + layout.location.x;
        let y = parent_y + layout.location.y;
        if let Some(leaf) = self.taffy.get_node_context(node) {
            let width = layout.size.width.floor().max(0.0) as usize;
            let lines = wrap_atoms(&leaf.atoms(), width)
                .into_iter()
                .map(|line| PlacedLine { runs: styled_runs(&line) })
                .collect();
            placed.push(Placed {
                x: x.round() as usize,
                y: y.round() as usize,
                width,
                lines,
                tag: leaf.tag().map(str::to_owned),
            });
        }
        let children = self.taffy.children(node).expect("children");
        for child in children {
            self.collect(child, x, y, placed);
        }
    }
}

/// Text measurement contract for leaves (plain and styled alike — the
/// flattened stream is style-agnostic): definite constraints wrap to the
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
    let atoms = leaf.atoms();
    let (width, height) = match definite {
        Some(width) => {
            let width = width.floor().max(0.0) as usize;
            let lines = wrap_atoms(&atoms, width);
            let longest = lines
                .iter()
                .map(|line| line.iter().map(|atom| atom.width).sum())
                .max()
                .unwrap_or(0);
            (longest, lines.len())
        }
        None if available.width == AvailableSpace::MinContent => {
            (atoms_min_content_width(&atoms), 1)
        }
        None => (atoms_max_content_width(&atoms), 1),
    };
    Size { width: width as f32, height: height as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap::min_content_width;

    const STYLE: PaintStyle = PaintStyle::new();
    const BOLD: PaintStyle = PaintStyle::new().add_modifier(ratatui::style::Modifier::BOLD);

    fn span(text: &str, style: PaintStyle) -> StyledSpan {
        StyledSpan { text: text.to_owned(), style, link: None }
    }

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
        assert_eq!(computed.placed[0].line_texts(), vec!["hi".to_owned()]);
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
        let label = computed.placed.iter().find(|p| p.line_texts()[0].starts_with("Read")).unwrap();
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
        assert_eq!(placed.lines.len(), 4, "wraps within 10 cells: {:?}", placed.line_texts());
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
            placed.line_texts(),
            vec!["データ".to_owned(), "ベース".to_owned(), "の設定".to_owned()]
        );
        assert_eq!(min_content_width(text), 2);
    }

    /// Gate-9 finding 5: the layout-level **`MinContent` measurement
    /// arm** — a flex item WITHOUT the `min_width(0.0)` bypass gets
    /// taffy's automatic minimum, which the leaf measures intrinsically
    /// (the seam's longest run: one wide CJK grapheme = 2 cells). With
    /// only 1 column left after the fixed icon, the solver floors the
    /// label at its intrinsic minimum (2), proving the arm fired — a
    /// dead arm would resolve 1.
    #[test]
    fn auto_min_width_measures_intrinsic_min_content_for_cjk() {
        let text = "データベースの設定";
        let mut tree = LayoutTree::new();
        let icon = tree.text_sized("→", STYLE, &BoxSpec::default().width(2.0).shrink(0.0));
        // No min_width(0.0) bypass: the automatic minimum applies.
        let label = tree.text_sized(text, STYLE, &BoxSpec::default().grow(1.0).shrink(1.0));
        let row = tree.box_node(&BoxSpec::row().width(3.0), &[icon, label]);
        tree.set_root(row);
        let computed = tree.compute((3.0, AvailableSpace::MaxContent));
        let label = &computed.placed[1];
        assert_eq!(
            label.width, 2,
            "automatic minimum = intrinsic MinContent (one wide grapheme), not the 1 column left"
        );
        assert_eq!(
            label.line_texts(),
            vec!["デ", "ー", "タ", "ベ", "ー", "ス", "の", "設", "定"],
            "wraps at grapheme granularity at the intrinsic minimum"
        );
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

    // -- styled-segment leaves ------------------------------------------------

    /// A styled leaf lays out, measures and wraps identically to the
    /// plain leaf of the same concatenated text — the styled path
    /// subsumes the plain one.
    #[test]
    fn styled_leaf_layouts_like_the_equivalent_plain_leaf() {
        let text = "設定 configの値 wraps through the seam";
        let styled = {
            let mut tree = LayoutTree::new();
            let leaf = tree.text_styled_flex(vec![span(text, BOLD)]);
            let root = tree.box_node(&BoxSpec::column().width(10.0), &[leaf]);
            tree.set_root(root);
            tree.compute((10.0, AvailableSpace::MaxContent)).placed.remove(0)
        };
        let plain = {
            let mut tree = LayoutTree::new();
            let leaf = tree.text_flex(text, STYLE);
            let root = tree.box_node(&BoxSpec::column().width(10.0), &[leaf]);
            tree.set_root(root);
            tree.compute((10.0, AvailableSpace::MaxContent)).placed.remove(0)
        };
        assert_eq!(styled.width, plain.width);
        assert_eq!(styled.line_texts(), plain.line_texts());
        assert_eq!(styled.lines.len(), plain.lines.len());
    }

    /// A wrap boundary landing mid-span keeps the span's style on BOTH
    /// fragments, and runs re-assemble per contiguous style at collect
    /// time (see also the frame-level tests in `golden_tests`).
    #[test]
    fn styled_leaf_wraps_mid_span_with_runs_per_contiguous_style() {
        let mut tree = LayoutTree::new();
        let leaf = tree.text_styled_flex(vec![
            span("plain ", STYLE),
            span("**bold run**", BOLD),
            span(" tail", STYLE),
        ]);
        let root = tree.box_node(&BoxSpec::column().width(12.0), &[leaf]);
        tree.set_root(root);
        let computed = tree.compute((12.0, AvailableSpace::MaxContent));
        let placed = &computed.placed[0];
        assert_eq!(placed.line_texts(), vec!["plain **bold", "run** tail"]);
        assert_eq!(placed.lines[0].runs, vec![span("plain ", STYLE), span("**bold", BOLD)]);
        assert_eq!(placed.lines[1].runs, vec![span("run**", BOLD), span(" tail", STYLE)]);
        assert_eq!(placed.lines[0].display_width(), 12);
    }

    /// `MinContent` / `MaxContent` measurement on styled leaves through the
    /// solver: the styled leaf floors at the same intrinsic minimum as
    /// the plain leaf of the same text (finding 5's arm, styled side).
    #[test]
    fn styled_leaf_measures_min_content_through_the_solver() {
        let spans = vec![span("設定 ", STYLE), span("config", BOLD), span("の値", STYLE)];
        let atoms = flatten_spans(&spans);
        assert_eq!(atoms_min_content_width(&atoms), 8, "the `configの` run");
        assert_eq!(atoms_max_content_width(&atoms), crate::wrap::display_width("設定 configの値"));
        // Through the solver: an auto-width box around the styled leaf
        // resolves the box to the single-line (max-content) width.
        let mut tree = LayoutTree::new();
        let leaf = tree.text_styled(spans.clone(), &BoxSpec::default());
        let box_node = tree.box_node(&BoxSpec::column(), &[leaf]);
        tree.set_root(box_node);
        let computed = tree.compute((200.0, AvailableSpace::MaxContent));
        assert_eq!(computed.placed[0].width, atoms_max_content_width(&atoms));
    }
}
