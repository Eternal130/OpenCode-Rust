//! Lane M5: the session sidebar pane on opentui-solid — the
//! right-hand docked surface (`routes/session/sidebar.tsx` + the
//! `sidebar.content`/`sidebar.footer` slots) re-homed the M1..M4 way
//! (see `docs/migration-m1-tabstrip.md`).
//!
//! **What this module is.** A structural port of the pane's VISIBLE
//! mount as an opentui-solid builder tree, driven by the SAME walk
//! the hand-rolled renderer ([`crate::components::sidebar::
//! render_sidebar`]) produces — the M4 flat-units shape: the pane is
//! a coordinate frame, every placed line and fill rect an
//! absolutely-positioned element keyed `{tag|#index}` in ONE `For`
//! (no row carriers, no nesting). The old walk stays the solver:
//! both painters consume the ONE [`SidebarPaint`] output.
//!
//! **The title shimmer.** The tsx's `<title_shimmer>` is a raster
//! `TextRenderable` subclass whose animated phases (the rename-pending
//! pulse, the arrival wipe) draw through a scratch buffer +
//! colorMatrix (`component/title-shimmer.tsx`). The rename-pending
//! state NOW flows end to end on our side: the store tracks the arm
//! (`StoreSignals::title_pending` / `set_title_pending`, wired to the
//! rename POST in app.rs) and the G-327 arrival wipe (`title_arrival`
//! — a title change landing while pending was held), and the walk
//! ([`crate::components::sidebar::render_sidebar`]) materializes both
//! phases as per-cell styled runs (the sustain's 40% dim + sweep band
//! and the one-shot 450 ms wipe) inside the SAME [`SidebarPaint`] this
//! tree consumes — so both painters stay cell-for-cell identical.
//! AT REST — `animating` false, every state where neither phase is
//! live — it renders as its base `TextRenderable` with BOLD
//! attributes (sidebar.tsx:41-45), i.e. plain bold `text.base` over
//! `raised.base`: the resting shape this tree paints (a text unit
//! carrying the BOLD bit), and what the old renderer paints.
//!
//! **The scrim overlay rides the tree (D3.2).** The ≤120-col manual
//! open floats the pane over an rgba(0,0,0,70) backdrop
//! (session-frame.tsx:385-397): the ref's scrim is a full-frame box
//! with `alignItems="flex-end"` CONTAINING the sidebar, so this lane
//! mounts the same shape — the `surface-sidebar` wrapper takes the
//! scrim rect (full frame below the strip) with the pane as its
//! right-aligned child, and a [`ChromeStamp::DimBg`] pass dims every
//! covered RGB background at 70/255 exactly like the retired
//! `apply_scrim` walk (glyphs survive; the pane's own rect is the
//! stamp's keep-rect — the old paste-over-dim net bytes).
//!
//! **The scratch renderer is retired (D3.2).** The pane used to own
//! a private `SolidRenderer` + `frame_from_buffer` copy; it now
//! mounts into the shared-tree hub like every other family
//! (`chrome_solid::mount_family_wrapper`), so its pixels reach the
//! emitted frame through the ONE composite seam.
//!
//! **Selection flag (retired).** The lane-M5 dual-path flag
//! (`OC_SIDEBAR_OPENTUI`) is gone: this module's opentui-solid tree
//! is the pane's ONLY renderer — the hand-rolled placed-runs
//! fallback was deleted after the one-release probation (the
//! retirement lane).

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::cell::RefCell;
use std::rc::Rc;

use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{Children, ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal};

use crate::components::chrome_solid::{family_wrapper_live, mount_family_wrapper, ChromeStamp};
use crate::components::session_tabs::SESSION_SIDEBAR_WIDTH;
use crate::components::sidebar::SidebarView;
use crate::layout::{Placed, PlacedLine};
use crate::render::Frame;
use crate::theme::ThemeStyles;

// ---------------------------------------------------------------------------
// colors + chunk helpers (the M1..M4 shape)
// ---------------------------------------------------------------------------

/// An RGB triple (the port's cell currency).
type Rgb = (u8, u8, u8);

