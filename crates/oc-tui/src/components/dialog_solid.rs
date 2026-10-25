//! Lane M2: the dialog family re-homed onto opentui-solid — the
//! first at-scale replication of the M1 tab-strip pattern
//! (`docs/migration-m1-tabstrip.md`).
//!
//! **What this module is.** A 1:1 structural port of the dialog
//! family's view layer — `ui/dialog-select.tsx` (the select family:
//! palette, models, themes, worktrees, queued prompts, server lists,
//! message actions, working directory, plugin selects, the
//! delete-confirm), `ui/dialog-prompt.tsx` (rename / worktree name),
//! `ui/dialog-help.tsx`, and the plugin rows-only content — as an
//! opentui-solid builder tree, driven by the SAME store data the
//! hand-rolled renderer (`crate::components::dialog` /
//! `dialog_select`) consumes. The dialog state machine, the modal key
//! layer, the mouse routing (`option_dialog_rows` → `app.rs`), and
//! the dispatcher are NOT migrated: only the view layer.
//!
//! **Selection flag (retired).** The lane-M2 dual-path flag
//! (`OC_DIALOGS_OPENTUI`) is gone: this module's opentui-solid tree
//! is the dialog family's ONLY renderer — the hand-rolled walk was
//! deleted after the one-release probation (the retirement lane).
//!
//! **The bridge** (the M1 shape, plus what this family taught — see
//! the M2 section of the pattern doc):
//!
//! * *inputs*: one [`Signal<DialogInputs>`] snapshot per paint —
//!   family, geometry, theme colors, the visible window (derived
//!   with the SAME solver the old path uses: `grouped`/
//!   `flat_indices`/`viewport_height`), the filter, the flat
//!   selection, actions.
//! * *reactive shape*: the option list is a `For` keyed by option
//!   index over the windowed rows (the CARRY-THE-EMPTY-STATE trick:
//!   the no-match row is a For item, not a sibling `Show` — one
//!   element carries one dynamic-children source); per-row paint
//!   (background, marker, title chunks, footer) are dynamic props
//!   over per-key mapped views, so a selection move patches
//!   `backgroundColor`/`content` in place without remounting rows.
//! * *frame out*: the tree paints through a real
//!   [`opentui_core::renderer::CliRenderer`] (taffy + raster) into an
//!   `OptimizedBuffer` sized exactly `(box_width, 1 + content_rows)`
//!   — the height PRE-COMPUTED by the same walk that sizes the old
//!   path's row vector; the M1 cell-copy bridge (`frame_from_buffer`)
//!   converts it to oc-tui cells composited onto the route frame at
//!   the chrome box's rect. The mount is keyed on (family, width,
//!   colors, height): height-changing frames (a filter shrinking the
//!   window) rebuild, the hot frames (selection moves, scroll) patch.
//! * *backdrop* (NEW-S2a): the scrim stays a bridge-side compositing
//!   step, NOT a tree node — the opentui raster alpha-composites over
//!   its OWN buffer, which does not contain the route content, so
//!   v2's observable semantics (bg always dimmed; fg dimmed only on
//!   glyph cells) is applied per cell over the base frame by the same
//!   `blend_backdrop` the old path uses.
//!
//! **Divergence class (documented, matrix-bounded — option rows
//! CLOSED by lane F15).** The old renderer once painted long text
//! runs past their box's padding edge (a title ran to the frame
//! edge; the footer then overwrote). Lane F15 ported the ref's
//! two-stage truncation into BOTH painters (stage-1 `Locale.
//! truncate(title, 61)` + the footer-respecting stage-2 clip — see
//! `dialog_select::truncated_title`/`title_clip_end`), so option
//! rows now clip at the title box edge exactly like the tsx's
//! `overflow: hidden`. The remaining documented divergence is the
//! non-option chrome (headers/empty rows), which carries no
//! overflow cases in the parity matrix.

// Terminal dimensions/colors are bounded u8/u16/usize→f32 casts, the
// same allowance the hand-rolled renderer carries.
// Bounded terminal dimensions (usize→f32 for taffy points).
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
use opentui_solid::{box_, for_list, map, show, text, Element, Signal, SolidRenderer};

use crate::components::dialog_select::{
    content_rows, flat_indices, grouped, title_clip_end, truncated_title, viewport_height,
    DialogStyles, SelectOption, SelectProps, SelectState,
};
use crate::render::Frame;
use crate::store::{DialogEntry, DialogKind, PluginDialogContent};
use crate::theme::ThemeState;
use crate::wrap::{display_width, take_width};

// ---------------------------------------------------------------------------
// theme colors (pre-hexed once per sync)
// ---------------------------------------------------------------------------

/// An RGB triple (the port's cell currency).
type Rgb = (u8, u8, u8);

/// Theme-derived colors the tree needs, resolved once per paint from
/// [`DialogStyles`] so the builders only format hex strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DialogColors {
    text: Rgb,
    muted: Rgb,
    accent: Rgb,
    surface: Rgb,
    selected_bg: Rgb,
    selected_fg: Rgb,
    current_fg: Rgb,
    formfield_bg: Rgb,
    formfield_fg: Rgb,
    formfield_base_fg: Rgb,
    destructive_fg: Rgb,
    destructive_bg: Rgb,
    /// `text.feedback.error.base` — the pair dialog's failed-load
    /// header (dialog-pair.tsx:121).
    error_fg: Rgb,
}

impl DialogColors {
    fn of(styles: &DialogStyles) -> Self {
        let rgb = |color: ratatui::style::Color| -> Rgb {
            match color {
                ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
                // The theme tokens always resolve to RGB; the
                // non-RGB arms are unreachable defensive fallbacks.
                _ => (128, 128, 128),
            }
        };
        DialogColors {
            text: rgb(styles.text),
            muted: rgb(styles.muted),
            accent: rgb(styles.accent),
            surface: rgb(styles.surface_bg),
            selected_bg: rgb(styles.selected_bg),
            selected_fg: rgb(styles.selected_fg),
            current_fg: rgb(styles.current_fg),
            formfield_bg: rgb(styles.formfield_bg),
            formfield_fg: rgb(styles.formfield_fg),
            formfield_base_fg: rgb(styles.formfield_base_fg),
            destructive_fg: rgb(styles.destructive_fg),
            destructive_bg: rgb(styles.destructive_bg),
            error_fg: rgb(styles.error_fg),
        }
    }
}

fn rgba(rgb: Rgb) -> Rgba {
    Rgba::rgb(rgb.0, rgb.1, rgb.2)
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

fn chunk(text: String, fg: Rgb, bg: Option<Rgb>, attributes: u32) -> TextChunk {
    TextChunk { text, fg: Some(rgba(fg)), bg: bg.map(rgba), attributes, link: None }
}

fn styled(chunks: Vec<TextChunk>) -> StyledText {
    StyledText::new(chunks)
}

// ---------------------------------------------------------------------------
// inputs (the store → view bridge)
// ---------------------------------------------------------------------------

/// The dialog family — which tsx tree mounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// `ui/dialog-select.tsx`.
    Select,
    /// `ui/dialog-prompt.tsx` (the session rename).
    RenamePrompt,
    /// `component/dialog-worktree-name.tsx` (the prompt-family twin).
    WorktreeNamePrompt,
    /// Lane FINAL: the connect-flow prompts (`KeyMethod` /
    /// `OAuthCode`, dialog-integration.tsx:471-506/676-728) — the
    /// prompt tree with a dynamic title + description rows.
    ConnectPrompt,
    /// `ui/dialog-help.tsx`.
    Help,
    /// The delegation payload's rows-only shape.
    PluginRows,
    /// The tab context menu (lane KM — `TabContextMenu`'s scrollbox,
    /// session-tabs.tsx:449-534): a rows-only column — no title
    /// block, no filter input, no footer.
    Menu,
    /// TUI-25: `ui/dialog-alert.tsx` — title + muted message + the
    /// right-justified `ok` pill.
    Alert,
    /// TUI-25: `ui/dialog-confirm.tsx` — title + muted message + the
    /// cancel/confirm pills (the active one carries the focused
    /// primary background).
    Confirm,
    /// TUI-24: `component/dialog-error-details.tsx` — title + source
    /// path, the 20-row scrollable error body, the reference row, the
    /// `i investigate / c copy details / ↑/↓ scroll` footer.
    ErrorDetails,
    /// TUI-10: `component/dialog-image-preview.tsx` — the `Image N
    /// of M` header, the image area (the cells paint in the bridge's
    /// post-pass), the `← previous / caption / next →` footer.
    ImagePreview,
    /// F15 fix round: `component/dialog-pair.tsx` — a CUSTOM dialog
    /// (never a `DialogSelect`): the bold `Pair` header + `esc`
    /// chrome, then the resource states (loading / error / the
    /// word-wrapped intro + link blocks + loopback hint).
    Pair,
}

