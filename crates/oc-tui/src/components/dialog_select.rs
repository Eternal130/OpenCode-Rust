//! `DialogSelect` — the fuzzy list picker (`ui/dialog-select.tsx`):
//! filter input, grouped/flat option list, current marker, selected-row
//! highlight, right-aligned per-row footers, footer action hints and
//! the scroll window. The fuzzy ordering contract is [`fuzzy`] (a
//! faithful fuzzysort port pinned by golden query orders); selection
//! semantics follow `ui/select-controller.ts`.
//!
//! Rendering mirrors the reference box geometry exactly (proven by the
//! `dialog-select` + `command-palette` tier-a goldens, text grid AND
//! canonical ANSI at both widths):
//!
//! ```text
//! <box gap=1 paddingBottom=1>                    root (fills or wraps)
//!   <box paddingLeft=4 paddingRight=4>           title + filter
//!     row: [title bold]        …        [esc muted]   (space-between)
//!     <box paddingTop=1><input placeholder="Search"/></box>
//!   </box>
//!   <scrollbox paddingLeft=1 paddingRight=1 maxHeight=min(rows, H/2-6)>
//!     <box paddingLeft=3 paddingTop=i>   group header (accent, bold)
//!     <box paddingLeft=3|1 paddingRight=3 gap=1>  option row
//!       ([● current marker] gap title+description … footer)
//!   </scrollbox>
//!   <box paddingLeft=4 paddingRight=2>    footer actions (gap 2)
//! </box>
//! ```

pub mod fuzzy;

use crate::render::{Cell, CellColor, Frame};
use crate::wrap::flatten;
use ratatui::style::{Color, Style};

/// One selectable row (`DialogSelectOption`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub title: String,
    pub value: String,
    pub description: Option<String>,
    pub category: Option<String>,
    /// Extra searchable text (the palette joins `id + description`).
    pub search_text: Option<String>,
    /// Right-aligned footer shown while filtering (the palette's
    /// `group · binding`).
    pub search_footer: Option<String>,
    /// Right-aligned footer shown while not filtering.
    pub footer: Option<String>,
}

impl SelectOption {
    #[must_use]
    pub fn titled(title: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            value: value.into(),
            description: None,
            category: None,
            search_text: None,
            search_footer: None,
            footer: None,
        }
    }

    #[must_use]
    pub fn described(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub fn categorized(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    #[must_use]
    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    #[must_use]
    pub fn with_search(mut self, text: impl Into<String>) -> Self {
        self.search_text = Some(text.into());
        self
    }

    #[must_use]
    pub fn with_search_footer(mut self, footer: impl Into<String>) -> Self {
        self.search_footer = Some(footer.into());
        self
    }
}

/// A footer action (`DialogSelectAction`): a keymap command rendered
/// `title label` in the footer bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectAction {
    pub command: &'static str,
    pub title: String,
    /// Binding display ("ctrl+d") — the keymap command's default
    /// binding, the reference's `shortcuts.all(command)`.
    pub label: String,
}

impl SelectAction {
    #[must_use]
    pub fn new(command: &'static str, title: impl Into<String>, label: impl Into<String>) -> Self {
        Self { command, title: title.into(), label: label.into() }
    }
}

/// Component props (`DialogSelectProps`, the wave-1 subset). Owned so
/// the keypress pump can rebuild them per dialog kind.
#[derive(Debug, Clone)]
pub struct SelectProps {
    pub title: String,
    /// Filter placeholder ("Search").
    pub placeholder: String,
    pub options: Vec<SelectOption>,
    /// The currently-active value (marked `●`, focused on open).
    pub current: Option<String>,
    pub actions: Vec<SelectAction>,
    /// `flat`: group headers collapse while filtering (the palette).
    pub flat: bool,
    /// Normalized fuzzy threshold (the palette's `0.7`).
    pub filter_threshold: Option<f64>,
    /// SE-F4 (E12): the session list's filter semantics — the ref's
    /// `dialog-session-list` matches a TRIMMED, case-folded, byte-exact
    /// SUBSTRING (never fuzzy, never accent-folded; a whitespace-only
    /// query trims to empty = the full list). The palette and the rest
    /// keep the fuzzysort contract.
    pub substring_filter: bool,
    /// v2's `emptyView` copy override ("No worktrees available").
    pub empty_text: Option<String>,
    /// v2's `noMatchView` copy override ("No worktrees found").
    pub no_match_text: Option<String>,
    /// Lane FINAL: the prompt-family description rows (the connect
    /// prompts' URL/instructions/error block).
    pub hints: Vec<String>,
    /// The destructive confirm variant (auditB #8): the selected row
    /// paints `action.destructive.focused` instead of primary
    /// (dialog-worktrees' confirm + drop-confirm selects).
    pub destructive: bool,
}