fn rgb_of(color: crate::style::Color) -> Rgb {
    match color {
        crate::style::Color::Rgb(r, g, b) => (r, g, b),
        // The pane's theme tokens are all truecolor; the fallback
        // only guards a future non-RGB token.
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
/// (bold 1<<0 …) — the exact inverse of the bits `render::paint::
/// modifier_bits` writes (the sidebar carries BOLD only; the full
/// table mirrors `transcript_solid::ecma_attributes`).
fn ecma_attributes(modifier: crate::style::Modifier) -> u32 {
    use crate::style::Modifier;
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

/// One pane paint unit — a pre-split placed LINE (text) or the card's
/// background fill rect — at pane-local absolute coordinates.
#[derive(Debug, Clone)]
struct PaneItem {
    key: String,
    left: usize,
    top: usize,
    body: PaneBody,
}

#[derive(Debug, Clone)]
enum PaneBody {
    Text(StyledText),
    Fill { width: usize, height: usize, color: Rgb },
}

/// The pane's theme-derived mount colors (the mount key's color arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneColors {
    /// `background.raised.base` — the pane fill.
    raised: Rgb,
    /// `background.raised.high` — the onboarding card.
    card: Rgb,
}

impl PaneColors {
    fn of(theme: &ThemeStyles) -> Self {
        PaneColors { raised: rgb_of(theme.background_raised), card: rgb_of(theme.action_hovered) }
    }
}

/// One frame's pane snapshot — the ONLY input the tree reads. The
/// pane's geometry (42 × height) is MOUNT-STATIC; the units patch in
/// place (a title change, a plugin-section refresh, the card
/// dismissing all reconcile on the keys).
#[derive(Clone)]
struct PaneInputs {
    items: Vec<PaneItem>,
}

/// The placed key: the walk's leaf tag (title, section headers, the
/// card's affordances, the location line) or `u`, plus the placed
/// ROW — POSITION-STABLE identity (the pane's chrome is bottom-
/// anchored: the card and location never move when content above
/// grows, so a plugin section landing inserts two clean keys instead
/// of churning every index-suffixed one; the tagged keys collide per
/// tag alone — every plugin-section header shares the tag — and the
/// row disambiguates). A same-key collision (two placed at one row)
/// falls back to an index suffix.
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

/// The pane's inner cell width (paddingLeft/Right 2 inside 42).
const PANE_TEXT_WIDTH: usize = SESSION_SIDEBAR_WIDTH;

/// Emit one placed line's chunks with the OLD painter's merge
/// semantics applied: ratatui's `Cell::set_style` MERGES into the
/// cell (fg/bg overwrite, modifiers OR through) — a unit painted over
/// an earlier unit's bold cells inherits the bold bit. The tree's
/// raster is pure last-writer-wins, so the bridge bakes the
/// accumulated attribute word into the chunks (per cell, via the
/// atom walk `paint_run` itself uses).
fn merged_line_chunks(
    line: &PlacedLine,
    attrs_grid: &[u32],
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
            let width = atom.width.max(1);
            let word = attrs_grid
                .get(top * PANE_TEXT_WIDTH + x)
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
            x += width;
        }
    }
    flush(&mut chunks, &mut pending);
    StyledText::new(chunks)
}

