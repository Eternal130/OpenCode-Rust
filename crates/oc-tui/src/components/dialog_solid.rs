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
//! * *frame out* (D2.2 dialogs lane): the tree rides the SHARED tree
//!   hub — `mount_dialog_tree` seats the family's chrome element
//!   under the `surface-dialogs` wrapper (skeleton index 5, above
//!   footer) through `chrome_solid::mount_family_wrapper`; pixels
//!   reach the frame through the single compositor seam inside
//!   `render_app` (`composite_chrome_tree`), never a per-family
//!   buffer copy. The mount is keyed on (family, width, colors,
//!   height): height-changing frames (a filter shrinking the window)
//!   rebuild the content subtree, the hot frames (selection moves,
//!   scroll) patch in place; rect changes (resize) patch via
//!   `set_prop`. The retired one-shot scratch renderer (a private
//!   `SolidRenderer` + the `frame_from_buffer` cell copy) is gone.
//! * *backdrop* (NEW-S2a, D2.2 reshaped): the scrim stays a
//!   frame-side compositing step, NOT a tree node — the opentui
//!   raster alpha-composites over its OWN buffer, which cannot see
//!   the route frame beneath (and the seam's hole rule would erase
//!   the glyphs underneath). `DialogSeat::scrim_around` dims every
//!   composited frame cell OUTSIDE the box rect with the same
//!   `blend_backdrop` the old path used (the box's own cells were
//!   dimmed-then-overwritten — skipping them is byte-identical).
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
use opentui_solid::element::{Children, DynChildren, ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, show, text, Element, Signal};

use crate::components::dialog_select::{
    content_rows, flat_indices, grouped, title_clip_end, truncated_title, viewport_height,
    DialogStyles, SelectOption, SelectProps, SelectState,
};
use crate::render::Frame;
use crate::store::{DialogEntry, DialogKind, ExportOptionsState, PluginDialogContent};
use crate::theme::ThemeState;
use crate::wrap::{display_width, take_width, take_width_window};

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
    /// G-487 — the export form's pill/button tokens
    /// (dialog-export-options.tsx:100-268).
    formfield_selected_bg: Rgb,
    formfield_base_bg: Rgb,
    raised_high_bg: Rgb,
    action_base_bg: Rgb,
    action_base_fg: Rgb,
}

impl DialogColors {
    fn of(styles: &DialogStyles) -> Self {
        let rgb = |color: crate::style::Color| -> Rgb {
            match color {
                crate::style::Color::Rgb(r, g, b) => (r, g, b),
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
            formfield_selected_bg: rgb(styles.formfield_selected_bg),
            formfield_base_bg: rgb(styles.formfield_base_bg),
            raised_high_bg: rgb(styles.raised_high_bg),
            action_base_bg: rgb(styles.action_base_bg),
            action_base_fg: rgb(styles.action_base_fg),
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
    /// G-475 — the v2 `dialog.show` face (api.tsx
    /// `createDialogApi.show`): the flattened render mounts RAW as
    /// the ref `Dialog`'s children (ui/dialog.tsx:70-73 — the
    /// paddingTop-1 box, no title block, no `esc` chrome, no side
    /// padding). Distinct from the legacy `ui.dialog.open` payload
    /// ([`Family::PluginRows`], which carries its own title row).
    PluginShow,
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
    /// RE-13: `/btw`'s question prompt (btw.tsx:64-65 — `ctx.ui.
    /// dialog.prompt({ title: "/btw", placeholder: "Ask anything"
    /// })`): the prompt tree with the /btw chrome.
    BtwPrompt,
    /// G-29: the plugin install prompt (`plugins.install`): the
    /// prompt tree titled "Install plugin" over the package-spec
    /// input.
    PluginInstallPrompt,
    /// G-237: `ui/dialog-prompt.tsx` — the generic prompt tree
    /// (`ctx.ui.prompt`, api.tsx:367-386): bold title + `esc`
    /// chrome, the description rows ABOVE the 1-row input, the
    /// placeholder while empty, the `enter submit` footer.
    GenericPrompt,
    /// RE-13: `/btw`'s `Answer` dialog (btw.tsx:88-172): the `/btw`
    /// header + question, the markdown answer window, the `c copy /
    /// j/k ↑/↓ scroll` footer.
    BtwAnswer,
    /// W4 (dialog-status.tsx:22-61): a CUSTOM dialog, never a
    /// `DialogSelect` — the bold `Status` header + `esc` chrome at
    /// paddingLeft 2, then the count line + per-server bullet rows
    /// (or the muted-branch "No MCP servers" fallback), `gap 1`,
    /// `paddingBottom 1`. No filter input.
    Status,
    /// G-487 — `ui/dialog-export-options.tsx`: a CUSTOM form, never a
    /// `DialogSelect` — the bold `Export session` header + `esc`
    /// chrome, the `Export as: ◉ Markdown ○ JSON` pill row, the
    /// format-scoped `[x]` toggles, and the right-justified
    /// Copy/Export buttons. Tab cycles the active control; return
    /// activates it.
    ExportOptions,
    /// G-485② — `DiffViewerHelpDialog` (diff-viewer.tsx:1098-1192): a
    /// CUSTOM centered dialog — the bold `Diff shortcuts` header +
    /// `esc close` chrome, then the key groups (bold header rows,
    /// `[shortcut@17][label]` rows) over the `gap 1` column, no
    /// filter row. The lines ride `hint_lines` with a leading
    /// `\u{1}` marking the group headers.
    DiffHelp,
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
// The bool set mirrors the ref's per-family prop bag (renderFilter,
// flat, destructive, pair_split …) — collapsing them into enums would
// diverge from the 1:1 prop mapping.
#[allow(clippy::struct_excessive_bools)]
struct DialogInputs {
    family: Family,
    box_width: usize,
    /// G-466.3 — the terminal width (the footer's `<60` column-wrap
    /// gate, ui/dialog-select.tsx:838).
    terminal_width: usize,
    /// G-485③ — `renderFilter` (dialog-select.tsx:33): the filter
    /// input row stays unmounted while false.
    render_filter: bool,
    /// G-485① — the `footer` prop's muted text (dialog-select.tsx:20,
    /// 835-845): the `DiffBase` picker's `Remembered until the TUI
    /// exits` — the footer bar's left content.
    hint_footer: String,
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
    actions: Vec<(String, String, bool, bool)>, // (title, label, bold-title hint, muted-gap)
    /// A right-side footer hint (`footerHints side:"right"`): bold
    /// title + muted ` {label}` in the footer bar's right box.
    footer_hint: Option<(String, String)>,
    /// G-465 — the `titleView` suffix: muted text right after the
    /// bold title (the Sessions dialog's ` for {project}`).
    title_suffix: String,
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
    /// The word-wrapped TEXT column (loading / error / loaded) —
    /// [`crate::components::dialog_pair::rows`] stacked (QR rows
    /// included below the text) or
    /// [`crate::components::dialog_pair::split_columns`]'s left
    /// column in the ≥96 split.
    pair_rows: Vec<crate::components::dialog_pair::PairRow>,
    /// The ≥96 horizontal split (dialog-pair.tsx:39): the QR column
    /// renders BESIDE the text column (:75-84), not below it.
    pair_split: bool,
    /// The QR block's lines for the split column (empty stacked —
    /// there the QR rows ride inside `pair_rows`).
    pair_qr: Vec<String>,
    /// RE-13: the `/btw` answer window — the plain-text lines (the
    /// For identity) and the styled rows (the markdown paint).
    btw_window: Vec<String>,
    btw_styled: Vec<StyledText>,
    /// G-487 — the export form's live state (the store's
    /// `ExportOptionsState` snapshot: format + focus + toggles).
    export: crate::store::ExportOptionsState,
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
    /// RE-13: the `/btw` answer's full markdown render (styled rows
    /// at the dialog's inner width — the render needs the theme, so
    /// it is precomputed by the store-owning path) + the window's
    /// scroll offset.
    pub btw_rows: Vec<StyledText>,
    pub btw_scroll: usize,
    /// G-487 — the export form's state snapshot.
    pub export: crate::store::ExportOptionsState,
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
    let viewport = viewport_height(content_rows(&groups, &props.options), terminal_height);
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
        // G-466.3: below 60 cols the left footer box is a COLUMN
        // (ui/dialog-select.tsx:838) — one row per left action.
        // G-485③: without the filter input the title block is 1 row.
        Family::Select => {
            // G-512 — the visible (label-carrying) actions own the
            // footer bar (visibleActions, dialog-select.tsx:168-173):
            // a bind-less plugin action renders no row.
            let visible_actions =
                inputs.actions.iter().filter(|(_, label, _, _)| !label.is_empty()).count();
            let footer_rows = if visible_actions == 0
                && inputs.footer_hint.is_none()
                && inputs.hint_footer.is_empty()
            {
                0
            } else if inputs.terminal_width < 60 {
                visible_actions.max(1)
            } else {
                1
            };
            let title_block = if inputs.render_filter { 3 } else { 1 };
            title_block + 1 + inputs.window.len() + 1 + footer_rows + 1
        }
        // title, gap, input, gap, footer, the footer's paddingBottom.
        // RE-13: the /btw question prompt and the G-29 plugin install
        // prompt share the same prompt tree (title, gap, input, gap,
        // footer, the footer's pad).
        Family::RenamePrompt
        | Family::WorktreeNamePrompt
        | Family::BtwPrompt
        | Family::PluginInstallPrompt => 6,
        // G-237 (dialog-prompt.tsx:98-145): title, gap, description
        // rows, gap, input, gap, footer, the footer's pad. The rows
        // are the PRE-WRAPPED description (the ref's `<text>` wraps
        // at the box content width).
        Family::GenericPrompt => inputs.hint_lines.len() + 7,
        // title, gap, input, gap, hints, gap, footer, pad.
        Family::ConnectPrompt => inputs.hint_lines.len() + 8,
        // title, gap, wrapped lines, hint paddingBottom, root gap,
        // button, button-row paddingBottom.
        Family::Help => inputs.hint_lines.len() + 6,
        // title, gap, rows, root paddingBottom.
        Family::PluginRows => inputs.plugin_rows.len() + 3,
        // G-475 — the v2 dialog.show: the RAW children (the ref
        // box's own paddingTop 1 is the chrome's pad — no title
        // block, no gap, no root pad).
        Family::PluginShow => inputs.plugin_rows.len(),
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
        // In the ≥96 split the QR column sits BESIDE the text
        // column (:44 row flow) — the content height is the MAX of
        // the two columns, not the stacked sum.
        Family::Pair => {
            let rows = if inputs.pair_split {
                inputs.pair_rows.len().max(inputs.pair_qr.len())
            } else {
                inputs.pair_rows.len()
            };
            rows + 3
        }
        // RE-13 (btw.tsx:127-171): the `/btw` + question header, the
        // root's gap 1, the markdown window, gap, the action footer,
        // the root paddingBottom 1.
        Family::BtwAnswer => inputs.btw_window.len() + 5,
        // W4 (dialog-status.tsx:22-61) + G-485② (diff-viewer.tsx:
        // 1143-1191): the title row, the root's gap 1, the rows
        // (status rows / the shortcut-group rows — the inter-group
        // blank lines ride `hint_lines`), the root paddingBottom 1.
        Family::Status | Family::DiffHelp => inputs.hint_lines.len() + 3,
        // G-487 (dialog-export-options.tsx:85-270): the root's gap 1
        // separates all four children — title, pill row, the
        // format-scoped toggle rows (2 markdown / 1 json), and the
        // Copy/Export button row with its paddingBottom 1:
        // markdown = 5 rows + 3 gaps + 1 pad = 10; json = 4 + 2 + 1
        // = 7.
        Family::ExportOptions => {
            if inputs.export.format == crate::store::ExportFormat::Markdown {
                10
            } else {
                7
            }
        }
    }
}

/// The per-row paint state (the tsx per-option `createMemo` cluster:
/// `active`, `current`, the text-color ladder).
struct RowPaint {
    selected: bool,
    current: bool,
    /// The gutter glyph (the failed plugin row's `x`), `None` = the
    /// plain paddingLeft-3 row.
    gutter: Option<char>,
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
        gutter: option.gutter,
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
    // G-522 — `flatten` covers the search marker too (the flat shape
    // from the EMPTY query; the category rides the row footer,
    // dialog-select.tsx:803).
    let footer = if inputs.flatten {
        option
            .search_footer
            .clone()
            .or_else(|| option.category.clone())
            .or_else(|| option.footer.clone())
    } else {
        option.footer.clone()
    };
    // An EMPTY footer renders nothing on the ref (`<Show
    // when={props.footer}>` — "" is falsy, dialog-select.tsx:904), so
    // it reserves no columns and no gap either.
    footer.filter(|footer| !footer.is_empty())
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
    // less that offset. G-488 — the tab menu's rows are NOT select
    // rows: `paddingLeft 1` + title + `paddingRight 1`
    // (session-tabs.tsx:460-462), no marker column, no footer — the
    // budget is the box width less the two padding columns.
    let clip = if inputs.family == Family::Menu {
        inputs.box_width.saturating_sub(2)
    } else {
        title_clip_end(inputs.box_width, footer_text(inputs, index).as_deref()).saturating_sub(4)
    };
    let attrs = u32::from(paint.bold);
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut used = 0usize;
    let mut push = |chunks: &mut Vec<TextChunk>, text: &str, fg: Rgb| {
        let run_fg = if paint.bold { paint.title_fg } else { fg };
        for atom in crate::wrap::flatten(
            text,
            crate::style::Style::new().fg(crate::style::Color::Rgb(run_fg.0, run_fg.1, run_fg.2)),
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
/// G-425b — the gutter mark (the failed plugin row's red `x`):
/// mounted like the marker with dyn width/content; the row's
/// paddingLeft drops to 1 the same way (`current() || gutter`,
/// dialog-select.tsx:783), so the title keeps its column (4).
fn gutter_text_element(
    inputs: &Signal<DialogInputs>,
    index: usize,
) -> opentui_solid::ElementBuilder {
    let paint_at = move |inputs: &DialogInputs| row_paint(inputs, index);
    text()
        .dyn_prop(
            "width",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                usize::from(paint_at(i).is_some_and(|paint| paint.gutter.is_some()))
            })),
        )
        .dyn_prop(
            "marginRight",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                usize::from(paint_at(i).is_some_and(|paint| paint.gutter.is_some()))
            })),
        )
        .dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), move |i: &DialogInputs| match paint_at(i) {
                Some(paint) => match paint.gutter {
                    Some(glyph) => {
                        styled(vec![chunk(glyph.to_string(), i.colors.error_fg, Some(paint.bg), 0)])
                    }
                    None => styled(Vec::new()),
                },
                None => styled(Vec::new()),
            })),
        )
}