impl SelectProps {
    #[must_use]
    pub fn new(title: impl Into<String>, options: Vec<SelectOption>) -> Self {
        Self {
            title: title.into(),
            placeholder: "Search".to_owned(),
            options,
            current: None,
            actions: Vec::new(),
            flat: false,
            filter_threshold: None,
            substring_filter: false,
            empty_text: None,
            no_match_text: None,
            destructive: false,
            hints: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_current(mut self, current: Option<String>) -> Self {
        self.current = current;
        self
    }

    #[must_use]
    pub fn with_actions(mut self, actions: Vec<SelectAction>) -> Self {
        self.actions = actions;
        self
    }

    /// Append one footer action (the Plugins dialog's hint pair).
    #[must_use]
    pub fn with_action(mut self, action: SelectAction) -> Self {
        self.actions.push(action);
        self
    }

    #[must_use]
    pub fn flat(mut self, flat: bool) -> Self {
        self.flat = flat;
        self
    }
    /// Mark the select destructive (the confirm rows' selected pair).
    #[must_use]
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    #[must_use]
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.filter_threshold = Some(threshold);
        self
    }

    /// SE-F4: the ref's session-list semantics — trimmed, case-folded,
    /// byte-exact substring (see the field docs).
    #[must_use]
    pub fn substring_filter(mut self) -> Self {
        self.substring_filter = true;
        self
    }

    /// v2's `emptyView` / `noMatchView` copy (the Worktrees dialog's
    /// "No worktrees available" / "No worktrees found").
    #[must_use]
    pub fn with_empty_texts(mut self, empty: &str, no_match: &str) -> Self {
        self.empty_text = Some(empty.to_owned());
        self.no_match_text = Some(no_match.to_owned());
        self
    }

    /// The prompt-family description rows (the connect prompts).
    #[must_use]
    pub fn with_hints(mut self, hints: Vec<String>) -> Self {
        self.hints = hints;
        self
    }
}

/// Interactive state (the solid `createStore`): filter text, selected
/// flat index, and the first visible content row (headers included in
/// the scroll coordinate space).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectState {
    pub filter: String,
    pub selected: usize,
    pub scroll: usize,
}

/// `reconcileSelection`: clamp into `[0, count)`.
#[must_use]
pub fn reconcile_selection(selected: usize, count: usize) -> usize {
    selected.min(count.saturating_sub(1))
}

/// `moveSelection` with the wrap policy (the dialog's mover).
#[must_use]
pub fn move_selection(selected: usize, count: usize, delta: i64) -> usize {
    if count == 0 {
        return 0;
    }
    let count_i = i64::try_from(count).unwrap_or(i64::MAX);
    let next = i64::try_from(selected).unwrap_or(0) + delta;
    if next < 0 {
        count - 1
    } else if next >= count_i {
        0
    } else {
        usize::try_from(next).unwrap_or(selected)
    }
}

// -- filtering / grouping ----------------------------------------------------