/// Collect the pane's paint units from the SHARED [`render_sidebar`]
/// walk — fills first, then placed lines (the For's DOM order is the
/// paint order: the old painter stamps every fill before any text,
/// and the card's fill always precedes its own glyphs). The text
/// units' attribute words are pre-merged through the same
/// accumulation order (see [`merged_line_chunks`]).
fn shape(view: &SidebarView, theme: &ThemeStyles, height: usize, scroll: usize) -> Vec<PaneItem> {
    let paint = crate::components::sidebar::render_sidebar_scrolled(view, theme, height, scroll);
    // Pass 1 — the attribute accumulation grid: every text unit ORs
    // its modifier word onto the cells it covers, in paint order
    // (fills carry no modifiers).
    let mut attrs_grid = vec![0u32; PANE_TEXT_WIDTH * height];
    for placed in &paint.placed {
        for (line_index, line) in placed.lines.iter().enumerate() {
            let top = placed.y + line_index;
            if top >= height {
                continue;
            }
            let mut x = placed.x;
            for run in &line.runs {
                let word = ecma_attributes(run.style.add_modifier);
                for atom in crate::wrap::flatten(&run.text, run.style) {
                    let width = atom.width.max(1);
                    for cell in x..(x + width).min(PANE_TEXT_WIDTH) {
                        if let Some(slot) = attrs_grid.get_mut(top * PANE_TEXT_WIDTH + cell) {
                            *slot |= word;
                        }
                    }
                    x += width;
                    if x >= PANE_TEXT_WIDTH {
                        break;
                    }
                }
            }
        }
    }
    // Pass 2 — the units (fills first, paint order). The keys are
    // position-stable (see [`placed_key`]).
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::with_capacity(paint.fills.len() + paint.placed.len());
    for (index, fill) in paint.fills.iter().enumerate() {
        items.push(PaneItem {
            key: format!("f:{index}"),
            left: usize::from(fill.x),
            top: usize::from(fill.y),
            body: PaneBody::Fill {
                width: usize::from(fill.width),
                height: usize::from(fill.height),
                color: rgb_of(fill.style.bg.unwrap_or(crate::style::Color::Reset)),
            },
        });
    }
    for (index, placed) in paint.placed.iter().enumerate() {
        for (line_index, line) in placed.lines.iter().enumerate() {
            let top = placed.y + line_index;
            items.push(PaneItem {
                key: placed_key(placed, top, index, &mut seen),
                left: placed.x,
                top,
                body: PaneBody::Text(merged_line_chunks(line, &attrs_grid, placed.x, top)),
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
fn item_element(inputs: &Signal<PaneInputs>, key: &str) -> Element {
    fn find_item<'a>(inputs: &'a PaneInputs, key: &str) -> Option<&'a PaneItem> {
        inputs.items.iter().find(|item| item.key == key)
    }
    let key = key.to_owned();
    let is_fill = inputs
        .get()
        .items
        .iter()
        .any(|item| item.key == key && matches!(item.body, PaneBody::Fill { .. }));
    if is_fill {
        fn fill_body(i: &PaneInputs, key: &str) -> Option<(usize, usize, Rgb)> {
            find_item(i, key).and_then(|item| match item.body {
                PaneBody::Fill { width, height, color } => Some((width, height, color)),
                PaneBody::Text(_) => None,
            })
        }
        box_()
            .position("absolute")
            .dyn_prop(
                "left",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| find_item(i, &key).map_or(0, |item| item.left)
                })),
            )
            .dyn_prop(
                "top",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| find_item(i, &key).map_or(0, |item| item.top)
                })),
            )
            .dyn_prop(
                "width",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| fill_body(i, &key).map_or(0, |(width, _, _)| width)
                })),
            )
            .dyn_prop(
                "height",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| fill_body(i, &key).map_or(0, |(_, height, _)| height)
                })),
            )
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| {
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
                    move |i: &PaneInputs| find_item(i, &key).map_or(0, |item| item.left)
                })),
            )
            .dyn_prop(
                "top",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| find_item(i, &key).map_or(0, |item| item.top)
                })),
            )
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), {
                    let key = key.clone();
                    move |i: &PaneInputs| {
                        find_item(i, &key).map_or_else(
                            || StyledText::new(Vec::new()),
                            |item| match &item.body {
                                PaneBody::Text(chunks) => chunks.clone(),
                                PaneBody::Fill { .. } => StyledText::new(Vec::new()),
                            },
                        )
                    }
                })),
            )
            .into_element()
    }
}

