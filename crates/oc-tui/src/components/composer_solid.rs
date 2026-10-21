//! Lane M3: the composer mount (the prompt box + the footer row) on
//! opentui-solid — the largest single v2 surface
//! (`component/prompt/index.tsx`, the 2011-LOC mount) re-homed the
//! M1/M2 way (see `docs/migration-m1-tabstrip.md`).
//!
//! **What this module is.** A 1:1 structural port of the composer's
//! VISIBLE mount as an opentui-solid builder tree (`box_`/`text`/
//! `for_list`/dyn props — the JSX-equivalent surface), driven by the
//! SAME oc-tui store data the shared solvers
//! (`composer::render`) consume. The text EDITING stays on the store's
//! `oc_tui_core::text_edit` engine (the editor-engine swap is a later
//! subsystem lane): this lane renders the composer AROUND it — the
//! `┃` rail chrome, the prompt-bg box + insets, the placeholder
//! presentation, the metadata row (`agent · model provider · variant`
//! with the drop ladder), the `╹▀` transition, the footer row
//! (location label + `truncateFilePath` budget, the plugin hints +
//! `{key} agents`/usage `{key} commands` right block, the interrupt
//! hint, the #33 hover lift), and the autocomplete popup above.
//!
//! **The bridge shape** (the M1/M2 template, verbatim):
//!
//! * *inputs*: one [`Signal<ComposerInputs>`] snapshot carrying
//!   everything the tree reads — pre-shaped by the SAME solver walk
//!   the old renderer paints with (`layout_draft_lines` wrap model,
//!   [`footer_plan`] geometry, `MetadataLine::layout`, the popup
//!   rows) — synced at the app loop's existing redraw cadence.
//! * *reactive shape*: the mount is keyed on GEOMETRY (width, the
//!   composer's top, popup height, content height, frame height,
//!   colors) — height-stable frames (the hot ones: typing within the
//!   last row, caret moves, hover) patch content dyn-props in place;
//!   height-changing frames (a wrap lands a new row, the popup
//!   opens) rebuild the small tree (the M2 content-height rule).
//! * *frame out*: the tree paints through a real headless
//!   [`SolidRenderer`] (taffy + raster) into an `OptimizedBuffer`;
//!   [`frame_from_buffer`] copies the cells into the oc-tui
//!   [`Frame`] the route compositor already pastes. No glow-range
//!   fixups (the composer's fills are all fg-white — opentui's own
//!   fill signature); the ONE bridge-level fixup is the software
//!   caret block (below).
//!
//! **The caret (NEW-S1a).** v2's live caret is the TERMINAL cursor:
//! the route computes the caret's screen cell
//! ([`render::prompt_caret_screen_cell`], the same wrap walk) and
//! hands it to `Store::set_caret_cell` → the app's
//! `set_cursor_position` emission. That seam is UNCHANGED by this
//! lane — `composer_mount_frame` still derives it from the store
//! draft, whichever renderer paints the box. opentui-core's own
//! terminal-cursor output (`CursorOutputState`, renderer.rs:105) is
//! driven by a FOCUSED textarea renderable inside the tree — the
//! editor-engine lane's seam: when the engine moves in, the caret
//! cell emission moves onto the renderer's cursor state (a 1-based
//! `set_position` on the buffer-relative cell) and the store seam
//! retires. Until then the solid tree carries no textarea and the
//! hardware cursor keeps flowing exactly as before. The GOLDEN-only
//! software block (`PromptView::caret` — the blink-phase block the
//! golden harness pins) renders as a bridge-level cell recolor over
//! the copied buffer, byte-identical to the old painter.
//!
//! **Selection flag (retired).** The lane-M3 dual-path flag
//! (`OC_COMPOSER_OPENTUI`) is gone: this module's opentui-solid tree
//! is the composer's ONLY renderer — the hand-rolled painter was
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
use opentui_solid::element::{Children, ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal, SolidRenderer};

use crate::components::composer::autocomplete::popup_height;
use crate::components::composer::render::{
    caret_cell, footer_plan, layout_draft_lines, palette, truncate_width, PromptView,
};
use crate::components::session_tabs_solid::frame_from_buffer;
use crate::render::{CellColor, Frame};
use crate::theme::ThemeState;
use crate::wrap::display_width;