/// An indexed option target + one search-key extractor (the dialog's
/// `title` / `category` / `searchText` keys).
type IndexedOption<'a> = (usize, &'a SelectOption);
type OptionKey = fn(&IndexedOption<'_>) -> Option<String>;

/// The filtered option indices in fuzzy order (`filtered()`): no
/// filter keeps registry order; a filter ranks through [`fuzzy`] with
/// the dialog's `title×2 + category + searchText` weighting.
/// `substring_filter` selects the session list's contract instead
/// (SE-F4): trim + case-fold + byte-exact substring, registry order.
#[must_use]
pub fn filter_indices(props: &SelectProps, filter: &str) -> Vec<usize> {
    if props.substring_filter {
        let needle = filter.trim().to_lowercase();
        if needle.is_empty() {
            return props.options.iter().enumerate().map(|(index, _)| index).collect();
        }
        return props
            .options
            .iter()
            .enumerate()
            .filter(|(_, option)| option.title.to_lowercase().contains(&needle))
            .map(|(index, _)| index)
            .collect();
    }
    if filter.is_empty() {
        return props.options.iter().enumerate().map(|(index, _)| index).collect();
    }
    let needle = filter.to_lowercase();
    let targets: Vec<IndexedOption<'_>> = props.options.iter().enumerate().collect();
    let keys: &[OptionKey] = &[
        |entry: &(usize, &SelectOption)| Some(entry.1.title.clone()),
        |entry: &(usize, &SelectOption)| entry.1.category.clone(),
        |entry: &(usize, &SelectOption)| entry.1.search_text.clone(),
    ];
    fuzzy::go_keys(
        &needle,
        &targets,
        keys,
        |scores| scores[0] * 2.0 + scores[1] + scores[2],
        props.filter_threshold,
    )
    .into_iter()
    .map(|ranked| ranked.target.0)
    .collect()
}

/// One rendered group: its category header (`None` = no header) and
/// the option indices, in list order (`grouped()`).
#[derive(Debug, Clone, PartialEq)]
pub struct SelectGroup {
    pub category: Option<String>,
    pub indices: Vec<usize>,
}

/// Group the filtered indices by category in first-appearance order
/// (`groupBy`); a `flat` select under an active filter yields one
/// headerless group.
#[must_use]
pub fn grouped(props: &SelectProps, filter: &str) -> Vec<SelectGroup> {
    let indices = filter_indices(props, filter);
    if props.flat && !filter.is_empty() {
        return vec![SelectGroup { category: None, indices }];
    }
    let mut groups: Vec<SelectGroup> = Vec::new();
    for index in indices {
        let category = props.options[index].category.clone();
        match groups.last_mut() {
            Some(group) if group.category == category => group.indices.push(index),
            _ => groups.push(SelectGroup { category, indices: vec![index] }),
        }
    }
    groups
}

/// The flat option-index list across groups (`flat()`).
#[must_use]
pub fn flat_indices(groups: &[SelectGroup]) -> Vec<usize> {
    groups.iter().flat_map(|group| group.indices.iter().copied()).collect()
}

/// Content-row count: 1 per option plus 1 per header (2 for every
/// header after the first — `rows()`; details rows are wave-2).
#[must_use]
pub fn content_rows(groups: &[SelectGroup]) -> usize {
    let mut rows = 0usize;
    for (i, group) in groups.iter().enumerate() {
        if group.category.is_some() {
            rows += if i == 0 { 1 } else { 2 };
        }
        rows += group.indices.len();
    }
    rows
}

/// The scroll window height: `min(rows, max(5, floor(terminal_height /
/// 2) - 6))` (`height()`). SE-F6: the ref clamps the window to a
/// MINIMUM of 5 rows — a bare `floor(h/2) - 6` collapses to 0 at
/// short viewports (12-row terminals: 0 list rows, the session dialog
/// renders chrome only and the sessions stay invisible).
#[must_use]
pub fn viewport_height(rows: usize, terminal_height: usize) -> usize {
    rows.min((terminal_height / 2).saturating_sub(6).max(5))
}

/// The DIALOG-CONTENT row of each flat option (the mouse surface,
/// `dialog-select.tsx:757-775`: pointer motion over an option row
/// moves the selection; a release submits it). Content layout: row 0
/// title, row 1 gap, row 2 filter, row 3 gap, then the scroll window
/// — an option's row is `4 + (content_row - scroll)` when the window
/// shows it, `None` when scrolled away.
#[must_use]
pub fn option_dialog_rows(
    props: &SelectProps,
    state: &SelectState,
    terminal_height: usize,
) -> Vec<Option<usize>> {
    let groups = grouped(props, &state.filter);
    let indices = flat_indices(&groups);
    let rows_total = content_rows(&groups);
    let viewport = viewport_height(rows_total, terminal_height);
    (0..indices.len())
        .map(|flat| {
            let content_row = row_of_index(&groups, flat);
            if content_row >= state.scroll && content_row < state.scroll + viewport {
                Some(4 + (content_row - state.scroll))
            } else {
                None
            }
        })
        .collect()
}

/// The content row of the flat `selected` index (headers included).
#[must_use]
pub fn row_of_index(groups: &[SelectGroup], flat_index: usize) -> usize {
    let mut row = 0usize;
    let mut remaining = flat_index;
    for (i, group) in groups.iter().enumerate() {
        if group.category.is_some() {
            row += if i == 0 { 1 } else { 2 };
        }
        if remaining < group.indices.len() {
            return row + remaining;
        }
        row += group.indices.len();
        remaining -= group.indices.len();
    }
    row
}

impl SelectState {
    /// Focus the `current` option on open (the mount effect).
    pub fn focus_current(&mut self, props: &SelectProps) {
        let groups = grouped(props, &self.filter);
        let indices = flat_indices(&groups);
        if let Some(current) = props.current.clone() {
            if let Some(position) =
                indices.iter().position(|index| props.options[*index].value == current)
            {
                self.selected = position;
            }
        }
    }

    /// Type into the filter: replace, select the first result, scroll
    /// to the top (`setFilter` + the filter effect's `moveTo(0)`).
    /// Clearing back to empty re-focuses the `current` option and
    /// centers it (the filter effect's empty branch: `moveTo(
    /// currentIndex, true)` + center scroll — dialog-select.tsx:342-358);
    /// with no `current` the selection stays put (the effect returns
    /// before any move).
    pub fn set_filter(&mut self, filter: &str, props: &SelectProps, terminal_height: usize) {
        let was_empty = self.filter.is_empty();
        filter.clone_into(&mut self.filter);
        if self.filter.is_empty() {
            if !was_empty && props.current.is_some() {
                self.focus_current(props);
                self.center_on_selection(props, terminal_height);
            }
            return;
        }
        self.selected = 0;
        self.scroll = 0;
    }

    /// Move the selection (wrap policy) and edge-reveal it
    /// (`move` + `scrollToSelection`).
    pub fn move_selection(&mut self, props: &SelectProps, delta: i64) {
        let groups = grouped(props, &self.filter);
        let indices = flat_indices(&groups);
        if indices.is_empty() {
            return;
        }
        self.selected = move_selection(self.selected, indices.len(), delta);
    }

    /// Jump to the first/last item (`home`/`end`).
    pub fn move_to_edge(&mut self, props: &SelectProps, last: bool) {
        let groups = grouped(props, &self.filter);
        let indices = flat_indices(&groups);
        if indices.is_empty() {
            return;
        }
        self.selected = if last { indices.len() - 1 } else { 0 };
    }

    /// Clamp the selection into range and edge-reveal it in the scroll
    /// window (`reconcileSelection` + `scrollToSelection`).
    pub fn reveal(&mut self, props: &SelectProps, terminal_height: usize) {
        let groups = grouped(props, &self.filter);
        let indices = flat_indices(&groups);
        if indices.is_empty() {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        self.selected = reconcile_selection(self.selected, indices.len());
        let rows = content_rows(&groups);
        let viewport = viewport_height(rows, terminal_height);
        self.scroll = self.scroll.min(rows.saturating_sub(viewport));
        let row = row_of_index(&groups, self.selected);
        if row < self.scroll {
            self.scroll = row;
        }
        if viewport > 0 && row >= self.scroll + viewport {
            self.scroll = row + 1 - viewport;
        }
    }

    /// Wheel-scroll the option WINDOW `delta` content rows (negative
    /// up), clamped by the scrollbar's extent — v2's dialog scrollbox
    /// scrolls the viewport WITHOUT moving the selection (the rows
    /// track the pointer independently, `dialog-select.tsx`'s
    /// scrollbox wheel). Terminal-wheel semantics: 3 rows a notch
    /// (opentui's own `direction === "up" ? -3 : 3` delta); `true`
    /// when the window moved (the redraw key).
    pub fn scroll_window_by(
        &mut self,
        props: &SelectProps,
        terminal_height: usize,
        delta: i32,
    ) -> bool {
        let groups = grouped(props, &self.filter);
        let rows = content_rows(&groups);
        let viewport = viewport_height(rows, terminal_height);
        let max = rows.saturating_sub(viewport);
        let current = i64::try_from(self.scroll).unwrap_or(i64::MAX);
        let ceiling = i64::try_from(max).unwrap_or(i64::MAX);
        let next = current.saturating_add(i64::from(delta)).clamp(0, ceiling);
        let next = usize::try_from(next).unwrap_or(0);
        if next == self.scroll {
            return false;
        }
        self.scroll = next;
        true
    }

    /// Center the selection in the scroll window (`scrollToSelection(
    /// true)`: `scrollBy(y - floor(height / 2))`, clamped by the
    /// scrollbar's `round(clamp(…))` at the ends) — the mount/focus
    /// scroll of `scrollAfterLayout(true, …)` (dialog-select.tsx:123-150).
    pub fn center_on_selection(&mut self, props: &SelectProps, terminal_height: usize) {
        let groups = grouped(props, &self.filter);
        let indices = flat_indices(&groups);
        if indices.is_empty() || self.selected >= indices.len() {
            return;
        }
        let rows = content_rows(&groups);
        let viewport = viewport_height(rows, terminal_height);
        if viewport == 0 {
            return;
        }
        let row = row_of_index(&groups, self.selected);
        let center_offset = viewport / 2;
        // scrollBy clamps through the scrollbar math; i64 to allow the
        // un-clamped negative side before saturating at 0.
        let target = i64::try_from(row).unwrap_or(0) - i64::try_from(center_offset).unwrap_or(0);
        self.scroll =
            usize::try_from(target.max(0)).unwrap_or(0).min(rows.saturating_sub(viewport));
    }

    /// The selected option, if any (`selected()`).
    #[must_use]
    pub fn selected_option<'a>(&self, props: &'a SelectProps) -> Option<&'a SelectOption> {
        let groups = grouped(props, &self.filter);
        flat_indices(&groups).get(self.selected).map(|index| &props.options[*index])
    }

    /// Submit: the selected option (`submit()`).
    #[must_use]
    pub fn submit<'a>(&self, props: &'a SelectProps) -> Option<&'a SelectOption> {
        self.selected_option(props)
    }
}