/// One rendered row of the scroll window — the same walk the old
/// renderer performs (`render_select`'s content-row loop), collected
/// as row descriptors instead of painted cells. The pre-header pad
/// is its OWN row kind (the old renderer checks its visibility
/// independently of the header's — the window edge can split them).
#[derive(Debug, Clone, PartialEq, Eq)]
enum WindowRow {
    /// The blank row above every group header after the first group.
    HeaderPad,
    /// A group header (accent, bold).
    Header { category: String },
    /// One option (the props.options index — the For identity).
    Option { index: usize },
    /// The empty/no-match message (rendered only when the filtered
    /// list is empty; replaces the window, paddingLeft 4).
    Empty { message: String },
}

impl WindowRow {
    /// The For key — carries the row's payload so per-row maps can
    /// re-derive paint from the inputs (the M1 keyed-by-identity
    /// pattern; `o{index}` is stable across filter reorders).
    fn key(&self) -> String {
        match self {
            WindowRow::HeaderPad => "p".to_owned(),
            WindowRow::Header { category } => format!("h:{category}"),
            WindowRow::Option { index } => format!("o:{index}"),
            WindowRow::Empty { message } => format!("e:{message}"),
        }
    }
}

/// One frame's dialog snapshot — the ONLY input the tree reads.
#[derive(Clone)]
struct DialogInputs {
    family: Family,
    box_width: usize,
    colors: DialogColors,
    // -- the select family -------------------------------------------------
    title: String,
    placeholder: String,
    filter: String,
    options: Vec<SelectOption>,
    /// The full flat option-index list (post-filter) — the selection
    /// coordinate space.
    flat: Vec<usize>,
    selected: usize,
    /// The current option's VALUE (the `●` marker).
    current: Option<String>,
    /// A `flat` select under an active filter (search footers).
    flatten: bool,
    /// The destructive confirm variant (auditB #8 — the selected row
    /// paints `action.destructive.focused`).
    destructive: bool,
    /// The visible window (the For source).
    window: Vec<WindowRow>,
    /// (title, label) footer actions.
    actions: Vec<(String, String)>,
    // -- the prompt family -------------------------------------------------
    prompt_value: String,
    // -- help ---------------------------------------------------------------
    hint_lines: Vec<String>,
    // -- plugin rows ---------------------------------------------------------
    plugin_title: String,
    plugin_rows: Vec<String>,
    // -- TUI-25: the alert/confirm message + the confirm pills ------
    message: String,
    /// (cancel label, confirm label, the active pill is confirm).
    confirm_labels: (String, String, bool),
    // -- TUI-24: the error details' rows --------------------------------
    /// The source path row ("" hides it).
    error_source: String,
    /// The scroll WINDOW of the wrapped error body (already sliced).
    error_window: Vec<String>,
    /// The `Reference: …` row ("" hides it).
    diagnostic: String,
    // -- TUI-10: the image preview's chrome rows ------------------------
    /// The `Image N of M` header.
    preview_title: String,
    /// (← previous, the caption, next →) — the outer two "" when a
    /// single image.
    preview_footer: (String, String, String),
    /// The image area's row count (`max(3, height - 8)`).
    preview_rows: usize,
    // -- F15: the pair dialog's content rows ---------------------------
    /// The word-wrapped content walk (loading / error / loaded) —
    /// [`crate::components::dialog_pair::rows`].
    pair_rows: Vec<crate::components::dialog_pair::PairRow>,
}

/// TUI-10/24 + F15: the custom families' store-side sidecar — the
/// image preview's live state, the error-details scroll offset, the
/// pair dialog's resource state. Built by
/// [`crate::components::dialog::render_overlay`] (which owns the
/// store); the embedder API path carries the default.
#[derive(Debug, Default, Clone)]
pub struct DialogSidecar {
    pub preview: crate::store::ImagePreviewState,
    pub error_scroll: usize,
    /// `DialogPair`'s resource (`store.pair_state()` — the fetch
    /// seam itself is untouched; the sidecar only carries the
    /// snapshot the content walk reads).
    pub pair: crate::store::PairState,
}

/// The windowed rows — the old renderer's walk (`render_select`),
/// verbatim: per group, the pad above every headered group after
/// group 0, the header, then the options; each row's visibility
/// checked independently against `[scroll, scroll + viewport)`.
fn visible_window(
    props: &SelectProps,
    state: &SelectState,
    terminal_height: usize,
) -> (Vec<WindowRow>, Vec<usize>) {
    let groups = grouped(props, &state.filter);
    let indices = flat_indices(&groups);
    let viewport = viewport_height(content_rows(&groups), terminal_height);
    let visible = |row: usize| row >= state.scroll && row < state.scroll + viewport;
    let mut window: Vec<WindowRow> = Vec::new();
    let mut content_row = 0usize;
    for (group_i, group) in groups.iter().enumerate() {
        if group.category.is_some() && group_i > 0 {
            let row = content_row;
            content_row += 1;
            if visible(row) {
                window.push(WindowRow::HeaderPad);
            }
        }
        if let Some(category) = group.category.as_ref() {
            let row = content_row;
            content_row += 1;
            if visible(row) {
                window.push(WindowRow::Header { category: category.clone() });
            }
        }
        for &index in &group.indices {
            let row = content_row;
            content_row += 1;
            if visible(row) {
                window.push(WindowRow::Option { index });
            }
        }
    }
    // The empty states live OUTSIDE the scrollbox in the tsx (the
    // flexGrow fallback) — the old renderer pushes the message row
    // into the window only when the FILTERED list is empty.
    if indices.is_empty() {
        let message = if state.filter.is_empty() {
            props.empty_text.clone().unwrap_or_else(|| "No items available".to_owned())
        } else {
            props.no_match_text.clone().unwrap_or_else(|| "No results found".to_owned())
        };
        window.push(WindowRow::Empty { message });
    }
    (window, indices)
}

/// The content height (rows BELOW the chrome's paddingTop 1) — must
/// equal the old path's `content.rows.len()` exactly; the buffer is
/// sized from it.
fn content_height(inputs: &DialogInputs) -> usize {
    match inputs.family {
        // title block (title + paddingTop gap + input = 3) + root
        // gap + window + root gap + [footer] + paddingBottom 1.
        Family::Select => 4 + inputs.window.len() + 1 + usize::from(!inputs.actions.is_empty()) + 1,
        // title, gap, input, gap, footer, the footer's paddingBottom.
        Family::RenamePrompt | Family::WorktreeNamePrompt => 6,
        // title, gap, input, gap, hints, gap, footer, pad.
        Family::ConnectPrompt => inputs.hint_lines.len() + 8,
        // title, gap, wrapped lines, hint paddingBottom, root gap,
        // button, button-row paddingBottom.
        Family::Help => inputs.hint_lines.len() + 6,
        // title, gap, rows, root paddingBottom.
        Family::PluginRows => inputs.plugin_rows.len() + 3,
        // rows only (the menu's scrollbox).
        Family::Menu => inputs.window.len(),
        // TUI-25 (dialog-alert.tsx:37-57): title, gap, message + its
        // paddingBottom, gap, the ok pill + the row's paddingBottom.
        Family::Alert | Family::Confirm => 7,
        // TUI-24 (dialog-error-details.tsx:139-178): header (title +
        // the optional source row), gap, body window + the optional
        // reference row, gap, the action footer, root paddingBottom.
        Family::ErrorDetails => {
            1 + usize::from(!inputs.error_source.is_empty())
                + 1
                + inputs.error_window.len()
                + usize::from(!inputs.diagnostic.is_empty())
                + 1
                + 1
                + 1
        }
        // TUI-10 (dialog-image-preview.tsx:48-72): title, gap, the
        // image area (max(3, height-8) rows), gap, the footer, root
        // paddingBottom.
        Family::ImagePreview => inputs.preview_rows + 5,
        // F15 (dialog-pair.tsx:89-129): the `Pair` title row, the
        // root's gap 1, the content rows, the root paddingBottom 1.
        Family::Pair => inputs.pair_rows.len() + 3,
    }
}

/// The per-row paint state (the tsx per-option `createMemo` cluster:
/// `active`, `current`, the text-color ladder).
struct RowPaint {
    selected: bool,
    current: bool,
    /// The row background: selected → `selected_bg`; else the SURFACE
    /// (byte-identical to the old path's blank row — painting the
    /// surface over the chrome's surface leaves the same cells, and
    /// it keeps `backgroundColor` an always-set prop the reconciler
    /// patches in place).
    bg: Rgb,
    title_fg: Rgb,
    bold: bool,
    marker_fg: Rgb,
    footer_fg: Rgb,
}