// G-487 — one row per select-family row shape (the marker/title/
// description/footer budget walk), mirrored span for span.
#[allow(clippy::too_many_lines)]
fn option_element(inputs: &Signal<DialogInputs>, index: usize) -> Element {
    let paint_at = move |inputs: &DialogInputs| row_paint(inputs, index);
    let gutter_text = gutter_text_element(inputs, index);

    // ● — mounted always, width 0 without the marker.
    let marker_text = text()
        .dyn_prop(
            "width",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                usize::from(
                    paint_at(i).is_some_and(|paint| paint.current && paint.gutter.is_none()),
                )
            })),
        )
        .dyn_prop(
            "marginRight",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                usize::from(
                    paint_at(i).is_some_and(|paint| paint.current && paint.gutter.is_none()),
                )
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
        // G-488 — the tab menu's rows are NOT select rows: the ref's
        // menu row is `paddingLeft 1` + title + `paddingRight 1`
        // straight inside the scrollbox (session-tabs.tsx:460-462) —
        // no scrollbox side pads, no 3-wide select marker column.
        .dyn_prop(
            "marginLeft",
            dyn_num(map(inputs.clone(), |i: &DialogInputs| usize::from(i.family != Family::Menu))),
        )
        .dyn_prop(
            "marginRight",
            dyn_num(map(inputs.clone(), |i: &DialogInputs| usize::from(i.family != Family::Menu))),
        )
        .dyn_prop(
            "paddingRight",
            dyn_num(map(
                inputs.clone(),
                |i: &DialogInputs| {
                    if i.family == Family::Menu {
                        1
                    } else {
                        3
                    }
                },
            )),
        )
        .dyn_prop(
            "paddingLeft",
            dyn_num(map(inputs.clone(), move |i: &DialogInputs| {
                if i.family == Family::Menu {
                    return 1;
                }
                paint_at(i).map_or(
                    3,
                    |paint| {
                        if paint.current || paint.gutter.is_some() {
                            1
                        } else {
                            3
                        }
                    },
                )
            })),
        )
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), move |i: &DialogInputs| {
                paint_at(i).map_or(i.colors.surface, |paint| paint.bg)
            })),
        )
        .children(Children::Static(vec![
            gutter_text.into_element(),
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
            // G-512 — the visible actions only (visibleActions,
            // dialog-select.tsx:168-173 `.filter((item) => item.label)`):
            // a bind-less plugin action carries an empty label and
            // renders no footer row.
            i.actions
                .iter()
                .filter(|(_, label, _, _)| !label.is_empty())
                .map(|(title, _label, _hint, _gap)| ForItem {
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
    // G-466.3: below 60 cols the box is a column with gap 0
    // (ui/dialog-select.tsx:838) — no marginLeft.
    let column = inputs.get().terminal_width < 60;
    Rc::new(for_list(Rc::new(source), move |item: &ForItem, index: usize| {
        let margin = if index > 0 && !column { 2.0 } else { 0.0 };
        let title = item.key.clone();
        let title_for_label = title.clone();
        let label_at = move |i: &DialogInputs| {
            i.actions
                .iter()
                .find(|pair| pair.0 == title_for_label)
                .map(|pair| (pair.1.clone(), pair.2, pair.3))
        };
        box_()
            .flex_direction("row")
            .flex_shrink(0.0)
            .margin_left(margin)
            .children(Children::Static(vec![
                text()
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                            // G-425b — a footerHints pair renders its
                            // title BOLD (the non-action
                            // `FooterAction`, dialog-select.tsx:
                            // 626-633); an action pair's only while
                            // focused.
                            let bold = i
                                .actions
                                .iter()
                                .find(|pair| pair.0 == title)
                                .is_some_and(|pair| pair.2);
                            styled(vec![chunk(title.clone(), i.colors.text, None, u32::from(bold))])
                        })),
                    )
                    .into_element(),
                text()
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                            let Some((label, hint, muted_gap)) = label_at(i) else {
                                return styled(Vec::new());
                            };
                            if hint && !muted_gap {
                                // G-425b — the non-action FooterAction
                                // (dialog-select.tsx:626-633): the
                                // separator space rides the BASE span
                                // (`</b>{" "}`), only the label is
                                // muted. A custom footer prop
                                // (plugins.tsx:248-255) carries the
                                // space inside the muted run.
                                styled(vec![
                                    chunk(" ".to_owned(), i.colors.text, None, 0),
                                    chunk(label, i.colors.muted, None, 0),
                                ])
                            } else {
                                styled(vec![chunk(format!(" {label}"), i.colors.muted, None, 0)])
                            }
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
// W4: the right-side footer hint grows the tree past the lint budget
// (the F1 footer + hint carrier pair).
#[allow(clippy::too_many_lines)] // the DialogSelect root, ui/dialog-select.tsx:672-847
fn select_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    // The title row: bold title left, `esc` right (ends at the
    // paddingRight-4 edge). G-465: when a `titleView` suffix rides
    // the row the left pair is a nested ROW box (the space-between
    // spread must stay two-sided); without one the ORIGINAL single
    // title text stands — the tree scenario captures pin the
    // wrapper-free structure.
    let title_text = text()
        .dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![chunk(i.title.clone(), i.colors.text, None, 1)])
            })),
        )
        .into_element();
    let title_left = if inputs.get().title_suffix.is_empty() {
        title_text
    } else {
        box_()
            .flex_direction("row")
            .children(Children::Static(vec![
                title_text,
                // G-465 — the muted `titleView` suffix rides the same
                // row right after the bold title
                // (dialog-session-list.tsx:198-207).
                text()
                    .wrap_mode("none")
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                            styled(vec![chunk(i.title_suffix.clone(), i.colors.muted, None, 0)])
                        })),
                    )
                    .into_element(),
            ]))
            .into_element()
    };
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            title_left,
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
    let build_input_box = move |inputs: &Signal<DialogInputs>| {
        box_()
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
            ))
    };
    let inputs_for_input = inputs.clone();
    let input_gate = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| i.render_filter)) as Rc<dyn Reactive<bool>>,
        move || {
            vec![box_().padding_top(1.0).child(build_input_box(&inputs_for_input)).into_element()]
        },
    );

    let title_block = box_().padding_left(4.0).padding_right(4.0).children(Children::Static(vec![
        title_row.into_element(),
        // G-485③ — the input block (its paddingTop gap + the row)
        // unmounts under `renderFilter: false`
        // (dialog-select.tsx:685-712); mounted as a DYN carrier (the
        // Show is a children source, not a static element).
        box_().children_dyn(Rc::new(input_gate)).into_element(),
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
        Rc::new(map(inputs.clone(), |i: &DialogInputs| {
            // G-512 — the visible (label-carrying) actions gate the
            // bar (visibleActions, dialog-select.tsx:168-173).
            i.actions.iter().any(|(_, label, _, _)| !label.is_empty())
                || i.footer_hint.is_some()
                || !i.hint_footer.is_empty()
        })) as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs_for_footer;
            move || {
                // The right-side footer hint (`footerHints
                // side:"right"`, dialog-select.tsx:842-845 + the
                // non-action `FooterAction` :626-633): bold title +
                // muted ` {label}`.
                let hint_gate = {
                    let inputs = inputs.clone();
                    show(
                        Rc::new(map(inputs.clone(), |i: &DialogInputs| i.footer_hint.is_some()))
                            as Rc<dyn Reactive<bool>>,
                        {
                            let inputs = inputs.clone();
                            move || {
                                vec![box_()
                                    .flex_direction("row")
                                    .children(Children::Static(vec![
                                        text()
                                            .wrap_mode("none")
                                            .flex_shrink(0.0)
                                            .dyn_prop(
                                                "content",
                                                dyn_styled(map(
                                                    inputs.clone(),
                                                    |i: &DialogInputs| {
                                                        match i.footer_hint.as_ref() {
                                                            Some((title, _)) => styled(vec![
                                                                chunk(
                                                                    title.clone(),
                                                                    i.colors.text,
                                                                    None,
                                                                    1,
                                                                ),
                                                                // The separator space rides the
                                                                // base span (`</b>{" "}`,
                                                                // dialog-select.tsx:631 — text.base,
                                                                // not muted).
                                                                chunk(
                                                                    " ".to_owned(),
                                                                    i.colors.text,
                                                                    None,
                                                                    0,
                                                                ),
                                                            ]),
                                                            None => styled(Vec::new()),
                                                        }
                                                    },
                                                )),
                                            )
                                            .into_element(),
                                        text()
                                            .wrap_mode("none")
                                            .flex_shrink(0.0)
                                            .dyn_prop(
                                                "content",
                                                dyn_styled(map(
                                                    inputs.clone(),
                                                    |i: &DialogInputs| match i.footer_hint.as_ref()
                                                    {
                                                        Some((_, label)) => styled(vec![chunk(
                                                            label.clone(),
                                                            i.colors.muted,
                                                            None,
                                                            0,
                                                        )]),
                                                        None => styled(Vec::new()),
                                                    },
                                                )),
                                            )
                                            .into_element(),
                                    ]))
                                    .into_element()]
                            }
                        },
                    )
                };
                let right_carrier = box_().children_dyn(Rc::new(hint_gate));
                vec![box_()
                    .flex_direction("row")
                    .justify_content("space-between")
                    .padding_left(4.0)
                    .padding_right(2.0)
                    .flex_shrink(0.0)
                    .margin_top(1.0)
                    .children(Children::Static(vec![
                        if inputs.get().hint_footer.is_empty() {
                            // The original shape: the actions as the
                            // left box's DIRECT dyn children (a wrapper
                            // box would stack them — column flow).
                            box_()
                                .flex_direction(if inputs.get().terminal_width < 60 {
                                    "column"
                                } else {
                                    "row"
                                })
                                .children_dyn(actions_for(&inputs))
                                .into_element()
                        } else {
                            // G-485① — the `footer` prop's muted text
                            // rides the left box FIRST
                            // (dialog-select.tsx:838: `{props.
                            // footer}<For each={left()}>`).
                            box_()
                                .flex_direction(if inputs.get().terminal_width < 60 {
                                    "column"
                                } else {
                                    "row"
                                })
                                .children(Children::Static(vec![
                                    text()
                                        .wrap_mode("none")
                                        .dyn_prop(
                                            "content",
                                            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                                                styled(vec![chunk(
                                                    i.hint_footer.clone(),
                                                    i.colors.muted,
                                                    None,
                                                    0,
                                                )])
                                            })),
                                        )
                                        .into_element(),
                                    box_().children_dyn(actions_for(&inputs)).into_element(),
                                ]))
                                .into_element()
                        },
                        right_carrier.into_element(),
                    ]))
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
    let (title, placeholder) = if worktree {
        ("Name worktree", "Worktree name")
    } else {
        ("Rename session", "Session title")
    };
    prompt_builder_titled(inputs, title, placeholder, worktree)
}

/// RE-13: the parameterized prompt tree — the /btw question prompt
/// reuses the exact chrome with its own title/placeholder (btw.tsx:
/// 64-65: `{ title: "/btw", placeholder: "Ask anything" }`).
fn prompt_builder_titled(
    inputs: &Signal<DialogInputs>,
    title: &'static str,
    placeholder: &'static str,
    worktree: bool,
) -> opentui_solid::ElementBuilder {
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

/// G-237: the generic prompt tree (`ui/dialog-prompt.tsx:98-145`):
/// the prompt-family root (paddingLeft/right 2, gap 1) — the bold
/// DYNAMIC title + `esc` chrome, the description rows ABOVE the
/// 1-row input (`props.description?.()` seats before the textarea),
/// the placeholder while empty (default "Enter text"), the
/// `enter submit` footer (dialog-prompt.tsx:131-140). The cursor
/// paints `text.base` (the rename input's tone).
fn generic_prompt_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
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
    // The description block (the ref's `props.description?.()`
    // element) — a For over the hint rows, muted.
    let lines = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.hint_lines
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("P{n}"),
                    value: PropValue::Str(line.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let line_inputs = inputs.clone();
    let description = for_list(std::rc::Rc::new(lines), move |item: &ForItem, _index: usize| {
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
    let description_box = box_().children_dyn(std::rc::Rc::new(description));
    // The input row: placeholder muted / typed value in
    // text.formfield.base with the block cursor.
    let input = text().wrap_mode("none").dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            let width = i.box_width.saturating_sub(4);
            if i.prompt_value.is_empty() {
                styled(vec![chunk(i.placeholder.clone(), i.colors.muted, None, 0)])
            } else {
                // The ref's textarea viewport (dialog-prompt.tsx:79
                // `gotoLineEnd` — the cursor rides the END; upstream
                // `ensure_cursor_visible`'s horizontal arm, wrapMode
                // none): margin_cols = min(max(1, floor(W*0.2)),
                // (W-1)/2) (the 0.2 default scroll margin); when
                // cursor_col + margin_cols >= W the viewport scrolls
                // to cursor_col + margin_cols - W + 1 — the visible
                // window is the TAIL (the pre-fix head take clipped
                // the seeded value's end).
                let cursor_col = display_width(&i.prompt_value);
                let margin_cols = (width / 5).max(1).min(width.saturating_sub(1) / 2);
                let offset_x = if cursor_col + margin_cols >= width {
                    cursor_col + margin_cols - width + 1
                } else {
                    0
                };
                let typed = take_width_window(&i.prompt_value, offset_x, width);
                let end = display_width(&typed);
                let mut chunks = vec![chunk(typed, i.colors.formfield_base_fg, None, 0)];
                if end < width {
                    chunks.push(chunk(" ".to_owned(), i.colors.surface, Some(i.colors.text), 0));
                }
                styled(chunks)
            }
        })),
    );
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(vec![
        title_row.into_element(),
        description_box.margin_top(1.0).into_element(),
        input.margin_top(1.0).into_element(),
        prompt_footer_element(inputs, false).margin_top(1.0).into_element(),
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

/// G-475 — the v2 `dialog.show` family (api.tsx's `dialogApi.show` →
/// the ref `Dialog`'s children, ui/dialog.tsx:70-73): the flattened
/// render rows mount RAW under the chrome's paddingTop 1 — no title
/// row, no `esc` chrome, no side padding (the legacy
/// [`plugin_rows_builder`] chrome belongs to `ui.dialog.open` only).
/// G-510: blank rows render and overlong rows arrive PRE-WRAPPED at
/// the box width ([`build_inputs`] word-wraps the show family —
/// the ref's default wrap grows the box); the `take_width` here is
/// a defensive no-op on already-fitting rows.
fn plugin_show_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
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
                    let text = take_width(&line, i.box_width.max(1));
                    styled(vec![chunk(text, i.colors.muted, None, 0)])
                })),
            )
            .into_element()
    });
    box_().children_dyn(Rc::new(rows_for))
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
// RE-13: the /btw Answer tree (feature-plugins/prompt/btw.tsx:127-171)
// ---------------------------------------------------------------------------