// -- rendering ----------------------------------------------------------------

/// The dialog surface's paint tokens (the theme's `dialog` surface
/// view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogStyles {
    /// Renderer default foreground (opentui `defaultForeground()`).
    pub default_fg: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    /// The dialog surface background (`surface("dialog")` base —
    /// what the chrome box paints).
    pub surface_bg: Color,
    /// `background.action.primary.focused` — the selected-row bg.
    pub selected_bg: Color,
    /// `text.action.primary.focused` — the selected-row fg.
    pub selected_fg: Color,
    /// `text.formfield.selected` — the current (not selected) fg.
    pub current_fg: Color,
    /// `background.formfield.focused` — the filter input bg.
    pub formfield_bg: Color,
    /// `text.formfield.focused` — typed filter fg + cursor color.
    pub formfield_fg: Color,
    /// `text.formfield.base` — the PROMPT inputs' typed-value fg
    /// (`DialogPrompt`'s textarea/`DialogWorktreeName`'s input set
    /// `textColor`/`focusedTextColor` to the base, not the focused
    /// variant — dialog-prompt.tsx:125-126, dialog-worktree-name.tsx:64).
    pub formfield_base_fg: Color,
    /// `text.action.destructive.focused` (auditB #8).
    pub destructive_fg: Color,
    /// `background.action.destructive.focused`.
    pub destructive_bg: Color,
}

impl DialogStyles {
    #[must_use]
    pub fn from_theme(theme: &crate::theme::ThemeState) -> Self {
        let tokens = &theme.resolved.dialog;
        let color = |rgba: oc_theme::Rgba| Color::Rgb(rgba.r, rgba.g, rgba.b);
        Self {
            default_fg: Color::Rgb(255, 255, 255),
            text: color(tokens.text.base),
            muted: color(tokens.text.muted),
            accent: color(tokens.semantic(oc_theme::types::SemanticHue::Accent).s200),
            surface_bg: color(tokens.background.base),
            selected_bg: color(tokens.background.action.primary.focused),
            selected_fg: color(tokens.text.action.primary.focused),
            current_fg: color(tokens.text.formfield.selected),
            formfield_bg: color(tokens.background.formfield.focused),
            formfield_fg: color(tokens.text.formfield.focused),
            formfield_base_fg: color(tokens.text.formfield.base),
            destructive_fg: color(tokens.text.action.destructive.focused),
            destructive_bg: color(tokens.background.action.destructive.focused),
        }
    }
}

const BOLD: u16 = 1;

/// A one-row cell accumulator — the byte-exact painting surface for
/// the dialog geometry (paddings, gaps, right alignment).
struct Row {
    cells: Vec<Cell>,
}

impl Row {
    fn blank(width: usize, fg: Color, bg: Color) -> Self {
        Self { cells: vec![cell(' ', 0, fg, bg); width] }
    }

