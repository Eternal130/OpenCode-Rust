//! The footer family on the shared tree (D2.2 footer lane — the
//! chrome/composer/transcript paradigm, ruling attempt 2 plan (e)).
//!
//! The footer painters are op-shaped (they paint through
//! [`Canvas`]'s `fill_bg`/`spans`/`spans_right` vocabulary), so the
//! bridge is ONE shared substrate: the recorded op stream is shaped
//! into pre-placed paint units (ONE 1-row fill BOX per `fill_bg`,
//! one `wrapMode none` + `flexShrink 0` text per `spans` line, in
//! PAINT ORDER — the raster z-orders siblings last-wins, exactly the
//! Canvas's overwrite semantics) and mounted as the `surface-footer`
//! family wrapper's content subtree. Pixels reach the emitted
//! oc-tui [`Frame`] through the ONE full-buffer compositor seam
//! (`composite_chrome_tree` inside `render_app`); the old one-shot
//! scratch renderer (`canvas_frame_solid` + its private
//! `SolidRenderer` + `frame_from_buffer` copy) is retired.
//!
//! * **Transparent regions** (`clear_bg` — the composer textarea's
//!   canonical bg `49`) cannot be expressed in the tree: they ride
//!   as a [`ChromeStamp::Transparent`] seam stamp (blank cells inside
//!   the rect reset their bg to Default — exactly the retired
//!   bridge's post-copy fixup).
//! * **Non-RGB colors** (indexed/default paints) are unreachable for
//!   this family — the footer palette is all-RGB
//!   (`FooterTheme::from_resolved`); an untranslatable op mounts
//!   nothing (D3.2 retired the defensive hand-rolled replay with the
//!   seam's frame-overlay slot).
//! * The MAIN footer row (the prompt composer's hint row) is the
//!   composer family's wrapper (the composer lane, M3) — this lane
//!   owns the family's BLOCKER bodies (permission/form) and the
//!   picker PANEL bodies that the old `App::frame` pasted over the
//!   composed frame.
//!
//! Parity: every `*_canvas` builder in the family keeps a
//! cell-for-cell unit gate in this module's tests
//! (`solid_footer_*_equals_the_old_renderer`) over the golden
//! fixtures' scenario matrices, through the headless twin seam.

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
use opentui_solid::element::ForItem;
use opentui_solid::element::PropValue;
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal};

use super::canvas::{Canvas, CanvasOp, FILL_BG};
use crate::components::chrome_solid::{
    composite_chrome_tree, family_wrapper_live, mount_family_wrapper, reset_hub, ChromeStamp,
};
use crate::render::{CellColor, Frame};

// ---------------------------------------------------------------------------
// colors + chunk helpers
// ---------------------------------------------------------------------------

/// The dump-harness fill as an opentui color.
fn fill_bg() -> Rgba {
    let CellColor::Rgb(r, g, b) = FILL_BG else { return Rgba::rgb(0, 0, 0) };
    Rgba::rgb(r, g, b)
}