/// The pane mount: a positioned root carrying the raised background,
/// one flat keyed `For` of the pane's absolute units. Geometry bakes
/// at build time (the mount key); every content slot is a dyn prop
/// over the inputs signal. `left` is the root's offset inside its
/// parent (0 docked; the scrim container's right-aligned seat in the
/// manual-open overlay).
fn pane_element(inputs: &Signal<PaneInputs>, height: usize, raised: Rgb, left: usize) -> Element {
    let source = ForItemSource {
        source: map(inputs.clone(), |inputs: &PaneInputs| {
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
        .width(SESSION_SIDEBAR_WIDTH as f32)
        .height(height as f32)
        .position("absolute")
        .left(left as f32)
        .top(0.0)
        .background_color(hex(raised))
        .children_dyn(Rc::new(units))
        .into_element()
}

// ---------------------------------------------------------------------------
// the shared-tree mount (D3.2 sidebar lane — the D2.2 family paradigm)
// ---------------------------------------------------------------------------

/// The sidebar family's skeleton index under the shared tree's root
/// (`surface-sidebar`): 3, ABOVE the composer (2 — the old frame
/// pasted the pane after the prompt mount) and BELOW the footer (4)
/// and dialogs (5) — those pasted after `render_app` / after the
/// composite seam today, so they covered the pane. This lane bumps
/// footer/dialogs/overlays by one to open the seat; the relative
/// paint order is unchanged (bytes arbitrate).
pub(crate) const SIDEBAR_SKELETON_INDEX: usize = 3;

/// The mounted pane's reactive state, kept across frames so content
/// churn patches dyn-props in place (the M2 rule); a geometry-key
/// change (pane height, theme colors, seat, scrim mode) rebuilds the
/// wrapper's content subtree.
struct SidebarEntry {
    signal: Signal<PaneInputs>,
    key: (usize, PaneColors, usize, Option<usize>),
}

thread_local! {
    /// The sidebar family's content state (one mount per
    /// thread-process — the wrapper itself lives in the shared-tree
    /// hub, `chrome_solid::mount_family_wrapper`).
    static SIDEBAR_TREE: RefCell<Option<SidebarEntry>> = const { RefCell::new(None) };
}

/// The wrapper's content: the pane alone when docked, or — the ref's
/// manual-open shape (`alignItems="flex-end"` parent,
/// session-frame.tsx:385-397) — a full-scrim box carrying the pane at
/// its right-aligned seat. `scrim_width` is `Some` only in overlay
/// mode; the pane's `left` is then wrapper-relative.
fn sidebar_content(
    inputs: &Signal<PaneInputs>,
    height: usize,
    raised: Rgb,
    left: usize,
    scrim_width: Option<usize>,
) -> Element {
    match scrim_width {
        Some(width) => box_()
            .width(width as f32)
            .height(height as f32)
            .children(Children::Static(vec![pane_element(inputs, height, raised, left)]))
            .into_element(),
        None => pane_element(inputs, height, raised, 0),
    }
}

/// Sync the sidebar family into the shared tree (the family's ONLY
/// live mount entry — no Frame comes back; pixels reach the emitted
/// frame through `composite_chrome_tree`, the single compositor seam
/// inside `render_app`). `left`/`top` are the OLD pane paste site (the
/// imperative source); `scrim_width` is `Some(frame_width)` for the
/// ≤120 manual-open overlay, whose wrapper takes the full scrim rect
/// and carries a `70/255` `DimBg` stamp (the pane rect is its keep-rect
/// — the old dim-then-paste net bytes).
#[allow(clippy::too_many_arguments)] // the family mount seam (rect/pane/scrim)
pub fn sidebar_mount(
    view: &SidebarView,
    theme: &ThemeStyles,
    height: usize,
    scroll: usize,
    left: usize,
    top: usize,
    scrim_width: Option<usize>,
) {
    if height == 0 {
        // The old pane painter's degenerate shape: nothing mounts —
        // the stale wrapper unmounts at the seam.
        return;
    }
    let colors = PaneColors::of(theme);
    let items = shape(view, theme, height, scroll);
    let key = (height, colors, left, scrim_width);
    // Rebuild iff the geometry key changed OR the wrapper left the
    // tree (a hub reset between harness frames, a seam prune).
    let rebuild = !family_wrapper_live("sidebar")
        || SIDEBAR_TREE.with_borrow(|slot| !matches!(slot, Some(entry) if entry.key == key));
    let (content, content_key) = if rebuild {
        let signal = Signal::new(PaneInputs { items: items.clone() });
        let element = sidebar_content(&signal, height, colors.raised, left, scrim_width);
        SIDEBAR_TREE.with_borrow_mut(|slot| {
            *slot = Some(SidebarEntry { signal, key });
        });
        (Some(element), Some(format!("sidebar-content@{height}x{left}x{scrim_width:?}@{colors:?}")))
    } else {
        SIDEBAR_TREE.with_borrow(|slot| {
            slot.as_ref().expect("checked above").signal.set(PaneInputs { items });
        });
        (None, None)
    };
    let (wrapper_left, wrapper_width) = match scrim_width {
        Some(width) => (0, width),
        None => (left, SESSION_SIDEBAR_WIDTH),
    };
    let stamps: Vec<ChromeStamp> = scrim_width
        .map(|width| ChromeStamp::DimBg {
            left: 0,
            top: 0,
            width,
            height,
            alpha: 70,
            keep_left: left,
            keep_top: 0,
            keep_width: SESSION_SIDEBAR_WIDTH,
            keep_height: height,
        })
        .into_iter()
        .collect();
    mount_family_wrapper(
        "sidebar",
        SIDEBAR_SKELETON_INDEX,
        wrapper_left,
        top,
        wrapper_width,
        height,
        content_key,
        content,
        &stamps,
        &[],
        false,
    );
}

/// The golden/unit harness twin (the transcript/footer shape): mount
/// ONE docked pane into a fresh hub and read it back as a standalone
/// Frame (composite over a blank frame of its own rect) — same raster,
/// same cell semantics, by construction.
#[must_use]
pub fn paint_solid_sidebar_pane(
    view: &SidebarView,
    theme: &ThemeStyles,
    height: usize,
    scroll: usize,
) -> Frame {
    if height == 0 {
        // The old pane painter's degenerate shape.
        return Frame {
            cells: vec![Vec::new(); height],
            width: SESSION_SIDEBAR_WIDTH,
            height,
            clusters: None,
        };
    }
    crate::components::chrome_solid::reset_hub();
    sidebar_mount(view, theme, height, scroll, 0, 0, None);
    let mut frame = crate::components::chrome_solid::blank_frame(SESSION_SIDEBAR_WIDTH, height);
    crate::components::chrome_solid::composite_chrome_tree(&mut frame);
    frame
}

/// The pane frame's renderer seam (headless twin): the pane renders
/// through its opentui-solid tree — the only renderer since the
/// retirement lane deleted the hand-rolled placed-runs fallback.
/// `scroll` is the content slot's wheel offset (auditA #1) — the
/// render re-slices the sections; the title/footer chrome stays put.
#[must_use]
pub fn sidebar_pane_frame(
    view: &SidebarView,
    theme: &ThemeStyles,
    height: usize,
    scroll: usize,
) -> Frame {
    paint_solid_sidebar_pane(view, theme, height, scroll)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::sidebar::{SidebarMode, SidebarSection};
    use crate::render::{Cell, CellColor};
    use crate::theme::ThemeState;

    /// The reactive path patches in place: a title change + a plugin
    /// section landing re-render the SAME mount (same pane geometry) —
    /// an absolute self-check since the retirement.
    #[test]
    fn content_churn_patches_the_mounted_pane() {
        let theme = ThemeState::v2_dark().styles();
        let mut view = SidebarView::fixture("First title", "/w:main");
        let mut seen: Vec<(String, Frame)> = Vec::new();
        for title in ["First title", "Renamed", "First title"] {
            view.title = title.to_owned();
            let frame = paint_solid_sidebar_pane(&view, &theme, 30, 0);
            if let Some((_, prior)) = seen.iter().find(|(t, _)| *t == view.title) {
                assert_eq!(&frame, prior, "same inputs paint the same pane: {title:?}");
            }
            seen.push((view.title.clone(), frame));
        }
        view.sections.plugins = vec![SidebarSection {
            plugin_id: "omos".into(),
            title: "Preset".into(),
            rows: vec!["zhipuai · own/glm-5.3".into()],
            clickable: Vec::new(),
        }];
        let with_plugin = paint_solid_sidebar_pane(&view, &theme, 30, 0);
        let grid = with_plugin.text_grid().join("\n");
        assert!(grid.contains("Preset"), "the plugin section paints: {grid}");
        assert!(family_wrapper_live("sidebar"), "the pane rides the shared-tree wrapper");
    }

    /// A resize rebuilds cleanly — the mount-key rule (the pane's
    /// height keys the mount; absolute self-check since the
    /// retirement).
    #[test]
    fn reshape_rebuilds_cleanly() {
        let theme = ThemeState::v2_dark().styles();
        let view = SidebarView::fixture("Resize", "/w:main");
        let mut first: Option<Frame> = None;
        for height in [30usize, 24, 40, 30] {
            let frame = paint_solid_sidebar_pane(&view, &theme, height, 0);
            assert_eq!(frame.height, height, "full-height pane at height {height}");
            if height == 30 {
                match &first {
                    None => first = Some(frame),
                    Some(p) => assert_eq!(&frame, p, "same inputs paint the same pane"),
                }
            }
        }
    }

    /// D3.2 construction equivalence, docked: the wrapper's pane rect
    /// carries exactly the retired pane frame's cells, and nothing
    /// outside the 42-wide rect is written (the old right-edge paste).
    #[test]
    fn docked_pane_rect_equals_the_pane_frame() {
        let theme = ThemeState::v2_dark().styles();
        let view = SidebarView::fixture("Docked", "/w:main");
        let pane = paint_solid_sidebar_pane(&view, &theme, 24, 0);
        crate::components::chrome_solid::reset_hub();
        let mut frame = crate::components::chrome_solid::blank_frame(100, 30);
        sidebar_mount(&view, &theme, 24, 0, 50, 5, None);
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);
        for y in 0..24 {
            for x in 0..SESSION_SIDEBAR_WIDTH {
                assert_eq!(frame.cells[5 + y][50 + x], pane.cells[y][x], "pane cell ({x},{y})");
            }
        }
        for (y, row) in frame.cells.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                let inside = (5..29).contains(&y) && (50..92).contains(&x);
                assert!(
                    inside || *cell == Cell::default(),
                    "docked mount wrote outside its rect at ({x},{y}): {cell:?}"
                );
            }
        }
    }

    /// D3.2 scrim byte equivalence, overlay: every covered cell outside
    /// the pane dims at `70/255` exactly like the retired
    /// `session.rs::apply_scrim` walk (RGB bg blend, glyph/fg survive),
    /// the pane rect keeps the retired pane frame's bytes, and cells
    /// outside the scrim rect are untouched.
    #[test]
    fn overlay_scrim_matches_the_retired_blend_and_keeps_the_pane() {
        let theme = ThemeState::v2_dark().styles();
        let view = SidebarView::fixture("Overlay", "/w:main");
        let (width, frame_height, top, height, left) = (60usize, 24usize, 2usize, 20usize, 18usize);
        let pane = paint_solid_sidebar_pane(&view, &theme, height, 0);

        // A base frame with a deterministic RGB checkerboard (the
        // "route content" the old scrim dimmed in place).
        let mut frame = crate::components::chrome_solid::blank_frame(width, frame_height);
        for (y, row) in frame.cells.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                cell.ch = '.';
                cell.fg = CellColor::Rgb(200, 30, 40);
                cell.bg = CellColor::Rgb(
                    u8::try_from((x * 7) % 256).unwrap(),
                    u8::try_from((y * 11) % 256).unwrap(),
                    u8::try_from((x * 13 + y * 3) % 256).unwrap(),
                );
            }
        }
        let before = frame.clone();

        crate::components::chrome_solid::reset_hub();
        sidebar_mount(&view, &theme, height, 0, left, top, Some(width));
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);

        // The retired apply_scrim formula, verbatim.
        let dim = |channel: u8| -> u8 {
            let alpha = 70.0 / 255.0;
            (f64::from(channel) * (1.0 - alpha)).round() as u8
        };
        for row in top..top + height {
            for col in 0..width {
                if (left..left + SESSION_SIDEBAR_WIDTH).contains(&col) {
                    let expected = pane.cells[row - top][col - left];
                    assert_eq!(frame.cells[row][col], expected, "pane byte ({col},{row})");
                    continue;
                }
                let original = before.cells[row][col];
                let CellColor::Rgb(r, g, b) = original.bg else { unreachable!() };
                assert_eq!(
                    frame.cells[row][col],
                    Cell {
                        ch: '.',
                        attrs: 0,
                        fg: original.fg,
                        bg: CellColor::Rgb(dim(r), dim(g), dim(b)),
                    },
                    "scrim byte ({col},{row})"
                );
            }
        }
        // Outside the scrim rect the base is untouched.
        assert_eq!(frame.cells[0][0], before.cells[0][0]);
        assert_eq!(
            frame.cells[frame_height - 1][width - 1],
            before.cells[frame_height - 1][width - 1]
        );
    }

    /// The visibility rule stays the old module's (auto > 120, hide
    /// never) — the solid path is a paint-only change.
    #[test]
    fn visibility_rule_is_unchanged() {
        use crate::components::sidebar::sidebar_visible;
        assert!(sidebar_visible(SidebarMode::Auto, 160));
        assert!(!sidebar_visible(SidebarMode::Auto, 120));
        assert!(!sidebar_visible(SidebarMode::Hide, 160));
    }
}