fn row_paint(inputs: &DialogInputs, index: usize) -> Option<RowPaint> {
    let option = inputs.options.get(index)?;
    let selected = inputs.flat.iter().position(|flat| *flat == index) == Some(inputs.selected);
    let current = inputs.current.as_deref() == Some(option.value.as_str());
    let (focus_background, focus_foreground) = if inputs.destructive {
        (inputs.colors.destructive_bg, inputs.colors.destructive_fg)
    } else {
        (inputs.colors.selected_bg, inputs.colors.selected_fg)
    };
    let title_fg = if selected {
        focus_foreground
    } else if current {
        inputs.colors.current_fg
    } else {
        inputs.colors.text
    };
    Some(RowPaint {
        selected,
        current,
        bg: if selected { focus_background } else { inputs.colors.surface },
        title_fg,
        bold: selected,
        marker_fg: if selected { focus_foreground } else { inputs.colors.current_fg },
        footer_fg: if selected { focus_foreground } else { inputs.colors.muted },
    })
}

/// The row's right-aligned footer text: while flattened (a `flat`
/// select under an active filter) the search footer, else the plain
/// footer (`Option.footer`).
fn footer_text(inputs: &DialogInputs, index: usize) -> Option<String> {
    let option = inputs.options.get(index)?;
    if inputs.flatten {
        option
            .search_footer
            .clone()
            .or_else(|| option.category.clone())
            .or_else(|| option.footer.clone())
    } else {
        option.footer.clone()
    }
}

/// The option row's title chunks: `[title][ " description"]` — one
/// styled run per fg/attr change, contiguous columns. F15: the ref's
/// two-stage truncation (dialog-select.tsx:885-907) — stage 1 caps
/// the TITLE at `titleWidth ?? 61` (`Locale.truncate`,
/// [`truncated_title`]); stage 2 clips the whole run at the title
/// text box's edge: the row content edge less the footer's
/// `flexShrink=0` reservation and its `gap=1` column
/// ([`title_clip_end`]) — the footer keeps its columns and the gap
/// never shows a title glyph.
fn title_chunks(inputs: &DialogInputs, paint: &RowPaint, index: usize) -> StyledText {
    let Some(option) = inputs.options.get(index) else {
        return styled(Vec::new());
    };
    let description = match option.description.as_deref() {
        Some(description) if Some(description) != option.category.as_deref() => Some(description),
        _ => None,
    };
    // Stage 2 — the clip column is absolute (the row content edge
    // less the footer reservation); the chunk walk counts cells from
    // the title's start at box column 4, so the budget is the clip
    // less that offset.
    let clip =
        title_clip_end(inputs.box_width, footer_text(inputs, index).as_deref()).saturating_sub(4);
    let attrs = u32::from(paint.bold);
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut used = 0usize;
    let mut push = |chunks: &mut Vec<TextChunk>, text: &str, fg: Rgb| {
        let run_fg = if paint.bold { paint.title_fg } else { fg };
        for atom in crate::wrap::flatten(
            text,
            ratatui::style::Style::new()
                .fg(ratatui::style::Color::Rgb(run_fg.0, run_fg.1, run_fg.2)),
        ) {
            let step = atom.width.max(1);
            if used + step > clip {
                break;
            }
            chunks.push(chunk(atom.text, run_fg, Some(paint.bg), attrs));
            used += step;
        }
    };
    push(&mut chunks, &truncated_title(option), paint.title_fg);
    if let Some(description) = description {
        let fg = if paint.selected { paint.title_fg } else { inputs.colors.muted };
        push(&mut chunks, &format!(" {description}"), fg);
    }
    styled(chunks)
}

// ---------------------------------------------------------------------------
// dynamic-prop adapters
// ---------------------------------------------------------------------------

fn dyn_num<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<usize> + 'static,
{
    opentui_solid::dyn_(source, |value: &usize| PropValue::Num(*value as f32))
}

fn dyn_color<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<Rgb> + 'static,
{
    opentui_solid::dyn_(source, |value: &Rgb| PropValue::Color(hex(*value)))
}

fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

// ---------------------------------------------------------------------------
// the select family tree (ui/dialog-select.tsx)
// ---------------------------------------------------------------------------

/// One option row element — the tsx `<For each={options}>` body:
///
/// ```text
/// <box flexDirection="row" paddingLeft={current?1:3} paddingRight={3}
///      marginLeft={1} marginRight={1} backgroundColor={…}>
///   <text width={current?1:0} marginRight={current?1:0}>●</text>
///   <text flexGrow={1} wrapMode="none">{title}{" "+description}</text>
///   <box flexShrink={0}><text>{footer}</text></box>
/// </box>
/// ```
///
/// The scrollbox's paddingLeft/Right 1 become the row box's MARGINS
/// (the selected-row background must span `[1, width-1)` — the box
/// fills inside its margins), which lets the empty-state row
/// (paddingLeft 4, NO margins) land at the same x=4 the old renderer
/// pins. The marker is always mounted with dyn width/content (the M1
/// ✕ pattern); its marginRight replaces the tsx `gap={1}` so a
/// marker-less row puts the title at the same column (4) as a current
/// row: `1 + 1 + 1 + 1` or `1 + 3 + 0 + 0`.
fn option_element(inputs: &Signal<DialogInputs>, index: usize) -> Element {
    let paint_at = move |inputs: &DialogInputs| row_paint(inputs, index);

    // ● — mounted always, width 0 without the marker.
    let marker_text = text()
        .dyn_prop(
            "width",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                usize::from(paint_at(i).is_some_and(|paint| paint.current))
            })),
        )
        .dyn_prop(
            "marginRight",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                usize::from(paint_at(i).is_some_and(|paint| paint.current))
            })),
        )
        .dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), move |i: &DialogInputs| match paint_at(i) {
                Some(paint) if paint.current => {
                    styled(vec![chunk("●".to_owned(), paint.marker_fg, Some(paint.bg), 0)])
                }
                _ => styled(Vec::new()),
            })),
        );

    // The title + description run. Natural width + wrap none: the
    // run's chunks are pre-clipped by the same two-stage walk as the
    // old renderer (stage-1 cap + the footer-respecting stage-2
    // edge in `title_chunks`), and `flexShrink 0` keeps the
    // box-clipped glyphs painting exactly like the old renderer's
    // atom walk.
    let title_text = text().flex_shrink(0.0).wrap_mode("none").dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| match paint_at(i) {
            Some(paint) => title_chunks(i, &paint, index),
            None => styled(Vec::new()),
        })),
    );

    // The right-aligned footer: ABSOLUTE, right 3 — ends at the row
    // box's content edge exactly. Since lane F15 the title stops at
    // the gap before the footer's reservation, so the two never
    // collide; the footer stays later in DOM order (paint order
    // parity with the old renderer's footer-last paint).
    let footer_text_el = text().position("absolute").right(3.0).dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
            let Some(paint) = paint_at(i) else {
                return styled(Vec::new());
            };
            match footer_text(i, index) {
                Some(footer) => styled(vec![chunk(footer, paint.footer_fg, Some(paint.bg), 0)]),
                None => styled(Vec::new()),
            }
        })),
    );

    box_()
        .flex_direction("row")
        .margin_left(1.0)
        .margin_right(1.0)
        .padding_right(3.0)
        .dyn_prop(
            "paddingLeft",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                paint_at(i).map_or(3, |paint| if paint.current { 1 } else { 3 })
            })),
        )
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), move |i: &DialogInputs| {
                paint_at(i).map_or(i.colors.surface, |paint| paint.bg)
            })),
        )
        .children(Children::Static(vec![
            marker_text.into_element(),
            title_text.into_element(),
            footer_text_el.into_element(),
        ]))
        .into_element()
}

/// The window `For` — one row per visible entry, keyed by identity
/// (`o{index}` stable across filter reorders; the header/pad/empty
/// rows keyed by their own payloads).
fn window_for(inputs: &Signal<DialogInputs>) -> Rc<dyn opentui_solid::DynChildren> {
    let source = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.window
                .iter()
                .map(|row| ForItem { key: row.key(), value: PropValue::Str(row.key()) })
                .collect::<Vec<ForItem>>()
        }),
    };
    let inputs = inputs.clone();
    Rc::new(for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        let key = item.key.as_str();
        if let Some(index) = key.strip_prefix("o:").and_then(|rest| rest.parse::<usize>().ok()) {
            return option_element(&inputs, index);
        }
        if key == "p" {
            // The pre-header pad: one blank row of the surface.
            return box_().height(1.0).into_element();
        }
        if let Some(category) = key.strip_prefix("h:") {
            let category = category.to_owned();
            // The tsx header box: paddingLeft 3 inside the
            // scrollbox's 1-column pads (a BOX, not leaf-text
            // padding — see the pattern doc's leaf-padding gotcha).
            return box_()
                .margin_left(1.0)
                .margin_right(1.0)
                .padding_left(3.0)
                .child(text().dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                        styled(vec![chunk(category.clone(), i.colors.accent, None, 1)])
                    })),
                ))
                .into_element();
        }
        if let Some(message) = key.strip_prefix("e:") {
            let message = message.to_owned();
            return box_()
                .padding_left(4.0)
                .padding_right(4.0)
                .child(text().dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                        styled(vec![chunk(message.clone(), i.colors.muted, None, 0)])
                    })),
                ))
                .into_element();
        }
        box_().into_element()
    }))
}

