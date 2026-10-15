//! Lane M6: the composer's subagent picker panel on opentui-solid —
//! the M4/M5 flat-units shape over the SHARED
//! [`crate::components::subagent_picker::picker_panel_paint`] walk
//! (see `docs/migration-m1-tabstrip.md`, the M6 section).
//!
//! **What this module is.** The panel (tabs row / 5-row body / hints
//! row at the corpus3 geometry) painted as an opentui-solid builder
//! tree: every placed line and fill rect of the shared walk becomes
//! an absolutely-positioned element in ONE keyed `For` — no row
//! carriers, no nesting. The hand-rolled painter
//! ([`crate::components::subagent_picker::picker_panel_frame`]) stays
//! the ONE solver: both painters consume the same `PickerPaint`
//! output, so the geometry can never drift.
//!
//! **The tree.** A positioned root carrying the raised panel
//! background; fills first, texts after (the For's DOM order is the
//! paint order — the `Running` status overwrites the entry label's
//! tail exactly like the painter's later-placed run). The keys
//! are the walk's own tags (`picker:entry:{sessionID}`,
//! `picker:border`, …), position-stable across selection moves and
//! filter flips — a selection change patches one row's chunks in
//! place, nothing remounts.
//!
//! **Selection flag (retired).** The lane-M6 dual-path flag
//! (`OC_SUBAGENT_OPENTUI`) is gone: this module's opentui-solid tree
//! is the picker panel's ONLY renderer — the hand-rolled painter was
//! deleted after the one-release probation (the retirement lane).

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::cell::RefCell;
use std::rc::Rc;

use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal, SolidRenderer};

use crate::components::session_tabs_solid::frame_from_buffer;
use crate::components::subagent_picker::{
    picker_panel_paint, PickerColors, PickerPaint, PANEL_ROWS,
};
use crate::layout::{Placed, PlacedLine};
use crate::render::Frame;
use crate::store::{PickerEntry, PickerState};
use crate::theme::ThemeState;

// ---------------------------------------------------------------------------
// colors + chunk helpers (the M1..M5 shape)
// ---------------------------------------------------------------------------

/// An RGB triple (the port's cell currency).
type Rgb = (u8, u8, u8);

fn rgb_of(color: ratatui::style::Color) -> Rgb {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}

fn rgba(rgb: Rgb) -> Rgba {
    Rgba::rgb(rgb.0, rgb.1, rgb.2)
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// ratatui modifier bits → opentui's ECMA-ordered attribute word
/// (bold 1<<0 …) — the picker carries BOLD only; the full table
/// mirrors `transcript_solid::ecma_attributes`.
fn ecma_attributes(modifier: ratatui::style::Modifier) -> u32 {
    use ratatui::style::Modifier;
    let mut bits = 0u32;
    if modifier.contains(Modifier::BOLD) {
        bits |= 1 << 0;
    }
    if modifier.contains(Modifier::DIM) {
        bits |= 1 << 1;
    }
    if modifier.contains(Modifier::ITALIC) {
        bits |= 1 << 2;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        bits |= 1 << 3;
    }
    if modifier.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK) {
        bits |= 1 << 4;
    }
    if modifier.contains(Modifier::REVERSED) {
        bits |= 1 << 5;
    }
    if modifier.contains(Modifier::HIDDEN) {
        bits |= 1 << 6;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        bits |= 1 << 7;
    }
    bits
}

// ---------------------------------------------------------------------------
// inputs (the shared walk → view bridge)
// ---------------------------------------------------------------------------

/// One panel paint unit — a pre-split placed LINE (text) or a fill
/// rect — at panel-local absolute coordinates.
#[derive(Debug, Clone)]
struct PanelItem {
    key: String,
    left: usize,
    top: usize,
    body: PanelBody,
}

#[derive(Debug, Clone)]
enum PanelBody {
    Text(StyledText),
    Fill { width: usize, height: usize, color: Rgb },
}

/// One frame's panel snapshot — the ONLY input the tree reads. The
/// panel's geometry (width × 11 rows) is MOUNT-STATIC; the units
/// patch in place (a selection move, a filter flip, an entry landing
/// all reconcile on the keys).
#[derive(Clone)]
struct PanelInputs {
    items: Vec<PanelItem>,
}

/// The placed key: the walk's leaf tag (`picker:entry:{id}`…) or `u`,
/// plus the placed ROW — position-stable identity; a same-key
/// collision (two placed at one row, e.g. the border tag on every
/// row) falls back to the row, then an index suffix.
fn placed_key(
    placed: &Placed,
    y: usize,
    index: usize,
    seen: &mut std::collections::HashSet<String>,
) -> String {
    let base = match placed.tag.as_deref() {
        Some(tag) => format!("t:{tag}@{y}"),
        None => format!("u@{y}"),
    };
    if seen.insert(base.clone()) {
        base
    } else {
        let fallback = format!("{base}#{index}");
        seen.insert(fallback.clone());
        fallback
    }
}