/// A `#rrggbb` prop string (the builder's color currency).
fn hex(color: Rgba) -> String {
    let channel = |index: usize| u8::try_from(color.0[index]).unwrap_or(0);
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

fn rgba(color: CellColor) -> Option<Rgba> {
    match color {
        CellColor::Rgb(r, g, b) => Some(Rgba::rgb(r, g, b)),
        // Indexed/default paints keep the hand-rolled replay (the
        // footer palette is all-RGB; this is defensive).
        CellColor::Indexed(_) | CellColor::Default => None,
    }
}

// ---------------------------------------------------------------------------
// inputs (the op stream → paint units)
// ---------------------------------------------------------------------------

/// One placed paint unit — a pre-split styled LINE (text) or a
/// 1-row background FILL — at body-local absolute coordinates, in
/// paint order (the raster z-orders siblings last-wins = the
/// Canvas's overwrite semantics).
#[derive(Debug, Clone)]
struct FooterUnit {
    /// `f{op}` fill / `t{op}` text / `r{op}.{run}` restore — stable
    /// across frames for the same op index (the For reconciles on
    /// it; a body change patches content under the same keys).
    key: String,
    left: usize,
    top: usize,
    body: UnitBody,
}

#[derive(Debug, Clone)]
enum UnitBody {
    Text(StyledText),
    Fill { width: usize, color: Rgba },
}

/// Shape the Canvas op stream into the wrapper's paint units plus
/// the seam stamps (transparent `clear_bg` rects — the retired
/// bridge's post-copy fixup, kept as stamps so the tree stays
/// declarative). `None` = an untranslatable color keeps the
/// hand-rolled replay (never triggered by the family's palette).
fn shape_ops(ops: &[CanvasOp]) -> Option<(Vec<FooterUnit>, Vec<ChromeStamp>)> {
    // The MERGE shadow (the M5 sidebar rule): the old `fill_bg` only
    // overwrites the CELL BACKGROUND — glyphs an earlier span painted
    // under the rect survive with their fg/attrs. The raster's box
    // fill overwrites whole cells, so surviving glyphs are re-emit as
    // a text element AFTER the fill (same DOM order trick as M2's
    // footer-overwrites-title). The shadow tracks the glyph cells in
    // paint order (a later span REPLACES an earlier glyph).
    let mut glyphs: std::collections::HashMap<(usize, usize), (char, CellColor, u32)> =
        std::collections::HashMap::new();
    let note_span =
        |x: usize,
         y: usize,
         spans: &[super::canvas::Span],
         glyphs: &mut std::collections::HashMap<(usize, usize), (char, CellColor, u32)>| {
            let mut x = x;
            for span in spans {
                for atom in crate::wrap::flatten(&span.text, crate::style::Style::new()) {
                    glyphs.insert(
                        (x, y),
                        (atom.text.chars().next().unwrap_or(' '), span.fg, u32::from(span.bold)),
                    );
                    x += atom.width.max(1);
                }
            }
        };
    let restore_chunks =
        |x0: usize,
         y: usize,
         w: usize,
         glyphs: &std::collections::HashMap<(usize, usize), (char, CellColor, u32)>,
         bg: CellColor|
         -> Vec<(usize, StyledText)> {
            // Contiguous runs of surviving glyphs, each one text element
            // (explicit chunk bg = the fill's bg).
            let Some(rgba_bg) = rgba(bg) else { return Vec::new() };
            let mut runs: Vec<(usize, StyledText)> = Vec::new();
            let mut index = x0;
            while index < x0 + w {
                let Some(&(_ch, fg, attrs)) = glyphs.get(&(index, y)) else {
                    index += 1;
                    continue;
                };
                let start = index;
                let mut chars = String::new();
                let run_fg = fg;
                let run_attrs = attrs;
                while index < x0 + w {
                    match glyphs.get(&(index, y)) {
                        Some(&(_, next_fg, next_attrs))
                            if next_fg == run_fg && next_attrs == run_attrs =>
                        {
                            chars.push(glyphs[&(index, y)].0);
                            index += 1;
                        }
                        Some(_) | None => break,
                    }
                }
                if let Some(rgba_fg) = rgba(run_fg) {
                    runs.push((
                        start,
                        StyledText::new(vec![TextChunk {
                            text: chars,
                            fg: Some(rgba_fg),
                            bg: Some(rgba_bg),
                            attributes: run_attrs,
                            link: None,
                        }]),
                    ));
                }
            }
            runs
        };
    let mut units: Vec<FooterUnit> = Vec::with_capacity(ops.len());
    let mut stamps: Vec<ChromeStamp> = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        match op {
            CanvasOp::Fill { y, x, width: w, bg } => {
                if *bg == CellColor::Default {
                    // A transparent region: the tree paints nothing;
                    // the seam stamp resets blank cells' bg to Default
                    // (the retired bridge's fixup, verbatim).
                    stamps.push(ChromeStamp::Transparent { left: *x, top: *y, width: *w });
                } else {
                    let Some(color) = rgba(*bg) else { return None };
                    units.push(FooterUnit {
                        key: format!("f{index}"),
                        left: *x,
                        top: *y,
                        body: UnitBody::Fill { width: *w, color },
                    });
                }
                // The surviving glyphs ride OVER the fill (the old
                // `fill_bg`'s bg-only merge).
                for (run, (start, content)) in
                    restore_chunks(*x, *y, *w, &glyphs, *bg).into_iter().enumerate()
                {
                    units.push(FooterUnit {
                        key: format!("r{index}.{run}"),
                        left: start,
                        top: *y,
                        body: UnitBody::Text(content),
                    });
                }
            }
            CanvasOp::Spans { y, x, spans } => {
                let mut chunks: Vec<TextChunk> = Vec::new();
                for span in spans {
                    let (Some(fg), Some(bg)) = (rgba(span.fg), rgba(span.bg)) else {
                        return None;
                    };
                    chunks.push(TextChunk {
                        text: span.text.clone(),
                        fg: Some(fg),
                        bg: Some(bg),
                        // ECMA bit 0 = bold (the bridge's frame_attrs
                        // inverse).
                        attributes: u32::from(span.bold),
                        link: None,
                    });
                }
                note_span(*x, *y, spans, &mut glyphs);
                units.push(FooterUnit {
                    key: format!("t{index}"),
                    left: *x,
                    top: *y,
                    body: UnitBody::Text(StyledText::new(chunks)),
                });
            }
        }
    }
    Some((units, stamps))
}

