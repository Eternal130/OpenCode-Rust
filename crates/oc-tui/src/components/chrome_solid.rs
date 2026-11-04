//! Lane R1 (the overlay-chrome tail): ONE shared opentui-solid bridge
//! for the seven overlay-chrome surfaces — toast, the console
//! overlay, the devtools bar, the plugin page (+ its missing-route
//! fallback), the location-missing question, the migration overlay,
//! the app-state overlays (reconnecting scrim box + startup pill)
//! and the vertical session-tab rail (+ its compact hover tooltip).
//!
//! D2.2 (ruling attempt 2, plan (e)): the chrome FAMILY's surfaces
//! mount as absolutely-positioned wrapper subtrees under ONE shared
//! `surface-chrome` container in the shared-tree hub (the live
//! CliRenderer) — pixels reach the emitted [`Frame`] through ONE
//! full-buffer compositor seam ([`composite_chrome_tree`]; bg
//! alpha==0 cells are holes and skip, so the hand-painted frame
//! survives underneath). The family-6 OVERLAY surfaces (console /
//! which-key / toast / the reconnecting box) keep per-surface scratch
//! mounts + the cell copy ([`overlay_frame`]) until their own lane.
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

use opentui_core::Rgba;
use opentui_core::buffer::{
    OptimizedBuffer, grapheme_id_from_char, is_continuation_char, is_grapheme_char,
};
use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{Element, Signal, SolidRenderer, box_, for_list, map, text};

use crate::render::{CONTINUATION, Cell, CellColor, ClusterMap, Frame};

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
    // The bridge's byte assumption (zero golden re-pins prove it
    // holds today): a chrome unit never carries an INDEXED color —
    // it has no faithful opentui channel, so it would fall to the
    // transparent slot and render fg Default/white. Fail loudly at
    // the shaping seam instead of silently mis-rendering (oracle
    // gate F3); `Default` is legitimate and stays transparent.
    debug_assert!(
        !matches!(color, CellColor::Indexed(_)),
        "chrome bridge: CellColor::Indexed has no opentui channel (paints Default)"
    );
    match color {
        CellColor::Rgb(r, g, b) => Some(Rgba::rgb(r, g, b)),
        // Default/Indexed have no faithful opentui channel: leave
        // the slot transparent — the cell copy maps an untouched
        // channel back to `Default`.
        CellColor::Default | CellColor::Indexed(_) => None,
    }
}

fn background_rgba(color: CellColor) -> Rgba {
    // The same F3 guard for the mount's background slot.
    debug_assert!(
        !matches!(color, CellColor::Indexed(_)),
        "chrome bridge: CellColor::Indexed has no opentui channel (paints Default)"
    );
    match color {
        CellColor::Rgb(r, g, b) => Rgba::rgb(r, g, b),
        // Transparent — `frame_color` maps alpha 0 back to Default.
        CellColor::Default | CellColor::Indexed(_) => Rgba::from_ints(0, 0, 0, 0),
    }
}