    /// Paint one styled string at `x` (grapheme-cluster aware); returns
    /// the end column.
    fn text(&mut self, x: usize, text: &str, fg: Color, bg: Color, attrs: u16) -> usize {
        put(&mut self.cells, x, text, fg, bg, attrs)
    }

    /// Paint a styled string right-aligned so it ENDS at `end`
    /// (exclusive), returning its start column.
    fn text_end(&mut self, end: usize, text: &str, fg: Color, bg: Color, attrs: u16) -> usize {
        let start = end.saturating_sub(crate::wrap::display_width(text));
        self.text(start, text, fg, bg, attrs);
        start
    }
}

fn cell(ch: char, attrs: u16, fg: Color, bg: Color) -> Cell {
    Cell { ch, attrs, fg: cell_color(fg), bg: cell_color(bg) }
}

fn cell_color(color: Color) -> CellColor {
    match color {
        Color::Rgb(r, g, b) => CellColor::Rgb(r, g, b),
        Color::Indexed(index) => CellColor::Indexed(index),
        _ => CellColor::Default,
    }
}

/// The rendered content of one `DialogSelect`: full-width rows of cells
/// (`width` wide). `rows` is the auto-height block; a standalone mount
/// stretches the scroll region (see [`render_select`]).
pub struct SelectView {
    pub rows: Vec<Vec<Cell>>,
    pub width: usize,
}

/// Render the select content.
///
/// * `surface_bg` — the background under the select (the terminal base
///   for a standalone mount, the dialog box surface inside [`crate::components::dialog`]);
/// * `width` — the content width (terminal width standalone, dialog
///   box width wrapped);
/// * `terminal_height` — feeds the scroll `maxHeight` formula;
/// * `fill_height` — `Some(h)` stretches the scroll region so the
///   block covers `h` rows (the standalone flexGrow mount); `None`
///   wraps at content height (the in-dialog auto-height mount).
#[must_use]
pub fn render_select(
    props: &SelectProps,
    state: &SelectState,
    styles: &DialogStyles,
    surface_bg: Color,
    width: usize,
    terminal_height: usize,
    fill_height: Option<usize>,
) -> SelectView {
    let default = styles.default_fg;
    let groups = grouped(props, &state.filter);
    let indices = flat_indices(&groups);
    let rows_total = content_rows(&groups);
    let viewport = viewport_height(rows_total, terminal_height);
    let filtering = !state.filter.is_empty();
    let flatten_mode = props.flat && filtering;
    let visible = |row: usize| row >= state.scroll && row < state.scroll + viewport;

    // -- scroll window (content-row space) ---------------------------------
    let mut window: Vec<Vec<Cell>> = Vec::new();
    let mut content_row = 0usize;
    let mut flat_seen = 0usize;
    for (group_i, group) in groups.iter().enumerate() {
        if let Some(category) = group.category.as_ref() {
            if group_i > 0 {
                // Header paddingTop: one blank row above every header
                // after the first.
                let row = content_row;
                content_row += 1;
                if visible(row) {
                    window.push(Row::blank(width, default, surface_bg).cells);
                }
            }
            let row = content_row;
            content_row += 1;
            if visible(row) {
                let mut header = Row::blank(width, default, surface_bg);
                header.text(4, category, styles.accent, surface_bg, BOLD);
                window.push(header.cells);
            }
        }
        for &index in &group.indices {
            let row = content_row;
            content_row += 1;
            if visible(row) {
                let mut option_row = Row::blank(width, default, surface_bg);
                paint_option_row(
                    &mut option_row.cells,
                    &props.options[index],
                    flat_seen == state.selected,
                    props.current.as_deref() == Some(props.options[index].value.as_str()),
                    flatten_mode,
                    styles,
                    surface_bg,
                    width,
                );
                window.push(option_row.cells);
            }
            flat_seen += 1;
        }
    }
    // Empty states (outside the scrollbox, paddingLeft 4).
    if indices.is_empty() {
        let mut row = Row::blank(width, default, surface_bg);
        let message = if filtering {
            props.no_match_text.as_deref().unwrap_or("No results found")
        } else {
            props.empty_text.as_deref().unwrap_or("No items available")
        };
        row.text(4, message, styles.muted, surface_bg, 0);
        window.push(row.cells);
    }

    // -- assemble ------------------------------------------------------------
    let mut rows: Vec<Vec<Cell>> = Vec::new();

    // Title row: [pad 4][title bold]…[esc muted][pad 4] (space-between:
    // the esc ends at the content edge `width - 4`).
    let mut title = Row::blank(width, default, surface_bg);
    title.text(4, &props.title, styles.text, surface_bg, BOLD);
    title.text_end(width - 4, "esc", styles.muted, surface_bg, 0);
    rows.push(title.cells);

    // gap 1
    rows.push(Row::blank(width, default, surface_bg).cells);

    // Filter input row: [pad 4][input bg formfield (width-8)][pad 4].
    let mut input = Row::blank(width, default, surface_bg);
    let input_width = width.saturating_sub(8);
    for slot in input.cells.iter_mut().skip(4).take(input_width) {
        *slot = cell(' ', 0, default, styles.formfield_bg);
    }
    if state.filter.is_empty() {
        input.text(4, &props.placeholder, styles.muted, styles.formfield_bg, 0);
    } else {
        // No painted cursor: v2's input shows the hardware cursor
        // only — past the typed text the row stays default-fg over
        // the formfield bg (the #8 ruling's raw-ANSI mirror).
        input.text(4, &state.filter, styles.formfield_fg, styles.formfield_bg, 0);
    }
    rows.push(input.cells);

    // gap 1
    rows.push(Row::blank(width, default, surface_bg).cells);

    // Scroll region: the viewport rows, stretched under fill_height
    // (the standalone flexGrow mount).
    rows.extend(window);
    if let Some(fill) = fill_height {
        let footer_reserve = if props.actions.is_empty() { 2 } else { 3 };
        let scroll_region = fill.saturating_sub(4 + footer_reserve);
        while rows.len() < 4 + scroll_region {
            rows.push(Row::blank(width, default, surface_bg).cells);
        }
    }

    // gap 1 (before the footer actions / the empty fallback box)
    rows.push(Row::blank(width, default, surface_bg).cells);

    // Footer actions: [pad 4][title][ label][gap 2]…
    if !props.actions.is_empty() {
        let mut footer = Row::blank(width, default, surface_bg);
        let mut at = 4usize;
        for action in &props.actions {
            at = footer.text(at, &action.title, styles.text, surface_bg, 0);
            at = footer.text(at, &format!(" {}", action.label), styles.muted, surface_bg, 0);
            at += 2; // gap
        }
        rows.push(footer.cells);
    }

    // paddingBottom 1
    rows.push(Row::blank(width, default, surface_bg).cells);

    SelectView { rows, width }
}