/// The defensive fallback was retired with the seam's frame-overlay
/// slot (D3.2): the op stream never carries a non-RGB color.

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

/// `dyn_(src, to PropValue::Color)` — an Rgb-valued dynamic prop.
fn dyn_color<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<Rgba> + 'static,
{
    opentui_solid::dyn_(source, |value: &Rgba| PropValue::Color(hex(*value)))
}

/// `dyn_(src, to PropValue::Styled)` — styled-chunk content.
fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

/// One `For` item element: a text unit (absolute, natural width,
/// pre-wrapped content — `wrapMode none` + `flexShrink 0` so nothing
/// re-wraps or shrinks) or a fill rect (absolute box, explicit
/// points), in paint order. The unit's KIND is baked into its key
/// (`f`/`t`/`r` prefix) — stable per identity, so the element type
/// is chosen once at mount.
fn unit_element(inputs: &Signal<Vec<FooterUnit>>, key: &str) -> Element {
    let key = key.to_owned();
    let is_fill = inputs
        .get()
        .iter()
        .any(|unit| unit.key == key && matches!(unit.body, UnitBody::Fill { .. }));
    let left_of = {
        let key = key.clone();
        move |units: &Vec<FooterUnit>| {
            units.iter().find(|unit| unit.key == key).map_or(0, |unit| unit.left)
        }
    };
    let top_of = {
        let key = key.clone();
        move |units: &Vec<FooterUnit>| {
            units.iter().find(|unit| unit.key == key).map_or(0, |unit| unit.top)
        }
    };
    if is_fill {
        box_()
            .position("absolute")
            .dyn_prop("left", dyn_num(map(inputs.clone(), left_of)))
            .dyn_prop("top", dyn_num(map(inputs.clone(), top_of)))
            .dyn_prop(
                "width",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |units: &Vec<FooterUnit>| {
                        units.iter().find(|unit| unit.key == key).map_or(0, |unit| {
                            match &unit.body {
                                UnitBody::Fill { width, .. } => *width,
                                UnitBody::Text(_) => 0,
                            }
                        })
                    }
                })),
            )
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), {
                    let key = key.clone();
                    move |units: &Vec<FooterUnit>| {
                        units.iter().find(|unit| unit.key == key).map_or(fill_bg(), |unit| {
                            match &unit.body {
                                UnitBody::Fill { color, .. } => *color,
                                UnitBody::Text(_) => fill_bg(),
                            }
                        })
                    }
                })),
            )
            .into_element()
    } else {
        text()
            .position("absolute")
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop("left", dyn_num(map(inputs.clone(), left_of)))
            .dyn_prop("top", dyn_num(map(inputs.clone(), top_of)))
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), {
                    let key = key.clone();
                    move |units: &Vec<FooterUnit>| {
                        units.iter().find(|unit| unit.key == key).map_or_else(
                            || StyledText::new(Vec::new()),
                            |unit| match &unit.body {
                                UnitBody::Text(chunks) => chunks.clone(),
                                UnitBody::Fill { .. } => StyledText::new(Vec::new()),
                            },
                        )
                    }
                })),
            )
            .into_element()
    }
}