/// The footer action bar — `<For each={left()}>` over the actions,
/// each `title label` with the label's leading space in the muted
/// run (`FooterAction`).
fn actions_for(inputs: &Signal<DialogInputs>) -> Rc<dyn opentui_solid::DynChildren> {
    let source = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.actions
                .iter()
                .map(|(title, _label)| ForItem {
                    key: title.clone(),
                    value: PropValue::Str(title.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let inputs = inputs.clone();
    // The port has no `gap` prop — the tsx's gap 2 between action
    // pairs is the SECOND-ONWARD children's marginLeft (index-aware
    // at mount; the pair set changes only with the dialog kind).
    Rc::new(for_list(Rc::new(source), move |item: &ForItem, index: usize| {
        let title = item.key.clone();
        let title_for_label = title.clone();
        let label_at = move |i: &DialogInputs| {
            i.actions.iter().find(|pair| pair.0 == title_for_label).map(|pair| pair.1.clone())
        };
        box_()
            .flex_direction("row")
            .flex_shrink(0.0)
            .margin_left(if index > 0 { 2.0 } else { 0.0 })
            .children(Children::Static(vec![
                text()
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                            styled(vec![chunk(title.clone(), i.colors.text, None, 0)])
                        })),
                    )
                    .into_element(),
                text()
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                            let Some(label) = label_at(i) else {
                                return styled(Vec::new());
                            };
                            styled(vec![chunk(format!(" {label}"), i.colors.muted, None, 0)])
                        })),
                    )
                    .into_element(),
            ]))
            .into_element()
    }))
}

/// The select family tree — `DialogSelect`'s root
/// (`ui/dialog-select.tsx:672-847`):
///
/// ```text
/// <box gap={1} paddingBottom={1}>
///   <box paddingLeft={4} paddingRight={4}>
///     row(space-between): [title bold] [esc muted]
///     <box paddingTop={1}><box height=1 backgroundColor=formfield>…input…</box></box>
///   </box>
///   <box flexGrow={1} flexShrink={1}>      ← the area; the scrollbox's
///     <box>…For window rows…</box>           paddings are per-row margins
///   </box>
///   [footer bar: paddingLeft 4 paddingRight 2, space-between]
/// </box>
/// ```
fn select_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    // The title row: bold title left, `esc` right (ends at the
    // paddingRight-4 edge).
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk(i.title.clone(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );

    // The filter input: a formfield-background box. No painted
    // cursor — v2's input shows the hardware cursor only (the #8
    // ruling; the row past the typed text stays the input's own bg).
    let input_box = box_()
        .height(1.0)
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.formfield_bg)),
        )
        .child(text().wrap_mode("none").dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                let width = i.box_width.saturating_sub(8);
                if i.filter.is_empty() {
                    styled(vec![chunk(i.placeholder.clone(), i.colors.muted, None, 0)])
                } else {
                    let typed = take_width(&i.filter, width);
                    styled(vec![chunk(typed, i.colors.formfield_fg, None, 0)])
                }
            })),
        ));

    let title_block = box_().padding_left(4.0).padding_right(4.0).children(Children::Static(vec![
        title_row.into_element(),
        box_().padding_top(1.0).child(input_box).into_element(),
    ]));

    // The scroll area: the flexGrow box wrapping the window For.
    let scroll_carrier = box_().children_dyn(window_for(inputs));
    let area = box_().flex_grow(1.0).flex_shrink(1.0).child(scroll_carrier);

    // The footer bar (mounted only while actions exist — the tsx's
    // `Show` fallback is an empty flexShrink-0 box). One element
    // carries ONE children source: the Show rides a bg-transparent
    // CARRIER box (the M1 carrier pattern), a static sibling of the
    // title block and the area.
    let inputs_for_footer = inputs.clone();
    let footer = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| !i.actions.is_empty()))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs_for_footer;
            move || {
                vec![box_()
                    .flex_direction("row")
                    .justify_content("space-between")
                    .padding_left(4.0)
                    .padding_right(2.0)
                    .flex_shrink(0.0)
                    .margin_top(1.0)
                    .child(box_().flex_direction("row").children_dyn(actions_for(&inputs)))
                    .into_element()]
            }
        },
    );
    let footer_carrier = box_().children_dyn(Rc::new(footer));

    // The tsx root's gap 1 → marginTop 1 on the second and third
    // children (the port has no column-gap prop).
    box_().padding_bottom(1.0).children(Children::Static(vec![
        title_block.into_element(),
        area.margin_top(1.0).into_element(),
        footer_carrier.into_element(),
    ]))
}

// ---------------------------------------------------------------------------
// the prompt family (ui/dialog-prompt.tsx / dialog-worktree-name.tsx)
// ---------------------------------------------------------------------------

/// The menu family tree (lane KM — `TabContextMenu`'s scrollbox,
/// session-tabs.tsx:449-478): a bare column of option rows — width
/// `100%`, one row each, the SELECTED row painting the raised-high
/// action background (`actionHovered`) over the rest. The rows reuse
/// the select window's `For` (keyed, identity-stable).
fn menu_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    box_().children_dyn(window_for(inputs))
}

/// The prompt-family footer: `enter submit` (worktree name adds
/// `tab generate one` as a second pair — the tsx row's gap 2). Each
/// hint is one text whose leading label run (space included) carries
/// `text.base` — the rig-pinned styled-space finding.
fn prompt_footer_element(
    inputs: &Signal<DialogInputs>,
    worktree: bool,
) -> opentui_solid::ElementBuilder {
    let enter_pair = |inputs: &Signal<DialogInputs>| {
        text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![
                    chunk("enter ".to_owned(), i.colors.text, None, 0),
                    chunk("submit".to_owned(), i.colors.muted, None, 0),
                ])
            })),
        )
    };
    if !worktree {
        return box_().flex_direction("row").padding_bottom(1.0).child(enter_pair(inputs));
    }
    let tab_pair = text().dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            styled(vec![
                chunk("tab ".to_owned(), i.colors.text, None, 0),
                chunk("generate one".to_owned(), i.colors.muted, None, 0),
            ])
        })),
    );
    box_().flex_direction("row").padding_bottom(1.0).children(Children::Static(vec![
        enter_pair(inputs).into_element(),
        tab_pair.margin_left(2.0).into_element(),
    ]))
}

/// The connect-flow prompt (`KeyMethod` / `OAuthCode`): the prompt
/// tree with a DYNAMIC title + placeholder and the description rows
/// (URL / instructions / inline error) under the input
/// (dialog-integration.tsx:485-505, 700-727).
fn connect_prompt_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk(i.title.clone(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
    let input = text().wrap_mode("none").dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            let width = i.box_width.saturating_sub(4);
            if i.prompt_value.is_empty() {
                styled(vec![chunk(i.placeholder.clone(), i.colors.muted, None, 0)])
            } else {
                let typed = take_width(&i.prompt_value, width);
                let end = display_width(&typed);
                let mut chunks = vec![chunk(typed, i.colors.formfield_base_fg, None, 0)];
                if end < width {
                    chunks.push(chunk(" ".to_owned(), i.colors.surface, Some(i.colors.text), 0));
                }
                styled(chunks)
            }
        })),
    );
    // The description block — a For over the hint rows (the same
    // keyed-list shape the help tree uses), muted like
    // `DialogPrompt`'s description slot.
    let lines = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.hint_lines
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("C{n}"),
                    value: PropValue::Str(line.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let line_inputs = inputs.clone();
    let hints = for_list(std::rc::Rc::new(lines), move |item: &ForItem, _index: usize| {
        let line = match &item.value {
            PropValue::Str(line) => line.clone(),
            _ => String::new(),
        };
        let inputs = line_inputs.clone();
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| {
                    styled(vec![chunk(line.clone(), i.colors.muted, None, 0)])
                })),
            )
            .into_element()
    });
    let hint_box = box_().children_dyn(std::rc::Rc::new(hints));
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(vec![
        title_row.into_element(),
        input.margin_top(1.0).into_element(),
        hint_box.margin_top(1.0).into_element(),
        prompt_footer_element(inputs, false).margin_top(1.0).into_element(),
    ]))
}