/// One option row: scroll pad 1; the row box spans `[1, width-2)`;
/// paddingLeft 3 (1 + `●` marker + gap when current); paddingRight 3;
/// the footer right-aligned ending at `width - 4`.
#[allow(clippy::too_many_arguments)]
fn paint_option_row(
    cells: &mut [Cell],
    option: &SelectOption,
    selected: bool,
    current: bool,
    flatten_mode: bool,
    styles: &DialogStyles,
    surface_bg: Color,
    width: usize,
) {
    let default = styles.default_fg;
    let row_bg = if selected { styles.selected_bg } else { surface_bg };
    let title_fg_color = if selected {
        styles.selected_fg
    } else if current {
        styles.current_fg
    } else {
        styles.text
    };
    let title_attrs = if selected { BOLD } else { 0 };

    // The selected row's background spans the whole row box first.
    if selected {
        pad_cells(cells, 1, width.saturating_sub(2), default, styles.selected_bg);
    }

    let at = if current {
        // paddingLeft 1, marker, gap 1, title at 4.
        let marker_fg = if selected { styles.selected_fg } else { styles.current_fg };
        put(cells, 2, "●", marker_fg, row_bg, 0);
        put(cells, 4, &option.title, title_fg_color, row_bg, title_attrs)
    } else {
        put(cells, 4, &option.title, title_fg_color, row_bg, title_attrs)
    };
    // Description (" " + text) — hidden when it merely repeats the
    // category; the selected row paints it in the row color (bold).
    let description = match option.description.as_deref() {
        Some(description) if Some(description) != option.category.as_deref() => Some(description),
        _ => None,
    };
    if let Some(description) = description {
        let fg = if selected { title_fg_color } else { styles.muted };
        put(cells, at, &format!(" {description}"), fg, row_bg, title_attrs);
    }

    // Right-aligned footer: while flattened (filtering on a flat
    // select) the search footer (group · binding), else the plain
    // footer.
    let footer = if flatten_mode {
        option
            .search_footer
            .clone()
            .or_else(|| option.category.clone())
            .or_else(|| option.footer.clone())
    } else {
        option.footer.clone()
    };
    if let Some(footer) = footer {
        let fg = if selected { styles.selected_fg } else { styles.muted };
        let end = width.saturating_sub(4);
        let start = end.saturating_sub(crate::wrap::display_width(&footer));
        put(cells, start, &footer, fg, row_bg, 0);
    }
}

/// Blank `count` cells starting at `x`.
fn pad_cells(cells: &mut [Cell], x: usize, count: usize, fg: Color, bg: Color) {
    for slot in cells.iter_mut().skip(x).take(count) {
        *slot = cell(' ', 0, fg, bg);
    }
}

/// Paint one styled string into `cells` at `x`; returns the end column.
fn put(cells: &mut [Cell], x: usize, text: &str, fg: Color, bg: Color, attrs: u16) -> usize {
    let mut at = x;
    for atom in flatten(text, Style::new().fg(fg).bg(bg)) {
        if at >= cells.len() {
            break;
        }
        cells[at] = cell(atom.text.chars().next().unwrap_or(' '), attrs, fg, bg);
        at += atom.width.max(1);
    }
    at
}