/// The footer body's content subtree: a positioned root carrying
/// the canvas's terminal fill (the dump-harness base — every cell
/// inside the wrapper's rect is covered ink, never a hole), one
/// flat keyed `For` of the body's absolute units. Geometry bakes at
/// build time (the mount key); every content slot is a dyn prop
/// over the inputs signal.
fn footer_element(inputs: &Signal<Vec<FooterUnit>>, width: usize, height: usize) -> Element {
    let source = ForItemSource {
        source: map(inputs.clone(), |units: &Vec<FooterUnit>| {
            units
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
        .width(width as f32)
        .height(height as f32)
        .position("relative")
        .background_color(hex(fill_bg()))
        .children_dyn(Rc::new(units))
        .into_element()
}

// ---------------------------------------------------------------------------
// the shared-tree mount (D2.2 footer lane — the chrome/composer/
// transcript paradigm)
// ---------------------------------------------------------------------------

/// The footer family's seat under the shared tree's root (the
/// skeleton table's index 4, after the sidebar): the bodies the
/// old `App::frame` pasted AFTER `render_app` (over the composed
/// chrome/transcript/composer/sidebar pixels), so the wrapper sits above
/// all four. Registered deviation (bytes arbitrate): the family-6
/// overlays (console/which-key/toast) paste AFTER the composite seam
/// and so paint OVER the footer bodies (today's paste order had the
/// bodies last); no golden or PTY corpus observes the inverted cells,
/// and the dialogs lane (family 5) now seats its wrapper above this
/// one by the same construction.
const FOOTER_SKELETON_INDEX: usize = 4;

/// The mounted footer body's reactive state, kept across frames so
/// content changes (a moving selection, typed reject text) patch
/// content dyn-props in place (the M2 rule); a geometry-key change
/// (width × mount height — the resize ladder) rebuilds the
/// wrapper's content subtree.
struct FooterEntry {
    signal: Signal<Vec<FooterUnit>>,
    key: (usize, usize),
}

thread_local! {
    /// The footer family's content state (one mount per
    /// thread-process — the wrapper itself lives in the shared-tree
    /// hub, `chrome_solid::mount_family_wrapper`).
    static FOOTER_TREE: RefCell<Option<FooterEntry>> = const { RefCell::new(None) };
}

/// Sync the footer family's live body into the shared tree (the
/// family's ONLY live mount entry — no Frame comes back; pixels
/// reach the emitted frame through `composite_chrome_tree`, the
/// single compositor seam inside `render_app`). The wrapper rect is
/// the OLD paste site (`(0, size.1 - mount_height)`, the imperative
/// source; resize patches via `set_prop`); the content subtree
/// rebuilds only when the geometry key (width × mount height)
/// changes. NOT calling this for a frame unmounts the wrapper at
/// the seam (the old paste branches' else arms).
pub(crate) fn mount_footer_ops(
    ops: &[CanvasOp],
    width: usize,
    height: usize,
    left: usize,
    top: usize,
) {
    let Some((units, stamps)) = shape_ops(ops) else {
        // The footer palette is all-RGB (`FooterTheme::from_resolved`),
        // so an untranslatable op is unreachable; nothing mounts (the
        // seam prunes the stale wrapper). D3.2 retired the defensive
        // hand-rolled replay with the seam's frame-overlay slot.
        return;
    };
    let inputs = units;
    let key = (width, height);
    // Rebuild iff the geometry key changed OR the wrapper left the
    // tree (a hub reset between harness frames, a seam prune).
    let rebuild = !family_wrapper_live("footer")
        || FOOTER_TREE.with_borrow(|slot| !matches!(slot, Some(entry) if entry.key == key));
    let (content, content_key) = if rebuild {
        let signal = Signal::new(inputs.clone());
        let element = footer_element(&signal, width, height);
        FOOTER_TREE.with_borrow_mut(|slot| {
            *slot = Some(FooterEntry { signal, key });
        });
        (Some(element), Some(format!("footer-content@{width}x{height}")))
    } else {
        FOOTER_TREE.with_borrow(|slot| {
            slot.as_ref().expect("checked above").signal.set(inputs);
        });
        (None, None)
    };
    mount_family_wrapper(
        "footer",
        FOOTER_SKELETON_INDEX,
        left,
        top,
        width,
        height,
        content_key,
        content,
        &stamps,
        &[],
        false,
    );
}

/// The family's dispatch seam: every `render_*` fn in the footer
/// family finishes through here — the HEADLESS twin (the golden/
/// unit/mini harness shape): mount ONE body into a fresh hub and
/// read it back as a standalone Frame (composite over a blank frame
/// of its own rect) — same raster, same cell semantics, by
/// construction.
#[must_use]
pub fn finish(canvas: Canvas) -> Frame {
    let (width, height) = canvas.dims();
    ops_frame(canvas.ops(), width, height)
}

/// [`finish`]'s op-level entry (the parity gates' and the frame
/// budget's instrument).
#[must_use]
pub(crate) fn ops_frame(ops: &[CanvasOp], width: usize, height: usize) -> Frame {
    if width == 0 || height == 0 {
        return Frame { cells: vec![vec![]; height], width, height, clusters: None };
    }
    reset_hub();
    mount_footer_ops(ops, width, height, 0, 0);
    let mut frame = crate::components::chrome_solid::blank_frame(width, height);
    composite_chrome_tree(&mut frame);
    frame
}

#[cfg(test)]
use crate::render::Cell;

/// Compare two frames cell for cell; on divergence, report the first
/// differing cell (the parity gates' instrument).
#[cfg(test)]
pub(crate) fn first_divergence(old: &Frame, solid: &Frame) -> Option<(usize, usize, Cell, Cell)> {
    if (old.width, old.height) != (solid.width, solid.height) {
        return Some((usize::MAX, usize::MAX, Cell::default(), Cell::default()));
    }
    for (y, (old_row, solid_row)) in old.cells.iter().zip(&solid.cells).enumerate() {
        for (x, (a, b)) in old_row.iter().zip(solid_row).enumerate() {
            if a != b {
                return Some((x, y, *a, *b));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// the parity gates — cell for cell against the hand-rolled painters,
// over the golden fixtures' scenario matrices (lane R2 step 2), now
// through the shared-tree twin seam
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::form::{form_body_canvas, FormBodyState};
    use super::super::form_authorization::{form_authorization_canvas, FormAuthorizationState};
    use super::super::panel::{FooterMenuState, PANEL_LIST_ROWS};
    use super::super::panels::{MiniSettings, RunAgent, RunModel, RunProvider};
    use super::super::permission::{
        create_permission_body_state, permission_body_canvas, reject_field_canvas,
        PermissionBodyState, PermissionOption, PermissionRequest, PermissionStage,
    };
    use super::super::prompt_idle::{prompt_idle_canvas, PromptIdleState};
    use super::super::state::{
        resolve_mini_settings, FieldKind, FormField, FormOption, FormRequest,
    };
    use super::super::theme::FooterTheme;
    use super::*;

    #[allow(clippy::needless_pass_by_value)] // consumes the canvas: dims + ops + old frame
    fn assert_parity(id: &str, canvas: Canvas) {
        let (width, height) = canvas.dims();
        let old = canvas.clone().into_frame();
        let solid = ops_frame(canvas.ops(), width, height);
        assert_eq!(
            (old.width, old.height),
            (solid.width, solid.height),
            "{id}: dims {width}x{height}"
        );
        match first_divergence(&old, &solid) {
            None => {}
            Some((x, y, a, b)) => {
                panic!("{id} @{width}x{height} diverged at ({x},{y}): old {a:?} vs solid {b:?}")
            }
        }
    }

    fn permission(action: &str) -> PermissionRequest {
        PermissionRequest {
            id: "per_r2".into(),
            session_id: "ses_r2".into(),
            action: action.into(),
            resources: vec!["src/index.ts".into()],
            ..PermissionRequest::default()
        }
    }

    #[test]
    fn solid_footer_permission_body_equals_the_old_renderer() {
        let theme = FooterTheme::fallback_dark();
        let mut edit = permission("edit");
        edit.metadata = serde_json::json!({
            "files": [ { "diff": "@@ -1,2 +1,2 @@\n const old = 1\n-const gone = 2\n+const fresh = 3\n" } ]
        });
        let mut always = permission("read");
        always.save = vec!["src/**".into()];
        let rejecting = PermissionBodyState {
            stage: PermissionStage::Reject,
            selected: PermissionOption::Reject,
            message: "no".into(),
            ..create_permission_body_state(&permission("read"))
        };
        let selected_always = PermissionBodyState {
            selected: PermissionOption::Always,
            ..create_permission_body_state(&always)
        };
        for request in [&permission("read"), &permission("grep"), &edit, &always] {
            let state = create_permission_body_state(request);
            // NOTE: heights ≥3 only — the old painter has a latent
            // `panel_end - 2` underflow below that (production never
            // mounts the body smaller; left untouched, the parity
            // contract is the painter AS IS).
            for width in [80usize, 160, 56, 40, 24] {
                for height in [16usize, 12, 8, 4, 3] {
                    assert_parity(
                        &format!("permission {} @{width}x{height}", request.action),
                        permission_body_canvas(request, &state, width, height, &theme),
                    );
                }
            }
        }
        for state in [&rejecting, &selected_always] {
            for width in [80usize, 160, 40] {
                assert_parity(
                    &format!("permission stage @{width}"),
                    permission_body_canvas(&always, state, width, 16, &theme),
                );
            }
        }
    }

    #[test]
    fn solid_footer_reject_field_equals_the_old_renderer() {
        let theme = FooterTheme::fallback_dark();
        for visible in [true, false] {
            for width in [80usize, 160, 40] {
                for height in [18usize, 1, 0] {
                    assert_parity(
                        &format!("reject-field {visible} @{width}x{height}"),
                        reject_field_canvas(visible, width, height, &theme),
                    );
                }
            }
        }
    }

    fn deployment_form() -> FormRequest {
        FormRequest {
            id: "frm_r2".into(),
            session_id: "ses".into(),
            title: "Deployment".into(),
            fields: vec![FormField::text("service", "Service"), FormField::text("notes", "Notes")],
        }
    }

    #[test]
    fn solid_footer_form_body_equals_the_old_renderer() {
        let theme = FooterTheme::fallback_dark();
        let request = deployment_form();
        let multiselect = FormRequest {
            id: "m".into(),
            session_id: "s".into(),
            title: "Pick".into(),
            fields: vec![FormField {
                key: "k".into(),
                title: Some("K".into()),
                description: None,
                required: true,
                kind: FieldKind::Multiselect {
                    options: vec![
                        FormOption { value: "a".into(), label: "Alpha".into(), description: None },
                        FormOption { value: "b".into(), label: "Beta".into(), description: None },
                    ],
                    custom: Some(true),
                },
            }],
        };
        let review = FormBodyState { field: 2, ..FormBodyState::new(&request) };
        let editing = FormBodyState {
            editing: true,
            draft: "custom".into(),
            ..FormBodyState::new(&multiselect)
        };
        for (id, request, state) in [
            ("textual", &request, &FormBodyState::new(&request)),
            ("review", &request, &review),
            ("multiselect", &multiselect, &FormBodyState::new(&multiselect)),
            ("editing", &multiselect, &editing),
        ] {
            for width in [80usize, 160, 56, 40] {
                for height in [16usize, 12, 8, 2, 1] {
                    assert_parity(
                        &format!("form {id} @{width}x{height}"),
                        form_body_canvas(request, state, width, height, &theme),
                    );
                }
            }
        }
    }

    #[test]
    fn solid_footer_form_authorization_equals_the_old_renderer() {
        let theme = FooterTheme::fallback_dark();
        let request = FormRequest {
            id: "frm_ext".into(),
            session_id: "ses".into(),
            title: "Authorization required".into(),
            fields: vec![FormField {
                key: "authorization".into(),
                title: Some("Authorize access".into()),
                description: None,
                required: false,
                kind: FieldKind::External { url: "https://example.com/authorize".into() },
            }],
        };
        for state in [
            FormAuthorizationState::new(),
            FormAuthorizationState {
                external_ready: true,
                acknowledged: true,
                ..FormAuthorizationState::new()
            },
        ] {
            for width in [80usize, 160, 40] {
                for height in [20usize, 12, 3] {
                    assert_parity(
                        &format!("authorization @{width}x{height}"),
                        form_authorization_canvas(&request, &state, width, height, &theme),
                    );
                }
            }
        }
    }

    #[test]
    fn solid_footer_prompt_idle_equals_the_old_renderer() {
        let theme = FooterTheme::fallback_dark();
        let state = PromptIdleState::default();
        let with_status =
            PromptIdleState { status_detail: "3 subagents".into(), ..PromptIdleState::default() };
        for state in [&state, &with_status] {
            for width in [80usize, 160, 40, 20] {
                for height in [12usize, 2, 1] {
                    assert_parity(
                        &format!("prompt-idle @{width}x{height}"),
                        prompt_idle_canvas(state, width, height, &theme),
                    );
                }
            }
        }
    }

    #[test]
    fn solid_footer_panels_equals_the_old_renderer() {
        let theme = FooterTheme::fallback_dark();
        let providers = vec![RunProvider {
            id: "opencode".into(),
            name: "opencode".into(),
            models: vec![
                RunModel {
                    id: "gpt-5".into(),
                    name: "GPT-5".into(),
                    cost_input: 1.0,
                    deprecated: false,
                },
                RunModel {
                    id: "free".into(),
                    name: "GPT Free".into(),
                    cost_input: 0.0,
                    deprecated: false,
                },
                RunModel {
                    id: "old".into(),
                    name: "Old".into(),
                    cost_input: 1.0,
                    deprecated: true,
                },
            ],
        }];
        let agents = vec![
            RunAgent {
                id: "build".into(),
                name: "Build".into(),
                description: Some("Build software".into()),
                mode: "all".into(),
                hidden: false,
            },
            RunAgent {
                id: "review".into(),
                name: "Review".into(),
                description: None,
                mode: "primary".into(),
                hidden: false,
            },
        ];
        let variants = vec!["high".to_owned(), "minimal".to_owned()];
        let settings: MiniSettings = resolve_mini_settings();
        let _ = PANEL_LIST_ROWS;
        let _ = FooterMenuState::new();
        for width in [80usize, 160, 56, 40] {
            for height in [17usize, 16, 8, 1] {
                assert_parity(
                    &format!("command-menu @{width}x{height}"),
                    super::super::panels::command_menu_canvas(width, height, &theme),
                );
                assert_parity(
                    &format!("model-select @{width}x{height}"),
                    super::super::panels::model_select_canvas(
                        &providers,
                        Some(("opencode", "gpt-5")),
                        width,
                        height,
                        &theme,
                    ),
                );
                assert_parity(
                    &format!("agent-select @{width}x{height}"),
                    super::super::panels::agent_select_canvas(
                        &agents,
                        Some("review"),
                        width,
                        height,
                        &theme,
                    ),
                );
                assert_parity(
                    &format!("variant-select @{width}x{height}"),
                    super::super::panels::variant_select_canvas(
                        &variants,
                        Some("high"),
                        width,
                        height,
                        &theme,
                    ),
                );
                assert_parity(
                    &format!("settings @{width}x{height}"),
                    super::super::panels::settings_canvas(&settings, width, height, &theme),
                );
            }
        }
    }

    // -- the D2.2 seam gates (the chrome/composer/transcript set) ----

    fn permission_canvas() -> Canvas {
        let theme = FooterTheme::fallback_dark();
        let request = permission("edit");
        let state = create_permission_body_state(&request);
        permission_body_canvas(&request, &state, 80, 16, &theme)
    }

    /// D2.2's mixed-window hole rule, pinned at the footer family's
    /// own seam boundary: pixels OUTSIDE the wrapper's rect survive
    /// the composite (the not-yet-migrated hand painters), pixels
    /// INSIDE are the tree's (the terminal-fill root covers,
    /// exactly the old paste).
    #[test]
    fn seam_covers_the_rect_and_keeps_outside_hand_pixels() {
        let canvas = permission_canvas();
        reset_hub();
        mount_footer_ops(canvas.ops(), 80, 16, 10, 4);
        let mut frame = crate::components::chrome_solid::blank_frame(100, 24);
        // Hand-paint a glyph EVERYWHERE (the stand-in for every
        // unmigrated Frame painter; '#' appears in no fixture glyph).
        for row in &mut frame.cells {
            for cell in row.iter_mut() {
                cell.ch = '#';
            }
        }
        composite_chrome_tree(&mut frame);
        for y in 4..20usize {
            for x in 10..90usize {
                assert_ne!(frame.cell(y, x).ch, '#', "hand pixel survived inside at ({y},{x})");
            }
        }
        for y in 0..24usize {
            for x in 0..100usize {
                let inside = (4..20).contains(&y) && (10..90).contains(&x);
                if !inside {
                    assert_eq!(frame.cell(y, x).ch, '#', "hand pixel lost outside at ({y},{x})");
                }
            }
        }
        reset_hub();
    }

    /// The scheduler-fairness gate for the footer family (the hub's
    /// generic guard): N syncs in one frame batch → exactly ONE
    /// flush at the seam.
    #[test]
    fn footer_syncs_flush_once_at_the_seam() {
        reset_hub();
        let canvas = permission_canvas();
        for _ in 0..3 {
            mount_footer_ops(canvas.ops(), 80, 16, 0, 8);
        }
        let hub = crate::components::chrome_solid::hub_for(80, 24);
        assert!(hub.borrow_mut().flush_if_pending(), "a pending sync batch flushes exactly once");
        assert!(
            !hub.borrow_mut().flush_if_pending(),
            "a second flush with no intervening sync is a no-op (no double raster)"
        );
        reset_hub();
    }

    /// The footer is a geometry-reactive family: a resize (the width
    /// ladder / the mount-height clamp against `size.1`) rebuilds
    /// the content subtree on the geometry key, and the wrapper's
    /// rect follows via `set_prop` — pinned by painting successive
    /// geometries through the live mount (no hub reset between).
    #[test]
    fn resize_follows_the_geometry_key() {
        reset_hub();
        let theme = FooterTheme::fallback_dark();
        let request = permission("read");
        let state = create_permission_body_state(&request);
        for (width, height, top) in [(80usize, 16usize, 8usize), (40, 16, 8), (120, 12, 12)] {
            let canvas = permission_body_canvas(&request, &state, width, height, &theme);
            mount_footer_ops(canvas.ops(), width, height, 0, top);
            let mut frame = crate::components::chrome_solid::blank_frame(width, 24);
            composite_chrome_tree(&mut frame);
            // The wrapper covers its rect: some mount row carries the
            // body's ink, and the rows OUTSIDE the rect stay blank
            // (the hole rule — the base survives untouched).
            let inside_ink =
                (top..top + height).any(|y| (0..width).any(|x| frame.cell(y, x).ch != ' '));
            assert!(inside_ink, "the body painted at {width}x{height}");
            for y in (0..top).chain(top + height..24) {
                for x in 0..width {
                    assert_eq!(frame.cell(y, x).ch, ' ', "ink outside the rect at ({y},{x})");
                }
            }
        }
        reset_hub();
    }
}