/// The prompt family tree (`ui/dialog-prompt.tsx:98-145`): the root
/// is paddingLeft/right 2, gap 1 — title row, the 1-row input (NO
/// formfield background: the textarea paints text only), and the
/// footer whose box adds paddingBottom 1. The cursor color is
/// `text.base` for the rename textarea, `text.formfield.base` for
/// the worktree-name input.
fn prompt_builder(inputs: &Signal<DialogInputs>, worktree: bool) -> opentui_solid::ElementBuilder {
    let title: &'static str = if worktree { "Name worktree" } else { "Rename session" };
    let placeholder: &'static str = if worktree { "Worktree name" } else { "Session title" };
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk(title.to_owned(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );

    // The input row: placeholder muted / typed value in
    // text.formfield.base with the block cursor (fg surface over the
    // cursor color) — no background box (the chrome shows through).
    let input = text().wrap_mode("none").dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
            let width = i.box_width.saturating_sub(4);
            if i.prompt_value.is_empty() {
                styled(vec![chunk(placeholder.to_owned(), i.colors.muted, None, 0)])
            } else {
                let cursor_bg = if worktree { i.colors.formfield_base_fg } else { i.colors.text };
                let typed = take_width(&i.prompt_value, width);
                let end = display_width(&typed);
                let mut chunks = vec![chunk(typed, i.colors.formfield_base_fg, None, 0)];
                if end < width {
                    chunks.push(chunk(" ".to_owned(), i.colors.surface, Some(cursor_bg), 0));
                }
                styled(chunks)
            }
        })),
    );

    // The tsx root's gap 1 → marginTop 1 on the input and footer.
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(vec![
        title_row.into_element(),
        input.margin_top(1.0).into_element(),
        prompt_footer_element(inputs, worktree).margin_top(1.0).into_element(),
    ]))
}

// ---------------------------------------------------------------------------
// the help tree (ui/dialog-help.tsx)
// ---------------------------------------------------------------------------

/// The help tree: the prompt-family root (paddingLeft/right 2, gap
/// 1), the word-wrapped palette hint (the lines are PRE-WRAPPED with
/// the same `wrap_label` the old path uses — the port's text reflow
/// is not a parity surface), and the right-justified `ok` button
/// (paddingLeft/Right 3 over the focused primary action background;
/// the box fill's hard-written white fg IS the rig-pinned padding
/// fg).
fn help_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("Help".to_owned(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc/enter".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );

    // The hint lines — a For keyed by line index over the
    // pre-wrapped lines.
    let lines = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.hint_lines
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("L{n}"),
                    value: PropValue::Str(line.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let line_inputs = inputs.clone();
    let hint_for = for_list(Rc::new(lines), move |item: &ForItem, _index: usize| {
        let line = match &item.value {
            PropValue::Str(line) => line.clone(),
            _ => String::new(),
        };
        let inputs = line_inputs.clone();
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| {
                    styled(vec![chunk(line.clone(), i.colors.muted, None, 0)])
                })),
            )
            .into_element()
    });
    let hint_box = box_().padding_bottom(1.0).children_dyn(Rc::new(hint_for));

    // The ok button: right-justified by the row's justify-end.
    let button = box_()
        .padding_left(3.0)
        .padding_right(3.0)
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.selected_bg)),
        )
        .child(text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![chunk("ok".to_owned(), i.colors.selected_fg, None, 0)])
            })),
        ));
    let button_row =
        box_().flex_direction("row").justify_content("flex-end").padding_bottom(1.0).child(button);

    // The tsx root's gap 1 → marginTop 1 on the hint box and button row.
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(vec![
        title_row.into_element(),
        hint_box.margin_top(1.0).into_element(),
        button_row.margin_top(1.0).into_element(),
    ]))
}

// ---------------------------------------------------------------------------
// the plugin rows tree
// ---------------------------------------------------------------------------

/// The rows-only plugin dialog: the select's title-block geometry
/// (bold title at paddingLeft 4 clipped at `width - 12`, `esc` at the
/// paddingRight-4 edge), then every row verbatim (muted, clipped at
/// `width - 8`), then the root paddingBottom.
fn plugin_rows_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        let title =
                            take_width(&i.plugin_title, i.box_width.saturating_sub(12).max(8));
                        styled(vec![chunk(title, i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );

    let rows = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.plugin_rows
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("R{n}"),
                    value: PropValue::Str(line.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let row_inputs = inputs.clone();
    let rows_for = for_list(Rc::new(rows), move |item: &ForItem, _index: usize| {
        let line = match &item.value {
            PropValue::Str(line) => line.clone(),
            _ => String::new(),
        };
        let inputs = row_inputs.clone();
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| {
                    let text = take_width(&line, i.box_width.saturating_sub(8).max(8));
                    styled(vec![chunk(text, i.colors.muted, None, 0)])
                })),
            )
            .into_element()
    });

    // The tsx root's gap 1 → marginTop 1 on the rows box.
    box_().padding_bottom(1.0).children(Children::Static(vec![
        box_().padding_left(4.0).padding_right(4.0).child(title_row).into_element(),
        box_()
            .padding_left(4.0)
            .padding_right(4.0)
            .children_dyn(Rc::new(rows_for))
            .margin_top(1.0)
            .into_element(),
    ]))
}

// ---------------------------------------------------------------------------
// TUI-25: the alert / confirm trees (ui/dialog-alert.tsx /
// ui/dialog-confirm.tsx)
// ---------------------------------------------------------------------------

/// `DialogAlert` (dialog-alert.tsx:37-57): the root paddingLeft/right
/// 2 gap 1; the muted message with paddingBottom 1; the right-justed
/// `ok` pill (padding 3 over the focused primary background).
fn alert_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let header = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .wrap_mode("none")
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        let clip = i.box_width.saturating_sub(8).max(8);
                        styled(vec![chunk(
                            take_width(&i.plugin_title, clip),
                            i.colors.text,
                            None,
                            1,
                        )])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
    let message = text().dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            styled(vec![chunk(i.message.clone(), i.colors.muted, None, 0)])
        })),
    );
    let message_box = box_().padding_bottom(1.0).child(message);
    let ok = box_()
        .padding_left(3.0)
        .padding_right(3.0)
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.selected_bg)),
        )
        .child(text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![chunk("ok".to_owned(), i.colors.selected_fg, None, 0)])
            })),
        ));
    let button_row =
        box_().flex_direction("row").justify_content("flex-end").padding_bottom(1.0).child(ok);
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(vec![
        header.into_element(),
        message_box.margin_top(1.0).into_element(),
        button_row.margin_top(1.0).into_element(),
    ]))
}

/// `DialogConfirm` (dialog-confirm.tsx:37-81): the alert shape with
/// the cancel/confirm pill PAIR — the active one carries the focused
/// primary background + fg, the inactive stays muted.
#[allow(clippy::too_many_lines)] // the pill pair's show/hide halves
fn confirm_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let header = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .wrap_mode("none")
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        let clip = i.box_width.saturating_sub(8).max(8);
                        styled(vec![chunk(
                            take_width(&i.plugin_title, clip),
                            i.colors.text,
                            None,
                            1,
                        )])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
    let message = text().dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            styled(vec![chunk(i.message.clone(), i.colors.muted, None, 0)])
        })),
    );
    let message_box = box_().padding_bottom(1.0).child(message);
    // The cancel pill (active when the focus is NOT confirm).
    let cancel_shown = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| !i.confirm_labels.2))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![box_()
                    .padding_left(1.0)
                    .padding_right(1.0)
                    .dyn_prop(
                        "backgroundColor",
                        dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.selected_bg)),
                    )
                    .child(text().dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                            styled(vec![chunk(
                                i.confirm_labels.0.clone(),
                                i.colors.selected_fg,
                                None,
                                0,
                            )])
                        })),
                    ))
                    .into_element()]
            }
        },
    );
    let cancel_hidden = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| i.confirm_labels.2))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![box_()
                    .padding_left(1.0)
                    .padding_right(1.0)
                    .child(text().dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                            styled(vec![chunk(i.confirm_labels.0.clone(), i.colors.muted, None, 0)])
                        })),
                    ))
                    .into_element()]
            }
        },
    );
    let cancel = box_().children(Children::Static(vec![
        box_().children_dyn(Rc::new(cancel_shown)).into_element(),
        box_().children_dyn(Rc::new(cancel_hidden)).into_element(),
    ]));
    // The confirm pill (active when the focus IS confirm).
    let confirm_shown = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| i.confirm_labels.2))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![box_()
                    .padding_left(1.0)
                    .padding_right(1.0)
                    .dyn_prop(
                        "backgroundColor",
                        dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.selected_bg)),
                    )
                    .child(text().dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                            styled(vec![chunk(
                                i.confirm_labels.1.clone(),
                                i.colors.selected_fg,
                                None,
                                0,
                            )])
                        })),
                    ))
                    .into_element()]
            }
        },
    );
    let confirm_hidden = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| !i.confirm_labels.2))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![box_()
                    .padding_left(1.0)
                    .padding_right(1.0)
                    .child(text().dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                            styled(vec![chunk(i.confirm_labels.1.clone(), i.colors.muted, None, 0)])
                        })),
                    ))
                    .into_element()]
            }
        },
    );
    let confirm = box_().children(Children::Static(vec![
        box_().children_dyn(Rc::new(confirm_shown)).into_element(),
        box_().children_dyn(Rc::new(confirm_hidden)).into_element(),
    ]));
    let button_row = box_()
        .flex_direction("row")
        .justify_content("flex-end")
        .padding_bottom(1.0)
        .children(Children::Static(vec![cancel.into_element(), confirm.into_element()]));
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(vec![
        header.into_element(),
        message_box.margin_top(1.0).into_element(),
        button_row.margin_top(1.0).into_element(),
    ]))
}