fn hex_of(color: Option<Rgba>) -> String {
    let rgba = color.unwrap_or(Rgba::from_ints(0, 0, 0, 0));
    if rgba.alpha() == 0 {
        // `parseColor` maps "transparent" to transparent black — the
        // box painter skips alpha-0 fills (holes skip at the seam).
        return "transparent".to_owned();
    }
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
            .map(|chunk| {
                // F3 guard: the chrome vocabulary is the low ECMA
                // byte (the word `render::ansi` serializes) — a unit
                // carrying bits ≥ 0x100 would be silently masked by
                // the copy and mis-render. Fail at the seam instead.
                debug_assert!(
                    chunk.attrs & 0xFF == chunk.attrs,
                    "chrome bridge: attrs {:#x} exceed the low ECMA byte",
                    chunk.attrs
                );
                TextChunk {
                    text: chunk.text.clone(),
                    fg: chunk_rgba(chunk.fg),
                    bg: chunk_rgba(chunk.bg),
                    attributes: u32::from(chunk.attrs & 0xFF),
                    link: None,
                }
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
        // The post-pass owns these cells — the tree mounts a zero-size
        // placeholder so the key reconciles without rasterizing (an
        // EMPTY text node is measurably pathological in layout; a
        // 0×0 box is free).
        box_().position("absolute").width(0.0).height(0.0).into_element()
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
// the family-6 overlay bridge (D2.2): per-surface scratch mounts for the
// paste_frame overlay family ONLY — the console strip, the which-key
// panel, the toast panel and the reconnecting box stay Frame painters
// until the overlays lane mounts them into the shared tree (ruling #3:
// they are paste_frame overlays, a separate family). The chrome family's
// own scratch path died with its migration (grep-zero:
// ChromeMount/MOUNTS/chrome_frame/chrome_frame_from_buffer are gone).
// ---------------------------------------------------------------------------

struct OverlayMount {
    inputs: Signal<ChromeInputs>,
    renderer: SolidRenderer,
    key: (usize, usize, CellColor),
}

impl OverlayMount {
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
    /// One scratch mount per overlay surface per thread-process — the
    /// app paints each overlay surface at most once per frame.
    static OVERLAY_MOUNTS: RefCell<HashMap<&'static str, OverlayMount>> =
        RefCell::new(HashMap::new());
}

// ---------------------------------------------------------------------------
// the shared tree hub (D2.2 first family — the chrome paradigm)
// ---------------------------------------------------------------------------

/// ONE `SolidRenderer` per thread-process (ruling #1, plan (a)): in
/// the live app its `.renderer` IS the D2.1 scheduling `CliRenderer`
/// (the pump's request_render/settle/resize/destroy all go through
/// it — constructed here, attached to the session in `app::run`);
/// headless frame builds (unit tests, goldens) lazily install their
/// own. Zero surfaces mount in the mechanical commit — the
/// per-surface `MOUNTS` path above stays untouched (zero diff); the
/// chrome family's wrappers mount under the `surface-chrome` family
/// container in the migration commit.
///
/// Family skeleton under the root (ruling #2 — z order IS child
/// order, fixed indices, aligned to `App::frame`/paste_frame's z
/// stack; later lanes mount their family wrapper at their index):
///
/// ```text
/// root
/// └─[0] surface-chrome   (this lane: devtools/stats/location-missing/
/// │                       plugin pages/rail+tooltip/app-state overlays)
/// └─[1] surface-composer  (D2.2 composer lane — mounted: the prompt
/// │                       box + popup + footer row wrapper)
/// └─[2] surface-transcript(D2.2 transcript lane)
/// └─[3] surface-footer    (D2.2 footer lane)
/// └─[4] surface-dialogs   (D2.2 dialogs lane)
/// └─[5] surface-overlays  (toast/console/which-key — the paste_frame
///                          overlay family, its own lane)
/// ```
///
/// No `z` prop is introduced (ruling #2: an upstream API change would
/// be a fresh divergence).
/// One mounted chrome-family surface: the wrapper's live/shadow key
/// (its mount id), the unit signal, the imperative rect and the
/// per-frame stamp pass inputs.
struct ChromeSurface {
    name: &'static str,
    signal: Signal<ChromeInputs>,
    key: String,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    background: CellColor,
    items: Vec<ChromeUnit>,
    stamps: Vec<ChromeStamp>,
    /// Set by this frame's [`chrome_mount`]; cleared at the composite
    /// seam — a surface nobody synced this frame unmounts (the old
    /// painters' visibility branches, e.g. `console_open`).
    fresh: bool,
}

/// A composite-time stamp over the surface's region (the frame-level
/// passes the old bridge ran after its cell copy — kept as stamps so
/// the tree itself stays declarative).
#[derive(Clone)]
pub(crate) enum ChromeStamp {
    /// A bg-colored region keeping each cell's ch/fg/attrs (the rail
    /// handle-hot column: `paint_rail_handle_hot`'s row walk).
    Bg { left: usize, top: usize, width: usize, height: usize, color: CellColor },
    /// A single-cell bg/fg recolor keeping the glyph (the composer
    /// family's golden software-caret block — the retired bridge's
    /// cell recolor, applied at the wrapper's origin).
    Caret { left: usize, top: usize, fg: CellColor, bg: CellColor },
    /// A single-cell fg⇄bg swap (the composer family's G-66 selection
    /// highlight — the ref's DEFAULT selection styling).
    Swap { left: usize, top: usize },
}

/// Apply one stamp at (`base_left`, `base_top`) — the surface/wrapper
/// origin the stamp's rect is relative to.
fn apply_stamp(frame: &mut Frame, base_left: usize, base_top: usize, stamp: &ChromeStamp) {
    match *stamp {
        ChromeStamp::Bg { left, top, width, height, color } => {
            for row in base_top + top..base_top + top + height {
                for col in base_left + left..base_left + left + width {
                    if let Some(cell) = frame.cells.get_mut(row).and_then(|line| line.get_mut(col)) {
                        cell.bg = color;
                    }
                }
            }
        }
        ChromeStamp::Caret { left, top, fg, bg } => {
            if let Some(cell) = frame
                .cells
                .get_mut(base_top + top)
                .and_then(|line| line.get_mut(base_left + left))
            {
                cell.fg = fg;
                cell.bg = bg;
            }
        }
        ChromeStamp::Swap { left, top } => {
            if let Some(cell) = frame
                .cells
                .get_mut(base_top + top)
                .and_then(|line| line.get_mut(base_left + left))
            {
                std::mem::swap(&mut cell.fg, &mut cell.bg);
            }
        }
    }
}

/// The chrome family's fixed draw order — today's paint order frozen
/// (ruling attempt 2: mount 非路由门控, 后者胜=树子序). The base pages
/// first, then the chrome that today pasted after them; pill/migration
/// last (today's frame tail). The family-6 overlays (console/which-key/
/// toast/reconnecting) still paste AFTER the composite seam and so stay
/// above the family, exactly today's z for them.
const SURFACE_ORDER: &[&str] = &[
    "stats_page",
    "stats_poster",
    "tab_rail",
    "tab_rail_tooltip",
    "location_missing",
    "devtools_bar",
    "plugin_page",
    "plugin_route_missing",
    "startup_pill",
    "migration_box",
];

/// One NON-chrome family's mounted wrapper (D2.2 family-ization, the
/// composer lane first): ONE absolutely-positioned wrapper subtree per
/// family under its own `surface-{family}` container at the skeleton
/// index — the rect is the family mount's imperative source (patches
/// via `set_prop`, never a remount), the content element is the
/// family's OWN tree (its lane rebuilds it when its geometry key
/// changes; the wrapper survives). Stamps run at the composite seam,
/// offset by the wrapper's origin.
struct FamilyWrapper {
    family: &'static str,
    /// The wrapper box's mount key.
    key: String,
    /// The mounted content subtree's key (`None` while content is
    /// between rebuilds — always `Some` in practice).
    content_key: Option<String>,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    stamps: Vec<ChromeStamp>,
    /// Set by this frame's sync; cleared at the composite seam — a
    /// family nobody synced this frame unmounts (the old painters'
    /// visibility branches).
    fresh: bool,
}

pub(crate) struct ChromeTree {
    solid: SolidRenderer,
    /// The double-flush guard (ruling flag): at most ONE shared-tree
    /// render per sync batch — a flush with no intervening sync is a
    /// no-op, so the shared renderer can never double-rasterize a
    /// frame (the "golden zero-diff but PTY timing divergence"
    /// failure mode).
    dirty: bool,
    /// The mounted chrome-family surfaces, always in SURFACE_ORDER
    /// (child order = z order).
    surfaces: Vec<ChromeSurface>,
    /// The non-chrome family wrappers (composer first), child order
    /// under their own containers = z order.
    wrappers: Vec<FamilyWrapper>,
    /// The family containers' mount keys (`surface-composer`, …) by
    /// skeleton index order.
    family_containers: Vec<(&'static str, String)>,
    /// The `surface-chrome` family container's mount key (skeleton
    /// index 0 under the root).
    family_key: Option<String>,
    width: u32,
    height: u32,
}

impl ChromeTree {
    fn new(config: CliRendererConfig) -> Self {
        let width = config.width;
        let height = config.height;
        Self {
            solid: SolidRenderer::new(config),
            dirty: false,
            surfaces: Vec::new(),
            wrappers: Vec::new(),
            family_containers: Vec::new(),
            family_key: None,
            width,
            height,
        }
    }

    /// The pump's draw gate (D2.1b): the live CliRenderer's scheduler.
    pub(crate) fn request_render(&mut self) {
        self.solid.renderer.request_render();
    }

    /// `settle(1)` paints exactly one frame per invalidation batch.
    pub(crate) fn settle(&mut self, max_frames: u64) -> u64 {
        self.solid.renderer.settle(max_frames)
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.solid.resize(width, height);
    }

    pub(crate) fn destroy(&mut self) {
        self.solid.renderer.destroy();
    }

    /// Sync (or first-mount) one chrome-family surface: the wrapper
    /// box keyed by the surface name, absolutely positioned at the
    /// imperative rect; rect/background changes patch via `set_prop`
    /// (never a remount — ruling #1). `stamps` replace the old
    /// bridge's post-copy frame passes.
    fn mount_surface(
        &mut self,
        surface: &'static str,
        left: usize,
        top: usize,
        width: usize,
        height: usize,
        background: CellColor,
        items: &[ChromeUnit],
        stamps: &[ChromeStamp],
    ) {
        // The canvas only grows (a later, larger surface resize-ups the
        // shared renderer; screen shrink rides the pump's resize).
        let need_w = (left.saturating_add(width.max(1))).max(1) as u32;
        let need_h = (top.saturating_add(height.max(1))).max(1) as u32;
        if need_w > self.width || need_h > self.height {
            let w = self.width.max(need_w);
            let h = self.height.max(need_h);
            self.solid.resize(w, h);
            self.width = w;
            self.height = h;
            if let Some(key) = self.family_key.clone() {
                self.solid.set_prop(&key, "width", PropValue::Num(w as f32));
                self.solid.set_prop(&key, "height", PropValue::Num(h as f32));
            }
        }
        // The family container (skeleton index 0 under the root).
        if self.family_key.is_none() {
            let element = box_()
                .key("surface-chrome")
                .position("absolute")
                .left(0.0)
                .top(0.0)
                .width(f64::from(self.width) as f32)
                .height(f64::from(self.height) as f32)
                .into_element();
            let key = self.solid.mount(SolidRenderer::ROOT, 0, element);
            self.family_key = Some(key);
        }
        if let Some(index) = self.surfaces.iter().position(|s| s.name == surface) {
            let existing = &mut self.surfaces[index];
            if existing.left != left {
                self.solid.set_prop(&existing.key, "left", PropValue::Num(left as f32));
            }
            if existing.top != top {
                self.solid.set_prop(&existing.key, "top", PropValue::Num(top as f32));
            }
            if existing.width != width {
                self.solid.set_prop(&existing.key, "width", PropValue::Num(width.max(1) as f32));
            }
            if existing.height != height {
                self.solid.set_prop(&existing.key, "height", PropValue::Num(height.max(1) as f32));
            }
            let bg = background_rgba(background);
            if existing.background != background {
                self.solid.set_prop(
                    &existing.key,
                    "backgroundColor",
                    PropValue::Color(hex_of(Some(bg))),
                );
            }
            existing.signal.set(ChromeInputs { items: items.to_vec() });
            existing.left = left;
            existing.top = top;
            existing.width = width;
            existing.height = height;
            existing.background = background;
            existing.items = items.to_vec();
            existing.stamps = stamps.to_vec();
            existing.fresh = true;
        } else {
            let signal = Signal::new(ChromeInputs { items: items.to_vec() });
            let element = surface_wrapper(surface, &signal, left, top, width, height, background);
            // Child order = z order: insert at the SURFACE_ORDER position
            // (mount is an anchored insert — later families take the
            // skeleton indices above this container).
            let index = self
                .surfaces
                .iter()
                .filter(|s| order_of(s.name) < order_of(surface))
                .count();
            let parent = self.family_key.clone().unwrap_or_else(|| {
                SolidRenderer::ROOT.to_owned() // unreachable: mounted above
            });
            let key = self.solid.mount(&parent, index, element);
            self.surfaces.push(ChromeSurface {
                name: surface,
                signal,
                key,
                left,
                top,
                width,
                height,
                background,
                items: items.to_vec(),
                stamps: stamps.to_vec(),
                fresh: true,
            });
        }
        self.dirty = true;
    }

    /// Unmount every surface this frame did not sync — the old
    /// painters' visibility branches (the mount call sites are the
    /// gating).
    fn prune_stale(&mut self) {
        let mut index = 0;
        while index < self.surfaces.len() {
            if self.surfaces[index].fresh {
                self.surfaces[index].fresh = false;
                index += 1;
            } else {
                let stale = self.surfaces.remove(index);
                let _ = self.solid.unmount(&stale.key);
                self.dirty = true;
            }
        }
        // The family wrappers share the rule (a family its lane did
        // not sync this frame unmounts, content first).
        let mut index = 0;
        while index < self.wrappers.len() {
            if self.wrappers[index].fresh {
                self.wrappers[index].fresh = false;
                index += 1;
            } else {
                let stale = self.wrappers.remove(index);
                if let Some(content) = stale.content_key {
                    let _ = self.solid.unmount(&content);
                }
                let _ = self.solid.unmount(&stale.key);
                self.dirty = true;
            }
        }
    }

    /// Grow the shared canvas to fit (`need_w`, `need_h`) and re-prop
    /// every mounted family container (the canvas only grows — screen
    /// shrink rides the pump's resize).
    fn grow_canvas(&mut self, need_w: u32, need_h: u32) {
        if need_w > self.width || need_h > self.height {
            let w = self.width.max(need_w);
            let h = self.height.max(need_h);
            self.solid.resize(w, h);
            self.width = w;
            self.height = h;
            if let Some(key) = self.family_key.clone() {
                self.solid.set_prop(&key, "width", PropValue::Num(w as f32));
                self.solid.set_prop(&key, "height", PropValue::Num(h as f32));
            }
            for (_, key) in &self.family_containers.clone() {
                self.solid.set_prop(key, "width", PropValue::Num(w as f32));
                self.solid.set_prop(key, "height", PropValue::Num(h as f32));
            }
        }
    }

    /// Sync (or first-mount) one NON-chrome family's wrapper — the
    /// composer lane's seam, chrome-isomorphic: the wrapper box keyed
    /// by the family name under the `surface-{family}` container at
    /// the skeleton index (child order = z), absolutely positioned at
    /// the imperative rect; rect changes patch via `set_prop` (never
    /// a remount — ruling #1). `content`/`content_key` rebuild the
    /// wrapper's single content subtree when the family's geometry
    /// key changed (`None` content = keep the mounted child).
    fn sync_family_wrapper(
        &mut self,
        family: &'static str,
        skeleton_index: usize,
        left: usize,
        top: usize,
        width: usize,
        height: usize,
        content_key: Option<String>,
        content: Option<Element>,
        stamps: &[ChromeStamp],
    ) {
        let width = width.max(1);
        let height = height.max(1);
        self.grow_canvas(
            (left.saturating_add(width)).max(1) as u32,
            (top.saturating_add(height)).max(1) as u32,
        );
        // The family container (its skeleton index under the root).
        let container = match self.family_containers.iter().position(|(f, _)| *f == family) {
            Some(index) => self.family_containers[index].1.clone(),
            None => {
                let element = box_()
                    .key(format!("surface-{family}"))
                    .position("absolute")
                    .left(0.0)
                    .top(0.0)
                    .width(f64::from(self.width) as f32)
                    .height(f64::from(self.height) as f32)
                    .into_element();
                let key = self.solid.mount(SolidRenderer::ROOT, skeleton_index, element);
                self.family_containers.push((family, key.clone()));
                key
            }
        };
        if let Some(index) = self.wrappers.iter().position(|w| w.family == family) {
            let existing = &mut self.wrappers[index];
            if existing.left != left {
                self.solid.set_prop(&existing.key, "left", PropValue::Num(left as f32));
            }
            if existing.top != top {
                self.solid.set_prop(&existing.key, "top", PropValue::Num(top as f32));
            }
            if existing.width != width {
                self.solid.set_prop(&existing.key, "width", PropValue::Num(width as f32));
            }
            if existing.height != height {
                self.solid.set_prop(&existing.key, "height", PropValue::Num(height as f32));
            }
            if content.is_some() && existing.content_key.as_deref() != content_key.as_deref() {
                if let Some(old) = existing.content_key.take() {
                    let _ = self.solid.unmount(&old);
                }
                if let (Some(_), Some(element)) = (&content_key, content) {
                    let parent = existing.key.clone();
                    let mounted = self.solid.mount(&parent, 0, element);
                    existing.content_key = Some(mounted);
                }
            }
            existing.left = left;
            existing.top = top;
            existing.width = width;
            existing.height = height;
            existing.stamps = stamps.to_vec();
            existing.fresh = true;
        } else {
            let element = box_()
                .key(format!("{family}-wrapper"))
                .position("absolute")
                .left(left as f32)
                .top(top as f32)
                .width(width as f32)
                .height(height as f32)
                .into_element();
            let key = self.solid.mount(&container, 0, element);
            let content_key = content.zip(content_key).map(|(element, _)| {
                let mounted = self.solid.mount(&key, 0, element);
                mounted
            });
            self.wrappers.push(FamilyWrapper {
                family,
                key,
                content_key,
                left,
                top,
                width,
                height,
                stamps: stamps.to_vec(),
                fresh: true,
            });
        }
        self.dirty = true;
    }

    /// The single compositor seam (ruling attempt 2, plan (e)): one
    /// full-buffer walk of the shared tree over the assembled
    /// [`Frame`] — bg alpha==0 cells (holes: unmigrated regions,
    /// transparent mount backgrounds) SKIP, so the hand-painted frame
    /// survives underneath; written cells carry the old copy's exact
    /// cell semantics (grapheme decode, low-byte attrs, the
    /// white→Default restoration). The per-surface stamp passes run
    /// after the walk, in family order.
    fn composite_into(&mut self, frame: &mut Frame) {
        let buffer = &self.solid.renderer.current_render_buffer;
        let rows = (buffer.height as usize).min(frame.height);
        let cols = (buffer.width as usize).min(frame.width);
        let mut clusters: Option<Box<ClusterMap>> = None.take();
        let mut cluster_map = ClusterMap::new();
        for y in 0..rows {
            for x in 0..cols {
                let Some(cell) = buffer.get(x as u32, y as u32) else { continue };
                let ch = if is_continuation_char(cell.char) {
                    CONTINUATION
                } else if is_grapheme_char(cell.char) {
                    let grapheme = buffer.grapheme_bytes(grapheme_id_from_char(cell.char));
                    if grapheme.chars().nth(1).is_some() {
                        cluster_map.insert((y, x), grapheme.to_owned());
                    }
                    grapheme.chars().next().unwrap_or(' ')
                } else {
                    char::from_u32(cell.char).unwrap_or(' ')
                };
                debug_assert!(
                    cell.attributes >> 8 == 0,
                    "chrome bridge: buffer attributes {:#x} carry bits past the ECMA byte",
                    cell.attributes
                );
                let attrs = (cell.attributes & 0xFF) as u16;
                // The hole rule (ruling attempt 2): a cell carrying
                // the raster's UNTOUCHED signature (space, no attrs,
                // the default-white fg, no background) is not chrome
                // ink — the frame's own pixel survives underneath
                // (the retired copy's white→Default restoration,
                // migrated: the blank a per-surface paste used to
                // write is now the hand-painted base's blank).
                if ch == ' '
                    && attrs == 0
                    && cell.fg == Rgba::rgb(255, 255, 255)
                    && cell.bg.alpha() == 0
                {
                    continue;
                }
                let fg = frame_color(cell.fg);
                if let Some(slot) = frame.cells.get_mut(y).and_then(|line| line.get_mut(x)) {
                    *slot = Cell { ch, attrs, fg, bg: frame_color(cell.bg) };
                }
            }
        }
        if !cluster_map.is_empty() {
            clusters = Some(Box::new(cluster_map));
        }
        if let Some(new_clusters) = clusters {
            let merged = frame.clusters.get_or_insert_with(|| Box::new(ClusterMap::new()));
            for (pos, grapheme) in new_clusters.iter() {
                merged.insert(*pos, grapheme.clone());
            }
        }
        // The stamp passes (the old bridge ran them right after its
        // cell copy): Spaces units first, then the bg overlays.
        for surface in &self.surfaces {
            stamp_units_at(frame, surface.left, surface.top, &surface.items);
            for stamp in &surface.stamps {
                apply_stamp(frame, surface.left, surface.top, stamp);
            }
        }
        // The family wrappers' stamps (the composer lane first — the
        // retired bridge's caret/selection recolors), same rule.
        for wrapper in &self.wrappers {
            for stamp in &wrapper.stamps {
                apply_stamp(frame, wrapper.left, wrapper.top, stamp);
            }
        }
    }

    /// Flush the shared reconciler iff a chrome sync left patches
    /// pending — the once-per-batch boundary (ruling flag: the shared
    /// flush rides after the D2.1 settle/request_render seam). Returns
    /// whether a render happened.
    pub(crate) fn flush_if_pending(&mut self) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;
        self.solid.flush();
        true
    }
}

thread_local! {
    /// The per-thread shared-tree hub. The live app installs it at
    /// boot (`install_live_hub`); headless frame builds fall back to
    /// `hub_for`'s lazy mount sized to the frame being composed.
    static HUB: RefCell<Option<Rc<RefCell<ChromeTree>>>> = const { RefCell::new(None) };
}

/// Install the LIVE hub in `app::run`: the returned handle's renderer
/// is the D2.1 scheduling CliRenderer (config + session attach happen
/// here, exactly where the bare renderer was built before).
pub(crate) fn install_live_hub(
    config: CliRendererConfig,
    attach: impl FnOnce(&mut opentui_core::renderer::CliRenderer),
) -> Rc<RefCell<ChromeTree>> {
    let mut tree = ChromeTree::new(config);
    attach(&mut tree.solid.renderer);
    let handle = Rc::new(RefCell::new(tree));
    HUB.with_borrow_mut(|slot| *slot = Some(Rc::clone(&handle)));
    handle
}

/// The thread's hub, lazily sized for a headless frame build (grows
/// by resize when a later surface needs a larger canvas — never
/// shrinks within a build).
pub(crate) fn hub_for(width: u32, height: u32) -> Rc<RefCell<ChromeTree>> {
    let existing = HUB.with_borrow(|slot| slot.clone());
    existing.unwrap_or_else(|| {
        let handle = Rc::new(RefCell::new(ChromeTree::new(CliRendererConfig {
            width: width.max(1),
            height: height.max(1),
            ..CliRendererConfig::default()
        })));
        HUB.with_borrow_mut(|slot| *slot = Some(Rc::clone(&handle)));
        handle
    })
}

fn order_of(surface: &str) -> usize {
    SURFACE_ORDER.iter().position(|name| *name == surface).unwrap_or(SURFACE_ORDER.len())
}

/// The surface wrapper: ONE absolutely-positioned box keyed by the
/// surface name — the rect is the mount's imperative source (top/left
/// from the surface's paste site), the children are the flat keyed
/// `For` of units (surface-local coordinates; the wrapper is their
/// containing block).
fn surface_wrapper(
    surface: &str,
    signal: &Signal<ChromeInputs>,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    background: CellColor,
) -> Element {
    let source = ForItemSource {
        source: map(signal.clone(), |inputs: &ChromeInputs| {
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
    let item_signal = signal.clone();
    let units = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        unit_element(&item_signal, &item.key)
    });
    box_()
        .key(surface.to_owned())
        .position("absolute")
        .left(left as f32)
        .top(top as f32)
        .width(width.max(1) as f32)
        .height(height.max(1) as f32)
        .background_color(PropValue::Color(hex_of(Some(background_rgba(background)))))
        .children_dyn(Rc::new(units))
        .into_element()
}

/// Sync one chrome-family surface into the shared tree (the family's
/// ONLY mount entry — no Frame comes back; pixels reach the emitted
/// frame through [`composite_chrome_tree`], the single compositor
/// seam inside `render_app`).
pub(crate) fn chrome_mount(
    surface: &'static str,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    background: CellColor,
    items: &[ChromeUnit],
    stamps: &[ChromeStamp],
) {
    let width = width.max(1);
    let height = height.max(1);
    let hub = hub_for(
        (left.saturating_add(width)).max(1) as u32,
        (top.saturating_add(height)).max(1) as u32,
    );
    hub.borrow_mut().mount_surface(surface, left, top, width, height, background, items, stamps);
}

/// Sync one NON-chrome family's wrapper into the shared tree (the
/// composer lane's ONLY mount entry — chrome-isomorphic): the rect is
/// the family mount's imperative source (the old paste site), the
/// content element is the family's own tree. No Frame comes back;
/// pixels reach the emitted frame through [`composite_chrome_tree`],
/// the single compositor seam inside `render_app`.
pub(crate) fn mount_family_wrapper(
    family: &'static str,
    skeleton_index: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    content_key: Option<String>,
    content: Option<Element>,
    stamps: &[ChromeStamp],
) {
    let width = width.max(1);
    let height = height.max(1);
    let hub = hub_for(
        (left.saturating_add(width)).max(1) as u32,
        (top.saturating_add(height)).max(1) as u32,
    );
    hub.borrow_mut().sync_family_wrapper(
        family, skeleton_index, left, top, width, height, content_key, content, stamps,
    );
}

/// Is the family's wrapper currently mounted in the thread hub? A
/// lane's own content state must rebuild when this is false (the hub
/// was reset, or the wrapper was pruned at a seam).
pub(crate) fn family_wrapper_live(family: &str) -> bool {
    let hub = HUB.with_borrow(|slot| slot.clone());
    hub.is_some_and(|hub| hub.borrow().wrappers.iter().any(|w| w.family == family))
}

/// The chrome family's single compositor seam: prune unsynced
/// surfaces, flush the shared tree (at most once per sync batch — the
/// double-flush guard), then walk the shared buffer over the frame.
/// GridEmitter stays the sole byte emitter; this seam only writes the
/// in-house Frame cells.
pub(crate) fn composite_chrome_tree(frame: &mut Frame) {
    let hub = HUB.with_borrow(|slot| slot.clone());
    let Some(hub) = hub else { return; };
    let mut tree = hub.borrow_mut();
    tree.prune_stale();
    tree.flush_if_pending();
    tree.composite_into(frame);
}

/// The headless test seam (ruling attempt 2: the hub reset is
/// MANDATORY, not optional): drop the thread's hub so the next mount
/// starts from a fresh tree — `cargo test` runs a binary's tests in
/// one process and mounts are stateful.
pub(crate) fn reset_hub() {
    HUB.with_borrow_mut(|slot| *slot = None);
}

/// A blank frame of the given size (Default cells) — the rect a
/// migrated surface's old full-screen/mount paste used to blank.
pub(crate) fn blank_frame(width: usize, height: usize) -> Frame {
    Frame {
        cells: vec![vec![Cell::default(); width]; height],
        width,
        height,
        clusters: None,
    }
}

/// The golden/unit harness twin of the retired per-surface copy:
/// mount ONE surface into a fresh hub and read it back as a
/// standalone Frame (composite over a blank frame of its own rect —
/// the bytes the old `chrome_frame` returned, by construction: same
/// raster, same cell semantics, holes over blank = the old Default).
pub(crate) fn mounted_surface_frame(
    surface: &'static str,
    width: usize,
    height: usize,
    background: CellColor,
    items: &[ChromeUnit],
) -> Frame {
    reset_hub();
    chrome_mount(surface, 0, 0, width, height, background, items, &[]);
    let mut frame = blank_frame(width, height);
    composite_chrome_tree(&mut frame);
    frame
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

/// The buffer → [`Frame`] copy for the family-6 overlay bridge:
/// grapheme decoding as `frame_from_buffer`, attributes restored 1:1
/// on the low ECMA byte (the overlay painters' attrs ARE the ECMA
/// word that `render::ansi` serializes — no strikethrough remap,
/// unlike the transcript grid's bit-8 packing).
fn overlay_frame_from_buffer(buffer: &OptimizedBuffer) -> Frame {
    let width = buffer.width as usize;
    let mut cells = Vec::with_capacity(buffer.height as usize);
    // The cluster sidecar (lane B3b): a grapheme cell whose
    // `grapheme_bytes` carries more than one codepoint (ZWJ families,
    // VS16 flags, NFD marks) keeps the FULL symbol here — `Cell::ch`
    // holds only the first char and the GridEmitter prints the whole
    // cluster (the retired ratatui backend's behavior).
    let mut clusters = ClusterMap::new();
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
                let grapheme = buffer.grapheme_bytes(grapheme_id_from_char(cell.char));
                if grapheme.chars().nth(1).is_some() {
                    clusters.insert((y as usize, x as usize), grapheme.to_owned());
                }
                grapheme.chars().next().unwrap_or(' ')
            } else {
                char::from_u32(cell.char).unwrap_or(' ')
            };
            // F3 guard: the copy masks to the low ECMA byte — bits
            // 8-31 carry the raster's hyperlink id pool, and a chrome
            // unit never sets a link, so any high bits here would
            // mean a future unit type broke the bridge's vocabulary.
            debug_assert!(
                cell.attributes >> 8 == 0,
                "chrome bridge: buffer attributes {:#x} carry bits past the ECMA byte",
                cell.attributes
            );
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
    let clusters = if clusters.is_empty() { None } else { Some(Box::new(clusters)) };
    Frame { cells, width, height: buffer.height as usize, clusters }
}

// ---------------------------------------------------------------------------
// shared shaping helpers
// ---------------------------------------------------------------------------

/// The first cell-for-cell divergence between two frames (the parity
/// gates' reporter): `(row, col, description)`.
#[allow(dead_code)] // the parity gates reporter, kept for the next tail surfaces
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
    let flush_glyphs = |units: &mut Vec<ChromeUnit>,
                        glyph_run: &mut String,
                        run_start: &mut usize| {
        if glyph_run.is_empty() {
            return;
        }
        if glyph_run.is_ascii()
            || !crate::wrap::flatten(glyph_run, crate::style::Style::new())
                .iter()
                .any(|atom| atom.width > 1)
        {
            // Width-1 graphemes (box-drawing rails included) lay out
            // identically as ONE chunk — the raster's atom walk IS
            // the painter's char cells.
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

/// Cluster-aware shaping for painters whose canvas advances by
/// grapheme cluster width but leaves continuation cells at the
/// destination fill (the footer Canvas `spans` seam): one Text unit
/// PER CLUSTER at its exact cell (wide clusters still plant their
/// raster continuation), plus [`ChromeBody::Spaces`] stamps over the
/// continuation cells (ch `' '`, the fill fg — the Canvas prefill's
/// white — attrs 0, the destination bg kept).
#[allow(clippy::too_many_arguments)] // the run-shaping seam (key/pos/text/style/fill)
pub(crate) fn chrome_cluster_units(
    key_base: &str,
    left: usize,
    top: usize,
    run_text: &str,
    fg: CellColor,
    bg: CellColor,
    attrs: u16,
    fill_fg: CellColor,
) -> Vec<ChromeUnit> {
    if run_text.is_ascii() && !run_text.is_empty() {
        return vec![ChromeUnit {
            key: format!("{key_base}@{top}"),
            left,
            top,
            body: ChromeBody::Text(vec![ChromeChunk::new(run_text, fg, bg, attrs)]),
        }];
    }
    let mut units = Vec::new();
    let mut x = left;
    for (index, atom) in
        crate::wrap::flatten(run_text, crate::style::Style::new()).into_iter().enumerate()
    {
        units.push(ChromeUnit {
            key: format!("{key_base}@{top}#{index}"),
            left: x,
            top,
            body: ChromeBody::Text(vec![ChromeChunk::new(atom.text.clone(), fg, bg, attrs)]),
        });
        if atom.width > 1 {
            units.push(ChromeUnit {
                key: format!("{key_base}@{top}#c{index}"),
                left: x + 1,
                top,
                body: ChromeBody::Spaces { count: atom.width - 1, fg: fill_fg, attrs: 0 },
            });
        }
        x += atom.width.max(1);
    }
    units
}

/// The bridge stamp pass: [`ChromeBody::Spaces`] units write their
/// cells over the COPIED frame (char/fg/attrs; the destination bg
/// kept — the old `put` loops never touch bg). A cell whose cluster
/// a LATER text unit starts is left alone — the raster's paint
/// order already decided it (the hint-space-over-title shape).
fn stamp_units_at(frame: &mut Frame, base_left: usize, base_top: usize, items: &[ChromeUnit]) {
    for (index, unit) in items.iter().enumerate() {
        let ChromeBody::Spaces { count, fg, attrs } = &unit.body else { continue };
        // Cluster-start cells of every LATER text unit (row-aware).
        let mut claimed = std::collections::HashSet::new();
        for later in items.iter().skip(index + 1) {
            let ChromeBody::Text(chunks) = &later.body else { continue };
            let mut x = later.left;
            for chunk in chunks {
                let style = crate::style::Style::new();
                for atom in crate::wrap::flatten(&chunk.text, style) {
                    claimed.insert((later.top, x));
                    x += atom.width.max(1);
                }
            }
        }
        for offset in 0..*count {
            let col = base_left + unit.left + offset;
            let row = base_top + unit.top;
            if claimed.contains(&(unit.top, unit.left + offset)) {
                continue;
            }
            if let Some(cell) = frame.cells.get_mut(row).and_then(|line| line.get_mut(col)) {
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

/// Paint one family-6 overlay surface through the opentui-solid path:
/// shape the units (the surface's own walk — the SAME solver the old
/// painter used), sync + flush the surface's scratch mount, copy the
/// cells.
///
/// `background` is the mount's raster background (`Default` keeps
/// untouched cells at the in-house default, the plain-page
/// surfaces' vocabulary; a color fills the whole buffer).
pub(crate) fn overlay_frame(
    surface: &'static str,
    width: usize,
    height: usize,
    background: CellColor,
    items: &[ChromeUnit],
) -> Frame {
    OVERLAY_MOUNTS.with_borrow_mut(|mounts| {
        let key = (width, height, background);
        let mount = mounts
            .entry(surface)
            .or_insert_with(|| OverlayMount::new(width, height, background, items));
        if mount.key == key {
            mount.paint(items.to_vec());
        } else {
            *mount = OverlayMount::new(width, height, background, items);
        }
        let buffer = &mount.renderer.renderer.current_render_buffer;
        let mut frame = overlay_frame_from_buffer(buffer);
        stamp_units_at(&mut frame, 0, 0, items);
        frame
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_hub_flushes_at_most_once_per_sync_batch() {
        // The double-flush guard (ruling flag): the shared SolidRenderer
        // renders exactly once per sync batch — a second flush with no
        // intervening sync is a no-op, so no frame can double-rasterize.
        let hub = hub_for(8, 2);
        assert!(!hub.borrow_mut().flush_if_pending(), "nothing synced — no flush");
        // Mark a sync pending (the chrome mounts do this on inputs.set);
        // the first flush renders, the second without a new sync must not.
        {
            let mut tree = hub.borrow_mut();
            tree.dirty = true;
        }
        assert!(hub.borrow_mut().flush_if_pending(), "pending sync — exactly one render");
        assert!(
            !hub.borrow_mut().flush_if_pending(),
            "double flush is a no-op (the guard)"
        );
    }

    #[test]
    fn live_hub_install_reuses_the_scheduling_renderer() {
        // Plan (a): the hub's `.renderer` IS the D2.1 live CliRenderer —
        // install_live_hub hands the pump a handle whose scheduler is the
        // one the session attached to (the attach seam runs inside).
        let attached = std::cell::Cell::new(false);
        let handle = install_live_hub(CliRendererConfig::default(), |_renderer| {
            attached.set(true);
        });
        assert!(attached.get(), "the session attach ran against the hub renderer");
        // The hub is the thread's shared hub (hub_for returns the same one).
        let same = hub_for(4, 2);
        assert!(Rc::ptr_eq(&handle, &same), "one hub per thread-process");
    }

    /// Ruling attempt 2's composite-skip proof: the region of the frame
    /// NO family covers is transparent in the shared buffer (the
    /// untouched signature) — the hand-painted frame pixel SURVIVES
    /// the compositor seam, and the mounted surface's ink lands on
    /// top of its own rect only.
    #[test]
    fn composite_skips_unmounted_regions_and_keeps_frame_pixels() {
        reset_hub();
        // Base ink where NO chrome family mounts (a hand-painted
        // transcript cell) and where the surface's holes sit.
        let mut frame = blank_frame(20, 3);
        for row in 0..3 {
            for col in 0..20 {
                if let Some(cell) = frame.cells.get_mut(row).and_then(|l| l.get_mut(col)) {
                    cell.ch = 'x';
                    cell.fg = CellColor::Rgb(9, 9, 9);
                }
            }
        }
        // One opaque chunk at (2, 1): a 4-cell row; everything else in
        // the surface's rect stays a hole (transparent mount bg).
        let units = vec![ChromeUnit {
            key: "skip@1".into(),
            left: 2,
            top: 1,
            body: ChromeBody::Text(vec![ChromeChunk::new(
                "abcd",
                CellColor::Rgb(1, 2, 3),
                CellColor::Rgb(4, 5, 6),
                0,
            )]),
        }];
        chrome_mount("skip_probe", 0, 0, 20, 3, CellColor::Default, &units, &[]);
        composite_chrome_tree(&mut frame);
        // The ink landed.
        assert_eq!(frame.cell(1, 2).ch, 'a');
        assert_eq!(frame.cell(1, 2).bg, CellColor::Rgb(4, 5, 6));
        assert_eq!(frame.cell(1, 5).ch, 'd');
        // The hole (unpainted surface cell) and the unmounted region
        // both keep the hand-painted pixel.
        assert_eq!(frame.cell(1, 0).ch, 'x', "a hole keeps the frame pixel");
        assert_eq!(frame.cell(1, 6).ch, 'x');
        assert_eq!(frame.cell(0, 0).ch, 'x', "an unmounted region keeps the frame pixel");
        assert_eq!(frame.cell(1, 0).fg, CellColor::Rgb(9, 9, 9));
    }

    /// The restoration semantics, migrated from the retired
    /// per-surface copy: the raster's UNTOUCHED cell (space, attrs 0,
    /// default-white fg, transparent bg) is the hole signature — a
    /// cell the old copy restored to `Default` is now the skip; every
    /// WRITTEN cell (opaque bg or ink) carries `frame_color` mappings
    /// exactly as the old copy did (transparent slots → `Default`).
    #[test]
    fn composite_restoration_matches_the_old_copy_signature() {
        reset_hub();
        let units = vec![
            // A glyph with a TRANSPARENT bg slot (the plain-page
            // surfaces' vocabulary): written, bg → Default.
            ChromeUnit {
                key: "r@0".into(),
                left: 0,
                top: 0,
                body: ChromeBody::Text(vec![ChromeChunk::new(
                    "A",
                    CellColor::Rgb(7, 7, 7),
                    CellColor::Default,
                    0,
                )]),
            },
            // An explicit WHITE fg glyph: written, fg stays white (the
            // restore NEVER fires on written cells).
            ChromeUnit {
                key: "r@0#w".into(),
                left: 1,
                top: 0,
                body: ChromeBody::Text(vec![ChromeChunk::new(
                    "B",
                    CellColor::Rgb(255, 255, 255),
                    CellColor::Default,
                    0,
                )]),
            },
        ];
        let mut frame = blank_frame(4, 1);
        frame.cells[0][2].ch = 'z'; // hand-painted neighbor
        chrome_mount("restore_probe", 0, 0, 4, 1, CellColor::Default, &units, &[]);
        composite_chrome_tree(&mut frame);
        assert_eq!(
            (frame.cell(0, 0).ch, frame.cell(0, 0).fg, frame.cell(0, 0).bg),
            ('A', CellColor::Rgb(7, 7, 7), CellColor::Default),
            "a transparent bg slot maps to Default (the old copy's frame_color)"
        );
        assert_eq!(
            (frame.cell(0, 1).ch, frame.cell(0, 1).fg),
            ('B', CellColor::Rgb(255, 255, 255)),
            "an explicit white fg is ink, not the untouched signature"
        );
        assert_eq!(
            frame.cell(0, 2).ch, 'z',
            "the untouched cell is the hole — the frame pixel survives"
        );
        assert_eq!(frame.cell(0, 3).ch, ' ');
    }

    /// An unsynced surface unmounts at the seam (the old painters'
    /// visibility branches — e.g. `console_open`).
    #[test]
    fn unsynced_surface_unmounts_at_the_composite_seam() {
        reset_hub();
        let units = vec![ChromeUnit {
            key: "gone@0".into(),
            left: 0,
            top: 0,
            body: ChromeBody::Text(vec![ChromeChunk::new(
                "G",
                CellColor::Rgb(1, 1, 1),
                CellColor::Rgb(2, 2, 2),
                0,
            )]),
        }];
        let mut frame = blank_frame(4, 1);
        chrome_mount("gone_probe", 0, 0, 4, 1, CellColor::Default, &units, &[]);
        composite_chrome_tree(&mut frame);
        assert_eq!(frame.cell(0, 0).ch, 'G', "mounted and synced — painted");
        // Next frame: the surface's caller does not sync it (its
        // visibility branch closed) — the pixel leaves with it (the
        // live path builds a FRESH frame per pump pass; the tree
        // paints nothing into it).
        let mut next = blank_frame(4, 1);
        composite_chrome_tree(&mut next);
        assert_eq!(next.cell(0, 0).ch, ' ', "the unsynced surface unmounted");
    }

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
        let frame = overlay_frame("order_probe", 4, 1, CellColor::Default, &units);
        let cell = frame.cell(0, 1);
        assert_eq!(
            (cell.ch, cell.fg),
            ('B', CellColor::Rgb(2, 2, 2)),
            "the LATER unit must win (M4: the raster z-orders siblings, last wins)"
        );
    }
}