/// `/btw`'s `Answer`: the bold `/btw` + muted question header (the
/// question collapses its whitespace and truncates, btw.tsx:130-
/// 138), the markdown answer window (the styled rows ride
/// `btw_styled`, the For identity `btw_window`), and the
/// `c copy · j/k ↑/↓ scroll` footer (btw.tsx:161-169).
fn btw_answer_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .wrap_mode("none")
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        let clip = i.box_width.saturating_sub(10).max(8);
                        let label_width = display_width("/btw");
                        let question_budget = clip.saturating_sub(label_width + 1);
                        let question =
                            i.error_source.split_whitespace().collect::<Vec<_>>().join(" ");
                        let question = take_width(&question, question_budget);
                        styled(vec![
                            chunk("/btw".to_owned(), i.colors.text, None, 1),
                            chunk(
                                if question.is_empty() {
                                    String::new()
                                } else {
                                    format!(" {question}")
                                },
                                i.colors.muted,
                                None,
                                0,
                            ),
                        ])
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
    // The markdown window: one styled row per line (the identity is
    // the plain text; the paint comes from the index into
    // `btw_styled`).
    let body_lines = ForItemSource {
        source: map(inputs.clone(), |i: &DialogInputs| {
            i.btw_window
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("B{n}"),
                    value: PropValue::Str(line.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let body_inputs = inputs.clone();
    let body_for = for_list(Rc::new(body_lines), move |item: &ForItem, index: usize| {
        let fallback = match &item.value {
            PropValue::Str(line) => line.clone(),
            _ => String::new(),
        };
        let inputs = body_inputs.clone();
        text()
            .wrap_mode("none")
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| {
                    i.btw_styled.get(index).cloned().unwrap_or_else(|| {
                        styled(vec![chunk(fallback.clone(), i.colors.text, None, 0)])
                    })
                })),
            )
            .into_element()
    });
    let body =
        box_().padding_left(2.0).padding_right(2.0).child(box_().children_dyn(Rc::new(body_for)));
    // The action footer: `c copy`, `j/k ↑/↓ scroll` (gap 3).
    let footer = text().dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
            styled(vec![
                chunk("c".to_owned(), i.colors.text, None, 1),
                chunk(" copy".to_owned(), i.colors.muted, None, 0),
                chunk("   j/k ↑/↓ scroll".to_owned(), i.colors.muted, None, 0),
            ])
        })),
    );
    box_().children(Children::Static(vec![
        title_row.into_element(),
        body.margin_top(1.0).into_element(),
        footer.margin_top(1.0).into_element(),
    ]))
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
#[allow(clippy::too_many_lines)] // one builder per layout arm (stacked / ≥96 split)
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
                        crate::components::dialog_pair::PairRow::Text { text, .. }
                        | crate::components::dialog_pair::PairRow::Qr { text } => text.clone(),
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
    // The ≥96 split (dialog-pair.tsx:39-45,75-84): the content box
    // flips to `flexDirection="row"`, `alignItems="flex-start"` —
    // the text column keeps its FIXED 29 cells (`width={horizontal ?
    // 29 : "100%"}`, :45, `flexShrink={0}`) and the QR column
    // (`flexGrow`, `alignItems="flex-end"`, :76-79) renders BESIDE
    // it, the `gap={2}` (:44) as the QR column's marginLeft. The
    // branch is read at mount time (the mount rebuilds on every size
    // change, so the gate flip re-mounts the right shape).
    let content_children = Rc::new(content_for);
    let content = if inputs.get().pair_split {
        let qr_source = ForItemSource {
            source: map(inputs.clone(), |i: &DialogInputs| {
                i.pair_qr
                    .iter()
                    .enumerate()
                    .map(|(n, line)| ForItem {
                        key: format!("Q{n}"),
                        value: PropValue::Str(line.clone()),
                    })
                    .collect::<Vec<ForItem>>()
            }),
        };
        let qr_inputs = inputs.clone();
        let qr_for = for_list(Rc::new(qr_source), move |_item: &ForItem, index: usize| {
            let inputs = qr_inputs.clone();
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs, move |i: &DialogInputs| {
                        // The QR rows paint `theme.text.base`
                        // (dialog-pair.tsx:82).
                        match i.pair_qr.get(index) {
                            Some(line) => styled(vec![chunk(line.clone(), i.colors.text, None, 0)]),
                            None => styled(Vec::new()),
                        }
                    })),
                )
                .into_element()
        });
        let left = box_()
            .width(29.0)
            .flex_shrink(0.0)
            .children_dyn(Rc::clone(&content_children) as Rc<dyn DynChildren>);
        let right = box_()
            .flex_grow(1.0)
            .flex_shrink(0.0)
            .align_items("flex-end")
            .margin_left(2.0)
            .children_dyn(Rc::new(qr_for));
        box_()
            .flex_direction("row")
            .align_items("flex-start")
            .children(Children::Static(vec![left.into_element(), right.into_element()]))
    } else {
        // Stacked (:44 column flow): the QR box below the text (its
        // rows ride inside `pair_rows`).
        box_().children_dyn(content_children)
    };
    box_().padding_left(2.0).padding_right(2.0).padding_bottom(1.0).children(Children::Static(
        vec![title_row.into_element(), content.margin_top(1.0).into_element()],
    ))
}

