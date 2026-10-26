//! Lane R1 (the overlay-chrome tail): ONE shared opentui-solid bridge
//! for the seven overlay-chrome surfaces — toast, the console
//! overlay, the devtools bar, the plugin page (+ its missing-route
//! fallback), the location-missing question, the migration overlay,
//! the app-state overlays (reconnecting scrim box + startup pill)
//! and the vertical session-tab rail (+ its compact hover tooltip).
//!
//! The M4/M5/M6 flat-units shape, generalized: every surface's OLD
//! painter walk is re-expressed as a list of absolute paint UNITS —
//! a fill rect (opaque `backgroundColor`, the opentui fill's
//! fg=white signature matches the old painters' white-fg fills) or a
//! row of styled text chunks (explicit fg/bg, ECMA attribute word) —
//! and this module mounts them as ONE keyed `For` of
//! absolutely-positioned elements under a positioned root, flushes a
//! real [`SolidRenderer`] and copies the rasterized cells into the
//! in-house [`Frame`] (the compositor's currency — cell-for-cell the
//! old painters' bytes; each surface's unit-parity gate pins it).
//!
//! Per-surface rollback is the point of the tail: a surface whose
//! bytes diverge keeps its hand-rolled painter and records why (the
//! kill criterion in docs/FRAMEWORK-CONVERGENCE.md §R1).

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use opentui_core::buffer::{
    grapheme_id_from_char, is_continuation_char, is_grapheme_char, OptimizedBuffer,
};
use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal, SolidRenderer};

use crate::render::{Cell, CellColor, Frame, CONTINUATION};

// ---------------------------------------------------------------------------
// the unit model (the old painters' vocabulary, one shape)
// ---------------------------------------------------------------------------

/// One styled run of a text unit — the in-house cell vocabulary
/// (fg/bg as [`CellColor`], attrs as the oc-tui ECMA-ordered word),
/// converted to an opentui chunk at shape time. Non-RGB colors map
/// to transparent chunk slots (the raster leaves the cell's own
/// channel — `Default` round-trips through the copy as `Default`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChromeChunk {
    pub text: String,
    pub fg: CellColor,
    pub bg: CellColor,
    /// The oc-tui attribute word (ECMA-ordered low 8 bits — the
    /// same bits `render::ansi` serializes); packed 1:1 into the
    /// opentui chunk and restored 1:1 by the cell copy.
    pub attrs: u16,
}

impl ChromeChunk {
    pub(crate) fn new(text: impl Into<String>, fg: CellColor, bg: CellColor, attrs: u16) -> Self {
        Self { text: text.into(), fg, bg, attrs }
    }
}

