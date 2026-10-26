//! Lane R2 step 2: the footer family's painters on opentui-solid —
//! the M5 flat-units shape generalized to the WHOLE family.
//!
//! The footer painters were already op-shaped (they paint through
//! [`Canvas`]'s `fill_bg`/`spans`/`spans_right` vocabulary), so the
//! bridge is ONE shared substrate instead of one tree per surface:
//! the Canvas now RECORDS its op stream beside painting, and
//! [`canvas_frame_solid`] replays the ops as absolutely-positioned
//! opentui elements — one 1-row fill BOX per `fill_bg`, one
//! `wrapMode none` + `flexShrink 0` text per `spans` line, in PAINT
//! ORDER (the raster z-orders siblings last-wins, exactly the
//! Canvas's overwrite semantics). The tree paints through a real
//! [`SolidRenderer`] (taffy + raster) into an `OptimizedBuffer`;
//! `frame_from_buffer` (the M1 cell-copy bridge) produces the oc-tui
//! [`Frame`] the compositor already consumes.
//!
//! * **Transparent regions** (`clear_bg` — the composer textarea's
//!   canonical bg `49`) cannot be expressed in the buffer: they are
//!   applied as a bridge-level fixup (space cells inside the recorded
//!   rect read bg `Default`), the same class as M1's glow ranges.
//! * **Non-RGB colors** (indexed/default paints) fall back to the
//!   hand-rolled path for the whole frame — the footer palette is
//!   all-RGB (`FooterTheme::from_resolved`), so the fallback is
//!   defensive only.
//! * **Flag**: `OC_FOOTER_OPENTUI` (`0` = hand-rolled, the default
//!   until this lane's rig proof; read once per process at first
//!   paint — the M1 protocol).
//!
//! Parity: every `*_canvas` builder in the family has a cell-for-cell
//! unit gate in this module's tests (`solid_footer_*_equals_the_old_
//! renderer`) over the golden fixtures' scenario matrices.

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::sync::OnceLock;

use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::{box_, text, Children, Element, PropValue, SolidRenderer};

use super::canvas::{Canvas, CanvasOp, FILL_BG};
use crate::components::session_tabs_solid::frame_from_buffer;
use crate::render::{CellColor, Frame};

/// `OC_FOOTER_OPENTUI` — read once per process (the M1 protocol: a
/// running session never switches renderers). Default ON since the
/// R2 parity proof (9 footer goldens + 64 PTY tests green under the
/// solid path, worst-frame 0.61 ms vs the 10 ms gate); `=0` forces
/// the hand-rolled path during the one-release probation window.
fn solid_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var("OC_FOOTER_OPENTUI").map(|value| value.trim() != "0").unwrap_or(true))
}

/// The family's dispatch seam: every `render_*` fn in the footer
/// family finishes through here. Default (solid ON) replays the
/// recorded op stream through the opentui-solid tree; `OC_FOOTER_OPENTUI=0`
/// keeps the hand-rolled Canvas path byte-for-byte (probation rollback).
#[must_use]
pub fn finish(canvas: Canvas) -> Frame {
    if !solid_enabled() {
        return canvas.into_frame();
    }
    let (width, height) = canvas.dims();
    let ops = canvas.ops().to_vec();
    canvas_frame_solid(&ops, width, height)
}

// ---------------------------------------------------------------------------
// the op → element replay
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
        // Indexed/default paints keep the hand-rolled path (the
        // footer palette is all-RGB; this is defensive).
        CellColor::Indexed(_) | CellColor::Default => None,
    }
}