/// W4 — `DialogStatus` (dialog-status.tsx:22-61): the custom (non-
/// select) tree — the bold `Status` title + `esc` chrome row at
/// paddingLeft 2, `gap 1`, then the status rows (the count line /
/// per-server bullets / the "No MCP servers" fallback — all
/// `theme.text.base`), `paddingBottom 1`. The rows ride `hint_lines`.
fn status_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("Status".to_owned(), i.colors.text, None, 1)])
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
            i.hint_lines
                .iter()
                .enumerate()
                .map(|(n, line)| ForItem {
                    key: format!("S{n}"),
                    value: PropValue::Str(line.clone()),
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
                dyn_styled(map(inputs, move |i: &DialogInputs| match i.hint_lines.get(index) {
                    Some(line) => styled(vec![chunk(line.clone(), i.colors.text, None, 0)]),
                    None => styled(Vec::new()),
                })),
            )
            .into_element()
    });
    box_().padding_left(2.0).padding_right(2.0).padding_bottom(1.0).children(Children::Static(
        vec![
            title_row.into_element(),
            box_().children_dyn(Rc::new(content_for)).margin_top(1.0).into_element(),
        ],
    ))
}

/// G-487 — `DialogExportOptions` (dialog-export-options.tsx:85-270):
/// the custom export form — the bold `Export session` header + `esc`
/// chrome, the `Export as:` pill row (the focused pill carries the
/// formfield focused pair, the selected format the selected pair, the
/// rest the base pair), the format-scoped `[x]`/`[ ]` toggles, and
/// the right-justified Copy/Export buttons (Copy on raised.high,
/// Export on action.primary with its focused state while active).
// G-487 — the whole dialog-export-options.tsx component in one
// builder (title + pill row + format-scoped toggles + buttons),
// mirrored span for span.
#[allow(clippy::too_many_lines)]
fn export_options_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    use crate::store::{ExportActive, ExportFormat};
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("Export session".to_owned(), i.colors.text, None, 1)])
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
    // One pill: `◉ Markdown` / `○ JSON` (:98-125) — paddingLeft 1,
    // paddingRight 1, the radio glyph prefixed to the label.
    let pill = |inputs: &Signal<DialogInputs>, format: ExportFormat, label: &str| {
        let label = label.to_owned();
        box_()
            .padding_left(1.0)
            .padding_right(1.0)
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), move |i: &DialogInputs| {
                    let active = i.export.active == active_of(format);
                    if active {
                        i.colors.formfield_bg
                    } else if i.export.format == format {
                        i.colors.formfield_selected_bg
                    } else {
                        i.colors.formfield_base_bg
                    }
                })),
            )
            .child(text().dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                    let active = i.export.active == active_of(format);
                    let fg = if active {
                        i.colors.formfield_fg
                    } else if i.export.format == format {
                        i.colors.current_fg
                    } else {
                        i.colors.formfield_base_fg
                    };
                    let radio = if i.export.format == format { "◉" } else { "○" };
                    styled(vec![chunk(format!("{radio} {label}"), fg, None, 0)])
                })),
            ))
    };
    let pill_row = box_().flex_direction("row").children(Children::Static(vec![
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                    styled(vec![chunk("Export as:".to_owned(), i.colors.text, None, 0)])
                })),
            )
            .into_element(),
        pill(inputs, ExportFormat::Markdown, "Markdown").margin_left(1.0).into_element(),
        pill(inputs, ExportFormat::Json, "JSON").margin_left(1.0).into_element(),
    ]));
    // One `[x] Include thinking`-style toggle row (:128-204 markdown,
    // :206-245 json).
    let toggle = |inputs: &Signal<DialogInputs>, active_kind: ExportActive, label: &str| {
        let label = label.to_owned();
        box_()
            .flex_direction("row")
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), move |i: &DialogInputs| {
                    if i.export.active == active_kind {
                        i.colors.formfield_bg
                    } else {
                        i.colors.surface
                    }
                })),
            )
            .child(text().dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), move |i: &DialogInputs| {
                    let on = match active_kind {
                        ExportActive::Thinking => i.export.thinking,
                        ExportActive::Tools => i.export.tools,
                        ExportActive::Sanitize => i.export.sanitize,
                        _ => false,
                    };
                    let fg = if i.export.active == active_kind {
                        i.colors.formfield_fg
                    } else if on {
                        i.colors.current_fg
                    } else {
                        i.colors.formfield_base_fg
                    };
                    let mark = if on { "[x]" } else { "[ ]" };
                    styled(vec![chunk(format!("{mark} {label}"), fg, None, 0)])
                })),
            ))
    };
    // The Copy / Export buttons (:246-269) — padding 4, right
    // justified, gap 1, the row's paddingBottom 1.
    let copy_button = box_()
        .padding_left(4.0)
        .padding_right(4.0)
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), |i: &DialogInputs| i.colors.raised_high_bg)),
        )
        .child(text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                styled(vec![chunk("Copy".to_owned(), i.colors.text, None, 0)])
            })),
        ));
    let export_button = box_()
        .padding_left(4.0)
        .padding_right(4.0)
        .dyn_prop(
            "backgroundColor",
            dyn_color(map(inputs.clone(), |i: &DialogInputs| {
                if i.export.active == ExportActive::Export {
                    i.colors.selected_bg
                } else {
                    i.colors.action_base_bg
                }
            })),
        )
        .child(text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                if i.export.active == ExportActive::Export {
                    styled(vec![chunk("Export".to_owned(), i.colors.selected_fg, None, 0)])
                } else {
                    styled(vec![chunk("Export".to_owned(), i.colors.action_base_fg, None, 0)])
                }
            })),
        ));
    let button_row =
        box_().flex_direction("row").justify_content("flex-end").padding_bottom(1.0).children(
            Children::Static(vec![
                copy_button.into_element(),
                export_button.margin_left(1.0).into_element(),
            ]),
        );
    // The format-scoped toggle group mounts per the current format
    // (the Show swap rebuilds the rows — the builders are not Clone).
    let inputs_md = inputs.clone();
    let toggles_markdown = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| i.export.format == ExportFormat::Markdown))
            as Rc<dyn Reactive<bool>>,
        move || {
            vec![
                toggle(&inputs_md, ExportActive::Thinking, "Include thinking").into_element(),
                // The two toggles are ROOT siblings in the tsx
                // (:128-204) — the root's gap 1 separates them.
                toggle(&inputs_md, ExportActive::Tools, "Include tools")
                    .margin_top(1.0)
                    .into_element(),
            ]
        },
    );
    let inputs_js = inputs.clone();
    let toggles_json = show(
        Rc::new(map(inputs.clone(), |i: &DialogInputs| i.export.format == ExportFormat::Json))
            as Rc<dyn Reactive<bool>>,
        move || {
            vec![toggle(&inputs_js, ExportActive::Sanitize, "Sanitize sensitive data")
                .into_element()]
        },
    );
    let children: Vec<Element> = vec![
        title_row.into_element(),
        pill_row.margin_top(1.0).into_element(),
        // One box PER format group (a builder carries a single dyn
        // children slot — two children_dyn calls would clobber); the
        // root's gap 1 rides each child's margin (the hidden group's
        // box still takes its gap slot only while mounted — the Show
        // unmounts the whole box).
        box_().children_dyn(Rc::new(toggles_markdown)).margin_top(1.0).into_element(),
        box_().children_dyn(Rc::new(toggles_json)).margin_top(1.0).into_element(),
        button_row.margin_top(1.0).into_element(),
    ];
    box_().padding_left(2.0).padding_right(2.0).children(Children::Static(children))
}

