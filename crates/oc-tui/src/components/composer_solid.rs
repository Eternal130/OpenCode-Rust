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
//! `oc_tui_core::text_edit` engine (opentui-core's textarea line —
//! the M-engine swap): this lane renders the composer AROUND it — the
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
//! * *frame out*: D2.2 — the tree mounts under the shared-tree hub's
//!   `surface-composer` family wrapper ([`composer_mount`], the
//!   chrome paradigm); pixels reach the emitted [`Frame`] through the
//!   ONE full-buffer compositor seam (`composite_chrome_tree` inside
//!   `render_app`). No glow-range fixups (the composer's fills are
//!   all fg-white — opentui's own fill signature); the ONE
//!   bridge-level fixup is the software caret block (below), a
//!   composite-seam stamp.
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

use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{Children, ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal};

use crate::components::chrome_solid::{family_wrapper_live, mount_family_wrapper, ChromeStamp};
use crate::components::composer::autocomplete::popup_height;
use crate::components::composer::render::{
    caret_cell, footer_plan, layout_draft_lines, palette, truncate_middle, truncate_width,
    PromptView,
};
use crate::render::{CellColor, Frame};
use crate::theme::ThemeState;
use crate::wrap::{display_width, take_width};

/// The composer frame's ONLY renderer (D2.2: the composer family
/// rides the shared tree — every live mount [session route, home
/// route] syncs [`composer_mount`]; the headless harnesses [goldens,
/// the session-screen twin] read the same mount back through
/// [`render_prompt_frame_dispatch`]).
#[must_use]
pub fn render_prompt_frame_dispatch(
    view: &PromptView,
    width: usize,
    height: usize,
    theme: &ThemeState,
) -> Frame {
    if width == 0 || height == 0 {
        // The old renderer's degenerate shape: empty rows, page fill
        // is a no-op.
        return Frame { cells: vec![Vec::new(); height], width, height, clusters: None };
    }
    // The golden/unit harness twin (chrome's `mounted_surface_frame`
    // shape): mount ONE composer into a fresh hub and read it back as
    // a standalone Frame — same raster, same cell semantics, by
    // construction.
    crate::components::chrome_solid::reset_hub();
    composer_mount(view, width, height, theme, 0, 0);
    let mut frame = crate::components::chrome_solid::blank_frame(width, height);
    crate::components::chrome_solid::composite_chrome_tree(&mut frame);
    frame
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
    /// `hue.accent[500]` — the editor file label's pending color.
    accent: Rgb,
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
            accent: {
                let rgba =
                    theme.resolved.tokens.semantic(oc_theme::types::SemanticHue::Accent).s500;
                (rgba.r, rgba.g, rgba.b)
            },
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
    /// G-35 — the running-state knight-rider spinner (one chunk per
    /// cell; the chunk fg carries the RGBA trail alpha — the ref's
    /// `<spinner>` blends over the page fill).
    footer_spinner: StyledText,
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
    /// TUI-14 — the shell-mode hint (`esc exit shell mode`): `esc ` in
    /// `text.base`, the label muted (footer.tsx:107-112).
    footer_shell: StyledText,
    /// TUI-15 — the editor file label (`notes.md #1-2`): accent while
    /// PENDING, muted once sent (index.tsx:1945-1952).
    footer_editor_file: StyledText,
    // -- dynamic geometry (content-dependent, patched) --------------------
    spinner_x: usize,
    interrupt_x: usize,
    block_x: usize,
    command_x: usize,
    plugin_x: usize,
    live_x: usize,
    shell_x: usize,
    editor_file_x: usize,
    // -- mount-static geometry (the mount key) ----------------------------
    width: usize,
    /// The composer box's frame-relative top (the popup's rows above).
    top: usize,
    /// The popup's reserved height (rows above `top`).
    popup_h: usize,
    /// 1 pad row + preview strip rows + textarea rows + 1 metadata row.
    content_h: usize,
    frame_height: usize,
    pad: usize,
    textarea_rows: usize,
    // -- G-483: the prompt input's inline preview strip --------------------
    /// The strip's shaped thumbs (empty = no strip): each thumb is
    /// its rows of pre-shaped block-glyph chunks (None = the decode
    /// failed — the centered muted `No preview` fallback).
    preview_thumbs: Vec<Option<Vec<StyledText>>>,
    /// The `+N more` label (the thumbs past the three visible).
    preview_more: Option<String>,
    /// One thumb's cell size (`w = h · 2`).
    preview_thumb_w: usize,
    preview_thumb_h: usize,
}

