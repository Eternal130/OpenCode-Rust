//! Lane D4.1c — the mini footer's root-tree mount
//! (`mini/footer.ts:326`'s `render(() => createComponent(RunFooterView,
//! …), renderer)` contract port): the pane's composed footer frame —
//! the main footer row, the RD-19 launch row, the busy spinner cell,
//! and the TUI-17 subagent inspector — rides the split-footer
//! renderer's ROOT tree as one signal-driven row list; the engine's
//! band layout + `renderOffset` emission renders and addresses the
//! dock's rows, and the frame bytes leave through the renderer pump
//! exit (the retired `GridEmitter` dock channel is gone).
//!
//! The bridge's currency is the pane's composed
//! [`crate::render::Frame`] (the footer family's cell-level currency —
//! the draft/busy-cell/statusline post-processing is cell-shaped), so
//! the mount shapes each row into ONE absolute `wrapMode none` text
//! element (chunk-merged styled runs, the same shape the footer
//! family's own solid bridge mounts) and every content slot is a dyn
//! prop over the rows signal — a draw patches content in place, never
//! a remount (the M2 rule).

// Terminal dimensions are bounded usize→f32 casts (taffy points),
// the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::needless_pass_by_value)] // Signal clones are the builder currency

use std::rc::Rc;

use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::ForItem;
use opentui_solid::element::PropValue;
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal};

use crate::render::{Cell, CellColor, Frame};

// ---------------------------------------------------------------------------
// the row model (the frame → tree shaping)
// ---------------------------------------------------------------------------

/// One footer row's mount unit: `row{y}` keyed, absolutely placed at
/// `top = y`, content = the row's chunk-merged styled runs.
#[derive(Debug, Clone)]
pub(crate) struct FooterRowUnit {
    pub key: String,
    pub top: usize,
    pub content: StyledText,
}

/// A `CellColor` as an opentui color — `Rgb` carries, indexed/default
/// stay default (the footer palette is all-RGB; default is the
/// transparent composer region's canonical `49`).
fn rgba(color: CellColor) -> Option<Rgba> {
    match color {
        CellColor::Rgb(r, g, b) => Some(Rgba::rgb(r, g, b)),
        CellColor::Indexed(_) | CellColor::Default => None,
    }
}

/// A cell that carries nothing (a bare space over the default
/// background) — the leading/trailing padding that never mounts.
fn is_bare(cell: &Cell) -> bool {
    cell.ch == ' '
        && cell.attrs == 0
        && matches!(cell.fg, CellColor::Default)
        && matches!(cell.bg, CellColor::Default)
}

/// Shape the pane's composed frame into the row units (one text
/// element per row): consecutive same-style cells merge into one
/// chunk, bare leading/trailing padding trims, and a bare gap BETWEEN
/// ink runs stays a default-styled space chunk (the run continuity
/// the old cell paint had).
#[must_use]
pub(crate) fn frame_rows(frame: &Frame) -> Vec<FooterRowUnit> {
    let mut units = Vec::with_capacity(frame.cells.len());
    for (y, row) in frame.cells.iter().enumerate() {
        let mut chunks: Vec<TextChunk> = Vec::new();
        let mut run: Option<(String, Option<Rgba>, Option<Rgba>, u32)> = None;
        let flush =
            |chunks: &mut Vec<TextChunk>,
             run: &mut Option<(String, Option<Rgba>, Option<Rgba>, u32)>| {
                if let Some((text, fg, bg, attributes)) = run.take() {
                    chunks.push(TextChunk { text, fg, bg, attributes, link: None });
                }
            };
        let mut seen_ink = false;
        for cell in row {
            if is_bare(cell) && !seen_ink {
                continue; // leading padding
            }
            if is_bare(cell) {
                // A gap between ink runs keeps the run going as a
                // default-styled space (same-style merge).
                match &mut run {
                    Some((text, None, None, attrs)) if *attrs == 0 => text.push(' '),
                    Some(_) | None => {
                        flush(&mut chunks, &mut run);
                        run = Some((" ".to_owned(), None, None, 0));
                    }
                }
                continue;
            }
            seen_ink = true;
            let (fg, bg) = (rgba(cell.fg), rgba(cell.bg));
            let attributes = u32::from(cell.attrs); // bit 0 = bold (the family's only attr)
            match &mut run {
                Some((text, fg0, bg0, attrs0))
                    if *fg0 == fg && *bg0 == bg && *attrs0 == attributes =>
                {
                    text.push(cell.ch);
                }
                Some(_) | None => {
                    flush(&mut chunks, &mut run);
                    run = Some((cell.ch.to_string(), fg, bg, attributes));
                }
            }
        }
        // Trailing bare padding already merged as default-style spaces
        // trims off the tail.
        if let Some((mut text, fg, bg, attributes)) = run.take() {
            if fg.is_none() && bg.is_none() && attributes == 0 {
                while text.ends_with(' ') {
                    text.pop();
                }
            }
            if !text.is_empty() {
                chunks.push(TextChunk { text, fg, bg, attributes, link: None });
            }
        }
        units.push(FooterRowUnit {
            key: format!("row{y}"),
            top: y,
            content: StyledText::new(chunks),
        });
    }
    units
}

