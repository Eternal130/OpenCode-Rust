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
//! **Selection flag.** Default ON (flipped after this lane's rig
//! proof — cell-for-cell unit parity over the dialog-kind × width ×
//! state matrix, identical rig mismatch-step sets, both PTY paths
//! green, 1.4 ms worst frame vs the 40 ms budget): dialogs render
//! through this module. `OC_DIALOGS_OPENTUI=0` (or `false`/`off`)
//! selects the hand-rolled fallback renderer for one release. Read
//! once at the first dialog paint ("at boot") via `OnceLock`: a
//! running session never switches renderers.
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
//! **Divergence class (documented, matrix-bounded).** The old
//! renderer paints long text runs past their box's padding edge (a
//! title runs to the frame edge; the footer then overwrites). The
//! tree clips at the box edge (the tsx's own `overflow: hidden`
//! behavior). The parity matrix uses realistic lengths where the two
//! agree; extreme-overflow frames are a documented divergence, not a
//! parity regression.

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
    content_rows, flat_indices, grouped, viewport_height, DialogStyles, SelectOption, SelectProps,
    SelectState,
};
use crate::render::Frame;
use crate::store::{DialogEntry, DialogKind, PluginDialogContent};
use crate::theme::ThemeState;
use crate::wrap::{display_width, take_width};