/// The composer frame's ONLY renderer: every live mount (the session
/// route's `composer_mount_frame`, the home route, the session-screen
/// harness, the golden harnesses) paints through here.
#[must_use]
pub fn render_prompt_frame_dispatch(
    view: &PromptView,
    width: usize,
    height: usize,
    theme: &ThemeState,
) -> Frame {
    paint_solid_prompt_frame(view, width, height, theme)
}

// ---------------------------------------------------------------------------
// colors + chunk helpers (the M1/M2 shape)
// ---------------------------------------------------------------------------

/// An RGB triple (the port's cell currency).
type Rgb = (u8, u8, u8);

/// Theme-derived colors the tree needs, resolved once per paint from
/// the OLD renderer's own [`palette`] (one place per color both paths
/// consume — the parity contract's root).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComposerColors {
    /// `background.base` — the page fill + the footer row's bg.
    base: Rgb,
    /// `decrease(raised.base)` — the prompt box's fill.
    prompt_bg: Rgb,
    /// `border.base` — the popup rails (the box rail uses `rail`).
    border: Rgb,
    /// The agent-colored rail (`borderHighlight()` at fade 1) or
    /// `border.base` while no agent resolved.
    rail: Rgb,
    text_base: Rgb,
    muted: Rgb,
    /// `background.raised.high` — the popup rows.
    raised_high: Rgb,
    popup_selected_bg: Rgb,
    popup_selected_text: Rgb,
    /// `text.feedback.warning.base` — chips + the variant label.
    chip: Rgb,
}