// ---------------------------------------------------------------------------
// the element tree
// ---------------------------------------------------------------------------

/// `dyn_(src, to PropValue::Num)` — a usize-valued dynamic prop.
fn dyn_num<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<usize> + 'static,
{
    opentui_solid::dyn_(source, |value: &usize| PropValue::Num(*value as f32))
}

/// `dyn_(src, to PropValue::Styled)` — the row content slot.
fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

/// One row's text element: absolute at `(0, top)`, `wrapMode none` +
/// `flexShrink 0` so nothing re-wraps or shrinks, the content a dyn
/// prop over the rows signal (patched in place, never remounted).
fn row_element(rows: &Signal<Vec<FooterRowUnit>>, key: &str) -> Element {
    let key = key.to_owned();
    let top_of = {
        let key = key.clone();
        move |units: &Vec<FooterRowUnit>| {
            units.iter().find(|unit| unit.key == key).map_or(0, |unit| unit.top)
        }
    };
    let content_of = {
        let key = key.clone();
        move |units: &Vec<FooterRowUnit>| {
            units
                .iter()
                .find(|unit| unit.key == key)
                .map_or_else(|| StyledText::new(Vec::new()), |unit| unit.content.clone())
        }
    };
    text()
        .position("absolute")
        .wrap_mode("none")
        .flex_shrink(0.0)
        .left(0.0)
        .dyn_prop("top", dyn_num(map(rows.clone(), top_of)))
        .dyn_prop("content", dyn_styled(map(rows.clone(), content_of)))
        .into_element()
}