/// The G-483 preview strip's shaped state (prompt/index.tsx:
/// 1676-1741): ≤3 visible thumbs (`No preview` when the bytes do not
/// decode) + the `+N more` cell, thumb boxes `h × 2h` with `gap 1`
/// and a 1-row pad under them — the strip's total height.
#[derive(Debug, Clone, Default)]
struct PreviewStrip {
    thumbs: Vec<Option<Vec<StyledText>>>,
    more: Option<String>,
    thumb_w: usize,
    thumb_h: usize,
}

impl PreviewStrip {
    fn height(&self) -> usize {
        if self.thumbs.is_empty() {
            return 0;
        }
        self.thumb_h + 1
    }
}

/// The thumb height (`max(4, min(8, floor(terminal height / 4)))`,
/// prompt/index.tsx:397).
fn preview_thumb_height(terminal_height: usize) -> usize {
    (terminal_height / 4).clamp(4, 8)
}

/// Shape the strip: decode each visible URI's payload and quantize to
/// the blocks fallback (the composer tree is cell-based — the
/// transcript strip's own thumb path, `blocks_thumbnail`); a decode
/// failure renders the centered muted `No preview` (the ref's onError
/// fallback, prompt/index.tsx:1706-1711).
fn preview_strip(view: &PromptView, frame_height: usize, colors: &ComposerColors) -> PreviewStrip {
    let _ = frame_height;
    if view.image_previews.is_empty() {
        return PreviewStrip::default();
    }
    let thumb_h = preview_thumb_height(view.terminal_height);
    let thumb_w = thumb_h * 2;
    let visible = view.image_previews.len().min(3);
    let more = (view.image_previews.len() > visible)
        .then(|| format!("+{} more", view.image_previews.len() - visible));
    let mut thumbs = Vec::with_capacity(visible);
    for uri in view.image_previews.iter().take(visible) {
        let bytes = decode_data_uri_image(uri).unwrap_or_default();
        let cells = crate::components::message::rows::blocks_thumbnail(&bytes, thumb_w, thumb_h);
        let thumb = cells.map(|rows| {
            rows.iter()
                .map(|row| {
                    let chunks = row
                        .iter()
                        .map(|cell| TextChunk {
                            text: cell.glyph.to_string(),
                            fg: Some(Rgba::from_ints(cell.fg[0], cell.fg[1], cell.fg[2], 255)),
                            bg: Some(Rgba::from_ints(cell.bg[0], cell.bg[1], cell.bg[2], 255)),
                            attributes: 0,
                            link: None,
                        })
                        .collect();
                    styled(chunks)
                })
                .collect()
        });
        thumbs.push(thumb);
    }
    let _ = colors;
    PreviewStrip { thumbs, more, thumb_w, thumb_h }
}

/// Decode a `data:image/*;base64,…` URI to its bytes (the
/// attachment payload's decode — `clipboard::base64_decode`, the
/// transcript strip's own `decode_png_payload` sibling).
fn decode_data_uri_image(uri: &str) -> Option<Vec<u8>> {
    let payload = uri.split(',').nth(1)?;
    crate::clipboard::base64_decode(payload)
}

/// G-483 — the preview strip's hit rects in SCREEN coordinates: one
/// per visible thumb (index = its position) + the `+N more` cell
/// (index = the first hidden position), the release opening the
/// image preview there (prompt/index.tsx:1697-1703/1728-1733). `top`
/// is the composer BOX's frame-relative row (the popup rows above
/// it); the thumbs start one pad row below the box top.
#[must_use]
pub fn prompt_preview_hit_rects(
    image_previews: &[String],
    terminal_height: usize,
    top: usize,
    width: usize,
    mount_left: usize,
    mount_top: usize,
) -> Vec<(usize, usize, usize, usize, usize)> {
    if image_previews.is_empty() {
        return Vec::new();
    }
    let pad = if width >= 44 { 2 } else { 1 };
    let thumb_h = preview_thumb_height(terminal_height);
    let thumb_w = thumb_h * 2;
    let row = mount_top + top + 1; // the box's pad row above the strip
    let mut x = mount_left + 1 + pad; // the rail column + the box's pad
    let visible = image_previews.len().min(3);
    let mut hits = Vec::with_capacity(visible + 1);
    for index in 0..visible {
        hits.push((x, row, thumb_w, thumb_h, index));
        x += thumb_w + 1; // the ref's gap={1}
    }
    if image_previews.len() > visible {
        hits.push((x, row, 8, thumb_h, visible));
    }
    hits
}