fn rgb_of(color: CellColor) -> Rgb {
    match color {
        CellColor::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}

impl ComposerColors {
    fn of(theme: &ThemeState, view: &PromptView) -> Self {
        let p = palette(theme);
        let rail = view.rail.unwrap_or(p.border);
        ComposerColors {
            base: rgb_of(p.base),
            prompt_bg: rgb_of(p.prompt_bg),
            border: rgb_of(p.border),
            rail: rgb_of(rail),
            text_base: rgb_of(p.text_base),
            muted: rgb_of(p.muted),
            raised_high: rgb_of(p.raised_high),
            popup_selected_bg: rgb_of(p.popup_selected_bg),
            popup_selected_text: rgb_of(p.popup_selected_text),
            chip: rgb_of(p.chip_color),
        }
    }
}

fn rgba(rgb: Rgb) -> Rgba {
    Rgba::rgb(rgb.0, rgb.1, rgb.2)
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

fn chunk(text: String, fg: Rgb, attributes: u32) -> TextChunk {
    // bg None everywhere: chunks BLEND onto the destination fill
    // (prompt_bg inside the box, base on the footer row) — exactly
    // the old painter's explicit-bg result, because the destination
    // IS that bg.
    TextChunk { text, fg: Some(rgba(fg)), bg: None, attributes, link: None }
}

fn styled(chunks: Vec<TextChunk>) -> StyledText {
    StyledText::new(chunks)
}

fn one(text: String, fg: Rgb) -> StyledText {
    one_attr(text, fg, 0)
}

fn one_attr(text: String, fg: Rgb, attributes: u32) -> StyledText {
    if text.is_empty() {
        return styled(Vec::new());
    }
    styled(vec![chunk(text, fg, attributes)])
}

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

// ---------------------------------------------------------------------------
// inputs (the store → view bridge)
// ---------------------------------------------------------------------------

/// One pre-shaped popup row (the old painter's per-option walk).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PopupRowView {
    /// The title column, pre-padded to the registry's display width.
    title: String,
    /// The description in its `" {text}"` form (None = absent).
    description: Option<String>,
    selected: bool,
}

/// One frame's composer snapshot — the ONLY input the tree reads.
/// Geometry fields are MOUNT-STATIC (they key the mount: a change
/// rebuilds); the content fields patch in place.
#[derive(Clone)]
struct ComposerInputs {
    colors: ComposerColors,
    // -- content (in-place patches) --------------------------------------
    /// The popup's visible rows (empty while closed).
    popup_rows: Vec<PopupRowView>,
    /// The textarea's rows: one entry per visible line — the
    /// placeholder ride-along (`p`) when the draft is empty, else
    /// `L{index}` per wrapped line (pre-shaped chunks).
    rows: Vec<(String, StyledText)>,
    /// The metadata row's segments: `(x, chunks)` per surviving
    /// field, x relative to the content box (the old painter's
    /// `+width+1` walk). Empty = the blank golden row. SEPARATE
    /// elements, never separator spaces inside a chunk — a painted
    /// space carries the chunk's fg, the old row's gap cells keep the
    /// fill's white fg (the footer 2-gap rule, same trap).
    metadata_segments: Vec<(usize, StyledText)>,
    footer_location: StyledText,
    footer_interrupt: StyledText,
    /// The right block's first arm (`{key} agents` or the usage
    /// segment) + the `{key} commands` hint (two text elements — the
    /// 2-gap between them must stay page-fill, never a painted space
    /// chunk).
    footer_block: StyledText,
    footer_command: StyledText,
    footer_plugin: StyledText,
    /// TUI-12 — the live segment (`down 1 subagent · 1 shell`): the
    /// shortcut run `text.base` + the counts muted (base when
    /// hovered).
    footer_live: StyledText,
    // -- dynamic geometry (content-dependent, patched) --------------------
    interrupt_x: usize,
    block_x: usize,
    command_x: usize,
    plugin_x: usize,
    live_x: usize,
    // -- mount-static geometry (the mount key) ----------------------------
    width: usize,
    /// The composer box's frame-relative top (the popup's rows above).
    top: usize,
    /// The popup's reserved height (rows above `top`).
    popup_h: usize,
    /// 1 pad row + textarea rows + 1 metadata row.
    content_h: usize,
    frame_height: usize,
    pad: usize,
    textarea_rows: usize,
}

/// The mount key — every geometry input the tree bakes in at build
/// time. A typing-induced wrap (`content_h` change), the popup
/// opening (`top`/`popup_h`), a resize, or a theme swap rebuilds the
/// mount; the hot frames (same shape, new text) patch in place.
type MountKey = (usize, usize, usize, usize, usize, ComposerColors);

fn mount_key(i: &ComposerInputs) -> MountKey {
    (i.width, i.top, i.popup_h, i.content_h, i.frame_height, i.colors)
}

/// The textarea row chunks: one chunk per span — chips bold in the
/// warning color (`feedback.warning.base`, the extmark style), plain
/// spans in `text.base` (render.rs's `paint_composer_box` walk).
fn line_chunks(
    spans: &[crate::components::composer::render::Span],
    colors: &ComposerColors,
) -> StyledText {
    let chunks = spans
        .iter()
        .filter(|span| !span.text.is_empty())
        .map(|span| {
            if span.chip {
                chunk(span.text.clone(), colors.chip, 1) // BOLD = bit 1
            } else {
                chunk(span.text.clone(), colors.text_base, 0)
            }
        })
        .collect();
    styled(chunks)
}

/// The footer's hint chunks — `{key} label` as `key` (`text.base`)
/// then `label` (muted), contiguous (`paint_hint`'s split; the key
/// run carries the format's inter-word space).
fn hint_chunks(key: &str, label: &str, colors: &ComposerColors) -> StyledText {
    let mut chunks = Vec::new();
    if !key.is_empty() {
        chunks.push(chunk(key.to_owned(), colors.text_base, 0));
    }
    if !label.is_empty() {
        chunks.push(chunk(label.to_owned(), colors.muted, 0));
    }
    styled(chunks)
}

/// Shape one frame's [`ComposerInputs`] — the shared solver walk
/// (the composer geometry + [`footer_plan`]), collected as pre-painted
/// strings and chunks.
#[allow(clippy::too_many_lines)] // the full mount geometry, mirrored span for span
fn shape(view: &PromptView, width: usize, height: usize, theme: &ThemeState) -> ComposerInputs {
    let colors = ComposerColors::of(theme, view);
    let pad = if width >= 44 { 2 } else { 1 };
    let inner = width.saturating_sub(1 + pad * 2).max(1);
    let lines = layout_draft_lines(view.draft, inner);
    let max_height = (height / 3).max(6);
    let textarea_rows = (lines.len().max(1) + 1).min(max_height);
    let content_h = 1 + textarea_rows + 1;

    // The textarea: the placeholder (truncated to inner − 2, the
    // caret column reserve) or the wrapped lines.
    let rows: Vec<(String, StyledText)> = if view.draft.is_empty() {
        vec![(
            "p".to_owned(),
            one(truncate_width(&view.placeholder, inner.saturating_sub(2)), colors.muted),
        )]
    } else {
        lines
            .iter()
            .take(textarea_rows)
            .enumerate()
            .map(|(index, spans)| (format!("L{index}"), line_chunks(spans, &colors)))
            .collect()
    };

    // The metadata row: `Show when={(shell || agent) && (agent || model)}`
    // — blank (promptBg fill) while nothing resolved.
    let layout = view.metadata.layout(view.terminal_width);
    let shell_or_agent = !view.metadata.agent.is_empty();
    let metadata_segments: Vec<(usize, StyledText)> = if (shell_or_agent
        || !layout.model.is_empty())
        && (!layout.agent.is_empty() || !layout.model.is_empty())
    {
        // The old painter's walk: each segment at x, then +width+1.
        let mut segments: Vec<(String, Rgb, u32)> = Vec::new();
        if !layout.agent.is_empty() {
            segments.push((layout.agent.clone(), colors.rail, 0));
        }
        if layout.auto {
            segments.push(("auto".to_owned(), colors.muted, 0));
        }
        if !layout.model.is_empty() {
            if !layout.agent.is_empty() {
                segments.push(("·".to_owned(), colors.muted, 0));
            }
            segments.push((layout.model.clone(), colors.text_base, 0));
        }
        if !layout.provider.is_empty() {
            segments.push((layout.provider.clone(), colors.muted, 0));
        }
        if !layout.variant.is_empty() {
            segments.push(("·".to_owned(), colors.muted, 0));
            segments.push((layout.variant.clone(), colors.chip, 1));
        }
        let mut x = pad;
        segments
            .into_iter()
            .map(|(text, fg, attrs)| {
                let at = x;
                x += display_width(&text) + 1; // the gap-1 separator
                (at, one_attr(text, fg, attrs))
            })
            .collect()
    } else {
        Vec::new()
    };

    // The footer row's plan (ONE source of truth with the old path).
    let plan = footer_plan(view, width);
    let location_fg = if view.footer.location_hovered { colors.text_base } else { colors.muted };
    let footer_location = one(plan.location.clone(), location_fg);
    let footer_interrupt = match plan.interrupt_x {
        Some(_) => styled(vec![
            chunk("esc ".to_owned(), colors.text_base, 0),
            chunk(plan.interrupt_label.clone(), colors.muted, 0),
        ]),
        None => styled(Vec::new()),
    };
    let footer_block = if plan.block_x.is_some() {
        if plan.usage_first {
            one(plan.usage.clone(), colors.muted)
        } else {
            hint_chunks(&plan.agent_key, "agents", &colors)
        }
    } else {
        styled(Vec::new())
    };
    let footer_command = if plan.command_x.is_some() {
        hint_chunks(&plan.command_key, "commands", &colors)
    } else {
        styled(Vec::new())
    };
    let footer_plugin = match &plan.plugin {
        Some((_, segment)) if !segment.is_empty() => one(segment.clone(), colors.muted),
        _ => styled(Vec::new()),
    };
    // TUI-12 — the live segment: the `session.child.first` shortcut
    // run in `text.base`, the counts in muted (base when hovered —
    // footer.tsx:70-84's `fg={liveHovered() ? text.base : muted}`).
    let footer_live = match &plan.live {
        Some((_, label)) if !label.is_empty() => {
            let counts_fg = if view.footer.live_hovered { colors.text_base } else { colors.muted };
            let shortcut_width = display_width(&view.footer.live_shortcut);
            let mut chunks = Vec::new();
            if shortcut_width > 0 {
                chunks.push(chunk(view.footer.live_shortcut.clone(), colors.text_base, 0));
            }
            if !view.footer.live.is_empty() {
                chunks.push(chunk(view.footer.live.clone(), counts_fg, 0));
            }
            styled(chunks)
        }
        _ => styled(Vec::new()),
    };

    // The popup above the box (rows y = top − popup_h + i).
    let popup_rows: Vec<PopupRowView> = match &view.autocomplete {
        Some(popup) if view.top > 0 => {
            let popup_h = popup_height(popup.options.len(), view.top).min(view.top);
            popup
                .options
                .iter()
                .take(popup_h)
                .enumerate()
                .map(|(index, option)| PopupRowView {
                    title: format!("{:width$}", option.display, width = popup.title_width),
                    description: option.description.as_ref().map(|d| format!(" {d}")),
                    selected: index == popup.selected,
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let popup_h = match &view.autocomplete {
        Some(popup) if view.top > 0 => popup_height(popup.options.len(), view.top).min(view.top),
        _ => 0,
    };

    ComposerInputs {
        colors,
        popup_rows,
        rows,
        metadata_segments,
        footer_location,
        footer_interrupt,
        footer_block,
        footer_command,
        footer_plugin,
        footer_live,
        interrupt_x: plan.interrupt_x.unwrap_or(0),
        block_x: plan.block_x.unwrap_or(0),
        command_x: plan.command_x.unwrap_or(0),
        plugin_x: plan.plugin.as_ref().map_or(0, |(x, _)| *x),
        live_x: plan.live.as_ref().map_or(0, |(x, _)| *x),
        width,
        top: view.top,
        popup_h,
        content_h,
        frame_height: height,
        pad,
        textarea_rows,
    }
}

// ---------------------------------------------------------------------------
// the element tree (the tsx structure, builder-for-JSX)
// ---------------------------------------------------------------------------

/// One popup row element — `┃` rails (border.base over the page bg),
/// the interior fill (`action.primary.focused` when selected, else
/// `raised.high`), the padded title column + the one-space-ahead
/// description (autocomplete.tsx's mount; render.rs's
/// `paint_autocomplete_popup` walk).
fn popup_row_element(
    inputs: &Signal<ComposerInputs>,
    key: &str,
    colors: ComposerColors,
) -> Element {
    let index: usize = key.strip_prefix('A').and_then(|rest| rest.parse().ok()).unwrap_or(0);
    let border_text = || {
        text()
            .width(1.0)
            .flex_shrink(0.0)
            .content(PropValue::Styled(one("┃".to_owned(), colors.border)))
            .into_element()
    };
    let title_inputs = inputs.clone();
    let title = text().wrap_mode("none").flex_shrink(0.0).dyn_prop(
        "content",
        dyn_styled(map(title_inputs, move |i: &ComposerInputs| match i.popup_rows.get(index) {
            Some(row) => one(
                row.title.clone(),
                if row.selected { i.colors.popup_selected_text } else { i.colors.text_base },
            ),
            None => styled(Vec::new()),
        })),
    );
    let desc_inputs = inputs.clone();
    let desc = text().wrap_mode("none").flex_shrink(0.0).dyn_prop(
        "content",
        dyn_styled(map(desc_inputs, move |i: &ComposerInputs| match i.popup_rows.get(index) {
            Some(row) => match &row.description {
                Some(description) => one(
                    description.clone(),
                    if row.selected { i.colors.popup_selected_text } else { i.colors.muted },
                ),
                None => styled(Vec::new()),
            },
            None => styled(Vec::new()),
        })),
    );
    let bg_inputs = inputs.clone();
    let interior = box_()
        .flex_grow(1.0)
        .flex_direction("row")
        .padding_left(1.0)
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(bg_inputs, move |i: &ComposerInputs| match i.popup_rows.get(index) {
                Some(row) if row.selected => i.colors.popup_selected_bg,
                _ => i.colors.raised_high,
            })),
        )
        .children(Children::Static(vec![title.into_element(), desc.into_element()]));
    box_()
        .height(1.0)
        .flex_shrink(0.0)
        .flex_direction("row")
        .children(Children::Static(vec![border_text(), interior.into_element(), border_text()]))
        .into_element()
}

/// The composer mount element — the tsx column, builder-for-JSX:
///
/// ```text
/// [spacer top−popup_h]                      ← the popup's headroom
/// [popup rows (For)]                        ← above the box
/// [┃ rail ×content_h | prompt-bg box]       ← pad row, textarea rows
/// │                                           (For), metadata row
/// [╹ ▀▀▀…]                                 ← the transition row
/// [footer row: absolute-positioned texts]  ← location/interrupt/
/// │                                           block/command/plugin
/// [trailing spacer]                        ← page fill to frame h
/// ```
///
/// Geometry is baked at build time (the mount key pins it); every
/// content slot is a dyn prop over the inputs signal.
#[allow(clippy::too_many_lines)] // one JSX element per section, mirroring the tsx mount
fn composer_element(inputs: &Signal<ComposerInputs>, geo: &ComposerInputs) -> Element {
    let colors = geo.colors;
    let mut children: Vec<Element> = Vec::new();

    // The popup's headroom above it (page fill rows).
    if geo.top > geo.popup_h {
        children.push(box_().height((geo.top - geo.popup_h) as f32).into_element());
    }
    // The popup rows — a For keyed by row index (selection moves
    // patch the per-row props in place).
    if geo.popup_h > 0 {
        let source = ForItemSource {
            source: map(inputs.clone(), |i: &ComposerInputs| {
                i.popup_rows
                    .iter()
                    .enumerate()
                    .map(|(index, _)| ForItem {
                        key: format!("A{index}"),
                        value: PropValue::Num(index as f32),
                    })
                    .collect::<Vec<ForItem>>()
            }),
        };
        let row_inputs = inputs.clone();
        let rows = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
            popup_row_element(&row_inputs, &item.key, colors)
        });
        children.push(box_().height(geo.popup_h as f32).children_dyn(Rc::new(rows)).into_element());
    }

    // The composer box: the `┃` rail column (static — content_h keys
    // the mount) beside the prompt-bg content column.
    let rail_cell = || {
        text()
            .width(1.0)
            .flex_shrink(0.0)
            .content(PropValue::Styled(one("┃".to_owned(), colors.rail)))
            .into_element()
    };
    let rail = box_()
        .width(1.0)
        .flex_shrink(0.0)
        .children(Children::Static((0..geo.content_h).map(|_| rail_cell()).collect()));
    // The textarea rows — a For keyed by row (`p` placeholder ride-
    // along / `L{index}` lines); a typed row patches its chunks.
    let rows_source = ForItemSource {
        source: map(inputs.clone(), |i: &ComposerInputs| {
            i.rows
                .iter()
                .map(|(key, _)| ForItem { key: key.clone(), value: PropValue::Str(key.clone()) })
                .collect::<Vec<ForItem>>()
        }),
    };
    let line_inputs = inputs.clone();
    let lines = for_list(Rc::new(rows_source), move |item: &ForItem, _index: usize| {
        let key = item.key.clone();
        text()
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop(
                "content",
                dyn_styled(map(line_inputs.clone(), move |i: &ComposerInputs| {
                    i.rows
                        .iter()
                        .find(|(row_key, _)| *row_key == key)
                        .map_or_else(|| styled(Vec::new()), |(_, chunks)| chunks.clone())
                })),
            )
            .into_element()
    });
    let metadata_text = |index: usize| {
        text()
            .position("absolute")
            .top(0.0)
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop(
                "left",
                dyn_num(map(inputs.clone(), move |i: &ComposerInputs| {
                    i.metadata_segments.get(index).map_or(0, |(x, _)| *x)
                })),
            )
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), move |i: &ComposerInputs| {
                    i.metadata_segments
                        .get(index)
                        .map_or_else(|| styled(Vec::new()), |(_, chunks)| chunks.clone())
                })),
            )
    };
    let content = box_()
        .flex_grow(1.0)
        .flex_direction("column")
        .background_color(hex(colors.prompt_bg))
        .children(Children::Static(vec![
            // paddingTop 1 (the pad row).
            box_().height(1.0).into_element(),
            // The textarea box: paddingLeft/Right = pad, one row per
            // wrapped line (+1 cursor row under the last — blank).
            box_()
                .height(geo.textarea_rows as f32)
                .flex_direction("column")
                .padding_left(geo.pad as f32)
                .padding_right(geo.pad as f32)
                .children_dyn(Rc::new(lines))
                .into_element(),
            // The metadata pad row: up to six absolutely positioned
            // segments (the separator cells stay the fill's white —
            // the footer 2-gap rule, same trap).
            box_()
                .height(1.0)
                .position("relative")
                .children(Children::Static(
                    // 7 = the full ladder (agent auto · model provider · variant).
                    (0..7).map(|index| metadata_text(index).into_element()).collect(),
                ))
                .into_element(),
        ]));
    children.push(
        box_()
            .height(geo.content_h as f32)
            .flex_direction("row")
            .children(Children::Static(vec![rail.into_element(), content.into_element()]))
            .into_element(),
    );

    // The transition row: the `╹` corner in the rail color + the
    // upper-half blocks in promptBg over the page bg.
    let transition_blocks = if geo.width > 1 {
        one("▀".repeat(geo.width - 1), colors.prompt_bg)
    } else {
        styled(Vec::new())
    };
    children.push(
        box_()
            .height(1.0)
            .flex_direction("row")
            .children(Children::Static(vec![
                text()
                    .width(1.0)
                    .flex_shrink(0.0)
                    .content(PropValue::Styled(one("╹".to_owned(), colors.rail)))
                    .into_element(),
                text()
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .content(PropValue::Styled(transition_blocks))
                    .into_element(),
            ]))
            .into_element(),
    );

    // The footer row: five absolutely positioned texts in the old
    // painter's order (location, interrupt, block, command, plugin —
    // later elements overwrite earlier ones exactly like the old
    // paint sequence; the 2-gaps between arms stay page fill).
    let footer_text = |left_of: fn(&ComposerInputs) -> usize,
                       content_of: fn(&ComposerInputs) -> StyledText|
     -> Element {
        text()
            .position("absolute")
            .top(0.0)
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop("left", dyn_num(map(inputs.clone(), left_of)))
            .dyn_prop("content", dyn_styled(map(inputs.clone(), content_of)))
            .into_element()
    };
    children.push(
        box_()
            .height(1.0)
            .position("relative")
            .flex_direction("row")
            .children(Children::Static(vec![
                footer_text(|_| 0, |i: &ComposerInputs| i.footer_location.clone()),
                footer_text(
                    |i: &ComposerInputs| i.interrupt_x,
                    |i: &ComposerInputs| i.footer_interrupt.clone(),
                ),
                footer_text(
                    |i: &ComposerInputs| i.block_x,
                    |i: &ComposerInputs| i.footer_block.clone(),
                ),
                footer_text(
                    |i: &ComposerInputs| i.command_x,
                    |i: &ComposerInputs| i.footer_command.clone(),
                ),
                footer_text(
                    |i: &ComposerInputs| i.plugin_x,
                    |i: &ComposerInputs| i.footer_plugin.clone(),
                ),
                footer_text(
                    |i: &ComposerInputs| i.live_x,
                    |i: &ComposerInputs| i.footer_live.clone(),
                ),
            ]))
            .into_element(),
    );

    // The trailing page-fill rows below the mount (the golden
    // harness's taller frames).
    let tail = geo.frame_height.saturating_sub(geo.top + geo.content_h + 2);
    if tail > 0 {
        children.push(box_().height(tail as f32).into_element());
    }

    box_()
        .width(geo.width as f32)
        .height(geo.frame_height as f32)
        .background_color(hex(colors.base))
        .children(Children::Static(children))
        .into_element()
}