/// The mini footer's root element: a relative box whose extents track
/// the band geometry signal (`resize/set_footer_height` patch in place)
/// carrying ONE keyed `For` of the row units.
fn footer_root(rows: &Signal<Vec<FooterRowUnit>>, geometry: &Signal<(usize, usize)>) -> Element {
    let source = ForItemSource {
        source: map(rows.clone(), |units: &Vec<FooterRowUnit>| {
            units
                .iter()
                .map(|unit| ForItem {
                    key: unit.key.clone(),
                    value: PropValue::Str(unit.key.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let item_rows = rows.clone();
    let units = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        row_element(&item_rows, &item.key)
    });
    box_()
        .position("relative")
        .dyn_prop("width", dyn_num(map(geometry.clone(), |g: &(usize, usize)| g.0)))
        .dyn_prop("height", dyn_num(map(geometry.clone(), |g: &(usize, usize)| g.1)))
        .children_dyn(Rc::new(units))
        .into_element()
}

/// The mounted footer tree's reactive state (the lifecycle's handle):
/// the rows signal every draw sets, and the band geometry signal the
/// draw keeps in step with the renderer's render extents.
pub(crate) struct FooterTree {
    rows: Signal<Vec<FooterRowUnit>>,
    geometry: Signal<(usize, usize)>,
}

impl FooterTree {
    /// Boot the mount's state + its root element (the caller mounts
    /// the element onto the mini renderer's root via
    /// `opentui_solid::render_into`).
    pub(crate) fn new(width: usize, height: usize) -> (Self, Element) {
        let rows = Signal::new(Vec::new());
        let geometry = Signal::new((width, height));
        let element = footer_root(&rows, &geometry);
        (Self { rows, geometry }, element)
    }

    /// One draw's update: the band geometry + the composed frame's
    /// row units (content patches in place through the signals).
    pub(crate) fn update(&self, width: usize, height: usize, frame: &Frame) {
        self.geometry.set((width, height));
        self.rows.set(frame_rows(frame));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Cell, CellColor};

    fn cell(ch: char, fg: CellColor, bg: CellColor) -> Cell {
        Cell { ch, attrs: 0, fg, bg }
    }

    fn frame(rows: Vec<Vec<Cell>>) -> Frame {
        let width = rows.first().map_or(0, Vec::len);
        let height = rows.len();
        Frame { cells: rows, width, height, clusters: None }
    }

    /// The shaping: same-style runs merge into ONE chunk, leading and
    /// trailing bare padding trims, a bare gap between ink runs stays
    /// a default-styled space chunk, and indexed/default colors keep
    /// their default channel (`None`).
    #[test]
    fn rows_merge_runs_trim_padding_and_keep_gaps() {
        let red = CellColor::Rgb(255, 0, 0);
        let frame = frame(vec![
            vec![
                cell(' ', CellColor::Default, CellColor::Default),
                cell(' ', CellColor::Default, CellColor::Default),
                cell('a', red, CellColor::Default),
                cell('b', red, CellColor::Default),
                cell(' ', CellColor::Default, CellColor::Default),
                cell('c', red, CellColor::Default),
                cell(' ', CellColor::Default, CellColor::Default),
                cell(' ', CellColor::Default, CellColor::Default),
            ],
            vec![cell(' ', CellColor::Default, CellColor::Default); 4],
        ]);
        let units = frame_rows(&frame);
        assert_eq!(units.len(), 2, "one unit per row");
        assert_eq!(units[0].key, "row0");
        assert_eq!(units[0].top, 0);
        let chunks = &units[0].content.chunks;
        // "ab" + the gap's default space + "c" — the trailing padding
        // trimmed; the gap keeps the run going as a default chunk.
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert_eq!(chunks[0].text, "ab");
        assert_eq!(chunks[0].fg, Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(chunks[1].text, " ");
        assert_eq!(chunks[1].fg, None);
        assert_eq!(chunks[2].text, "c");
        // An all-bare row mounts an empty content row.
        assert!(units[1].content.chunks.is_empty());
    }

    /// The root-tree round trip: the pane frame's ink lands in the
    /// SPLIT-FOOTER renderer's raster at the band rows (the engine's
    /// band layout), cell for cell — the D4.1c mount contract.
    #[test]
    fn root_tree_raster_matches_the_frame_in_the_band() {
        use opentui_core::renderer::{CliRendererConfig, ExternalOutputMode, ScreenMode};
        let red = CellColor::Rgb(255, 80, 80);
        let surface = CellColor::Rgb(20, 20, 30);
        let row0 = {
            let mut row = vec![cell(' ', CellColor::Default, surface); 10];
            row[0] = cell('┃', red, surface);
            row[2] = cell('h', CellColor::Default, CellColor::Default);
            row[3] = cell('i', CellColor::Default, CellColor::Default);
            row
        };
        let row1 = vec![cell(' ', CellColor::Default, surface); 10];
        let source = frame(vec![row0, row1]);
        let (footer, element) = FooterTree::new(10, 2);
        let renderer = opentui_core::renderer::create_cli_renderer(CliRendererConfig {
            width: 10,
            height: 12,
            screen_mode: ScreenMode::SplitFooter,
            footer_height: Some(2),
            external_output_mode: Some(ExternalOutputMode::Passthrough),
            ..CliRendererConfig::default()
        });
        assert_eq!((renderer.width(), renderer.height()), (10, 2), "the band extents");
        assert_eq!(renderer.render_offset(), 10);
        let mut solid = opentui_solid::render_into(renderer, element);
        footer.update(10, 2, &source);
        solid.flush();
        let buffer = &solid.renderer.current_render_buffer;
        // A grapheme-id cell decodes back through the buffer's pool
        // (multi-byte glyphs raster as graphemes).
        let glyph = |cell: opentui_core::buffer::Cell| -> char {
            if opentui_core::buffer::is_grapheme_char(cell.char) {
                buffer
                    .grapheme_bytes(opentui_core::buffer::grapheme_id_from_char(cell.char))
                    .chars()
                    .next()
                    .unwrap_or(' ')
            } else {
                char::from_u32(cell.char).unwrap_or(' ')
            }
        };
        for y in 0..2u32 {
            for x in 0..10u32 {
                let want = &source.cells[y as usize][x as usize];
                let got = buffer.get(x, y).expect("band cell");
                if is_bare(want) {
                    continue; // the transparent default — nothing to paint
                }
                assert_eq!(glyph(got), want.ch, "glyph at ({x},{y})");
                if let Some(fg) = rgba(want.fg) {
                    assert_eq!(fg, got.fg, "fg at ({x},{y})");
                }
                if let Some(bg) = rgba(want.bg) {
                    assert_eq!(bg, got.bg, "bg at ({x},{y})");
                }
            }
        }
    }
}