/// Paint a [`SelectView`] as a full-bleed frame (the standalone mount:
/// the select fills the terminal; the scroll region stretches).
#[must_use]
pub fn frame_from_view(
    view: &SelectView,
    size: (u16, u16),
    base_bg: Color,
    default_fg: Color,
) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let mut cells = vec![vec![cell(' ', 0, default_fg, base_bg); width]; height];
    for (y, row) in view.rows.iter().enumerate().take(height) {
        for (x, cell_data) in row.iter().enumerate().take(width) {
            cells[y][x] = *cell_data;
        }
    }
    Frame { cells, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn styles() -> DialogStyles {
        DialogStyles::from_theme(&ThemeState::v2_dark())
    }

    fn cell_color(cell: &crate::render::Cell) -> (u8, u8, u8) {
        match cell.bg {
            crate::render::CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        }
    }

    fn fg_color(cell: &crate::render::Cell) -> (u8, u8, u8) {
        match cell.fg {
            crate::render::CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        }
    }

    #[test]
    fn geometry_matches_the_reference_box_model() {
        let options = vec![
            SelectOption::titled("Alpha", "a").described("First item"),
            SelectOption::titled("Beta", "b").described("Second item"),
            SelectOption::titled("Gamma", "c").described("Third item"),
        ];
        let props = SelectProps::new("Items", options).with_current(Some("b".to_owned()));
        let mut state = SelectState::default();
        state.focus_current(&props);
        assert_eq!(state.selected, 1, "current focused on open");

        let styles = styles();
        let surface = Color::Rgb(10, 10, 10);
        let view = render_select(&props, &state, &styles, surface, 80, 20, Some(20));
        // Title / esc / input geometry.
        assert_eq!(view.rows[0][4].ch, 'I');
        assert_eq!(
            (view.rows[0][73].ch, view.rows[0][74].ch, view.rows[0][75].ch),
            ('e', 's', 'c')
        );
        assert_eq!(cell_color(&view.rows[2][4]), (20, 20, 20), "input formfield bg");
        assert_eq!(cell_color(&view.rows[2][3]), (10, 10, 10), "left of the input");
        assert_eq!(cell_color(&view.rows[2][75]), (20, 20, 20), "input right edge");
        assert_eq!(cell_color(&view.rows[2][76]), (10, 10, 10), "padding right of the input");
        // Options at col 4; the current row's marker at col 2.
        assert_eq!(view.rows[4][4].ch, 'A');
        assert_eq!(view.rows[5][2].ch, '●', "current marker");
        assert_eq!(view.rows[5][4].ch, 'B');
        assert_eq!(view.rows[6][4].ch, 'G');
        // The selected row's background spans the row box [1, 79).
        assert_eq!(cell_color(&view.rows[5][1]), (250, 178, 131));
        assert_eq!(cell_color(&view.rows[5][78]), (250, 178, 131));
        assert_eq!(cell_color(&view.rows[5][79]), (10, 10, 10), "scrollbox right pad");
        assert_eq!(
            cell_color(&view.rows[4][4]),
            (10, 10, 10),
            "unselected rows keep the surface bg"
        );
        // Selected text is bold; unselected is not.
        assert_ne!(view.rows[5][4].attrs & 1, 0);
        assert_eq!(view.rows[4][4].attrs & 1, 0);
        // Description muted on unselected rows.
        assert_eq!(fg_color(&view.rows[4][9]), (128, 128, 128));
    }

    #[test]
    fn footer_actions_layout() {
        let actions = vec![
            SelectAction::new("dialog.move_session.delete", "delete", "ctrl+d"),
            SelectAction::new("dialog.move_session.new", "new", "ctrl+a"),
        ];
        let props =
            SelectProps::new("Items", vec![SelectOption::titled("A", "a")]).with_actions(actions);
        let styles = styles();
        let view = render_select(
            &props,
            &SelectState::default(),
            &styles,
            Color::Rgb(10, 10, 10),
            80,
            20,
            Some(20),
        );
        let footer = &view.rows[18];
        let text: String = (4..29).map(|col| footer[col].ch).collect();
        assert_eq!(text, "delete ctrl+d  new ctrl+a");
        // Gap cells carry the default fg (renderer fill), not text color.
        assert_eq!(fg_color(&footer[17]), (255, 255, 255));
    }

    #[test]
    fn group_headers_appear_and_collapse_while_filtering() {
        let options = vec![
            SelectOption::titled("New session", "session.new").categorized("Session"),
            SelectOption::titled("Switch model", "model.list").categorized("Agent"),
        ];
        let props = SelectProps::new("Commands", options).flat(true).threshold(0.7);
        let groups = grouped(&props, "");
        assert_eq!(groups.len(), 2, "grouped by category while unfiltered");
        assert_eq!(groups[0].category.as_deref(), Some("Session"));
        assert_eq!(content_rows(&groups), 5, "header + option, blank + header + option");

        let groups = grouped(&props, "switch");
        assert_eq!(groups.len(), 1, "flat while filtering");
        assert!(groups[0].category.is_none());
        assert_eq!(groups[0].indices, vec![1], "fuzzy match on the title");
    }

    #[test]
    fn empty_states_render_per_filter_presence() {
        let props = SelectProps::new("Items", vec![]);
        let styles = styles();
        let view = render_select(
            &props,
            &SelectState::default(),
            &styles,
            Color::Rgb(10, 10, 10),
            40,
            20,
            Some(20),
        );
        assert!(view.rows.iter().any(|row| row_text(row).contains("No items available")));
        let mut state = SelectState::default();
        state.set_filter("zzz", &props, 24);
        let view = render_select(&props, &state, &styles, Color::Rgb(10, 10, 10), 40, 20, Some(20));
        assert!(view.rows.iter().any(|row| row_text(row).contains("No results found")));
    }

    fn row_text(row: &[crate::render::Cell]) -> String {
        row.iter().map(|cell| cell.ch).collect()
    }

    #[test]
    fn long_lists_scroll_with_edge_follow() {
        let options: Vec<SelectOption> = (0..30)
            .map(|index| SelectOption::titled(format!("Item {index}"), format!("item-{index}")))
            .collect();
        let props = SelectProps::new("Items", options);
        let groups = grouped(&props, "");
        assert_eq!(content_rows(&groups), 30);
        // Viewport at terminal height 20: min(30, max(5, 10 - 6)) = 5 rows
        // (SE-F6: the ref's min-5 window — never collapses to 0).
        assert_eq!(viewport_height(30, 20), 5);
        assert_eq!(viewport_height(30, 12), 5, "12-row terminals keep a 5-row window");
        assert_eq!(viewport_height(3, 12), 3, "never above the row count");

        let mut state = SelectState::default();
        for _ in 0..9 {
            state.move_selection(&props, 1);
        }
        state.reveal(&props, 20);
        assert_eq!(state.selected, 9);
        assert_eq!(state.scroll, 5, "edge-follow: selected row visible at the bottom");
        state.move_selection(&props, -1);
        state.reveal(&props, 20);
        assert_eq!(state.scroll, 5, "scroll does not jump upward while following");
        state.move_to_edge(&props, false);
        state.reveal(&props, 20);
        assert_eq!((state.selected, state.scroll), (0, 0), "home reveals from the top");
        state.move_to_edge(&props, true);
        state.reveal(&props, 20);
        assert_eq!(state.selected, 29);
        assert_eq!(state.scroll, 25, "end pins the last row into the viewport");

        // The rendered window matches the scroll offset.
        let styles = styles();
        let view = render_select(&props, &state, &styles, Color::Rgb(10, 10, 10), 40, 20, Some(20));
        let text: String = view.rows[4].iter().map(|cell| cell.ch).collect();
        assert!(text.contains("Item 25"), "first visible row at scroll 25: {text}");
        let text: String = view.rows[8].iter().map(|cell| cell.ch).collect();
        assert!(text.contains("Item 29"), "last visible row: {text}");
        let text: String = view.rows[9].iter().map(|cell| cell.ch).collect();
        assert_eq!(text.trim(), "", "row below the viewport is blank");
    }

    /// SE-F4: the session list's filter contract — trimmed,
    /// case-folded, byte-exact SUBSTRING (the ref's
    /// `dialog-session-list`); whitespace-only = the full list; no
    /// fuzzy cross-letter matches; no accent folding.
    #[test]
    fn substring_filter_matches_the_session_list_contract() {
        let options = vec![
            SelectOption::titled("Login Redirect Flow", "a"),
            SelectOption::titled("Zephyr Long Night", "b"),
            SelectOption::titled("Café notes", "c"),
        ];
        let props = SelectProps::new("Sessions", options).substring_filter();
        let ids = |filter: &str| {
            filter_indices(&props, filter)
                .into_iter()
                .map(|index| props.options[index].value.clone())
                .collect::<Vec<_>>()
        };
        // Substring, case-folded.
        assert_eq!(ids("login red"), vec!["a".to_owned()]);
        assert_eq!(ids("LONG NIGHT"), vec!["b".to_owned()]);
        // Fuzzy cross-letter sequences (the E12 divergence: zln)
        // must NOT match.
        assert_eq!(ids("zln"), [] as [std::string::String; 0]);
        // Trim: surrounding whitespace is insignificant…
        assert_eq!(ids("  café  "), vec!["c".to_owned()]);
        // …and a whitespace-only query is EMPTY = the full list.
        assert_eq!(ids("   ").len(), 3);
        // Byte-exact: no accent folding (cafe ≠ Café).
        assert_eq!(ids("cafe"), [] as [std::string::String; 0]);
        // The default (fuzzy) contract is untouched: zln still ranks
        // through fuzzysort for the palette.
        let fuzzy_props = SelectProps::new(
            "Commands",
            vec![
                SelectOption::titled("Login Redirect Flow", "a"),
                SelectOption::titled("Zephyr Long Night", "b"),
            ],
        );
        assert_ne!(filter_indices(&fuzzy_props, "zln"), [] as [usize; 0]);
    }

    #[test]
    fn selection_wraps_and_reconciles() {
        assert_eq!(move_selection(0, 3, -1), 2, "wraps backward");
        assert_eq!(move_selection(2, 3, 1), 0, "wraps forward");
        assert_eq!(move_selection(1, 3, 10), 0, "big deltas wrap through");
        assert_eq!(reconcile_selection(5, 3), 2);
        assert_eq!(reconcile_selection(0, 0), 0);

        // The selection reconciles when the filter shrinks the list.
        let options = vec![
            SelectOption::titled("Alpha", "a"),
            SelectOption::titled("Beta", "b"),
            SelectOption::titled("Gamma", "c"),
        ];
        let props = SelectProps::new("Items", options);
        let mut state = SelectState::default();
        state.move_selection(&props, 2);
        state.set_filter("alpha", &props, 24);
        assert_eq!(state.selected, 0, "typing resets to the first result");
        state.filter.clear();
        state.selected = 2;
        state.reveal(&props, 24);
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn filter_order_follows_the_fuzzy_ranking() {
        let options = vec![
            SelectOption::titled("Switch session", "session.list").categorized("Session"),
            SelectOption::titled("New session", "session.new").categorized("Session"),
            SelectOption::titled("Switch model", "model.list").categorized("Agent"),
        ];
        let props = SelectProps::new("Commands", options).threshold(0.7);
        let indices = filter_indices(&props, "new");
        assert_eq!(indices.first().copied(), Some(1), "title match outranks id matches");
    }
}