// ---------------------------------------------------------------------------
// the mount (one per thread, keyed on geometry)
// ---------------------------------------------------------------------------

/// The mounted composer: the inputs signal + a real headless
/// [`SolidRenderer`]. Rebuilt whenever the mount key changes (width,
/// top, popup height, content height, frame height, colors) — the
/// M2 height-keying rule; height-stable frames patch in place.
struct SolidComposer {
    inputs: Signal<ComposerInputs>,
    renderer: SolidRenderer,
    key: MountKey,
}

impl SolidComposer {
    fn new(inputs: &ComposerInputs) -> Self {
        let signal = Signal::new(inputs.clone());
        let renderer = opentui_solid::render(
            CliRendererConfig {
                width: inputs.width.max(1) as u32,
                height: inputs.frame_height.max(1) as u32,
                background_color: {
                    let base = inputs.colors.base;
                    Rgba::rgb(base.0, base.1, base.2)
                },
                ..CliRendererConfig::default()
            },
            composer_element(&signal, inputs),
        );
        Self { inputs: signal, renderer, key: mount_key(inputs) }
    }

    /// Sync one frame's snapshot, flush the reconciler patches, and
    /// render — the per-frame bridge cost.
    fn paint(&mut self, inputs: ComposerInputs) {
        self.inputs.set(inputs);
        self.renderer.flush();
    }
}