/// The renderer selection flag (lane M2, default FLIPPED ON after
/// the rig proof): `OC_DIALOGS_OPENTUI=0` (or `false`/`off`) boots
/// dialogs on the hand-rolled fallback renderer; every other value —
/// including unset — uses this module's opentui-solid path. Read once
/// (first dialog paint — the TUI's boot) and cached for the process
/// lifetime, so a running session never switches renderers
/// mid-flight. The old renderer stays compiled for one release as
/// the escape hatch.
#[must_use]
pub fn opentui_dialogs_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        !std::env::var("OC_DIALOGS_OPENTUI")
            .is_ok_and(|value| value == "0" || value == "false" || value == "off")
    })
}

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
    /// `ui/dialog-help.tsx`.
    Help,
    /// The delegation payload's rows-only shape.
    PluginRows,
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
        // title, gap, wrapped lines, hint paddingBottom, root gap,
        // button, button-row paddingBottom.
        Family::Help => inputs.hint_lines.len() + 6,
        // title, gap, rows, root paddingBottom.
        Family::PluginRows => inputs.plugin_rows.len() + 3,
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
    let title_fg = if selected {
        inputs.colors.selected_fg
    } else if current {
        inputs.colors.current_fg
    } else {
        inputs.colors.text
    };
    Some(RowPaint {
        selected,
        current,
        bg: if selected { inputs.colors.selected_bg } else { inputs.colors.surface },
        title_fg,
        bold: selected,
        marker_fg: if selected { inputs.colors.selected_fg } else { inputs.colors.current_fg },
        footer_fg: if selected { inputs.colors.selected_fg } else { inputs.colors.muted },
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
/// styled run per fg/attr change, contiguous columns, truncated by
/// atom walk at the row's inner width minus the right-aligned
/// footer's reservation (the old `put`'s break at the box edge).
fn title_chunks(inputs: &DialogInputs, paint: &RowPaint, index: usize) -> StyledText {
    let Some(option) = inputs.options.get(index) else {
        return styled(Vec::new());
    };
    let description = match option.description.as_deref() {
        Some(description) if Some(description) != option.category.as_deref() => Some(description),
        _ => None,
    };
    // The old `put` walks to the ROW edge (the box width, the
    // paddingRight zone included) and the right-aligned footer then
    // OVERWRITES its span — glyphs right of the footer SURVIVE. The
    // tree reproduces this: chunks run to the row edge, the footer
    // is absolutely positioned and later in DOM order (it
    // overwrites), and every chunk carries the ROW background (the
    // old `put` writes bg on every glyph cell, past the row box's
    // own fill span too).
    let clip = inputs.box_width.saturating_sub(4);
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
    push(&mut chunks, &option.title, paint.title_fg);
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
    // old `put` walks to the ROW edge (past the row box's padding
    // into the paddingRight zone), so the text must not be
    // box-clipped — flexShrink 0 lets its glyphs paint to the row
    // edge exactly like the old renderer's atom walk.
    let title_text = text().flex_shrink(0.0).wrap_mode("none").dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), move |i: &DialogInputs| match paint_at(i) {
            Some(paint) => title_chunks(i, &paint, index),
            None => styled(Vec::new()),
        })),
    );

    // The right-aligned footer: ABSOLUTE, right 3 — ends at the row
    // box's content edge exactly, and paints AFTER the title in DOM
    // order so it overwrites overflowing title glyphs (the old
    // renderer's footer-over-title paint order).
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

    // The filter input: a formfield-background box; the block cursor
    // is an explicit-bg chunk (fg surface over formfield_fg).
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
                    let end = display_width(&typed);
                    let mut chunks = vec![chunk(typed, i.colors.formfield_fg, None, 0)];
                    if end < width {
                        chunks.push(chunk(
                            " ".to_owned(),
                            i.colors.surface,
                            Some(i.colors.formfield_fg),
                            0,
                        ));
                    }
                    styled(chunks)
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
        Family::Help => help_builder(&family_inputs),
        Family::PluginRows => plugin_rows_builder(&family_inputs),
    };
    box_()
        .width(width as f32)
        .height(height as f32)
        .padding_top(1.0)
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
) -> (usize, usize, usize, usize) {
    let styles = DialogStyles::from_theme(theme);
    let colors = DialogColors::of(&styles);
    let width = usize::from(terminal.0);
    let height = usize::from(terminal.1);

    // Backdrop: alpha-composite black@150 over every covered cell —
    // the raster's observable semantics, applied at the bridge (the
    // solid renderer cannot see the route frame beneath).
    for row in &mut base.cells {
        for cell in row {
            crate::components::dialog::blend_backdrop(cell);
        }
    }

    // Chrome geometry — identical math to the old path.
    let box_width = entry.size.width().min(width.saturating_sub(2));
    let box_x = (width - box_width) / 2;
    let box_y = (height + 2) / 4;

    let inputs = build_inputs(entry, props, plugin, colors, box_width, height);
    let box_height = 1 + content_height(&inputs);

    DIALOG.with_borrow_mut(|slot| {
        let mount = slot.get_or_insert_with(|| SolidDialog::new(&inputs));
        if mount.key != mount_key(&inputs) {
            *mount = SolidDialog::new(&inputs);
        }
        mount.paint(inputs);
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

    (box_x, box_y, box_width.min(width - box_x), box_height.min(height.saturating_sub(box_y)))
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
) -> DialogInputs {
    let family = match &entry.kind {
        DialogKind::SessionRename { .. } => Family::RenamePrompt,
        DialogKind::WorktreeName => Family::WorktreeNamePrompt,
        DialogKind::Help => Family::Help,
        DialogKind::Plugin { .. } if plugin.is_some_and(|content| !content.is_select()) => {
            Family::PluginRows
        }
        _ => Family::Select,
    };
    match family {
        Family::Select => {
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
                window,
                actions: props
                    .actions
                    .iter()
                    .map(|action| (action.title.clone(), action.label.clone()))
                    .collect(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: String::new(),
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
            window: Vec::new(),
            actions: Vec::new(),
            prompt_value: entry.value.clone(),
            hint_lines: Vec::new(),
            plugin_title: String::new(),
            plugin_rows: Vec::new(),
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
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: crate::wrap::wrap_label(&hint, content_width.max(1)),
                plugin_title: String::new(),
                plugin_rows: Vec::new(),
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
                window: Vec::new(),
                actions: Vec::new(),
                prompt_value: String::new(),
                hint_lines: Vec::new(),
                plugin_title: content.title,
                plugin_rows: content.rows,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::command_palette::select_props_for;
    use crate::components::dialog::render_overlay_with_old;
    use crate::components::dialog_select::SelectOption;
    use crate::reactive::Runtime;
    use crate::render::{Cell, CellColor};
    use crate::store::{DialogKind, DialogSize, Route, Store};
    use crate::theme::ThemeState;

    /// The lane gate: the solid path's overlay must equal the old
    /// path's overlay CELL FOR CELL (char + attrs + fg + bg, backdrop
    /// included) — and rect for rect — across the dialog-type ×
    /// width × state matrix.
    struct Case {
        name: String,
        entry: DialogEntry,
        props: SelectProps,
        plugin: Option<PluginDialogContent>,
    }

    fn case(name: &str, entry: DialogEntry, props: SelectProps) -> Case {
        Case { name: name.to_owned(), entry, props, plugin: None }
    }

    /// A base frame with glyphs scattered over it (the backdrop's
    /// fg-dim path needs glyph cells under the scrim).
    fn blank_base(width: u16, height: u16) -> Frame {
        let cell = Cell {
            ch: ' ',
            attrs: 0,
            fg: CellColor::Rgb(255, 255, 255),
            bg: CellColor::Rgb(10, 10, 10),
        };
        let mut frame = Frame {
            cells: vec![vec![cell; usize::from(width)]; usize::from(height)],
            width: usize::from(width),
            height: usize::from(height),
        };
        for y in 0..usize::from(height) {
            for x in 0..usize::from(width) {
                if (x + y) % 7 == 0 {
                    frame.cells[y][x].ch = 'X';
                }
            }
        }
        frame
    }

    fn run(cases: Vec<Case>) {
        let theme = ThemeState::v2_dark();
        // (width, height) — the rig's two sizes plus the narrow
        // squeeze (the 60-col box clamps to 38).
        let sizes: [(u16, u16); 3] = [(80, 24), (120, 30), (40, 24)];
        let mut mismatches = Vec::new();
        for scenario in cases {
            for (width, height) in sizes {
                let mut old = blank_base(width, height);
                let mut new = blank_base(width, height);
                let rect_old = render_overlay_with_old(
                    &mut old,
                    &scenario.entry,
                    &scenario.props,
                    scenario.plugin.as_ref(),
                    &theme,
                    (width, height),
                );
                let rect_new = render_overlay_solid(
                    &mut new,
                    &scenario.entry,
                    &scenario.props,
                    scenario.plugin.as_ref(),
                    &theme,
                    (width, height),
                );
                if rect_old != rect_new {
                    mismatches.push(format!(
                        "{}-w{width}: rect old {rect_old:?} new {rect_new:?}",
                        scenario.name
                    ));
                }
                let mut cells = Vec::new();
                'outer: for y in 0..old.height.min(new.height) {
                    for x in 0..old.width.min(new.width) {
                        let (a, b) = (old.cell(y, x), new.cell(y, x));
                        if a != b {
                            cells.push(format!("({x},{y}): old {a:?} new {b:?}"));
                            if cells.len() >= 6 {
                                break 'outer;
                            }
                        }
                    }
                }
                if !cells.is_empty() {
                    if std::env::var("OC_M2_DEBUG").is_ok() {
                        eprintln!("== {} w{width} OLD ==", scenario.name);
                        for (y, row) in old.text_grid().iter().enumerate() {
                            eprintln!("{y:2}|{row}");
                        }
                        eprintln!("== {} w{width} NEW ==", scenario.name);
                        for (y, row) in new.text_grid().iter().enumerate() {
                            eprintln!("{y:2}|{row}");
                        }
                    }
                    mismatches.push(format!(
                        "{}-w{width}:\n    {}",
                        scenario.name,
                        cells.join("\n    ")
                    ));
                }
            }
        }
        assert!(
            mismatches.is_empty(),
            "solid dialog path diverged from the old renderer ({} cases × 3 widths):\n  {}",
            mismatches.len(),
            mismatches.join("\n  ")
        );
    }

    fn entry(kind: DialogKind) -> DialogEntry {
        DialogEntry {
            kind,
            size: DialogSize::Medium,
            select: SelectState::default(),
            value: String::new(),
        }
    }

    fn dialog(store: &Store, kind: &DialogKind) -> (DialogEntry, SelectProps) {
        store.replace_dialog(kind.clone(), DialogSize::Medium);
        let entry = store.top_dialog().expect("dialog mounted");
        (entry, select_props_for(kind, store))
    }

    // -- the matrix ------------------------------------------------------------

    /// The lane gate over the store-driven dialog kinds: the palette
    /// (open/browse/filter/no-match — flat + search footers), the
    /// model picker (current marker, Free footers, provider headers,
    /// the loading row), the theme list, the session list, queued
    /// prompts ("N of M" footers), the fixed-action selects (message
    /// actions, working directory, delete confirm), the worktrees
    /// dialog (its four-action footer bar) and the plugin dialogs.
    #[test]
    #[allow(clippy::too_many_lines)] // a scenario matrix, not logic
    fn solid_dialogs_equal_the_old_renderer_store_driven() {
        let mut cases: Vec<Case> = Vec::new();

        // The command palette: open, browse (mid-list), filtered,
        // no-match. The palette has dozens of options in several
        // groups — headers, pads, the flat filter mode, search
        // footers all exercise.
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Session one".to_owned()));
        {
            let (open, props) = dialog(&store, &DialogKind::CommandPalette);
            cases.push(case("palette-open", open, props));

            let (mut browse, props) = dialog(&store, &DialogKind::CommandPalette);
            browse.select.selected = 7;
            browse.select.scroll = 3;
            cases.push(case("palette-browse", browse, props));

            let (mut filtered, mut props) = dialog(&store, &DialogKind::CommandPalette);
            filtered.select.set_filter("session");
            props = SelectProps::new(props.title, props.options).flat(true).threshold(0.7);
            cases.push(case("palette-filter", filtered, props));

            let (mut no_match, props) = dialog(&store, &DialogKind::CommandPalette);
            no_match.select.set_filter("zzzz-nothing");
            cases.push(case("palette-no-match", no_match, props));
        }

        // The model picker: loaded rows with the current marker, Free
        // footers and provider-name headers, plus the loading row.
        {
            use crate::store::{ListState, ServerProvider};
            let store = Store::new(Runtime::new());
            store.open_tab("s1", None);
            store.navigate(Route::Session { session_id: "s1".to_owned() });
            let released = |secs: u64| {
                std::time::SystemTime::UNIX_EPOCH
                    .checked_add(std::time::Duration::from_secs(secs))
                    .map_or(0.0, |at| {
                        at.duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |since| since.as_secs_f64())
                    })
            };
            let model = |provider: &str, id: &str, name: &str, free: bool, released: f64| {
                crate::store::ServerModel {
                    provider_id: provider.into(),
                    id: id.into(),
                    name: name.into(),
                    free,
                    deprecated: false,
                    released,
                    context_limit: None,
                    cost_input: 0.0,
                    variants: Vec::new(),
                }
            };
            store.set_models(
                ListState::Loaded(vec![
                    model("anthropic", "claude", "Claude", true, released(1000)),
                    model("custom", "local-1", "Local One", false, released(2000)),
                ]),
                vec![ServerProvider { id: "anthropic".into(), name: "Anthropic".into() }],
            );
            store.select_model(
                "s1",
                &crate::store::ModelSelection {
                    provider_id: "anthropic".into(),
                    model_id: "claude".into(),
                    variant: None,
                },
            );
            let (open, props) = dialog(
                &store,
                &DialogKind::ServerList { command: "model.list", title: "Switch model" },
            );
            cases.push(case("models-current", open, props));
        }
        {
            let store = Store::new(Runtime::new());
            store.open_tab("s1", None);
            let (loading, props) = dialog(
                &store,
                &DialogKind::ServerList { command: "model.list", title: "Switch model" },
            );
            cases.push(case("models-loading", loading, props));
        }

        // The theme list: one group, the current theme's marker.
        {
            let store = Store::new(Runtime::new());
            let (themes, props) = dialog(&store, &DialogKind::ThemeList);
            cases.push(case("theme-list", themes, props));
        }

        // The session list: history rows.
        {
            use crate::data::{Ingest, SessionSnapshot};
            let store = Store::new(Runtime::new());
            store.ingest_all([Ingest::Resync {
                sessions: vec![
                    SessionSnapshot {
                        model: None,
                        id: "s_hist".into(),
                        title: Some("History row".into()),
                        time_idle: None,
                        viewed: None,
                    },
                    SessionSnapshot {
                        model: None,
                        id: "s_two".into(),
                        title: Some("Second history row".into()),
                        time_idle: None,
                        viewed: None,
                    },
                ],
                unread: Vec::new(),
                messages: Vec::new(),
            }]);
            let (list, props) = dialog(
                &store,
                &DialogKind::ServerList { command: "session.list", title: "Sessions" },
            );
            cases.push(case("session-list", list, props));
        }

        // The queued-prompts manager: "N of M" footers.
        {
            let store = Store::new(Runtime::new());
            store.open_tab("s1", None);
            store.navigate(Route::Session { session_id: "s1".to_owned() });
            store.queue_prompt("s1", "First queued prompt");
            store.queue_prompt("s1", "Second queued prompt");
            store.queue_prompt("s1", "Third queued prompt");
            let (queued, props) = dialog(&store, &DialogKind::QueuedPrompts);
            cases.push(case("queued-prompts", queued, props));
        }

        // The fixed-action selects.
        {
            let store = Store::new(Runtime::new());
            let (actions, props) = dialog(
                &store,
                &DialogKind::MessageActions { session_id: "s1".into(), message_id: "m1".into() },
            );
            cases.push(case("message-actions", actions, props));

            let (dir, props) = dialog(
                &store,
                &DialogKind::WorkingDirectory { directory: "/home/u/project".into() },
            );
            cases.push(case("working-directory", dir, props));

            let (confirm, props) = dialog(
                &store,
                &DialogKind::WorktreeDeleteConfirm {
                    directory: "/home/u/.worktrees/feat-a".into(),
                },
            );
            cases.push(case("delete-confirm", confirm, props));
        }

        // The worktrees dialog: the four-action footer bar + the
        // current marker + the empty texts.
        {
            use crate::components::dialog_worktrees::{worktree_props, WorktreeDialogState};
            use crate::store::ListState;
            let state = WorktreeDialogState {
                project_id: "p1".into(),
                current: Some("/home/u/project".into()),
                list: ListState::Loaded(vec![
                    crate::components::dialog_worktrees::WorktreeEntry {
                        directory: "/home/u/project".into(),
                        strategy: None,
                    },
                    crate::components::dialog_worktrees::WorktreeEntry {
                        directory: "/home/u/.worktrees/feat-a".into(),
                        strategy: Some("manual".into()),
                    },
                ]),
                pending_delete: None,
                ..WorktreeDialogState::default()
            };
            let props = worktree_props(&state, true, "/home/u");
            cases.push(case("worktrees", entry(DialogKind::Worktrees), props));

            let empty_state = WorktreeDialogState { list: ListState::Loaded(Vec::new()), ..state };
            let props = worktree_props(&empty_state, false, "/home/u");
            let mut empty = entry(DialogKind::Worktrees);
            empty.select.set_filter("nope");
            cases.push(case("worktrees-no-match", empty, props));
        }

        // The plugin dialogs: rows-only + select.
        {
            let plugin = PluginDialogContent {
                title: "About omos".into(),
                rows: vec!["One line of rows".into(), "Another line, a little longer".into()],
                options: Vec::new(),
            };
            let mut c = case(
                "plugin-rows",
                entry(DialogKind::Plugin { id: "p".into() }),
                SelectProps::new("", Vec::new()),
            );
            c.plugin = Some(plugin);
            cases.push(c);
        }

        run(cases);
    }

    /// The lane gate over the controlled-geometry selects + the
    /// prompt family + help: hand-built props pin the exact
    /// structures (group headers and their pads, the current marker,
    /// right-aligned footers, the actions bar, empty/no-match copy,
    /// the end-pinned scroll window, the xlarge clamp) and the
    /// prompts/help exercise their own chrome families.
    #[test]
    #[allow(clippy::too_many_lines)] // a scenario matrix, not logic
    fn solid_dialogs_equal_the_old_renderer_structural() {
        let mut cases: Vec<Case> = Vec::new();

        // Grouped: three categories (headers + pads), a current
        // marker mid-list, descriptions and footers.
        let grouped_options = vec![
            SelectOption::titled("New session", "session.new")
                .categorized("Session")
                .with_footer("ctrl+n"),
            SelectOption::titled("Switch session", "session.list")
                .categorized("Session")
                .with_footer("ctrl+l"),
            SelectOption::titled("Switch model", "model.list")
                .categorized("Agent")
                .described("the provider picker")
                .with_footer("ctrl+m"),
            SelectOption::titled("Share session", "session.share").categorized("Sharing"),
        ];
        let props = SelectProps::new("Commands", grouped_options)
            .with_current(Some("model.list".to_owned()));
        cases.push(case("grouped-current-footer", entry(DialogKind::CommandPalette), props));

        // The actions footer bar (the worktrees-style four hints).
        let actions = vec![
            crate::components::dialog_select::SelectAction::new(
                "dialog.move_session.delete",
                "delete",
                "ctrl+d",
            ),
            crate::components::dialog_select::SelectAction::new(
                "dialog.move_session.new",
                "new",
                "ctrl+a",
            ),
        ];
        let props = SelectProps::new(
            "Worktrees",
            vec![SelectOption::titled("/home/u/project", "/home/u/project")],
        )
        .with_actions(actions);
        cases.push(case("actions-footer", entry(DialogKind::Worktrees), props));

        // Empty + no-match with the custom copy.
        let props = SelectProps::new("Worktrees", Vec::new())
            .with_empty_texts("No worktrees available", "No worktrees found");
        cases.push(case("empty-copy", entry(DialogKind::Worktrees), props.clone()));
        let mut no_match = entry(DialogKind::Worktrees);
        no_match.select.set_filter("zz");
        cases.push(case("no-match-copy", no_match, props));

        // A long flat list, end-pinned (the scroll window) and
        // mid-scroll.
        let many: Vec<SelectOption> = (0..30)
            .map(|index| {
                SelectOption::titled(format!("Item {index:02}"), format!("item-{index}"))
                    .with_footer(format!("f{index}"))
            })
            .collect();
        let props = SelectProps::new("Items", many);
        let mut end = entry(DialogKind::CommandPalette);
        end.select.selected = 29;
        end.select.scroll = 26;
        cases.push(case("long-list-end", end, props.clone()));
        let mut top = entry(DialogKind::CommandPalette);
        top.select.selected = 0;
        top.select.scroll = 5;
        cases.push(case("long-list-scrolled-top", top, props));

        // The prompt family: rename (placeholder, seeded value) and
        // the worktree name (typed, the two-hint footer).
        cases.push(case(
            "rename-placeholder",
            entry(DialogKind::SessionRename { session_id: "s1".to_owned() }),
            SelectProps::new("", Vec::new()),
        ));
        let mut seeded = entry(DialogKind::SessionRename { session_id: "s1".to_owned() });
        seeded.value = "Parity fake says hello.".to_owned();
        cases.push(case("rename-seeded", seeded, SelectProps::new("", Vec::new())));

        cases.push(case(
            "worktree-name-placeholder",
            entry(DialogKind::WorktreeName),
            SelectProps::new("", Vec::new()),
        ));
        let mut typed = entry(DialogKind::WorktreeName);
        typed.value = "feat-a".to_owned();
        cases.push(case("worktree-name-typed", typed, SelectProps::new("", Vec::new())));

        // Help (the wrap count varies by width: 60/88/116 content).
        cases.push(case("help", entry(DialogKind::Help), SelectProps::new("", Vec::new())));

        // The xlarge box at the wide size (116 < 118 — no clamp) and
        // medium at 40 (the 60-col box clamps to 38).
        let mut xlarge = entry(DialogKind::Help);
        xlarge.size = DialogSize::Xlarge;
        cases.push(case("help-xlarge", xlarge, SelectProps::new("", Vec::new())));

        run(cases);
    }
}