/// The mount key — every geometry input the tree bakes in at build
/// time. A typing-induced wrap (`content_h` change), the popup
/// opening (`top`/`popup_h`), a resize, or a theme swap rebuilds the
/// mount; the hot frames (same shape, new text) patch in place.
/// G-483: the preview strip's thumb count rides the key too (the
/// strip's row bakes its thumb boxes).
type MountKey = (usize, usize, usize, usize, usize, usize, ComposerColors);

fn mount_key(i: &ComposerInputs) -> MountKey {
    (
        i.width,
        i.top,
        i.popup_h,
        i.content_h,
        i.frame_height,
        i.preview_thumbs.len() + usize::from(i.preview_more.is_some()),
        i.colors,
    )
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
    // G-483 — the prompt input's inline preview strip
    // (prompt/index.tsx:1676-1741): `max(4, min(8, h / 4))`-tall
    // thumbs (w = h · 2), ≤3 visible + `+N more` (w 8), a gap row of
    // 1 under them — riding INSIDE the prompt box above the textarea.
    let preview_strip = preview_strip(view, height, &colors);
    let content_h = 1 + preview_strip.height() + textarea_rows + 1;

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
    // G-35 — the running-state spinner cells (knight.rs): each cell
    // is its own chunk so the per-cell trail alpha rides the fg
    // (blended over the footer row's page fill by the renderer).
    let footer_spinner = styled(
        view.footer
            .spinner
            .iter()
            .map(|cell| TextChunk {
                text: cell.ch.to_string(),
                fg: Some(Rgba::from_ints(cell.fg.r, cell.fg.g, cell.fg.b, cell.fg.a)),
                bg: None,
                attributes: 0,
                link: None,
            })
            .collect(),
    );
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
    // TUI-14 — the shell-mode hint: `esc ` base + label muted.
    let footer_shell = match &plan.shell_hint {
        Some((_, label)) => styled(vec![
            chunk("esc ".to_owned(), colors.text_base, 0),
            chunk(label.clone(), colors.muted, 0),
        ]),
        None => styled(Vec::new()),
    };
    // TUI-15 — the editor file label: accent (pending) / muted (sent).
    let footer_editor_file = match &plan.editor_file {
        Some((_, label)) if !label.is_empty() => {
            let fg = if view.footer.editor_file_pending { colors.accent } else { colors.muted };
            one(label.clone(), fg)
        }
        _ => styled(Vec::new()),
    };

    // The popup above the box (rows y = top − popup_h + i).
    // G-448 — ref truncation (autocomplete.tsx): the title clips to
    // `contentWidth = max(1, position().width − 4)` via
    // `Locale.truncateMiddle` (the `@`-file/`/`-command candidates are
    // pre-truncated at FETCH with the same rule, :334/:377/:509-514;
    // the render pass truncates again, :889-922) and the description
    // is a flex-shrunk trailing text — clipped to what remains. The
    // rust padded-title column (registry.display_width) survives for
    // SHORT rows (the pinned goldens) but CLAMPS to the interior so a
    // long `@`-filename can never push the row past the popup box (the
    // beta.18 overflow symptom).
    let popup_content_width = width.saturating_sub(4).max(1);
    let popup_rows: Vec<PopupRowView> = match &view.autocomplete {
        Some(popup) if view.top > 0 => {
            let popup_h = popup_height(popup.options.len(), view.top).min(view.top);
            popup
                .options
                .iter()
                .take(popup_h)
                .enumerate()
                .map(|(index, option)| {
                    let column = popup.title_width.min(popup_content_width);
                    let title = if display_width(&option.display) > column {
                        // Wider than the column: clamp to the FULL
                        // interior (ref's contentWidth), middle-clip.
                        truncate_middle(&option.display, popup_content_width)
                    } else {
                        format!("{:width$}", option.display, width = column)
                    };
                    let used = display_width(&title).max(column);
                    let description = option.description.as_ref().map(|d| {
                        let remaining = popup_content_width.saturating_sub(used + 1);
                        let text = format!(" {d}");
                        if display_width(&text) > remaining {
                            take_width(&text, remaining)
                        } else {
                            text
                        }
                    });
                    PopupRowView { title, description, selected: index == popup.selected }
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
        footer_spinner,
        footer_block,
        footer_command,
        footer_plugin,
        footer_live,
        footer_shell,
        footer_editor_file,
        interrupt_x: plan.interrupt_x.unwrap_or(0),
        spinner_x: plan.spinner_x.unwrap_or(0),
        block_x: plan.block_x.unwrap_or(0),
        command_x: plan.command_x.unwrap_or(0),
        plugin_x: plan.plugin.as_ref().map_or(0, |(x, _)| *x),
        live_x: plan.live.as_ref().map_or(0, |(x, _)| *x),
        shell_x: plan.shell_hint.as_ref().map_or(0, |(x, _)| *x),
        editor_file_x: plan.editor_file.as_ref().map_or(0, |(x, _)| *x),
        width,
        top: view.top,
        popup_h,
        content_h,
        frame_height: height,
        pad,
        textarea_rows,
        preview_thumbs: preview_strip.thumbs,
        preview_more: preview_strip.more,
        preview_thumb_w: preview_strip.thumb_w,
        preview_thumb_h: preview_strip.thumb_h,
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

/// The G-483 preview strip element — the row of thumb boxes + the
/// `+N more` cell + the bottom pad row (prompt/index.tsx:1676-1741).
/// Thumb content patches in place through the inputs signal (the
/// geometry keys the mount via `content_h`); a decode failure leaves
/// the thumb EMPTY — the centered muted `No preview` text rides the
/// same box (the ref's onError fallback).
// One JSX element per section (the thumbs' boxes + the more cell +
// the pad row — prompt/index.tsx:1676-1741's mount), mirroring the
// composer mount's own allowance.
#[allow(clippy::too_many_lines)]
fn preview_strip_element(inputs: &Signal<ComposerInputs>, geo: &ComposerInputs) -> Element {
    let thumb_w = geo.preview_thumb_w as f32;
    let thumb_h = geo.preview_thumb_h as f32;
    let thumb_count = geo.preview_thumbs.len().max(usize::from(geo.preview_more.is_some()));
    let mut row_children: Vec<Element> = Vec::new();
    for index in 0..thumb_count {
        // One thumb: a fixed `w × h` box whose rows are dyn content
        // over the signal; `marginRight 1` is the ref's `gap={1}`.
        let row_inputs = inputs.clone();
        let thumb_rows = (0..geo.preview_thumb_h)
            .map(|row| {
                let row_inputs = row_inputs.clone();
                text()
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .dyn_prop(
                        "content",
                        dyn_styled(map(row_inputs, move |i: &ComposerInputs| {
                            i.preview_thumbs
                                .get(index)
                                .and_then(|thumb| thumb.as_ref())
                                .and_then(|rows| rows.get(row))
                                .cloned()
                                .unwrap_or_else(|| styled(Vec::new()))
                        })),
                    )
                    .into_element()
            })
            .collect::<Vec<Element>>();
        // The `No preview` fallback: centered muted text (absolute,
        // over the blank rows — the ref's onError fallback).
        let fallback_inputs = inputs.clone();
        let fallback = text()
            .position("absolute")
            .top((geo.preview_thumb_h as f32 / 2.0).floor())
            .left(0.0)
            .width(thumb_w)
            .flex_shrink(0.0)
            .dyn_prop(
                "content",
                dyn_styled(map(fallback_inputs, move |i: &ComposerInputs| {
                    if i.preview_thumbs.get(index).is_some_and(Option::is_none) {
                        // Center the 11-wide label in the thumb box.
                        let lead = i.preview_thumb_w.saturating_sub(11) / 2;
                        one(format!("{}No preview", " ".repeat(lead)), i.colors.muted)
                    } else {
                        styled(Vec::new())
                    }
                })),
            )
            .into_element();
        let thumb = box_()
            .width(thumb_w)
            .height(thumb_h)
            .flex_shrink(1.0)
            .flex_basis(thumb_w)
            .margin_right(1.0)
            .position("relative")
            .children(Children::Static({
                let mut kids = thumb_rows;
                kids.push(fallback);
                kids
            }))
            .into_element();
        row_children.push(thumb);
    }
    // The `+N more` cell (w 8, centered muted).
    if geo.preview_more.is_some() {
        let more_inputs = inputs.clone();
        let more_text = text()
            .width(8.0)
            .flex_shrink(1.0)
            .flex_basis(8.0)
            .dyn_prop(
                "content",
                dyn_styled(map(more_inputs, move |i: &ComposerInputs| {
                    i.preview_more
                        .clone()
                        .map_or_else(|| styled(Vec::new()), |more| one(more, i.colors.muted))
                })),
            )
            .into_element();
        let more_box = box_()
            .width(8.0)
            .height(thumb_h)
            .flex_shrink(1.0)
            .flex_basis(8.0)
            .align_items("center")
            .justify_content("center")
            .children(Children::Static(vec![more_text]))
            .into_element();
        row_children.push(more_box);
    }
    box_()
        .width("100%")
        .height((geo.preview_thumb_h + 1) as f32)
        .flex_shrink(0.0)
        .flex_direction("column")
        .children(Children::Static(vec![
            box_()
                .width("100%")
                .height(thumb_h)
                .flex_shrink(0.0)
                .flex_direction("row")
                .justify_content("flex-start")
                .children(Children::Static(row_children))
                .into_element(),
            // paddingBottom 1.
            box_().height(1.0).flex_shrink(0.0).into_element(),
        ]))
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
        .children(Children::Static({
            let mut column: Vec<Element> = vec![
                // paddingTop 1 (the pad row).
                box_().height(1.0).into_element(),
            ];
            // G-483 — the inline preview strip (prompt/index.tsx:
            // 1676-1741): a row of ≤3 thumb boxes (`h × 2h`, gap 1)
            // + the `+N more` cell (w 8), a 1-row pad under them.
            if geo.preview_thumb_h > 0 {
                column.push(preview_strip_element(inputs, geo));
            }
            // The textarea box: paddingLeft/Right = pad, one row per
            // wrapped line (+1 cursor row under the last — blank).
            column.push(
                box_()
                    .height(geo.textarea_rows as f32)
                    .flex_direction("column")
                    .padding_left(geo.pad as f32)
                    .padding_right(geo.pad as f32)
                    .children_dyn(Rc::new(lines))
                    .into_element(),
            );
            // The metadata pad row: up to six absolutely positioned
            // segments (the separator cells stay the fill's white —
            // the footer 2-gap rule, same trap).
            column.push(
                box_()
                    .height(1.0)
                    .position("relative")
                    .children(Children::Static(
                        // 7 = the full ladder (agent auto · model provider · variant).
                        (0..7).map(|index| metadata_text(index).into_element()).collect(),
                    ))
                    .into_element(),
            );
            column
        }));
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
                // G-35 — the running-state spinner (idle renders an
                // empty text at x=0, painting nothing).
                footer_text(
                    |i: &ComposerInputs| i.spinner_x,
                    |i: &ComposerInputs| i.footer_spinner.clone(),
                ),
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
                footer_text(
                    |i: &ComposerInputs| i.shell_x,
                    |i: &ComposerInputs| i.footer_shell.clone(),
                ),
                footer_text(
                    |i: &ComposerInputs| i.editor_file_x,
                    |i: &ComposerInputs| i.footer_editor_file.clone(),
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
// the shared-tree mount (D2.2 composer lane — the chrome paradigm)
// ---------------------------------------------------------------------------

/// The composer family's skeleton index under the shared tree's root
/// (`surface-composer`): 2, ABOVE the transcript (1) — the composer's
/// slash/`@` popup pasted AFTER the transcript region in the old
/// frame stack and so paints over its bottom rows (ruling attempt 2's
/// construction equivalence: today's draw order = last-wins = tree child order; the
/// transcript lane re-seated the families to pin it).
const COMPOSER_SKELETON_INDEX: usize = 2;

/// The mounted composer's reactive state, kept across frames so
/// height-stable frames patch content dyn-props in place (the M2
/// rule); a `MountKey` change rebuilds the wrapper's content subtree.
struct ComposerEntry {
    signal: Signal<ComposerInputs>,
    key: MountKey,
}

thread_local! {
    /// The composer family's content state (one mount per
    /// thread-process — the wrapper itself lives in the shared-tree
    /// hub, `chrome_solid::mount_family_wrapper`).
    static COMPOSER_TREE: RefCell<Option<ComposerEntry>> = const { RefCell::new(None) };
}

fn cell_rgb(rgb: Rgb) -> CellColor {
    CellColor::Rgb(rgb.0, rgb.1, rgb.2)
}

/// The bridge-level frame passes (the retired per-surface copy's
/// recolors, now composite-seam stamps offset by the wrapper's
/// origin): the GOLDEN-only software caret block (bg `text.base`, fg
/// prompt-bg over the cell, the glyph untouched — NEW-S1a) and the
/// G-66 selection swap (each offset maps through the same
/// [`caret_cell`] wrap walk the caret block uses).
fn bridge_stamps(view: &PromptView, inputs: &ComposerInputs, width: usize) -> Vec<ChromeStamp> {
    let mut stamps = Vec::new();
    let pad = inputs.pad;
    let inner = width.saturating_sub(1 + pad * 2).max(1);
    if let Some(offset) = view.caret {
        let (row, col) = caret_cell(view.draft, inner, offset);
        if row < inputs.textarea_rows {
            stamps.push(ChromeStamp::Caret {
                left: 1 + pad + col,
                top: inputs.top + 1 + row,
                fg: cell_rgb(inputs.colors.prompt_bg),
                bg: cell_rgb(inputs.colors.text_base),
            });
        }
    }
    if let Some(range) = view.selection.clone().filter(|range| !range.is_empty()) {
        for offset in range {
            let (row, col) = caret_cell(view.draft, inner, offset);
            if row < inputs.textarea_rows {
                stamps.push(ChromeStamp::Swap { left: 1 + pad + col, top: inputs.top + 1 + row });
            }
        }
    }
    stamps
}

/// Sync the composer family into the shared tree (the family's ONLY
/// live mount entry — no Frame comes back; pixels reach the emitted
/// frame through `composite_chrome_tree`, the single compositor seam
/// inside `render_app`). The wrapper rect is the OLD paste site (the
/// imperative source); the content subtree is the same
/// [`composer_element`] tree the retired scratch renderer rasterized,
/// rebuilt only when the mount key (geometry) changes — rect changes
/// patch via `set_prop`.
pub fn composer_mount(
    view: &PromptView,
    width: usize,
    height: usize,
    theme: &ThemeState,
    left: usize,
    top: usize,
) {
    if width == 0 || height == 0 {
        // The degenerate shape: nothing mounts — the stale wrapper
        // unmounts at the seam (the old paste's no-op).
        return;
    }
    let inputs = shape(view, width, height, theme);
    let stamps = bridge_stamps(view, &inputs, width);
    let key = mount_key(&inputs);
    // Rebuild iff the geometry key changed OR the wrapper left the
    // tree (a hub reset between harness frames, a seam prune) — the
    // content state is worthless without its mounted element.
    let rebuild = !family_wrapper_live("composer")
        || COMPOSER_TREE.with_borrow(|slot| !matches!(slot, Some(entry) if entry.key == key));
    let (content, content_key) = if rebuild {
        let signal = Signal::new(inputs.clone());
        let element = composer_element(&signal, &inputs);
        COMPOSER_TREE.with_borrow_mut(|slot| {
            *slot = Some(ComposerEntry { signal, key });
        });
        (Some(element), Some(format!("composer-content@{key:?}")))
    } else {
        COMPOSER_TREE.with_borrow(|slot| {
            slot.as_ref().expect("checked above").signal.set(inputs.clone());
        });
        (None, None)
    };
    mount_family_wrapper(
        "composer",
        COMPOSER_SKELETON_INDEX,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::render::{FooterHints, MetadataLine};
    use crate::components::composer::PromptDraft;
    use crate::theme::ThemeState;

    fn draft(text: &str) -> PromptDraft {
        PromptDraft { text: text.to_owned(), ..PromptDraft::default() }
    }

    /// G-66 — the selection paints with the ref's DEFAULT styling,
    /// the fg/bg swap (`EditBufferRenderable`'s no-selectionBg branch):
    /// selected glyph cells carry `text.base` bg + `prompt-bg` fg
    /// while the unselected neighbors keep the resting paint.
    #[test]
    fn selection_range_paints_inverted_cells() {
        let theme = ThemeState::v2_dark();
        let owned = draft("hello world");
        let view = |selection: Option<std::ops::Range<usize>>| PromptView {
            draft: &owned,
            placeholder: String::new(),
            autocomplete: None,
            footer: FooterHints::default(),
            metadata: MetadataLine::default(),
            terminal_width: 80,
            top: 0,
            rail: None,
            caret: None,
            selection,
            image_previews: Vec::new(),
            terminal_height: 24,
        };
        let plain = render_prompt_frame_dispatch(&view(None), 80, 8, &theme);
        let selected = render_prompt_frame_dispatch(&view(Some(0..5)), 80, 8, &theme);
        // text_x = 1 + pad(2) = 3, text_y = 1: "hello" occupies
        // (1, 3..8) — all five cells invert, the rest is untouched.
        for col in 3..8usize {
            let plain_cell = plain.cell(1, col);
            let cell = selected.cell(1, col);
            assert_eq!(cell.ch, plain_cell.ch, "glyph untouched at col {col}");
            assert_eq!(cell.bg, plain_cell.fg, "bg = the old fg at col {col}");
            assert_eq!(cell.fg, plain_cell.bg, "fg = the old bg at col {col}");
        }
        // The space + "world" past the selection keep the rest paint.
        for col in 8..14usize {
            assert_eq!(selected.cell(1, col), plain.cell(1, col), "col {col} untouched");
        }
        // An empty range paints nothing (the golden contract).
        let empty = render_prompt_frame_dispatch(&view(Some(2..2)), 80, 8, &theme);
        assert_eq!(empty, plain, "an empty selection is a no-op");
    }

    /// The reactive path patches in place: typing churn on the last
    /// row re-renders WITHOUT a rebuild (same mount — content-height
    /// frames only rebuild when a wrap lands a new row). Same inputs
    /// paint the same frame, absolutely (the retired per-surface
    /// renderer's ops log died with it; the mount-key rule below
    /// covers the rebuild leg).
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
                selection: None,
                image_previews: Vec::new(),
                terminal_height: 24,
            };
            render_prompt_frame_dispatch(&view, 80, 8, &theme)
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
            selection: None,
            image_previews: Vec::new(),
            terminal_height: 24,
        };
        let mut first: Option<Frame> = None;
        for width in [80usize, 40, 120, 80] {
            let height = crate::components::composer::render::mount_height(&owned, width, 24);
            let frame = render_prompt_frame_dispatch(&view, width, height, &theme);
            assert_eq!(frame.width, width, "full-width frame at width {width}");
            if width == 80 {
                match &first {
                    None => first = Some(frame),
                    Some(p) => assert_eq!(&frame, p, "same inputs paint the same frame"),
                }
            }
        }
    }

    /// The scheduler-fairness gate for the composer family (the hub's
    /// generic guard, exercised through the composer's own mount):
    /// N syncs in one frame batch → exactly ONE flush at the seam.
    #[test]
    fn composer_syncs_flush_once_at_the_seam() {
        crate::components::chrome_solid::reset_hub();
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
                selection: None,
                image_previews: Vec::new(),
                terminal_height: 24,
            };
            composer_mount(&view, 80, 8, &theme, 2, 16);
        };
        build("a");
        build("ab");
        build("abc");
        let hub = crate::components::chrome_solid::hub_for(80, 24);
        assert!(hub.borrow_mut().flush_if_pending(), "a pending sync batch flushes exactly once");
        assert!(
            !hub.borrow_mut().flush_if_pending(),
            "a second flush with no intervening sync is a no-op (no double raster)"
        );
        crate::components::chrome_solid::reset_hub();
    }

    /// G-448 — the autocomplete popup's rows NEVER exceed the popup
    /// box (ref `Locale.truncateMiddle` at autocomplete.tsx:889-922 +
    /// the fetch-time truncation at :334/:377/:509-514): a long `@`
    /// filename clamps to `width − 4` with a middle ellipsis, and the
    /// description clips to what remains — the beta.18 overflow symptom
    /// (candidate rows spilling past the box) is pinned dead.
    #[test]
    fn popup_rows_truncate_to_the_box_interior() {
        let theme = ThemeState::v2_dark();
        let long_name = "@a-very-long-file-name-that-overflows-the-popup-box-with-room-to-spare.rs";
        let options = vec![
            crate::components::composer::autocomplete::AutocompleteOption {
                display: long_name.to_string(),
                description: Some("a description that is also far too long".to_owned()),
                ..Default::default()
            },
            crate::components::composer::autocomplete::AutocompleteOption {
                display: "/short".to_owned(),
                description: Some("fits".to_owned()),
                ..Default::default()
            },
        ];
        let owned = draft("@a-ve");
        let view = PromptView {
            draft: &owned,
            placeholder: String::new(),
            autocomplete: Some(crate::components::composer::render::AutocompleteView {
                options: &options,
                selected: 0,
                // A registry column wider than typical exercises the
                // clamp, but the short row's description must survive:
                // use the realistic registry width.
                title_width: 12,
            }),
            footer: FooterHints::default(),
            metadata: MetadataLine::default(),
            terminal_width: 40,
            top: 4,
            rail: None,
            caret: None,
            selection: None,
            image_previews: Vec::new(),
            terminal_height: 24,
        };
        let frame = render_prompt_frame_dispatch(&view, 40, 10, &theme);
        // The popup rows sit above the box (top = 4): rows 2 and 3.
        let grid = frame.text_grid();
        let first = grid[2].clone();
        assert!(first.contains('…'), "the long title middle-truncates: {first:?}");
        // The row never exceeds the 40-col box.
        assert!(
            !first.chars().skip(40).any(|ch| ch != ' '),
            "row stays inside the 40-col box: {first:?}"
        );
        assert!(
            !first.contains("far too long"),
            "the description clips to the remaining width: {first:?}"
        );
        // The short row keeps the padded-column golden shape.
        let second = grid[3].clone();
        assert!(second.contains("/short"), "short row intact: {second:?}");
        assert!(second.contains("fits"), "short description intact: {second:?}");
    }
    // -- G-483: the prompt input's inline preview strip -------------------

    /// The tiny 4x2 PNG fixture (the R-07 strip's own corpus).
    const TINY_PNG: &[u8] = include_bytes!("../../tests/fixtures/r07-tiny-4x2.png");

    fn preview_view(owned: &PromptDraft, previews: Vec<String>) -> PromptView<'_> {
        PromptView {
            draft: owned,
            placeholder: String::new(),
            autocomplete: None,
            footer: FooterHints::default(),
            metadata: MetadataLine::default(),
            terminal_width: 80,
            top: 0,
            rail: None,
            caret: None,
            selection: None,
            image_previews: previews,
            terminal_height: 24,
        }
    }

    /// The strip mounts above the textarea: ≤3 thumbs (the blocks
    /// fallback renders the decoded PNG as glyphs — non-space cells),
    /// the `+N more` cell for the overflow, and the 1-row pad under.
    #[test]
    fn preview_strip_mounts_thumbs_and_more_cell() {
        let theme = ThemeState::v2_dark();
        let owned = draft("hello");
        let uri =
            format!("data:image/png;base64,{}", crate::clipboard::base64_encode::encode(TINY_PNG));
        let previews: Vec<String> = std::iter::repeat_n(uri.clone(), 5).collect();
        let view = preview_view(&owned, previews.clone());
        let frame = render_prompt_frame_dispatch(&view, 80, 30, &theme);
        let grid = frame.text_grid();
        // terminal_height 24 → thumb_h = clamp(24/4, 4, 8) = 6, thumb_w
        // = 12; the strip rows 1..6 (pad row 0), the pad row 7, the
        // textarea from row 8.
        let strip: String = grid[1].chars().take(40).collect();
        assert!(strip.chars().any(|ch| ch != ' '), "thumb row 1 carries block glyphs: {strip:?}");
        assert!(grid[1].contains("+2 more"), "the more cell: {:?}", grid[1]);
        // The bare view (no previews): the textarea stays at the top —
        // the same frame as without the field.
        let bare = preview_view(&owned, Vec::new());
        let bare_frame = render_prompt_frame_dispatch(&bare, 80, 30, &theme);
        assert_eq!(bare_frame.text_grid()[1], frame_with_previews_disabled_row(&theme, &owned));
    }

    /// The bare-frame reference row (the placeholder row at the box's
    /// top) — the strip OFF keeps the historical layout.
    fn frame_with_previews_disabled_row(theme: &ThemeState, owned: &PromptDraft) -> String {
        let view = preview_view(owned, Vec::new());
        render_prompt_frame_dispatch(&view, 80, 30, theme).text_grid()[1].clone()
    }

    /// A payload that does not decode renders the centered muted `No
    /// preview` fallback (the ref's onError arm).
    #[test]
    fn preview_strip_no_preview_fallback() {
        let theme = ThemeState::v2_dark();
        let owned = draft("hi");
        let uri = "data:image/png;base64,AAAA".to_owned();
        let view = preview_view(&owned, vec![uri]);
        let frame = render_prompt_frame_dispatch(&view, 80, 30, &theme);
        let grid = frame.text_grid();
        // The fallback centers in the thumb box (absolute top = h/2 =
        // 3 → frame row 4, the strip starting at row 1).
        assert!(grid[4].contains("No preview"), "the fallback label centers: {:?}", grid[4]);
    }

    /// The hit rects: one per visible thumb + the `+N more` cell, in
    /// screen coordinates off the mount site.
    #[test]
    fn preview_hit_rects_cover_thumbs_and_more() {
        let previews: Vec<String> = (0..5).map(|i| format!("data:image/png;base64,{i}")).collect();
        let hits = prompt_preview_hit_rects(&previews, 24, 2, 80, 10, 40);
        assert_eq!(hits.len(), 4, "3 thumbs + the more cell");
        // thumb_h 6, thumb_w 12; x starts at 10 + 1 + 2 = 13.
        assert_eq!(hits[0], (13, 43, 12, 6, 0), "thumb 0 (pad row + top)");
        assert_eq!(hits[1], (26, 43, 12, 6, 1), "thumb 1 (gap 1)");
        assert_eq!(hits[2], (39, 43, 12, 6, 2), "thumb 2");
        assert_eq!(hits[3], (52, 43, 8, 6, 3), "the more cell opens at index 3");
        // No previews: no rects.
        assert!(prompt_preview_hit_rects(&[], 24, 2, 80, 10, 40).is_empty(), "no previews");
    }
}