/// One placed line's chunks with the OLD painter's merge semantics
/// applied: ratatui's `Cell::set_style` MERGES into the cell
/// (modifiers OR through) — a unit painted over an earlier unit's
/// bold cells inherits the bit (the `Running` status over a bold
/// focused label's tail, the M5 sidebar finding verbatim). The
/// raster is last-writer-wins, so the bridge bakes the accumulated
/// attribute word into the chunks (per cell, via the atom walk).
fn line_chunks(
    line: &PlacedLine,
    attrs_grid: &[u32],
    width: usize,
    left: usize,
    top: usize,
) -> StyledText {
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut x = left;
    let mut pending: Option<(String, Option<Rgb>, Option<Rgb>, u32)> = None;
    let flush = |chunks: &mut Vec<TextChunk>,
                 pending: &mut Option<(String, Option<Rgb>, Option<Rgb>, u32)>| {
        if let Some((text, fore, back, word)) = pending.take() {
            chunks.push(TextChunk {
                text,
                fg: fore.map(rgba),
                bg: back.map(rgba),
                attributes: word,
                link: None,
            });
        }
    };
    for run in &line.runs {
        let fore = run.style.fg.map(rgb_of);
        let back = run.style.bg.map(rgb_of);
        for atom in crate::wrap::flatten(&run.text, run.style) {
            let cell_width = atom.width.max(1);
            let word = attrs_grid
                .get(top * width + x)
                .copied()
                .unwrap_or_else(|| ecma_attributes(run.style.add_modifier));
            let continues = matches!(&pending,
                Some((_, fore0, back0, attrs0)) if *fore0 == fore && *back0 == back && *attrs0 == word);
            if continues {
                if let Some((text, _, _, _)) = &mut pending {
                    text.push_str(&atom.text);
                }
            } else {
                flush(&mut chunks, &mut pending);
                pending = Some((atom.text.clone(), fore, back, word));
            }
            x += cell_width;
        }
    }
    flush(&mut chunks, &mut pending);
    StyledText::new(chunks)
}