/// The pill focus enum of a format (dialog-export-options.tsx:22).
fn active_of(format: crate::store::ExportFormat) -> crate::store::ExportActive {
    match format {
        crate::store::ExportFormat::Markdown => crate::store::ExportActive::Markdown,
        crate::store::ExportFormat::Json => crate::store::ExportActive::Json,
    }
}

/// G-485② — `DiffViewerHelpDialog` (diff-viewer.tsx:1143-1191): the
/// bold `Diff shortcuts` title + the `esc close` chrome, then the
/// group column — bold headers, `[shortcut@17]` base-fg cells with
/// the muted labels — `gap 1` between the groups (a blank line rides
/// every hint line after a header), `paddingBottom 1`. The rows
/// arrive pre-padded through `hint_lines`; a leading `\u{1}` marks a
/// group header.
fn diff_help_builder(inputs: &Signal<DialogInputs>) -> opentui_solid::ElementBuilder {
    let title_row = box_().flex_direction("row").justify_content("space-between").children(
        Children::Static(vec![
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("Diff shortcuts".to_owned(), i.colors.text, None, 1)])
                    })),
                )
                .into_element(),
            text()
                .dyn_prop(
                    "content",
                    dyn_styled(map(inputs.clone(), |i: &DialogInputs| {
                        styled(vec![chunk("esc close".to_owned(), i.colors.muted, None, 0)])
                    })),
                )
                .into_element(),
        ]),
    );
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
    let rows_for = for_list(Rc::new(lines), move |item: &ForItem, _index: usize| {
        let line = match &item.value {
            PropValue::Str(line) => line.clone(),
            _ => String::new(),
        };
        let inputs = line_inputs.clone();
        // A group header (bold, text.base) or a key row: the
        // 17-wide shortcut column in text.base then the muted label
        // (the pre-padded line carries both — split by DISPLAY
        // width, the arrows are multibyte).
        text()
            .dyn_prop(
                "content",
                dyn_styled(map(inputs, move |i: &DialogInputs| {
                    if let Some(title) = line.strip_prefix('\u{1}') {
                        styled(vec![chunk(title.to_owned(), i.colors.text, None, 1)])
                    } else {
                        let mut width = 0usize;
                        let mut split = line.len();
                        for (at, ch) in line.char_indices() {
                            if width >= 17 {
                                split = at;
                                break;
                            }
                            width += if ch.is_ascii() { 1 } else { 2 };
                        }
                        let (shortcut, label) = line.split_at(split);
                        // `label` keeps the row's `gap={2}` leading
                        // spaces (the column + gap = 19 before the
                        // label text).
                        styled(vec![
                            chunk(shortcut.to_owned(), i.colors.text, None, 0),
                            chunk(label.to_owned(), i.colors.muted, None, 0),
                        ])
                    }
                })),
            )
            .into_element()
    });
    box_().padding_left(2.0).padding_right(2.0).padding_bottom(1.0).children(Children::Static(
        vec![
            title_row.into_element(),
            box_().children_dyn(Rc::new(rows_for)).margin_top(1.0).into_element(),
        ],
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
        Family::PluginShow => plugin_show_builder(&family_inputs),
        Family::Menu => menu_builder(&family_inputs),
        Family::Alert => alert_builder(&family_inputs),
        Family::Confirm => confirm_builder(&family_inputs),
        Family::ErrorDetails => error_details_builder(&family_inputs),
        Family::ImagePreview => image_preview_builder(&family_inputs),
        Family::Pair => pair_builder(&family_inputs),
        Family::BtwPrompt => prompt_builder_titled(&family_inputs, "/btw", "Ask anything", false),
        Family::PluginInstallPrompt => prompt_builder_titled(
            &family_inputs,
            "Install plugin",
            "npm package or git specifier",
            false,
        ),
        Family::GenericPrompt => generic_prompt_builder(&family_inputs),
        Family::BtwAnswer => btw_answer_builder(&family_inputs),
        Family::Status => status_builder(&family_inputs),
        Family::ExportOptions => export_options_builder(&family_inputs),
        Family::DiffHelp => diff_help_builder(&family_inputs),
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

/// The mounted dialog tree's reactive state, kept across frames so
/// content changes (a moving selection, typed filter, scroll) patch
/// the content dyn-props in place (the M2 rule); a mount-key change
/// (family, box width, colors, content height) rebuilds the wrapper's
/// content subtree. The wrapper itself lives in the shared-tree hub
/// (`chrome_solid::mount_family_wrapper`, D2.2 dialog lane).
struct DialogTree {
    inputs: Signal<DialogInputs>,
    key: (usize, usize, usize, bool, DialogColors, usize),
}

fn mount_key(inputs: &DialogInputs) -> (usize, usize, usize, bool, DialogColors, usize) {
    // G-466.3: terminal_width rides the key — the footer's column
    // wrap is a build-time decision. G-465: so is the title-suffix
    // wrapper's presence (the wrapper-free structure is committed in
    // the tree-scenario captures).
    (
        inputs.family as usize,
        inputs.box_width,
        inputs.terminal_width,
        inputs.title_suffix.is_empty(),
        inputs.colors,
        1 + content_height(inputs),
    )
}

thread_local! {
    /// One dialog tree per thread-process — the app mounts exactly
    /// one overlay per frame; the harnesses reuse it across boots.
    static DIALOG: RefCell<Option<DialogTree>> = const { RefCell::new(None) };
}

/// The dialog family's seat under the shared tree's root (the
/// skeleton table's index 5, above sidebar/footer). The ref's
/// dialog box is a `zIndex 3000` absolute sibling rendered AFTER the
/// whole App subtree (ui/dialog.tsx:228-247), so it COVERS every
/// App-mounted overlay — but reseating the family above the
/// overlays (12) trips the opentui-solid keyed-For reconcile defect
/// (G-429's zombie class: the live palette's filter repaint stops
/// emitting, `pty_dialog_palette_cycle_solid_path` red). The
/// inversion stays bounded instead: the ONE evidenced overlap (the
/// toast under the /plugins box, plugin-fallback) is handled by the
/// toast's [`ToastRailCover`] clip — the toast interior and rails
/// never paint inside the dialog rect, and the scrim dims the rails
/// — matching the ref's covered pixels for every corpus shape.
const DIALOG_SKELETON_INDEX: usize = 5;

// ---------------------------------------------------------------------------
// the overlay mount (the tree seat; the retired paste's twin data)
// ---------------------------------------------------------------------------

/// Mount the top dialog's tree into the shared tree's `surface-dialogs`
/// wrapper — the D2.2 dialog lane's ONLY live mount entry. No Frame
/// comes back; pixels reach the emitted frame through
/// [`crate::components::chrome_solid::composite_chrome_tree`], the
/// single compositor seam inside `render_app`. The wrapper rect is the
/// old paste site (`(box_x, box_y)` — the imperative geometry source;
/// resize patches via `set_prop`); the content subtree rebuilds only
/// when the mount key changes. The backdrop is NOT applied here — the
/// scrim must dim the composited frame pixels (all families below),
/// so it rides [`DialogSeat::scrim_around`] after the seam.
#[must_use]
pub fn mount_dialog_tree(
    entry: &DialogEntry,
    props: &SelectProps,
    plugin: Option<&PluginDialogContent>,
    theme: &ThemeState,
    terminal: (u16, u16),
    sidecar: &DialogSidecar,
) -> DialogSeat {
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
        // G-488 — the ref's `width()` is exactly `min(term, max(20,
        // max(min(50, len+2))))` (session-tabs.tsx:424-426) and its
        // rows are `paddingLeft 1` + title + `paddingRight 1`
        // (:460-462) — the previous +6/4-cell marker column (a
        // registered approximation) is gone: the menu rows are NOT
        // select rows.
        let menu_width = width.min(longest.max(20));
        let menu_height = (props.options.len()).min(height).max(1) + 1;
        let DialogKind::TabMenu { x, y, .. } = &entry.kind else {
            unreachable!("the TabMenu guard")
        };
        let left = (*x).min(width.saturating_sub(menu_width));
        let top = (*y + 1).min(height.saturating_sub(menu_height));
        (menu_width, left, top, false)
    } else {
        let box_width = entry.size.width().min(width.saturating_sub(2));
        let box_x = (width - box_width) / 2;
        // TUI-10: the image preview is CENTERED (`dialog.
        // setCentered(true)`, dialog-image-preview.tsx:24) — the
        // content-height walk pins the box's height first. F15: the
        // pair dialog is centered too (dialog-pair.tsx:19).
        // G-485②: the diff shortcuts dialog as well
        // (diff-viewer.tsx:771's `centered: true`).
        // G-475: a plugin dialog's `ctx.ui.dialog.set({centered})`
        // flag joins them (api.tsx:337-340).
        let centered = entry.centered
            || matches!(
                entry.kind,
                DialogKind::ImagePreview | DialogKind::Pair | DialogKind::DiffHelp
            );
        let box_y = if centered {
            let inputs_probe = build_inputs(
                entry,
                props,
                plugin,
                DialogColors::of(&DialogStyles::from_theme(theme)),
                box_width,
                width,
                height,
                sidecar,
            );
            // The ref centers via flex `justifyContent: "center"`
            // (dialog.tsx:48) — taffy hands the odd leftover row to
            // the TOP half (36-23 → 7 above, 6 below): round up.
            (height + 1).saturating_sub(1 + content_height(&inputs_probe)) / 2
        } else {
            (height + 2) / 4
        };
        (box_width, box_x, box_y, true)
    };

    let inputs = build_inputs(entry, props, plugin, colors, box_width, width, height, sidecar);
    let box_height = 1 + content_height(&inputs);
    let key = mount_key(&inputs);

    // Rebuild iff the mount key changed OR the wrapper left the tree
    // (a hub reset between harness frames, a seam prune).
    let rebuild = !crate::components::chrome_solid::family_wrapper_live("dialogs")
        || DIALOG.with_borrow(|slot| !matches!(slot, Some(tree) if tree.key == key));
    let (content, content_key) = if rebuild {
        let signal = Signal::new(inputs.clone());
        let element =
            chrome_element(&signal, inputs.family, inputs.box_width.max(1), box_height.max(1));
        DIALOG.with_borrow_mut(|slot| *slot = Some(DialogTree { inputs: signal, key }));
        (Some(element), Some(format!("dialog-content@{key:?}")))
    } else {
        DIALOG.with_borrow(|slot| {
            slot.as_ref().expect("checked above").inputs.set(inputs.clone());
        });
        (None, None)
    };
    crate::components::chrome_solid::mount_family_wrapper(
        "dialogs",
        DIALOG_SKELETON_INDEX,
        box_x,
        box_y,
        box_width,
        box_height,
        content_key,
        content,
        &[],
        &[],
        false,
    );

    let rect =
        (box_x, box_y, box_width.min(width - box_x), box_height.min(height.saturating_sub(box_y)));
    DialogSeat { rect, dim_backdrop, button_hits: dialog_chrome_button_hits(entry, &inputs, rect) }
}

/// The mounted dialog's frame-side paint instructions: the chrome
/// rect (the outside-click dismissal surface), whether the backdrop
/// scrim must dim the composited frame around it (the old paste
/// dimmed the whole frame then overwrote the box — skipping the
/// box's own cells is byte-identical, the box paint is opaque), and
/// the chrome's pointer targets.
pub struct DialogSeat {
    /// The chrome box's absolute, screen-clamped rect.
    pub rect: (usize, usize, usize, usize),
    /// Whether this dialog dims the backdrop (`TabMenu` does not).
    pub dim_backdrop: bool,
    /// The chrome buttons (auditA #6): the header close label + the
    /// Help `ok` pill.
    pub button_hits: Vec<crate::store::DialogButtonHit>,
}

impl DialogSeat {
    /// The backdrop scrim as a band walk around the chrome rect:
    /// `blend_backdrop` every frame cell OUTSIDE the rect (the
    /// composited box pixels are opaque — dimming then overwriting
    /// them was the old paste's no-op half).
    pub fn scrim_around(&self, base: &mut Frame) {
        if !self.dim_backdrop {
            return;
        }
        let (box_x, box_y, box_width, box_height) = self.rect;
        for (y, row) in base.cells.iter_mut().enumerate() {
            let inside_row = y >= box_y && y < box_y + box_height;
            for (x, cell) in row.iter_mut().enumerate() {
                if !(inside_row && x >= box_x && x < box_x + box_width) {
                    crate::components::dialog::blend_backdrop(cell);
                }
            }
        }
    }
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
    // backdrop/escape/right-press close it). G-475: the v2
    // dialog.show face mounts RAW children (no title row, no `esc`
    // label — the ref Dialog adds none).
    if matches!(inputs.family, Family::Menu | Family::PluginShow) {
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
        | Family::Pair
        | Family::BtwPrompt
        | Family::PluginInstallPrompt
        | Family::GenericPrompt
        | Family::BtwAnswer
        | Family::Status
        // G-475 — unreachable (the early return above keeps the v2
        // dialog.show out of the chrome-button walk); the arm keeps
        // the match total.
        | Family::PluginShow
        | Family::ExportOptions => ("esc", 2),
        Family::Help => ("esc/enter", 2),
        // G-485② — the diff shortcuts' close hint.
        Family::DiffHelp => ("esc close", 2),
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
// One struct-literal builder for the dialog's input bundle; the 8th
// arg keeps every field explicit rather than hiding defaults.
#[allow(clippy::too_many_arguments)]
fn build_inputs(
    entry: &DialogEntry,
    props: &SelectProps,
    plugin: Option<&PluginDialogContent>,
    colors: DialogColors,
    box_width: usize,
    terminal_width: usize,
    terminal_height: usize,
    sidecar: &DialogSidecar,
) -> DialogInputs {
    let family = match &entry.kind {
        DialogKind::SessionRename { .. } => Family::RenamePrompt,
        // G-486 — the accounts view's rename prompt shares the
        // connect-prompt tree (dynamic title + placeholder).
        DialogKind::IntegrationRename { .. } => Family::ConnectPrompt,
        DialogKind::WorktreeName => Family::WorktreeNamePrompt,
        // RE-13: the /btw pair — the question prompt + the Answer
        // markdown dialog (btw.tsx:64-65, 88-172).
        DialogKind::BtwPrompt => Family::BtwPrompt,
        // G-29: the plugin install prompt.
        DialogKind::PluginInstall => Family::PluginInstallPrompt,
        // G-237: the generic prompt (`ctx.ui.prompt`) — title,
        // description and placeholder ride the kind.
        DialogKind::Prompt { .. } => Family::GenericPrompt,
        DialogKind::BtwAnswer { .. } => Family::BtwAnswer,
        // Lane FINAL: the connect-flow prompts (KeyMethod's API-key
        // input / OAuthCode's code entry) — the prompt tree with a
        // dynamic title + the attempt's URL/instructions rows.
        DialogKind::IntegrationKey { .. } | DialogKind::IntegrationCode { .. } => {
            Family::ConnectPrompt
        }
        // G-271④ — the form walk's textual fields reuse the connect
        // prompt family (dynamic title + the field's placeholder +
        // description/error rows); the select-style fields carry
        // option rows and fall to the generic Select family below.
        DialogKind::IntegrationForm { .. } if props.options.is_empty() => Family::ConnectPrompt,
        DialogKind::Help => Family::Help,
        DialogKind::TabMenu { .. } => Family::Menu,
        DialogKind::Plugin { request_id: Some(_), .. }
            if plugin.is_some_and(|content| !content.is_select()) =>
        {
            Family::PluginShow
        }
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
        // W4: the status dialog's CUSTOM tree (ref dialog-status.tsx
        // is not a DialogSelect).
        DialogKind::Status => Family::Status,
        // G-487: the export options' CUSTOM form (ref
        // dialog-export-options.tsx is not a DialogSelect).
        DialogKind::SessionExport { .. } => Family::ExportOptions,
        // G-485② — the diff shortcuts' custom tree (the props arm
        // feeds the rows through `hints`).
        DialogKind::DiffHelp => Family::DiffHelp,
        _ => Family::Select,
    };
    match family {
        Family::Select | Family::Menu => {
            let (window, flat) = visible_window(props, &entry.select, terminal_height);
            DialogInputs {
                family,
                box_width,
                terminal_width,
                colors,
                title: props.title.clone(),
                placeholder: props.placeholder.clone(),
                filter: entry.select.filter.clone(),
                options: props.options.clone(),
                flat,
                selected: entry.select.selected,
                current: props.current.clone(),
                flatten: props.has_search || (props.flat && !entry.select.filter.is_empty()),
                destructive: props.destructive,
                window,
                actions: props
                    .actions
                    .iter()
                    .map(|action| {
                        (action.title.clone(), action.label.clone(), action.hint, action.muted_gap)
                    })
                    .collect(),
                footer_hint: props.footer_hint.clone(),
                title_suffix: props.title_suffix.clone().unwrap_or_default(),
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
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: props.render_filter,
                hint_footer: props.hints.first().cloned().unwrap_or_default(),
                plugin_rows: Vec::new(),
            }
        }
        // RE-13/G-29: the btw + plugin-install prompts share the
        // renamed-prompt tree (empty title/placeholder entry).
        Family::RenamePrompt
        | Family::WorktreeNamePrompt
        | Family::BtwPrompt
        | Family::PluginInstallPrompt => DialogInputs {
            family,
            box_width,
            terminal_width,
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
            footer_hint: None,
            title_suffix: String::new(),
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
            pair_split: false,
            pair_qr: Vec::new(),
            btw_window: Vec::new(),
            btw_styled: Vec::new(),
            export: ExportOptionsState::default(),
            render_filter: true,
            hint_footer: String::new(),
        },
        // G-237: the generic prompt — title/placeholder/description
        // from the kind, the typed value from the entry. The
        // description PRE-WRAPS at the box content width
        // (dialog-prompt.tsx:113-114 — the ref's `<text>` wraps; the
        // port's text pipeline renders the same rows, but the height
        // model counts `hint_lines`, so the wrap must happen HERE —
        // the pre-fix single row clipped the overflow rows).
        Family::GenericPrompt => {
            let DialogKind::Prompt { title, description, placeholder, .. } = &entry.kind else {
                unreachable!("the GenericPrompt family guard")
            };
            DialogInputs {
                family,
                box_width,
                terminal_width,
                colors,
                title: title.clone(),
                placeholder: placeholder.clone(),
                filter: String::new(),
                options: Vec::new(),
                flat: Vec::new(),
                selected: 0,
                current: None,
                flatten: false,
                destructive: false,
                window: Vec::new(),
                actions: Vec::new(),
                footer_hint: None,
                title_suffix: String::new(),
                prompt_value: entry.value.clone(),
                hint_lines: if description.is_empty() {
                    Vec::new()
                } else {
                    crate::wrap::wrap_label(description, box_width.saturating_sub(4).max(1))
                },
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
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
            }
        }
        // RE-13: the /btw Answer dialog — the sidecar's markdown
        // rows sliced to the window (the ref's scrollbox maxHeight,
        // btw.tsx:101).
        Family::BtwAnswer => {
            let DialogKind::BtwAnswer { question, .. } = &entry.kind else {
                unreachable!("the BtwAnswer family guard")
            };
            let window_rows = (terminal_height * 3 / 5).max(8).min(sidecar.btw_rows.len().max(1));
            let btw_window: Vec<String> = sidecar
                .btw_rows
                .iter()
                .skip(sidecar.btw_scroll)
                .take(window_rows)
                .map(opentui_core::StyledText::plain_text)
                .collect();
            let btw_styled: Vec<StyledText> = sidecar
                .btw_rows
                .iter()
                .skip(sidecar.btw_scroll)
                .take(window_rows)
                .cloned()
                .collect();
            DialogInputs {
                family,
                box_width,
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: "/btw".to_owned(),
                plugin_rows: Vec::new(),
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: question.clone(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window,
                btw_styled,
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
            }
        }
        // The connect prompts carry their title/placeholder/hints
        // through the props (OAuthCode's description block holds the
        // attempt's URL + instructions, dialog-integration.tsx:
        // 719-725; the inline error rides the same rows).
        Family::ConnectPrompt => DialogInputs {
            family,
            box_width,
            terminal_width,
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
            footer_hint: None,
            title_suffix: String::new(),
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
            pair_split: false,
            pair_qr: Vec::new(),
            btw_window: Vec::new(),
            btw_styled: Vec::new(),
            export: ExportOptionsState::default(),
            render_filter: true,
            hint_footer: String::new(),
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
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
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
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
            }
        }
        Family::PluginRows | Family::PluginShow => {
            let content = plugin.cloned().unwrap_or_default();
            // G-510 — the `dialog.show` family mounts its rows like
            // the ref's raw Dialog children (ui/dialog.tsx:56-70):
            // blank rows render (a taller box) and an overlong row
            // WRAPS at the box width with the default word wrap
            // (dialogWidth(size) = 60 medium) growing the box — the
            // hard clip is the legacy `ui.dialog.open` family's
            // contract only.
            let plugin_rows = if matches!(family, Family::PluginShow) {
                content
                    .rows
                    .iter()
                    .flat_map(|line| crate::wrap::wrap_label(line, box_width.max(1)))
                    .collect::<Vec<String>>()
            } else {
                content.rows
            };
            DialogInputs {
                family,
                box_width,
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: content.title,
                plugin_rows,
                message: String::new(),
                confirm_labels: (String::new(), String::new(), true),
                error_source: String::new(),
                error_window: Vec::new(),
                diagnostic: String::new(),
                preview_title: String::new(),
                preview_footer: (String::new(), String::new(), String::new()),
                preview_rows: 0,
                pair_rows: Vec::new(),
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
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
                        // G-520 — `Locale.titlecase` is WORD-boundary
                        // (util/locale.ts:3-5 `/\b\w/g`): the same
                        // implementation the agent labels use
                        // (message/model.rs:1102). G-521 — an EXPLICIT
                        // empty label stays empty (the ref's
                        // `props.label?.[key] ?? key` only substitutes
                        // null/undefined; the null boundary already
                        // ran in plugin_ui.rs).
                        message_model_titlecase(cancel_label),
                        message_model_titlecase(confirm_label),
                        *active_confirm,
                    ),
                ),
                _ => (String::new(), String::new(), (String::new(), String::new(), true)),
            };
            DialogInputs {
                family,
                box_width,
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
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
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
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
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
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
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
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
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
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
                pair_split: false,
                pair_qr: Vec::new(),
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
            }
        }
        // -- F15: DialogPair — the custom (non-select) tree --------
        Family::Pair => {
            // The root box's inner width (paddingLeft/right 2,
            // dialog-pair.tsx:90); the stacked walk wraps to it.
            let content_width = box_width.saturating_sub(4).max(1);
            // The height<36 scrollbox (:104-112): the content window
            // clips to the budget (an invisible scrollbar).
            let budget = crate::components::dialog_pair::scroll_budget(terminal_height);
            let (pair_rows, pair_split, pair_qr) =
                if crate::components::dialog_pair::is_horizontal(terminal_width) {
                    // The ≥96 split (:39-45,75-84): the text column
                    // wraps to its fixed 29 cells, the QR column
                    // renders BESIDE it (both clipped to the budget).
                    let (mut text, mut qr) =
                        crate::components::dialog_pair::split_columns(&sidecar.pair);
                    text.truncate(budget);
                    qr.truncate(budget);
                    (text, true, qr)
                } else {
                    // Stacked (:44 column flow): the QR box is the
                    // content's SECOND child — below the text column.
                    let mut rows =
                        crate::components::dialog_pair::rows(&sidecar.pair, content_width);
                    rows.truncate(budget);
                    (rows, false, Vec::new())
                };
            DialogInputs {
                family,
                box_width,
                terminal_width,
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
                footer_hint: None,
                title_suffix: String::new(),
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
                pair_split,
                pair_qr,
                btw_window: Vec::new(),
                btw_styled: Vec::new(),
                export: ExportOptionsState::default(),
                render_filter: true,
                hint_footer: String::new(),
            }
        }
        // -- W4: DialogStatus — the custom (non-select) tree ------
        Family::Status => DialogInputs {
            family,
            box_width,
            terminal_width,
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
            footer_hint: None,
            title_suffix: String::new(),
            prompt_value: String::new(),
            // The status rows ride the hints (the props layer derives
            // them — see the `DialogKind::Status` props arm).
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
            pair_split: false,
            pair_qr: Vec::new(),
            btw_window: Vec::new(),
            btw_styled: Vec::new(),
            export: ExportOptionsState::default(),
            render_filter: true,
            hint_footer: String::new(),
        },
        // G-487 — the export form rides the SIDECAR's state snapshot
        // (the store's export cell — the pill focus + the toggles
        // live there, keyed by the store-owning mount path).
        Family::ExportOptions => DialogInputs {
            family,
            box_width,
            terminal_width,
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
            footer_hint: None,
            title_suffix: String::new(),
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
            pair_rows: Vec::new(),
            pair_split: false,
            pair_qr: Vec::new(),
            btw_window: Vec::new(),
            btw_styled: Vec::new(),
            export: sidecar.export,
            render_filter: true,
            hint_footer: String::new(),
        },
        // G-485② — the diff shortcuts' rows ride `hints` (the group
        // headers carry the `\u{1}` marker).
        Family::DiffHelp => DialogInputs {
            family,
            box_width,
            terminal_width,
            colors,
            title: props.title.clone(),
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
            footer_hint: None,
            title_suffix: String::new(),
            prompt_value: String::new(),
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
            pair_split: false,
            pair_qr: Vec::new(),
            btw_window: Vec::new(),
            btw_styled: Vec::new(),
            export: ExportOptionsState::default(),
            render_filter: true,
            hint_footer: String::new(),
        },
    }
}

/// G-521 — the confirm/cancel pill label: the ref's
/// `Locale.titlecase(props.label?.[key] ?? key)` word-boundary
/// titlecase (util/locale.ts:3-5) — the shared implementation
/// (message/model.rs:1102) re-exported here; the null→key default ran
/// at the plugin-ui crossing, so an explicit "" stays "" (an empty
/// pill, dialog-confirm.tsx:85).
fn message_model_titlecase(label: &str) -> String {
    crate::components::message::model::titlecase(label)
}

#[cfg(test)]
mod d22_tests {
    use super::*;
    use crate::store::{DialogEntry, DialogKind, DialogSize};

    fn select_entry() -> DialogEntry {
        DialogEntry {
            kind: DialogKind::CommandPalette,
            size: DialogSize::Medium,
            centered: false,
            select: crate::components::dialog_select::SelectState::default(),
            value: String::new(),
            theme_initial: None,
        }
    }

    /// The seam seats the wrapper at skeleton index 5 and a frame
    /// with no dialog prunes it (the old paste branch's else arm).
    #[test]
    fn dialog_wrapper_unmounts_when_no_dialog_mounts() {
        crate::components::chrome_solid::reset_hub();
        let theme = crate::theme::ThemeState::v2_dark();
        let props = crate::components::dialog_select::SelectProps::new(
            "Commands",
            vec![crate::components::dialog_select::SelectOption {
                title: "Switch theme".to_owned(),
                value: "theme".to_owned(),
                description: None,
                category: None,
                search_text: None,
                search_footer: None,
                footer: None,
                title_width: None,
                truncate_title: crate::components::dialog_select::TitleTruncate::default(),
                shimmer: None,
                details: Vec::new(),
                gutter: None,
                tone: None,
            }],
        );
        let sidecar = DialogSidecar::default();
        let seat = mount_dialog_tree(&select_entry(), &props, None, &theme, (80, 24), &sidecar);
        let mut frame = crate::components::chrome_solid::blank_frame(80, 24);
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);
        assert!(frame.text_grid().join("\n").contains("Commands"));
        seat.scrim_around(&mut frame);
        // The next seam with NO dialog sync prunes the wrapper: the
        // box pixels are gone from the composited frame.
        let mut fresh = crate::components::chrome_solid::blank_frame(80, 24);
        crate::components::chrome_solid::composite_chrome_tree(&mut fresh);
        assert!(!fresh.text_grid().join("\n").contains("Commands"));
        crate::components::chrome_solid::reset_hub();
    }

    /// G-475 — the v2 `dialog.show` family: the RAW rows mount under
    /// the chrome's pad at the box's content origin (no title/esc
    /// chrome, no side padding), the box height = 1 + rows.
    #[test]
    fn plugin_show_family_renders_raw_rows() {
        crate::components::chrome_solid::reset_hub();
        let theme = crate::theme::ThemeState::v2_dark();
        let store = crate::store::Store::new(crate::reactive::Runtime::new());
        store.open_plugin_dialog_show(
            "v21.dialog.tui",
            crate::store::PluginDialogContent {
                title: String::new(),
                rows: vec!["v21 dialog body".to_owned()],
                options: Vec::new(),
                placeholder: None,
                current: None,
                has_search: false,
            },
            7,
            Some("large"),
            false,
        );
        // G-508 — `dialog.set` on the open show dialog: an explicit
        // size mutates the entry (88); an ABSENT size RESETS to the
        // medium 60 (api.tsx:337-340 `options.size ?? "medium"`).
        let entry = store.dialogs().last().expect("the show entry").clone();
        let plugin = store.plugin_dialog_content(match &entry.kind {
            DialogKind::Plugin { id, .. } => id,
            _ => unreachable!(),
        });
        let props = crate::components::dialog_select::SelectProps::new("Commands", Vec::new());
        let sidecar = DialogSidecar::default();
        let seat = mount_dialog_tree(&entry, &props, plugin.as_ref(), &theme, (120, 36), &sidecar);
        // The ref's geometry (ui/dialog.tsx): width 88 for large,
        // paddingTop height/4 = 9, box height 1 + 1 row = 2.
        assert_eq!(seat.rect, (16, 9, 88, 2));
        // set({size:"large"}) — the entry widens.
        store.set_plugin_dialog_presentation(Some("large"), false);
        let entry = store.dialogs().last().expect("the show entry").clone();
        let seat = mount_dialog_tree(&entry, &props, plugin.as_ref(), &theme, (120, 36), &sidecar);
        assert_eq!(seat.rect.2, 88, "the explicit large set applies");
        // set({centered:true}) with an ABSENT size — the medium reset.
        store.set_plugin_dialog_presentation(None, true);
        let entry = store.dialogs().last().expect("the show entry").clone();
        let seat = mount_dialog_tree(&entry, &props, plugin.as_ref(), &theme, (120, 36), &sidecar);
        assert_eq!(seat.rect.2, 60, "the absent size resets to medium (G-508)");
        assert!(entry.centered);
        let mut frame = crate::components::chrome_solid::blank_frame(120, 36);
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("v21 dialog body"), "the raw row renders");
        // No esc chrome: the row is the ONLY text the dialog paints.
        assert!(!grid.contains("esc"), "the v2 dialog.show carries no esc chrome");
        crate::components::chrome_solid::reset_hub();
    }
}