thread_local! {
    /// One composer mount per thread-process — the app paints exactly
    /// one per frame; the harnesses reuse it across scenarios.
    static COMPOSER: RefCell<Option<SolidComposer>> = const { RefCell::new(None) };
}

/// Paint the composer mount through the opentui-solid path — the
/// new-path twin of [`render_prompt_frame`]: shape the inputs (the
/// old renderer's own walk), sync + flush the mount, copy the cells,
/// then apply the ONE bridge-level recolor (the golden software
/// caret block — the live caret is the terminal cursor, NEW-S1a,
/// which this path keeps emitting through the untouched
/// `prompt_caret_screen_cell` → `Store::set_caret_cell` seam).
#[must_use]
pub fn paint_solid_prompt_frame(
    view: &PromptView,
    width: usize,
    height: usize,
    theme: &ThemeState,
) -> Frame {
    if width == 0 || height == 0 {
        // The old renderer's degenerate shape: empty rows, page fill
        // is a no-op.
        return Frame { cells: vec![Vec::new(); height], width, height };
    }
    let inputs = shape(view, width, height, theme);
    let caret = view.caret;
    let pad = inputs.pad;
    let textarea_rows = inputs.textarea_rows;
    let colors = inputs.colors;
    let top = inputs.top;
    COMPOSER.with_borrow_mut(|slot| {
        let mount = slot.get_or_insert_with(|| SolidComposer::new(&inputs));
        if mount.key != mount_key(&inputs) {
            *mount = SolidComposer::new(&inputs);
        }
        mount.paint(inputs);
        let buffer = &mount.renderer.renderer.current_render_buffer;
        let mut frame = frame_from_buffer(buffer, &[]);
        // The software caret block (golden harness only — the live
        // routes pass `caret: None` and position the TERMINAL cursor
        // instead): bg = `text.base`, fg = prompt-bg over the cell,
        // the glyph itself untouched (render.rs:649-661, verbatim).
        if let Some(offset) = caret {
            let inner = width.saturating_sub(1 + pad * 2).max(1);
            let (row, col) = caret_cell(view.draft, inner, offset);
            if row < textarea_rows {
                let (x, y) = (1 + pad + col, top + 1 + row);
                if let Some(cell) = frame.cells.get_mut(y).and_then(|line| line.get_mut(x)) {
                    cell.bg =
                        CellColor::Rgb(colors.text_base.0, colors.text_base.1, colors.text_base.2);
                    cell.fg =
                        CellColor::Rgb(colors.prompt_bg.0, colors.prompt_bg.1, colors.prompt_bg.2);
                }
            }
        }
        frame
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::render::{FooterHints, MetadataLine};
    use crate::components::composer::PromptDraft;
    use crate::theme::ThemeState;

    fn draft(text: &str) -> PromptDraft {
        PromptDraft { text: text.to_owned(), ..PromptDraft::default() }
    }

    /// The reactive path patches in place: typing churn on the last
    /// row re-renders WITHOUT a rebuild (same mount — content-height
    /// frames only rebuild when a wrap lands a new row). The
    /// retirement-era residue of the old parity gate, now absolute.
    #[test]
    fn typing_churn_patches_the_mounted_composer() {
        let theme = ThemeState::v2_dark();
        let build = |text: &str| {
            let owned = draft(text);
            let view = PromptView {
                draft: &owned,
                placeholder: String::new(),
                autocomplete: None,
                footer: FooterHints::default(),
                metadata: MetadataLine::default(),
                terminal_width: 80,
                top: 0,
                rail: None,
                caret: None,
            };
            paint_solid_prompt_frame(&view, 80, 8, &theme)
        };
        let mut seen: Vec<(&str, Frame)> = Vec::new();
        for text in ["a", "ab", "abc", "ab"] {
            let new = build(text);
            // "ab" revisits earlier inputs -> the identical frame.
            if let Some((_, prior)) = seen.iter().find(|(t, _)| *t == text) {
                assert_eq!(&new, prior, "same inputs paint the same frame");
            }
            seen.push((text, new));
        }
        COMPOSER.with_borrow(|slot| {
            let mount = slot.as_ref().expect("composer mounted");
            assert!(!mount.renderer.ops_log().is_empty(), "the reconciler recorded its ops");
        });
    }

    /// A resize (or a wrap landing a new row) rebuilds cleanly — the
    /// mount-key rule (absolute self-check since the retirement).
    #[test]
    fn reshape_rebuilds_cleanly() {
        let theme = ThemeState::v2_dark();
        let owned = draft("hello world this wraps at forty columns nicely");
        let view = PromptView {
            draft: &owned,
            placeholder: String::new(),
            autocomplete: None,
            footer: FooterHints { location: "/tmp/x:main".into(), ..FooterHints::default() },
            metadata: MetadataLine::default(),
            terminal_width: 80,
            top: 0,
            rail: None,
            caret: None,
        };
        let mut first: Option<Frame> = None;
        for width in [80usize, 40, 120, 80] {
            let height = crate::components::composer::render::mount_height(&owned, width, 24);
            let frame = paint_solid_prompt_frame(&view, width, height, &theme);
            assert_eq!(frame.width, width, "full-width frame at width {width}");
            if width == 80 {
                match &first {
                    None => first = Some(frame),
                    Some(p) => assert_eq!(&frame, p, "same inputs paint the same frame"),
                }
            }
        }
    }
}