/// Collect the panel's paint units from the SHARED walk — fills
/// first, then placed lines (the For's DOM order is the paint order:
/// the old painter stamps every fill before any text, and the
/// `Running` status follows its entry's label). The text units'
/// attribute words are pre-merged through the same accumulation
/// order (see [`line_chunks`]).
fn shape(paint: &PickerPaint, width: usize) -> Vec<PanelItem> {
    // Pass 1 — the attribute accumulation grid: every text unit ORs
    // its modifier word onto the cells it covers, in paint order
    // (fills carry no modifiers).
    let grid_width = width.max(1);
    let mut attrs_grid = vec![0u32; grid_width * PANEL_ROWS];
    for placed in &paint.placed {
        for (line_index, line) in placed.lines.iter().enumerate() {
            let top = placed.y + line_index;
            if top >= PANEL_ROWS {
                continue;
            }
            let mut x = placed.x;
            for run in &line.runs {
                let word = ecma_attributes(run.style.add_modifier);
                for atom in crate::wrap::flatten(&run.text, run.style) {
                    let cell_width = atom.width.max(1);
                    for cell in x..(x + cell_width).min(grid_width) {
                        if let Some(slot) = attrs_grid.get_mut(top * grid_width + cell) {
                            *slot |= word;
                        }
                    }
                    x += cell_width;
                    if x >= width {
                        break;
                    }
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::with_capacity(paint.fills.len() + paint.placed.len());
    for (index, fill) in paint.fills.iter().enumerate() {
        items.push(PanelItem {
            key: format!("f:{index}"),
            left: usize::from(fill.x),
            top: usize::from(fill.y),
            body: PanelBody::Fill {
                width: usize::from(fill.width),
                height: usize::from(fill.height),
                color: rgb_of(fill.style.bg.unwrap_or(ratatui::style::Color::Reset)),
            },
        });
    }
    for (index, placed) in paint.placed.iter().enumerate() {
        for (line_index, line) in placed.lines.iter().enumerate() {
            let top = placed.y + line_index;
            items.push(PanelItem {
                key: placed_key(placed, top, index, &mut seen),
                left: placed.x,
                top,
                body: PanelBody::Text(line_chunks(line, &attrs_grid, grid_width, placed.x, top)),
            });
        }
    }
    items
}

// ---------------------------------------------------------------------------
// the element tree (the flat M4 unit shape)
// ---------------------------------------------------------------------------

/// `dyn_(src, to PropValue::Num)` — a usize-valued dynamic prop.
fn dyn_num<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<usize> + 'static,
{
    opentui_solid::dyn_(source, |value: &usize| PropValue::Num(*value as f32))
}

/// `dyn_(src, to PropValue::Color)` — an Rgb-valued dynamic prop.
fn dyn_color<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<Rgb> + 'static,
{
    opentui_solid::dyn_(source, |value: &Rgb| PropValue::Color(hex(*value)))
}

/// `dyn_(src, to PropValue::Styled)` — styled-chunk content.
fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

/// One `For` item element: a text unit (absolute, natural width,
/// pre-truncated content — `wrapMode none` + `flexShrink 0`) or a
/// fill rect (absolute box, explicit points). The unit's KIND is
/// baked into its key prefix (`f:` fill / `t:`/`u:` text) — stable
/// per identity, so the element type is chosen once at mount.
fn item_element(inputs: &Signal<PanelInputs>, key: &str) -> Element {
    fn find_item<'a>(inputs: &'a PanelInputs, key: &str) -> Option<&'a PanelItem> {
        inputs.items.iter().find(|item| item.key == key)
    }
    let key = key.to_owned();
    let is_fill = inputs
        .get()
        .items
        .iter()
        .any(|item| item.key == key && matches!(item.body, PanelBody::Fill { .. }));
    if is_fill {
        fn fill_body(i: &PanelInputs, key: &str) -> Option<(usize, usize, Rgb)> {
            find_item(i, key).and_then(|item| match item.body {
                PanelBody::Fill { width, height, color } => Some((width, height, color)),
                PanelBody::Text(_) => None,
            })
        }
        box_()
            .position("absolute")
            .dyn_prop(
                "left",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| find_item(i, &key).map_or(0, |item| item.left)
                })),
            )
            .dyn_prop(
                "top",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| find_item(i, &key).map_or(0, |item| item.top)
                })),
            )
            .dyn_prop(
                "width",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| fill_body(i, &key).map_or(0, |(width, _, _)| width)
                })),
            )
            .dyn_prop(
                "height",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| fill_body(i, &key).map_or(0, |(_, height, _)| height)
                })),
            )
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| {
                        fill_body(i, &key).map_or((0, 0, 0), |(_, _, color)| color)
                    }
                })),
            )
            .into_element()
    } else {
        text()
            .position("absolute")
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop(
                "left",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| find_item(i, &key).map_or(0, |item| item.left)
                })),
            )
            .dyn_prop(
                "top",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| find_item(i, &key).map_or(0, |item| item.top)
                })),
            )
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PanelInputs| {
                        find_item(i, &key).map_or_else(
                            || StyledText::new(Vec::new()),
                            |item| match &item.body {
                                PanelBody::Text(chunks) => chunks.clone(),
                                PanelBody::Fill { .. } => StyledText::new(Vec::new()),
                            },
                        )
                    }
                })),
            )
            .into_element()
    }
}