// ---------------------------------------------------------------------------
// TUI-24: the error-details tree (component/dialog-error-details.tsx)
// ---------------------------------------------------------------------------

/// `DialogErrorDetails`: the header (title + the source `FilePath`
/// row), the 20-row error body (the window arrives pre-sliced — the
/// port's scroll is the store's offset), the `Reference:` row, and
/// the `i investigate  c copy details  ↑/↓ scroll` action footer.
fn error_details_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .wrap_mode("none")
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        let clip = i.box_width.saturating_sub(8).max(8);
                        styled(vec![chunk(
                            take_width(&i.plugin_title, clip),
                            i.colors.text,
                            None,
                            1,
                        )])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
    let mut header_children = vec![title_row.into_element()];
    let source_row = text()
        .wrap_mode("none")
        .dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![chunk(i.error_source.clone(), i.colors.muted, None, 0)])
            })),
        )
        .into_element();
    header_children.push(source_row);
    let header = box_().children(Children::Static(header_children));
    // The body window + the reference row.
    let body_lines = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.error_window
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("E{n}"),
                    value: PropValue::Str(line.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let body_inputs = inputs.clone();
    let body_for = for_list(Rc::new(body_lines), move |item: &ForItem, _index: usize| {
        let line = match &item.value {
            PropValue::Str(line) => line.clone(),
            _ => String::new(),
        };
        let inputs = body_inputs.clone();
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| {
                    styled(vec![chunk(line.clone(), i.colors.text, None, 0)])
                })),
            )
            .into_element()
    });
    let mut body_children = vec![box_().children_dyn(Rc::new(body_for)).into_element()];
    let reference = text()
        .dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![chunk(format!("Reference: {}", i.diagnostic), i.colors.muted, None, 0)])
            })),
        )
        .into_element();
    body_children.push(reference);
    let body = box_().children(Children::Static(body_children));
    // The action footer: `i investigate`, `c copy details`,
    // `↑/↓ scroll` (gap 3, wrap).
    let footer = text().dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            styled(vec![
                chunk("i".to_owned(), i.colors.text, None, 1),
                chunk(" investigate   ".to_owned(), i.colors.muted, None, 0),
                chunk("c".to_owned(), i.colors.text, None, 1),
                chunk(" copy details   ".to_owned(), i.colors.muted, None, 0),
                chunk("↑/↓ scroll".to_owned(), i.colors.muted, None, 0),
            ])
        })),
    );
    box_().padding_left(2.0).padding_right(2.0).padding_bottom(1.0).children(Children::Static(
        vec![
            header.into_element(),
            body.margin_top(1.0).into_element(),
            footer.margin_top(1.0).into_element(),
        ],
    ))
}

// ---------------------------------------------------------------------------
// TUI-10: the image-preview tree (component/dialog-image-preview.tsx)
// ---------------------------------------------------------------------------

/// `DialogImagePreview`: the `Image N of M` header, the EMPTY image
/// area (the pixels paint in the bridge's post-pass over the copied
/// cells — blocks glyphs or a graphics-protocol placement), and the
/// `← previous / caption / next →` footer row.
fn image_preview_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk(i.preview_title.clone(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
    // The image area: an empty box the post-pass fills.
    let area =
        box_().dyn_prop("height", dyn_num(map(inputs.clone(), |i: &DialogInputs| i.preview_rows)));
    let footer =
        box_().flex_direction("row").justify_content("space-between").child(text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                let (previous, caption, next) = &i.preview_footer;
                styled(vec![
                    chunk(previous.clone(), i.colors.muted, None, 0),
                    chunk(caption.clone(), i.colors.muted, None, 0),
                    chunk(next.clone(), i.colors.muted, None, 0),
                ])
            })),
        ));
    box_().padding_left(2.0).padding_right(2.0).padding_bottom(1.0).children(Children::Static(
        vec![
            title_row.into_element(),
            area.margin_top(1.0).into_element(),
            footer.margin_top(1.0).into_element(),
        ],
    ))
}

// ---------------------------------------------------------------------------
// the pair tree (F15 fix round)
// ---------------------------------------------------------------------------

/// `DialogPair` (dialog-pair.tsx:89-129): the bold `Pair` header row
/// with the `esc` chrome label, then the content rows (loading /
/// error / the word-wrapped intro + link blocks + loopback hint) — a
/// `For` over the pre-wrapped walk, one text element per row. The
/// root mirrors the tsx: paddingLeft/right 2, paddingBottom 1, gap 1.
fn pair_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("Pair".to_owned(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
    // The content walk — one For item per row (the payload carries
    // the row's text; the paint re-derives from the inputs by index).
    let rows = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.pair_rows
                .iter()
                .enumerate()
                .map(|(n, row)| {
                    let text = match row {
                        crate::components::dialog_pair::PairRow::Gap => String::new(),
                        crate::components::dialog_pair::PairRow::Text { text, .. } => text.clone(),
                        crate::components::dialog_pair::PairRow::Qr { text } => text.clone(),
                    };
                    ForItem { key: format!("P{n}"), value: PropValue::Str(text) }
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let row_inputs = inputs.clone();
    let content_for = for_list(Rc::new(rows), move |_item: &ForItem, index: usize| {
        let inputs = row_inputs.clone();
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| match i.pair_rows.get(index) {
                    Some(crate::components::dialog_pair::PairRow::Gap) => {
                        styled(vec![chunk(" ".to_owned(), i.colors.muted, None, 0)])
                    }
                    Some(crate::components::dialog_pair::PairRow::Text { text, style }) => {
                        let (fg, bold) = match style {
                            crate::components::dialog_pair::PairStyle::Muted => (i.colors.muted, 0),
                            crate::components::dialog_pair::PairStyle::Base => (i.colors.text, 0),
                            crate::components::dialog_pair::PairStyle::ErrorBold => {
                                (i.colors.error_fg, 1)
                            }
                        };
                        styled(vec![chunk(text.clone(), fg, None, bold)])
                    }
                    // The QR rows paint `theme.text.base` (ref
                    // dialog-pair.tsx:82 — the `<text fg={theme.text.base}>`
                    // wrapping renderUnicodeCompact).
                    Some(crate::components::dialog_pair::PairRow::Qr { text }) => {
                        styled(vec![chunk(text.clone(), i.colors.text, None, 0)])
                    }
                    None => styled(Vec::new()),
                })),
            )
            .into_element()
    });
    let content = box_().children_dyn(Rc::new(content_for));
    box_().padding_left(2.0).padding_right(2.0).padding_bottom(1.0).children(Children::Static(
        vec![title_row.into_element(), content.margin_top(1.0).into_element()],
    ))
}

// ---------------------------------------------------------------------------
// the mount
// ---------------------------------------------------------------------------

/// The chrome box (`ui/dialog.tsx:44-73`): the dialog surface
/// background, paddingTop 1, wrapping the family content — the whole
/// overlay box, exactly the old path's rect fill + content rows.
/// Sized EXPLICITLY (buffer points): a percent width on the mount
/// root mis-sizes the tree in the port (lane M2 gotcha — the space-
/// between rows lose their trailing children), and the mount rebuilds
/// on any size change anyway.
fn chrome_element(
    inputs: &Signal<DialogInputs>,
    family: Family,
    width: usize,
    height: usize,
) -> Element {
    let family_inputs = inputs.clone();
    let family_builder = match family {
        Family::Select => select_builder(&family_inputs),
        Family::RenamePrompt => prompt_builder(&family_inputs, false),
        Family::WorktreeNamePrompt => prompt_builder(&family_inputs, true),
        Family::ConnectPrompt => connect_prompt_builder(&family_inputs),
        Family::Help => help_builder(&family_inputs),
        Family::PluginRows => plugin_rows_builder(&family_inputs),
        Family::Menu => menu_builder(&family_inputs),
        Family::Alert => alert_builder(&family_inputs),
        Family::Confirm => confirm_builder(&family_inputs),
        Family::ErrorDetails => error_details_builder(&family_inputs),
        Family::ImagePreview => image_preview_builder(&family_inputs),
        Family::Pair => pair_builder(&family_inputs),
    };
    box_()
        .width(width as f32)
        .height(height as f32)
        // The menu's scrollbox has no paddingTop (every other family
        // seats its content under the chrome's 1-row pad).
        .padding_top(if family == Family::Menu { 0.0 } else { 1.0 })
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.surface)),
        )
        .child(family_builder)
        .into_element()
}

/// The mounted dialog: the inputs signal + a real headless
/// [`SolidRenderer`]. Rebuilt whenever the mount key changes —
/// family, box width, theme colors, or CONTENT HEIGHT (the buffer is
/// sized `(box_width, 1 + content)`; a filter that shrinks the window
/// re-keys the mount, while the hot frames — selection moves, scroll
/// — keep a stable height and patch in place).
struct SolidDialog {
    inputs: Signal<DialogInputs>,
    renderer: SolidRenderer,
    key: (usize, usize, DialogColors, usize),
}