/// One absolute paint unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChromeBody {
    /// An opaque fill rect — the opentui fill writes fg=white on its
    /// space cells, exactly the old painters' `Fill`/`fill_bg`
    /// signature (`DEFAULT_FOREGROUND`/`FILL_FG` are white).
    Fill { width: usize, height: usize, color: CellColor },
    /// A row of styled chunks at (`left`, `top`) — `wrapMode none` +
    /// `flexShrink 0`, natural width, clipped by the buffer exactly
    /// like the old painters' row-edge walks.
    Text(Vec<ChromeChunk>),
    /// A run of forced spaces — the char-naive painters' `put` loops
    /// write `' '` + the run's fg OVER an existing glyph, where the
    /// raster's transparent-space fast path is a no-op
    /// (`try_set_transparent_text_cell`). These stamp the copied
    /// frame directly (the bridge recolor class — M3's caret, M6's
    /// per-glyph serializer): ch `' '`, the fg/attrs, the destination
    /// bg kept.
    Spaces { count: usize, fg: CellColor, attrs: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChromeUnit {
    /// Position-stable identity (the M5 rule: content tag + row, an
    /// index only as a collision suffix) — the `For` key.
    pub key: String,
    pub left: usize,
    pub top: usize,
    pub body: ChromeBody,
}

/// One frame's surface snapshot — the ONLY input the tree reads.
#[derive(Clone)]
struct ChromeInputs {
    items: Vec<ChromeUnit>,
}

// ---------------------------------------------------------------------------
// colors (CellColor ⇄ opentui)
// ---------------------------------------------------------------------------

fn chunk_rgba(color: CellColor) -> Option<Rgba> {
    match color {
        CellColor::Rgb(r, g, b) => Some(Rgba::rgb(r, g, b)),
        // Default/Indexed have no faithful opentui channel: leave
        // the slot transparent — the cell copy maps an untouched
        // channel back to `Default`.
        CellColor::Default | CellColor::Indexed(_) => None,
    }
}

fn background_rgba(color: CellColor) -> Rgba {
    match color {
        CellColor::Rgb(r, g, b) => Rgba::rgb(r, g, b),
        // Transparent — `frame_color` maps alpha 0 back to Default.
        CellColor::Default | CellColor::Indexed(_) => Rgba::from_ints(0, 0, 0, 0),
    }
}

fn hex_of(color: Option<Rgba>) -> String {
    let rgba = color.unwrap_or(Rgba::from_ints(0, 0, 0, 0));
    format!("#{:02x}{:02x}{:02x}", rgba.red(), rgba.green(), rgba.blue())
}

// ---------------------------------------------------------------------------
// the element tree (one flat keyed For of absolute units)
// ---------------------------------------------------------------------------

fn dyn_num<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<usize> + 'static,
{
    opentui_solid::dyn_(source, |value: &usize| PropValue::Num(*value as f32))
}

fn dyn_color<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<Option<Rgba>> + 'static,
{
    opentui_solid::dyn_(source, |value: &Option<Rgba>| PropValue::Color(hex_of(*value)))
}

fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

fn chunk_text(chunks: &[ChromeChunk]) -> StyledText {
    StyledText::new(
        chunks
            .iter()
            .map(|chunk| TextChunk {
                text: chunk.text.clone(),
                fg: chunk_rgba(chunk.fg),
                bg: chunk_rgba(chunk.bg),
                attributes: u32::from(chunk.attrs & 0xFF),
                link: None,
            })
            .collect::<Vec<TextChunk>>(),
    )
}

fn find_unit<'a>(inputs: &'a ChromeInputs, key: &str) -> Option<&'a ChromeUnit> {
    inputs.items.iter().find(|unit| unit.key == key)
}

fn fill_body(unit: &ChromeUnit) -> Option<(usize, usize, CellColor)> {
    match &unit.body {
        ChromeBody::Fill { width, height, color } => Some((*width, *height, *color)),
        _ => None,
    }
}

/// One `For` item element — the unit's KIND (fill vs text) is baked
/// into its mount-time probe, so the element type is chosen once.
/// A closing item's mapped views fire once more after it left the
/// inputs (M1) — every accessor degrades to empty, never panics.
fn unit_element(inputs: &Signal<ChromeInputs>, key: &str) -> Element {
    let key = key.to_owned();
    let is_fill = inputs
        .get()
        .items
        .iter()
        .any(|unit| unit.key == key && matches!(unit.body, ChromeBody::Fill { .. }));
    let is_spaces = inputs
        .get()
        .items
        .iter()
        .any(|unit| unit.key == key && matches!(unit.body, ChromeBody::Spaces { .. }));
    if is_fill {
        box_()
            .position("absolute")
            .dyn_prop(
                "left",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| find_unit(i, &key).map_or(0, |unit| unit.left)
                })),
            )
            .dyn_prop(
                "top",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| find_unit(i, &key).map_or(0, |unit| unit.top)
                })),
            )
            .dyn_prop(
                "width",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| {
                        find_unit(i, &key).and_then(fill_body).map_or(0, |(w, _, _)| w)
                    }
                })),
            )
            .dyn_prop(
                "height",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| {
                        find_unit(i, &key).and_then(fill_body).map_or(0, |(_, h, _)| h)
                    }
                })),
            )
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| {
                        find_unit(i, &key).and_then(fill_body).and_then(|(_, _, c)| chunk_rgba(c))
                    }
                })),
            )
            .into_element()
    } else if is_spaces {
        // The post-pass owns these cells — the tree mounts an empty
        // placeholder so the key reconciles without rasterizing.
        text().position("absolute").wrap_mode("none").flex_shrink(0.0).content("").into_element()
    } else {
        text()
            .position("absolute")
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop(
                "left",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| find_unit(i, &key).map_or(0, |unit| unit.left)
                })),
            )
            .dyn_prop(
                "top",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| find_unit(i, &key).map_or(0, |unit| unit.top)
                })),
            )
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &ChromeInputs| {
                        find_unit(i, &key).map_or_else(
                            || StyledText::new(Vec::new()),
                            |unit| match &unit.body {
                                ChromeBody::Text(chunks) => chunk_text(chunks),
                                _ => StyledText::new(Vec::new()),
                            },
                        )
                    }
                })),
            )
            .into_element()
    }
}