/// The panel mount: a positioned root carrying the raised background,
/// one flat keyed `For` of the panel's absolute units. Geometry bakes
/// at build time (the mount key); every content slot is a dyn prop
/// over the inputs signal.
fn panel_element(inputs: &Signal<PanelInputs>, width: usize, raised: Rgb) -> Element {
    let source = ForItemSource {
        source: map(inputs.clone(), |inputs: &PanelInputs| {
            inputs
                .items
                .iter()
                .map(|item| ForItem {
                    key: item.key.clone(),
                    value: PropValue::Str(item.key.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let item_inputs = inputs.clone();
    let units = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        item_element(&item_inputs, &item.key)
    });
    box_()
        .width(width.max(1) as f32)
        .height(PANEL_ROWS as f32)
        .position("relative")
        .background_color(hex(raised))
        .children_dyn(Rc::new(units))
        .into_element()
}

// ---------------------------------------------------------------------------
// the mount (one per thread, keyed on geometry)
// ---------------------------------------------------------------------------

/// The mounted panel: the inputs signal + a real headless
/// [`SolidRenderer`]. Rebuilt when the panel's geometry changes (a
/// resize moving the column width, the sidebar dock reflowing it, a
/// theme swap) — the M2 height-keying rule.
struct SolidPicker {
    inputs: Signal<PanelInputs>,
    renderer: SolidRenderer,
    key: (usize, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb),
}

impl SolidPicker {
    fn new(width: usize, colors: &PickerColors, inputs: &PanelInputs) -> Self {
        let signal = Signal::new(inputs.clone());
        let key = colors_key(colors, width);
        let renderer = opentui_solid::render(
            CliRendererConfig {
                width: u32::from(u16::try_from(width.max(1)).unwrap_or(u16::MAX)),
                height: u32::try_from(PANEL_ROWS).unwrap_or(11),
                background_color: rgba(rgb_of(colors.raised)),
                ..CliRendererConfig::default()
            },
            panel_element(&signal, width, rgb_of(colors.raised)),
        );
        Self { inputs: signal, renderer, key }
    }

    /// Sync one frame's snapshot, flush the reconciler patches, and
    /// render — the per-frame bridge cost.
    fn paint(&mut self, inputs: PanelInputs) {
        self.inputs.set(inputs);
        self.renderer.flush();
    }
}

/// The mount key's color arm — every color the tree bakes at build
/// time (the raised root + the row/focused action colors, so a theme
/// swap rebuilds instead of drifting).
fn colors_key(colors: &PickerColors, width: usize) -> (usize, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb) {
    (
        width,
        rgb_of(colors.raised),
        rgb_of(colors.row_bg),
        rgb_of(colors.row_fg),
        rgb_of(colors.row_fg_current),
        rgb_of(colors.focused_fill),
        rgb_of(colors.focused_text),
        // The raised root's own slot is `raised` above; the tuple
        // keeps seven color arms for the future without rebuilding
        // the key type.
        rgb_of(colors.raised),
    )
}

thread_local! {
    /// One panel mount per thread-process — the app paints exactly
    /// one picker per frame; the harnesses reuse it across scenarios.
    static PICKER: RefCell<Option<SolidPicker>> = const { RefCell::new(None) };
}

/// Paint the panel through the opentui-solid path — the new-path twin
/// of the hand-rolled [`picker_panel_frame`], fused: shape the units
/// from the SHARED walk, sync + flush the mount, copy the cells.
fn paint_solid_picker_panel(
    state: &PickerState,
    entries: &[PickerEntry],
    selected: usize,
    theme: &ThemeState,
    width: u16,
) -> Frame {
    let paint = picker_panel_paint(state, entries, selected, theme, width);
    let colors = PickerColors::resolve(theme);
    let width = usize::from(width);
    let items = shape(&paint, width);
    PICKER.with_borrow_mut(|slot| {
        let mount = slot.get_or_insert_with(|| {
            SolidPicker::new(width, &colors, &PanelInputs { items: items.clone() })
        });
        if mount.key != colors_key(&colors, width) {
            *mount = SolidPicker::new(width, &colors, &PanelInputs { items: items.clone() });
        }
        mount.paint(PanelInputs { items });
        let buffer = &mount.renderer.renderer.current_render_buffer;
        frame_from_buffer(buffer, &[])
    })
}

/// The panel frame's ONLY renderer (lane M6, retired): the
/// width × [`PANEL_ROWS`] frame the session route pastes in place of
/// the prompt mount.
#[must_use]
pub fn picker_panel_frame_dispatch(
    state: &PickerState,
    entries: &[PickerEntry],
    selected: usize,
    theme: &ThemeState,
    width: u16,
) -> Frame {
    paint_solid_picker_panel(state, entries, selected, theme, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;
    use crate::store::PickerTab;
    use crate::store::Store;

    fn theme() -> ThemeState {
        ThemeState::v2_dark()
    }

    fn entries(len: usize) -> Vec<PickerEntry> {
        (0..len)
            .map(|index| PickerEntry {
                session_id: format!("ses_{index}"),
                agent: if index % 2 == 0 { "General".into() } else { "Plan".into() },
                title: format!("Task number {index} with a description"),
                running: index == 0,
                current: index == 1,
                prefix: if index == 0 { String::new() } else { "└─ ".into() },
            })
            .collect()
    }

    /// The lane gate: the solid path's frame must equal the old
    /// path's frame CELL FOR CELL (char + attrs + fg + bg) across
    /// the picker state × width matrix — open/browse/show-inactive/
    /// The churn gate: a selection move / filter flip patches the
    /// mount in place (no rebuild) and keeps the frames equal.
    #[test]
    fn selection_churn_patches_in_place() {
        let store = Store::new(Runtime::new());
        let _ = store;
        let list = entries(4);
        let mut state = PickerState {
            open: true,
            tab: PickerTab::Subagents,
            filter_active: false,
            selected: 0,
            anchored_to: None,
        };
        let mut previous_mount = None;
        for selected in 0..4 {
            state.selected = selected;
            let solid = paint_solid_picker_panel(&state, &list, selected, &theme(), 80);
            assert_eq!(
                solid.height,
                crate::components::subagent_picker::PANEL_ROWS,
                "rows at selection {selected}"
            );
            let mount_count = usize::from(PICKER.with_borrow(Option::is_some));
            previous_mount = Some(mount_count);
        }
        assert_eq!(previous_mount, Some(1), "the mount survived the churn");
    }
}