fn mount_key(inputs: &DialogInputs) -> (usize, usize, DialogColors, usize) {
    (inputs.family as usize, inputs.box_width, inputs.colors, 1 + content_height(inputs))
}

impl SolidDialog {
    fn new(inputs: &DialogInputs) -> Self {
        let signal = Signal::new(inputs.clone());
        let height = (1 + content_height(inputs)).max(1);
        let renderer = opentui_solid::render(
            opentui_core::renderer::CliRendererConfig {
                width: inputs.box_width.max(1) as u32,
                height: height as u32,
                background_color: rgba(inputs.colors.surface),
                ..opentui_core::renderer::CliRendererConfig::default()
            },
            chrome_element(&signal, inputs.family, inputs.box_width.max(1), height),
        );
        Self { inputs: signal, renderer, key: mount_key(inputs) }
    }

    /// Sync one frame's snapshot, flush the reconciler patches, and
    /// render — the per-frame bridge cost.
    fn paint(&mut self, inputs: DialogInputs) {
        self.inputs.set(inputs);
        self.renderer.flush();
    }
}

thread_local! {
    /// One dialog mount per thread-process — the app paints exactly
    /// one overlay per frame; the harnesses reuse it across boots.
    static DIALOG: RefCell<Option<SolidDialog>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// the overlay compositor (the new-path twin of render_overlay_with)
// ---------------------------------------------------------------------------

/// Paint the top dialog's overlay through the opentui-solid path:
/// backdrop (NEW-S2a), chrome geometry, the tree's buffer, cell copy
/// — the twin of `crate::components::dialog::render_overlay_with`.
/// Returns the chrome box's absolute rect (the outside-click
/// dismissal surface).
pub fn render_overlay_solid(
    base: &mut Frame,
    entry: &DialogEntry,
    props: &SelectProps,
    plugin: Option<&PluginDialogContent>,
    theme: &ThemeState,
    terminal: (u16, u16),
    sidecar: &DialogSidecar,
) -> ((usize, usize, usize, usize), Vec<crate::store::DialogButtonHit>) {
    let styles = DialogStyles::from_theme(theme);
    let colors = DialogColors::of(&styles);
    let width = usize::from(terminal.0);
    let height = usize::from(terminal.1);

    // The tab menu (lane KM — `TabContextMenu`'s geometry,
    // session-tabs.tsx:428-436): sized by its actions (≥ 20, ≤ 50 +
    // 2 padding, terminal-capped), seated AT the pointer (left
    // clamped into the terminal, top one row under the press),
    // with NO backdrop dim (v2's capture layer is invisible).
    let tab_menu = matches!(entry.kind, DialogKind::TabMenu { .. }).then(|| (entry, props));
    let (box_width, box_x, box_y, dim_backdrop) = if let Some((entry, props)) = tab_menu {
        let longest = props
            .options
            .iter()
            .map(|option| crate::wrap::display_width(&option.title).min(50) + 2)
            .max()
            .unwrap_or(0);
        // The option rows reuse the select's marker column (title
        // starts 4 cells in), so the box adds that padding over
        // v2's 1-left scrollbox rows — registered approximation.
        let menu_width = width.min(longest.max(20) + 6);
        let menu_height = (props.options.len()).min(height).max(1) + 1;
        let DialogKind::TabMenu { x, y, .. } = &entry.kind else {
            unreachable!("the TabMenu guard")
        };
        let left = (*x).min(width.saturating_sub(menu_width));
        let top = (*y + 1).min(height.saturating_sub(menu_height));
        (menu_width, left, top, false)
    } else {
        // Backdrop: alpha-composite black@150 over every covered
        // cell — the raster's observable semantics, applied at the
        // bridge (the solid renderer cannot see the route frame
        // beneath).
        let box_width = entry.size.width().min(width.saturating_sub(2));
        let box_x = (width - box_width) / 2;
        // TUI-10: the image preview is CENTERED (`dialog.
        // setCentered(true)`, dialog-image-preview.tsx:24) — the
        // content-height walk pins the box's height first. F15: the
        // pair dialog is centered too (dialog-pair.tsx:19).
        let centered = matches!(entry.kind, DialogKind::ImagePreview | DialogKind::Pair);
        let box_y = if centered {
            let inputs_probe = build_inputs(
                entry,
                props,
                plugin,
                DialogColors::of(&DialogStyles::from_theme(theme)),
                box_width,
                height,
                sidecar,
            );
            height.saturating_sub(1 + content_height(&inputs_probe)) / 2
        } else {
            (height + 2) / 4
        };
        (box_width, box_x, box_y, true)
    };
    if dim_backdrop {
        for row in &mut base.cells {
            for cell in row {
                crate::components::dialog::blend_backdrop(cell);
            }
        }
    }

    let inputs = build_inputs(entry, props, plugin, colors, box_width, height, sidecar);
    let box_height = 1 + content_height(&inputs);

    DIALOG.with_borrow_mut(|slot| {
        let mount = slot.get_or_insert_with(|| SolidDialog::new(&inputs));
        if mount.key != mount_key(&inputs) {
            *mount = SolidDialog::new(&inputs);
        }
        mount.paint(inputs.clone());
        let buffer = &mount.renderer.renderer.current_render_buffer;
        let frame = crate::components::session_tabs_solid::frame_from_buffer(buffer, &[]);
        for (row, cells) in frame.cells.iter().enumerate() {
            let y = box_y + row;
            if y >= height {
                break;
            }
            for (x, cell) in cells.iter().enumerate().take(box_width) {
                if let Some(target) = base.cells.get_mut(y).and_then(|line| line.get_mut(box_x + x))
                {
                    *target = *cell;
                }
            }
        }
    });

    let rect =
        (box_x, box_y, box_width.min(width - box_x), box_height.min(height.saturating_sub(box_y)));
    (rect, dialog_chrome_button_hits(entry, &inputs, rect))
}

/// The dialog chrome's pointer targets (auditA #6 — lane/if item 5):
/// the title row's muted close label (every family renders one —
/// v2's ~22 dialog headers all carry `onMouseUp={() => dialog.clear()}`),
/// plus the Help dialog's `ok` pill. The rects mirror the builders'
/// geometry: the title row is the FIRST content row under the
/// chrome's paddingTop 1; the label right-aligns against the
/// family's title-block padding (4 for the select/plugin-rows
/// families, 2 for prompt/help). v2's labels carry no hover styling
/// — the hit rect alone is the affordance.
fn dialog_chrome_button_hits(
    entry: &DialogEntry,
    inputs: &DialogInputs,
    rect: (usize, usize, usize, usize),
) -> Vec<crate::store::DialogButtonHit> {
    use crate::store::{DialogButtonAction, DialogButtonHit, HitRect};
    let (box_x, box_y, box_width, _) = rect;
    let _ = entry;
    let mut hits = Vec::new();
    if box_width < 8 {
        return hits;
    }
    // The tab menu has no chrome buttons (v2's menu carries none —
    // backdrop/escape/right-press close it).
    if matches!(inputs.family, Family::Menu) {
        return hits;
    }
    let title_row = box_y + 1;
    let (label, pad) = match inputs.family {
        Family::Select | Family::PluginRows => ("esc", 4usize),
        Family::RenamePrompt
        | Family::WorktreeNamePrompt
        | Family::ConnectPrompt
        | Family::Menu
        | Family::Alert
        | Family::Confirm
        | Family::ErrorDetails
        | Family::ImagePreview
        | Family::Pair => ("esc", 2),
        Family::Help => ("esc/enter", 2),
    };
    let label_width = label.chars().count();
    let close_x = (box_x + box_width).saturating_sub(pad + label_width);
    hits.push(DialogButtonHit {
        action: DialogButtonAction::Close,
        rect: HitRect { x: close_x, y: title_row, width: label_width, height: 1 },
    });
    if matches!(inputs.family, Family::Help) {
        // The ok pill: the LAST content row (title, gap, L hint lines,
        // hint pad, gap, button, button pad) right-justified against
        // the root's paddingRight 2 — the BUTTON box is its padding
        // (3) + `ok` (2) + padding (3) = 8 wide.
        let lines = inputs.hint_lines.len();
        let button_row = box_y + lines + 5;
        hits.push(DialogButtonHit {
            action: DialogButtonAction::HelpOk,
            rect: HitRect { x: box_x + box_width - 10, y: button_row, width: 8, height: 1 },
        });
    }
    hits
}

/// Assemble one frame's [`DialogInputs`] from the store's dialog
/// entry + props (the same `select_props_for` data the old path
/// renders).
// The per-family arms grew with the family set; same shape as the
// dispatcher tables.
#[allow(clippy::too_many_lines)]
fn build_inputs(
    entry: &DialogEntry,
    props: &SelectProps,
    plugin: Option<&PluginDialogContent>,
    colors: DialogColors,
    box_width: usize,
    terminal_height: usize,
    sidecar: &DialogSidecar,
) -> DialogInputs {
    let family = match &entry.kind {
        DialogKind::SessionRename { .. } => Family::RenamePrompt,
        DialogKind::WorktreeName => Family::WorktreeNamePrompt,
        // Lane FINAL: the connect-flow prompts (KeyMethod's API-key
        // input / OAuthCode's code entry) — the prompt tree with a
        // dynamic title + the attempt's URL/instructions rows.
        DialogKind::IntegrationKey { .. } | DialogKind::IntegrationCode { .. } => {
            Family::ConnectPrompt
        }
        DialogKind::Help => Family::Help,
        DialogKind::TabMenu { .. } => Family::Menu,
        DialogKind::Plugin { .. } if plugin.is_some_and(|content| !content.is_select()) => {
            Family::PluginRows
        }
        // TUI-10/24/25: the custom-key families.
        DialogKind::ImagePreview => Family::ImagePreview,
        DialogKind::ErrorDetails { .. } => Family::ErrorDetails,
        DialogKind::Alert { .. } => Family::Alert,
        DialogKind::Confirm { .. } => Family::Confirm,
        // F15 fix round: the pair dialog's CUSTOM tree (ref
        // dialog-pair.tsx is not a DialogSelect).
        DialogKind::Pair => Family::Pair,
        _ => Family::Select,
    };
    match family {
        Family::Select | Family::Menu => {
            let (window, flat) = visible_window(props, &entry.select, terminal_height);
            DialogInputs {
                family,
                box_width,
                colors,
                title: props.title.clone(),
                placeholder: props.placeholder.clone(),
                filter: entry.select.filter.clone(),
                options: props.options.clone(),
                flat,
                selected: entry.select.selected,
                current: props.current.clone(),
                flatten: props.flat && !entry.select.filter.is_empty(),
                destructive: props.destructive,
                window,
                actions: props
                    .actions
                    .iter()
                    .map(|action| (action.title.clone(), action.label.clone()))
                    .collect(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: String::new(),
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
                plugin_rows: Vec::new(),
            }
        }
        Family::RenamePrompt | Family::WorktreeNamePrompt => DialogInputs {
            family,
            box_width,
            colors,
            title: String::new(),
            placeholder: String::new(),
            filter: String::new(),
            options: Vec::new(),
            flat: Vec::new(),
            selected: 0,
            current: None,
            flatten: false,
            destructive: false,
            window: Vec::new(),
            actions: Vec::new(),
            prompt_value: entry.value.clone(),
            hint_lines: Vec::new(),
            plugin_title: String::new(),
            plugin_rows: Vec::new(),
            message: String::new(),
            confirm_labels: (String::new(), String::new(), true),
            error_source: String::new(),
            error_window: Vec::new(),
            diagnostic: String::new(),
            preview_title: String::new(),
            preview_footer: (String::new(), String::new(), String::new()),
            preview_rows: 0,
            pair_rows: Vec::new(),
        },
        // The connect prompts carry their title/placeholder/hints
        // through the props (OAuthCode's description block holds the
        // attempt's URL + instructions, dialog-integration.tsx:
        // 719-725; the inline error rides the same rows).
        Family::ConnectPrompt => DialogInputs {
            family,
            box_width,
            colors,
            title: props.title.clone(),
            placeholder: props.placeholder.clone(),
            filter: String::new(),
            options: Vec::new(),
            flat: Vec::new(),
            selected: 0,
            current: None,
            flatten: false,
            destructive: false,
            window: Vec::new(),
            actions: Vec::new(),
            prompt_value: entry.value.clone(),
            hint_lines: props.hints.clone(),
            plugin_title: String::new(),
            plugin_rows: Vec::new(),
            message: String::new(),
            confirm_labels: (String::new(), String::new(), true),
            error_source: String::new(),
            error_window: Vec::new(),
            diagnostic: String::new(),
            preview_title: String::new(),
            preview_footer: (String::new(), String::new(), String::new()),
            preview_rows: 0,
            pair_rows: Vec::new(),
        },
        Family::Help => {
            let content_width = box_width.saturating_sub(4);
            let palette = crate::keymap::default_table()
                .iter()
                .find(|def| def.command == "command.palette.show")
                .map_or_else(
                    || "ctrl+p".to_owned(),
                    |def| {
                        crate::components::dialog::spec_display(
                            def.defaults.split(',').next().unwrap_or("none"),
                        )
                    },
                );
            let hint = format!(
                "Press {palette} to see all available actions and commands in any context."
            );
            DialogInputs {
                family,
                box_width,
                colors,
                title: String::new(),
                placeholder: String::new(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: crate::wrap::wrap_label(&hint, content_width.max(1)),
                plugin_title: String::new(),
                plugin_rows: Vec::new(),
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
            }
        }
        Family::PluginRows => {
            let content = plugin.cloned().unwrap_or_default();
            DialogInputs {
                family,
                box_width,
                colors,
                title: String::new(),
                placeholder: String::new(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: content.title,
                plugin_rows: content.rows,
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
            }
        }
        // -- TUI-25: DialogAlert / DialogConfirm --------------------
        Family::Alert | Family::Confirm => {
            let (plugin_title, message, confirm_labels) = match &entry.kind {
                DialogKind::Alert { title, message, .. } => {
                    (title.clone(), message.clone(), ("ok".to_owned(), "ok".to_owned(), true))
                }
                DialogKind::Confirm {
                    title,
                    message,
                    confirm_label,
                    cancel_label,
                    active_confirm,
                    ..
                } => (
                    title.clone(),
                    message.clone(),
                    (
                        titlecase(ccancel(cancel_label)),
                        titlecase(cconfirm(confirm_label)),
                        *active_confirm,
                    ),
                ),
                _ => (String::new(), String::new(), (String::new(), String::new(), true)),
            };
            DialogInputs {
                family,
                box_width,
                colors,
                title: String::new(),
                placeholder: String::new(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title,
                plugin_rows: Vec::new(),
                message,
                confirm_labels,
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
            }
        }
        // -- TUI-24: DialogErrorDetails ------------------------------
        Family::ErrorDetails => {
            let DialogKind::ErrorDetails { title, source, error, diagnostic_ref, .. } = &entry.kind
            else {
                unreachable!("the ErrorDetails family guard")
            };
            // The 20-row scrollbox's window (the store's offset).
            let content_width = box_width.saturating_sub(4).max(1);
            let wrapped = crate::wrap::wrap_label(error, content_width);
            let window: Vec<String> =
                wrapped.iter().skip(sidecar.error_scroll).take(20).cloned().collect();
            DialogInputs {
                family,
                box_width,
                colors,
                title: String::new(),
                placeholder: String::new(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: title.clone(),
                plugin_rows: Vec::new(),
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: source.clone(),
                error_window: window,
                diagnostic: diagnostic_ref.clone(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
            }
        }
        // -- TUI-10: DialogImagePreview ------------------------------
        Family::ImagePreview => {
            let state = sidecar.preview.clone();
            let count = state.images.len().max(1);
            let preview_rows = terminal_height.saturating_sub(8).max(3);
            let (previous, next) = if state.images.len() > 1 {
                ("← previous".to_owned(), "next →".to_owned())
            } else {
                (String::new(), String::new())
            };
            let caption = state
                .images
                .get(state.index)
                .and_then(|image| image.mention.clone())
                .unwrap_or_else(|| format!("Image {}", state.index + 1));
            DialogInputs {
                family,
                box_width,
                colors,
                title: String::new(),
                placeholder: String::new(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: String::new(),
                plugin_rows: Vec::new(),
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: format!("Image {} of {}", state.index + 1, count),
                preview_footer: (previous, caption, next),
                preview_rows,
                pair_rows: Vec::new(),
            }
        }
        // -- F15: DialogPair — the custom (non-select) tree --------
        Family::Pair => {
            // The root box's inner width (paddingLeft/right 2,
            // dialog-pair.tsx:90); the content walk wraps to it.
            let content_width = box_width.saturating_sub(4).max(1);
            let mut pair_rows = crate::components::dialog_pair::rows(&sidecar.pair, content_width);
            // The height<36 scrollbox (:104-112): the content window
            // clips to the budget (an invisible scrollbar).
            pair_rows.truncate(crate::components::dialog_pair::scroll_budget(terminal_height));
            DialogInputs {
                family,
                box_width,
                colors,
                title: String::new(),
                placeholder: String::new(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: String::new(),
                plugin_rows: Vec::new(),
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows,
            }
        }
    }
}

/// `Locale.titlecase` (util/locale.ts) — the first character upper.
fn titlecase(label: String) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => label,
    }
}

/// The cancel pill's default ("cancel" when the caller passed "").
fn ccancel(label: &str) -> String {
    if label.is_empty() {
        "cancel".to_owned()
    } else {
        label.to_owned()
    }
}

/// The confirm pill's default ("confirm" when the caller passed "").
fn cconfirm(label: &str) -> String {
    if label.is_empty() {
        "confirm".to_owned()
    } else {
        label.to_owned()
    }
}