/// Replay the Canvas op stream through a one-shot opentui-solid
/// render (the M6 farewell mount shape) and copy the cells into an
/// oc-tui [`Frame`].
#[must_use]
#[allow(clippy::too_many_lines, clippy::type_complexity)] // one bridge, the M5 merge bake inline
pub fn canvas_frame_solid(ops: &[CanvasOp], width: usize, height: usize) -> Frame {
    if width == 0 || height == 0 {
        return Frame { cells: vec![vec![]; height], width, height };
    }
    // Translate every op; an untranslatable color keeps the old path
    // (never triggered by the family's palette).
    let mut fills: Vec<(usize, usize, usize, Option<Rgba>)> = Vec::new();
    let mut texts: Vec<(usize, usize, StyledText)> = Vec::new();
    // The MERGE shadow (the M5 sidebar rule): the old `fill_bg` only
    // overwrites the CELL BACKGROUND — glyphs an earlier span painted
    // under the rect survive with their fg/attrs. The raster's box
    // fill overwrites whole cells, so surviving glyphs are re-emit as
    // a text element AFTER the fill (same DOM order trick as M2's
    // footer-overwrites-title). The shadow tracks the glyph cells in
    // paint order (a later span REPLACES an earlier glyph).
    let mut glyphs: Vec<Vec<Option<(char, CellColor, u32)>>> = vec![vec![None; width]; height];
    let mut restores: Vec<Vec<(usize, StyledText)>> = Vec::new();
    let note_span = |x: usize,
                     y: usize,
                     spans: &[super::canvas::Span],
                     glyphs: &mut Vec<Vec<Option<(char, CellColor, u32)>>>| {
        let mut x = x;
        for span in spans {
            for atom in crate::wrap::flatten(&span.text, ratatui::style::Style::new()) {
                if let Some(row) = glyphs.get_mut(y) {
                    if let Some(slot) = row.get_mut(x) {
                        *slot = Some((
                            atom.text.chars().next().unwrap_or(' '),
                            span.fg,
                            u32::from(span.bold),
                        ));
                    }
                }
                x += atom.width.max(1);
            }
        }
    };
    let restore_chunks = |x0: usize,
                          y: usize,
                          w: usize,
                          glyphs: &Vec<Vec<Option<(char, CellColor, u32)>>>,
                          bg: CellColor|
     -> Vec<(usize, StyledText)> {
        // Contiguous runs of surviving glyphs, each one text element
        // (explicit chunk bg = the fill's bg).
        let Some(rgba_bg) = rgba(bg) else { return Vec::new() };
        let mut runs: Vec<(usize, StyledText)> = Vec::new();
        let Some(row) = glyphs.get(y) else { return runs };
        let mut index = x0;
        while index < x0 + w && index < row.len() {
            let Some((_ch, fg, attrs)) = row[index] else {
                index += 1;
                continue;
            };
            let start = index;
            let mut chars = String::new();
            let run_fg = fg;
            let run_attrs = attrs;
            while index < x0 + w && index < row.len() {
                match row[index] {
                    Some((next_ch, next_fg, next_attrs))
                        if next_fg == run_fg && next_attrs == run_attrs =>
                    {
                        chars.push(next_ch);
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
    for op in ops {
        match op {
            CanvasOp::Fill { y, x, width: w, bg } => {
                if *bg == CellColor::Default {
                    // A transparent region: paint nothing in the
                    // tree, fix the cells up after the copy.
                    fills.push((*x, *y, *w, None));
                } else {
                    fills.push((*x, *y, *w, rgba(*bg)));
                }
                restores.push(restore_chunks(*x, *y, *w, &glyphs, *bg));
            }
            CanvasOp::Spans { y, x, spans } => {
                let mut chunks: Vec<TextChunk> = Vec::new();
                for span in spans {
                    let (Some(fg), Some(bg)) = (rgba(span.fg), rgba(span.bg)) else {
                        return fallback_frame(ops, width, height);
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
                texts.push((*x, *y, StyledText::new(chunks)));
            }
        }
    }
    // The element stream: fills and texts INTERLEAVED in paint order
    // (the raster's sibling z-order = DOM order = the Canvas's
    // overwrite semantics).
    let mut children: Vec<Element> = Vec::with_capacity(ops.len());
    let mut fill_iter = fills.iter();
    let mut text_iter = texts.iter();
    let mut restore_iter = restores.iter();
    for op in ops {
        match op {
            CanvasOp::Fill { .. } => {
                let Some(&(x, y, w, bg)) = fill_iter.next() else { continue };
                let mut builder = box_()
                    .position("absolute")
                    .left(x as f32)
                    .top(y as f32)
                    .width(w as f32)
                    .height(1.0)
                    .border(false);
                if let Some(bg) = bg {
                    builder = builder.background_color(hex(bg));
                }
                children.push(builder.into_element());
                // The surviving glyphs ride OVER the fill (the old
                // `fill_bg`'s bg-only merge).
                if let Some(restores) = restore_iter.next() {
                    for &(start, ref content) in restores {
                        children.push(
                            text()
                                .position("absolute")
                                .left(start as f32)
                                .top(y as f32)
                                .wrap_mode("none")
                                .flex_shrink(0.0)
                                .content(PropValue::Styled(content.clone()))
                                .into_element(),
                        );
                    }
                }
            }
            CanvasOp::Spans { .. } => {
                let Some(&(x, y, ref content)) = text_iter.next() else { continue };
                children.push(
                    text()
                        .position("absolute")
                        .left(x as f32)
                        .top(y as f32)
                        .wrap_mode("none")
                        .flex_shrink(0.0)
                        .content(PropValue::Styled(content.clone()))
                        .into_element(),
                );
            }
        }
    }
    let root = box_()
        .position("absolute")
        .left(0.0)
        .top(0.0)
        .width(width as f32)
        .height(height as f32)
        .border(false)
        .background_color(hex(fill_bg()))
        .children(Children::Static(children))
        .into_element();
    let renderer = SolidRenderer::new(CliRendererConfig {
        width: width as u32,
        height: height as u32,
        background_color: fill_bg(),
        ..CliRendererConfig::default()
    });
    let mut renderer = renderer;
    renderer.render_root(root);
    renderer.flush();
    let mut frame = frame_from_buffer(&renderer.renderer.current_render_buffer, &[]);
    // Transparent-region fixup: space cells the tree never painted a
    // chunk over keep the root's fill bg — the old `clear_bg` leaves
    // them at canonical bg `49`.
    for &(x, y, w, none) in &fills {
        if none.is_some() {
            continue;
        }
        if let Some(row) = frame.cells.get_mut(y) {
            for cell in row.iter_mut().skip(x).take(w) {
                if cell.ch == ' ' && cell.attrs == 0 {
                    cell.bg = CellColor::Default;
                }
            }
        }
    }
    frame
}

/// The defensive fallback: an untranslatable op keeps the
/// hand-rolled bytes (replayed through a private Canvas — the op
/// stream IS the painter's semantics).
fn fallback_frame(ops: &[CanvasOp], width: usize, height: usize) -> Frame {
    let mut canvas = Canvas::new(width, height);
    for op in ops {
        match op {
            CanvasOp::Fill { y, x, width: w, bg } => canvas.fill_bg(*y, *x, *w, *bg),
            CanvasOp::Spans { y, x, spans } => canvas.spans(*y, *x, spans),
        }
    }
    canvas.into_frame()
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
// over the golden fixtures' scenario matrices (lane R2 step 2)
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
        let solid = canvas_frame_solid(canvas.ops(), width, height);
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
}