fn surface_element(inputs: &Signal<ChromeInputs>, width: usize, height: usize) -> Element {
    let source = ForItemSource {
        source: map(inputs.clone(), |inputs: &ChromeInputs| {
            inputs
                .items
                .iter()
                .map(|unit| ForItem {
                    key: unit.key.clone(),
                    value: PropValue::Str(unit.key.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let item_inputs = inputs.clone();
    let units = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        unit_element(&item_inputs, &item.key)
    });
    box_()
        .width(width.max(1) as f32)
        .height(height.max(1) as f32)
        .position("relative")
        .children_dyn(Rc::new(units))
        .into_element()
}

// ---------------------------------------------------------------------------
// the mount (one per surface, keyed on geometry)
// ---------------------------------------------------------------------------

struct ChromeMount {
    inputs: Signal<ChromeInputs>,
    renderer: SolidRenderer,
    key: (usize, usize, CellColor),
}

impl ChromeMount {
    fn new(width: usize, height: usize, background: CellColor, items: &[ChromeUnit]) -> Self {
        let signal = Signal::new(ChromeInputs { items: items.to_vec() });
        let renderer = opentui_solid::render(
            CliRendererConfig {
                width: u32::try_from(width.max(1)).unwrap_or(u32::MAX),
                height: u32::try_from(height.max(1)).unwrap_or(u32::MAX),
                background_color: background_rgba(background),
                ..CliRendererConfig::default()
            },
            surface_element(&signal, width, height),
        );
        Self { inputs: signal, renderer, key: (width, height, background) }
    }

    fn paint(&mut self, items: Vec<ChromeUnit>) {
        self.inputs.set(ChromeInputs { items });
        self.renderer.flush();
    }
}

thread_local! {
    /// One mount per surface per thread-process — the app paints each
    /// chrome surface at most once per frame.
    static MOUNTS: RefCell<HashMap<&'static str, ChromeMount>> =
        RefCell::new(HashMap::new());
}

// ---------------------------------------------------------------------------
// the cell copy (raster → in-house Frame)
// ---------------------------------------------------------------------------

fn frame_color(color: Rgba) -> CellColor {
    if color.alpha() == 0 {
        CellColor::Default
    } else {
        CellColor::Rgb(color.red(), color.green(), color.blue())
    }
}

/// The buffer → [`Frame`] copy for the chrome family: grapheme
/// decoding as `frame_from_buffer`, attributes restored 1:1 on the
/// low ECMA byte (the chrome painters' attrs ARE the ECMA word that
/// `render::ansi` serializes — no strikethrough remap, unlike the
/// transcript grid's bit-8 packing).
fn chrome_frame_from_buffer(buffer: &OptimizedBuffer) -> Frame {
    let width = buffer.width as usize;
    let mut cells = Vec::with_capacity(buffer.height as usize);
    for y in 0..buffer.height {
        let mut row = Vec::with_capacity(width);
        for x in 0..buffer.width {
            let Some(cell) = buffer.get(x, y) else {
                row.push(Cell::default());
                continue;
            };
            let ch = if is_continuation_char(cell.char) {
                CONTINUATION
            } else if is_grapheme_char(cell.char) {
                buffer
                    .grapheme_bytes(grapheme_id_from_char(cell.char))
                    .chars()
                    .next()
                    .unwrap_or(' ')
            } else {
                char::from_u32(cell.char).unwrap_or(' ')
            };
            let attrs = (cell.attributes & 0xFF) as u16;
            // The raster's default-foreground snapshot is WHITE: an
            // untouched cell (transparent background, no attributes,
            // a space) reads fg=white, where the old painters' plain
            // pages carry the terminal default — the M1 fill
            // signature's dual. Restore Default on exactly that
            // signature; every painted glyph carries its own fg.
            let fg = if ch == ' '
                && attrs == 0
                && cell.fg == Rgba::rgb(255, 255, 255)
                && cell.bg.alpha() == 0
            {
                CellColor::Default
            } else {
                frame_color(cell.fg)
            };
            row.push(Cell { ch, attrs, fg, bg: frame_color(cell.bg) });
        }
        cells.push(row);
    }
    Frame { cells, width, height: buffer.height as usize }
}

// ---------------------------------------------------------------------------
// shared shaping helpers
// ---------------------------------------------------------------------------

/// The first cell-for-cell divergence between two frames (the parity
/// gates' reporter): `(row, col, description)`.
pub(crate) fn first_divergence(actual: &Frame, expected: &Frame) -> Option<(usize, usize, String)> {
    if actual.height != expected.height || actual.width != expected.width {
        return Some((
            0,
            0,
            format!(
                "size {}x{} vs {}x{}",
                actual.width, actual.height, expected.width, expected.height
            ),
        ));
    }
    for row in 0..expected.height {
        for col in 0..expected.width {
            let a = actual.cell(row, col);
            let b = expected.cell(row, col);
            if (a.ch, a.attrs, a.fg, a.bg) != (b.ch, b.attrs, b.fg, b.bg) {
                return Some((
                    row,
                    col,
                    format!(
                        "solid {:?} vs old {:?}",
                        (a.ch, a.attrs, a.fg, a.bg),
                        (b.ch, b.attrs, b.fg, b.bg)
                    ),
                ));
            }
        }
    }
    None
}

/// Shape one styled run as text units — glyph runs as single units
/// (the raster's atom walk == the painter's char cells), space runs
/// as [`ChromeBody::Spaces`] stamps (the painter writes `' '` + fg
/// OVER existing glyphs; the raster's transparent-space fast path is
/// a no-op there — see the variant doc), and one unit PER CHARACTER
/// when a glyph run is not ASCII: the char-naive painters
/// (`put_raw`-shaped loops) advance ONE CELL per `char` regardless
/// of cluster width, so a wide glyph's continuation cell is
/// overwritten by the NEXT char's unit (paint order — the raster
/// z-orders siblings, last wins), reproducing the old bytes exactly.
pub(crate) fn chrome_run_units(
    key_base: &str,
    left: usize,
    top: usize,
    run_text: &str,
    fg: CellColor,
    bg: CellColor,
    attrs: u16,
) -> Vec<ChromeUnit> {
    let mut units = Vec::new();
    let mut glyph_run = String::new();
    let mut run_start = usize::MAX;
    let mut spaces_start = 0usize;
    let mut spaces_len = 0usize;
    let mut index = 0usize;
    let mut flush_glyphs = |units: &mut Vec<ChromeUnit>,
                            glyph_run: &mut String,
                            run_start: &mut usize| {
        if glyph_run.is_empty() {
            return;
        }
        if glyph_run.is_ascii() {
            units.push(ChromeUnit {
                key: format!("{key_base}@{top}#{run_start}"),
                left: left + *run_start,
                top,
                body: ChromeBody::Text(vec![ChromeChunk::new(glyph_run.clone(), fg, bg, attrs)]),
            });
        } else {
            for (offset, ch) in glyph_run.chars().enumerate() {
                units.push(ChromeUnit {
                    key: format!("{key_base}@{top}#{},{}", *run_start + offset, offset),
                    left: left + *run_start + offset,
                    top,
                    body: ChromeBody::Text(vec![ChromeChunk::new(ch.to_string(), fg, bg, attrs)]),
                });
            }
        }
        glyph_run.clear();
        *run_start = usize::MAX; // sentinel: reset by the next segment
    };
    for ch in run_text.chars() {
        if ch == ' ' {
            if spaces_len == 0 {
                flush_glyphs(&mut units, &mut glyph_run, &mut run_start);
                spaces_start = index;
            }
            spaces_len += 1;
        } else {
            if spaces_len > 0 {
                units.push(ChromeUnit {
                    key: format!("{key_base}@{top}#s{spaces_start}"),
                    left: left + spaces_start,
                    top,
                    body: ChromeBody::Spaces { count: spaces_len, fg, attrs },
                });
                spaces_len = 0;
            }
            if run_start == usize::MAX {
                run_start = index;
            }
            glyph_run.push(ch);
        }
        index += 1;
    }
    flush_glyphs(&mut units, &mut glyph_run, &mut run_start);
    if spaces_len > 0 {
        units.push(ChromeUnit {
            key: format!("{key_base}@{top}#s{spaces_start}"),
            left: left + spaces_start,
            top,
            body: ChromeBody::Spaces { count: spaces_len, fg, attrs },
        });
    }
    // Single-run fast path keeps the M5 position-stable key shape.
    if units.len() == 1 {
        units[0].key = format!("{key_base}@{top}");
    }
    units
}

/// The bridge stamp pass: [`ChromeBody::Spaces`] units write their
/// cells over the COPIED frame (char/fg/attrs; the destination bg
/// kept — the old `put` loops never touch bg). A cell whose cluster
/// a LATER text unit starts is left alone — the raster's paint
/// order already decided it (the hint-space-over-title shape).
fn stamp_spaces(frame: &mut Frame, items: &[ChromeUnit]) {
    for (index, unit) in items.iter().enumerate() {
        let ChromeBody::Spaces { count, fg, attrs } = &unit.body else { continue };
        // Cluster-start cells of every LATER text unit (row-aware).
        let mut claimed = std::collections::HashSet::new();
        for later in items.iter().skip(index + 1) {
            let ChromeBody::Text(chunks) = &later.body else { continue };
            let mut x = later.left;
            for chunk in chunks {
                let style = ratatui::style::Style::new();
                for atom in crate::wrap::flatten(&chunk.text, style) {
                    claimed.insert((later.top, x));
                    x += atom.width.max(1);
                }
            }
        }
        for offset in 0..*count {
            let col = unit.left + offset;
            if claimed.contains(&(unit.top, col)) {
                continue;
            }
            if let Some(cell) = frame.cells.get_mut(unit.top).and_then(|line| line.get_mut(col)) {
                cell.ch = ' ';
                cell.fg = *fg;
                cell.attrs = *attrs;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the public bridge
// ---------------------------------------------------------------------------

/// Paint one chrome surface through the opentui-solid path: shape
/// the units (the surface's own walk — the SAME solver the old
/// painter used), sync + flush the surface's mount, copy the cells.
///
/// `background` is the mount's raster background (`Default` keeps
/// untouched cells at the in-house default, the plain-page
/// surfaces' vocabulary; a color fills the whole buffer).
pub(crate) fn chrome_frame(
    surface: &'static str,
    width: usize,
    height: usize,
    background: CellColor,
    items: Vec<ChromeUnit>,
) -> Frame {
    MOUNTS.with_borrow_mut(|mounts| {
        let key = (width, height, background);
        let mount = mounts
            .entry(surface)
            .or_insert_with(|| ChromeMount::new(width, height, background, &items));
        if mount.key != key {
            *mount = ChromeMount::new(width, height, background, &items);
        } else {
            mount.paint(items.clone());
        }
        let buffer = &mount.renderer.renderer.current_render_buffer;
        let mut frame = chrome_frame_from_buffer(buffer);
        stamp_spaces(&mut frame, &items);
        frame
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_units_paint_in_list_order() {
        let units = vec![
            ChromeUnit {
                key: "a".into(),
                left: 0,
                top: 0,
                body: ChromeBody::Text(vec![ChromeChunk::new(
                    "AA",
                    CellColor::Rgb(1, 1, 1),
                    CellColor::Default,
                    0,
                )]),
            },
            ChromeUnit {
                key: "b".into(),
                left: 1,
                top: 0,
                body: ChromeBody::Text(vec![ChromeChunk::new(
                    "B",
                    CellColor::Rgb(2, 2, 2),
                    CellColor::Default,
                    0,
                )]),
            },
        ];
        let frame = chrome_frame("order_probe", 4, 1, CellColor::Default, units);
        let cell = frame.cell(0, 1);
        assert_eq!(
            (cell.ch, cell.fg),
            ('B', CellColor::Rgb(2, 2, 2)),
            "the LATER unit must win (M4: the raster z-orders siblings, last wins)"
        );
    }
}
