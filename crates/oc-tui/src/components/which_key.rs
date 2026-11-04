//! The which-key panel: a full port of the reference TUI's local
//! which-key feature plugin (`feature-plugins/system/which-key.tsx`,
//! 607 lines at its last shipped revision — the v1 plugin runtime's
//! removal deleted the file but left its keybind table, currently
//! `config/keybind.ts:290-300`). The panel is NOT an
//! `@opentui/keymap` addon: the upstream keymap package exports only
//! the `ActiveKey` datum (`getActiveKeys`) and the engine; every pixel
//! of the panel is Solid UI local to the reference TUI.
//!
//! Ported surface (ref line tags are the last shipped file):
//! * **dock / overlay layouts** (`layout.toggle`, :67/:260-520):
//!   dock rides below the route (the route shrinks), overlay
//!   `position=absolute bottom=0 zIndex=3500`.
//! * **pending preview** (`pending.toggle`, :212-216): auto-shows
//!   while a chord is pending in overlay mode.
//! * **group tabs + group navigation** (`group.previous/next`,
//!   :151-171/:565-593): active keys grouped by the command's
//!   category (`System` for continuations), sorted (continuations
//!   first, then label/key); the selected tab's entries fill the
//!   panel.
//! * **scroll / paging / home / end** (:565-593): one column per
//!   `scroll.*` step, one full page per `page.*`; the header's
//!   `↑ ↓` indicator follows the offset.
//! * **pending mode** (:328-371): a pending chord flattens ALL groups
//!   (group headers + entries, multi-column), hides the tabs/footer,
//!   and binds only the six scroll commands.
//! * **footer hints** (:447-503): `toggle <binding>` and the next
//!   layout's name + `<binding>`; the home screen's
//!   "Show keyboard shortcuts with …" hint (:176-188).
//!
//! Shaping stays on the D2.2 shared tree: the panel is the
//! `surface-which-key` family wrapper (skeleton index 7) with a full
//! panel-background fill; the old `paste_frame` overwrote every cell
//! of the rect (transparent interior included), so the mount stays
//! BLANK-underlaid.

use super::chrome_solid::{
    blank_frame, chrome_run_units, composite_chrome_tree, reset_hub, ChromeBody, ChromeUnit,
};
use super::overlays_solid::{mount_overlay_surface, OVERLAYS_WHICH_KEY};
use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// `PANEL_HEIGHT_RATIO` / `MIN_PANEL_HEIGHT` / `MAX_PANEL_HEIGHT`
/// (which-key.tsx:34-36): 30% of the terminal, clamped to 8..16 rows.
const PANEL_HEIGHT_RATIO: f64 = 0.3;
const MIN_PANEL_HEIGHT: usize = 8;
const MAX_PANEL_HEIGHT: usize = 16;
/// `PANEL_TOP_PADDING` (which-key.tsx:37).
const PANEL_TOP_PADDING: usize = 1;
/// `FOOTER_HEIGHT` + `FOOTER_MARGIN` (which-key.tsx:38-39).
const FOOTER_HEIGHT: usize = 1;
const FOOTER_MARGIN: usize = 1;
/// `COLUMN_GAP` / `TAB_GAP` / `MIN_TAB_GAP` / `TAB_CONTENT_GAP`
/// (which-key.tsx:30-33).
const COLUMN_GAP: usize = 4;
const TAB_GAP: usize = 3;
const MIN_TAB_GAP: usize = 1;
const TAB_CONTENT_GAP: usize = 1;
/// `MAX_COLUMN_WIDTH` (which-key.tsx:32; `MIN_COLUMN_WIDTH` is
/// declared-but-unused upstream).
const MAX_COLUMN_WIDTH: usize = 44;
const MAX_COLUMNS: usize = 3;
/// The bold attribute bit (ECMA-48 weight, `render::ansi`).
const BOLD: u16 = 1;

/// `Layout` (which-key.tsx:45): dock renders below the route;
/// overlay floats over it. The port's default is overlay so the
/// pending-chord surface stays reachable at boot (the reference
/// plugin itself ships `enabled: false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhichKeyLayout {
    #[default]
    Overlay,
    Dock,
}

impl WhichKeyLayout {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Dock => Self::Overlay,
            Self::Overlay => Self::Dock,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dock => "dock",
            Self::Overlay => "overlay",
        }
    }
}

/// One reachable binding: the key display, the command's label, the
/// command's group (the tab), and whether the key continues a chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhichKeyEntry {
    pub key: String,
    pub label: String,
    pub group: String,
    pub continues: bool,
}

/// One group of entries (a header tab, or a pending-mode section).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhichKeyGroup {
    pub label: String,
    pub entries: Vec<WhichKeyEntry>,
}

/// The panel's view model (live store state — `Some` only while the
/// panel shows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhichKeyView {
    /// The pending prefix's display, `None` at the root (pinned).
    pub prefix: Option<String>,
    pub entries: Vec<WhichKeyEntry>,
    pub layout: WhichKeyLayout,
    pub pending_preview: bool,
    pub pinned: bool,
    pub offset: usize,
    pub active_group: Option<String>,
    /// `toggle` / `layout.toggle` binding displays for the footer
    /// (`commandShortcut`, which-key.tsx:157-160).
    pub toggle_hint: String,
    pub layout_hint: String,
}

/// The panel geometry derived from the terminal size and the mode
/// (which-key.tsx:218-343).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhichKeyGeometry {
    pub panel_height: usize,
    pub content_width: usize,
    pub columns: usize,
    pub rows: usize,
    pub page_size: usize,
    pub max_offset: usize,
    pub tabs_visible: bool,
    pub header_visible: bool,
    pub footer_visible: bool,
}

impl WhichKeyView {
    /// `pendingMode` (which-key.tsx:218): a live chord with reachable
    /// keys (the pending panel lists every group, flattened).
    #[must_use]
    pub fn pending_mode(&self) -> bool {
        self.prefix.is_some() && !self.entries.is_empty()
    }

    /// `grouped(entries)` (which-key.tsx:151-171): groups sorted by
    /// label; entries continuing-first, then label, then key.
    #[must_use]
    pub fn groups(&self) -> Vec<WhichKeyGroup> {
        let mut groups: Vec<WhichKeyGroup> = Vec::new();
        for entry in &self.entries {
            match groups.iter_mut().find(|group| group.label == entry.group) {
                Some(group) => group.entries.push(entry.clone()),
                None => groups.push(WhichKeyGroup {
                    label: entry.group.clone(),
                    entries: vec![entry.clone()],
                }),
            }
        }
        for group in &mut groups {
            group.entries.sort_by(|a, b| {
                b.continues
                    .cmp(&a.continues)
                    .then_with(|| a.label.cmp(&b.label))
                    .then_with(|| a.key.cmp(&b.key))
            });
        }
        groups.sort_by(|a, b| a.label.cmp(&b.label));
        groups
    }

    /// The selected group (which-key.tsx:274-277): the stored tab when
    /// it still exists, else the first group.
    #[must_use]
    pub fn current_group(&self, groups: &[WhichKeyGroup]) -> Option<usize> {
        let stored = self.active_group.as_deref();
        stored
            .and_then(|label| groups.iter().position(|group| group.label == label))
            .or(if groups.is_empty() { None } else { Some(0) })
    }

    /// The panel's item count at the current mode (pending mode counts
    /// each group header, :278-282).
    #[must_use]
    pub fn item_count(&self, groups: &[WhichKeyGroup]) -> usize {
        if self.pending_mode() {
            groups.iter().map(|group| 1 + group.entries.len()).sum()
        } else {
            self.current_group(groups).map_or(0, |index| groups[index].entries.len())
        }
    }

    /// The full geometry (which-key.tsx:218-343).
    #[must_use]
    pub fn geometry(&self, width: usize, height: usize) -> WhichKeyGeometry {
        let panel_height = panel_height(height);
        let content_width = width.saturating_sub(2).max(1);
        let columns = columns_for(content_width);
        let pending_mode = self.pending_mode();
        let groups = self.groups();
        let tabs_visible = !pending_mode && !groups.is_empty();
        let header_visible = tabs_visible || pending_mode;
        let footer_visible = !pending_mode;
        let rows = panel_height
            .saturating_sub(PANEL_TOP_PADDING)
            .saturating_sub(usize::from(header_visible))
            .saturating_sub(if tabs_visible { TAB_CONTENT_GAP } else { 0 })
            .saturating_sub(if footer_visible { FOOTER_MARGIN + FOOTER_HEIGHT } else { 0 })
            .max(1);
        let page_size = rows.saturating_mul(columns).max(1);
        let max_offset = self.item_count(&groups).saturating_sub(page_size);
        WhichKeyGeometry {
            panel_height,
            content_width,
            columns,
            rows,
            page_size,
            max_offset,
            tabs_visible,
            header_visible,
            footer_visible,
        }
    }

    /// `clamp` (which-key.tsx:344): the offset within `0..=maxOffset`.
    #[must_use]
    pub fn clamp_offset(&self, offset: usize, width: usize, height: usize) -> usize {
        offset.min(self.geometry(width, height).max_offset)
    }

    /// `shown` (which-key.tsx:283-295): the visible columns, each a
    /// vertical slice of `rows` items starting at `offset`.
    #[must_use]
    pub fn shown(&self, groups: &[WhichKeyGroup], geometry: &WhichKeyGeometry) -> Vec<Vec<PanelItem>> {
        let items = self.items(groups);
        let mut columns: Vec<Vec<PanelItem>> = Vec::new();
        let mut index = self.offset.min(items.len());
        for _ in 0..geometry.columns {
            if index >= items.len() {
                break;
            }
            let mut list = Vec::new();
            while list.len() < geometry.rows && index < items.len() {
                list.push(items[index].clone());
                index += 1;
            }
            columns.push(list);
        }
        columns
    }

    fn items(&self, groups: &[WhichKeyGroup]) -> Vec<PanelItem> {
        if self.pending_mode() {
            let mut items = Vec::new();
            for group in groups {
                items.push(PanelItem::Header(group.label.clone()));
                items.extend(group.entries.iter().cloned().map(PanelItem::Entry));
            }
            items
        } else {
            self.current_group(groups)
                .map(|index| groups[index].entries.iter().cloned().map(PanelItem::Entry).collect())
                .unwrap_or_default()
        }
    }
}

/// One positioned item of the flattened listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelItem {
    Header(String),
    Entry(WhichKeyEntry),
}

/// `panelHeight` (which-key.tsx:219-221).
#[must_use]
pub fn panel_height(height: usize) -> usize {
    let ratio = (height as f64 * PANEL_HEIGHT_RATIO).floor() as usize;
    ratio.clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT).min(height.max(1))
}

/// `columns` (which-key.tsx:223-226).
#[must_use]
pub fn columns_for(content_width: usize) -> usize {
    let inside = (content_width + COLUMN_GAP) / (MAX_COLUMN_WIDTH + COLUMN_GAP);
    inside.clamp(1, MAX_COLUMNS)
}

/// `columnWidth` (which-key.tsx:340-342).
#[must_use]
pub fn column_width(content_width: usize, columns: usize) -> usize {
    let gaps = columns.saturating_sub(1) * COLUMN_GAP;
    (content_width.saturating_sub(gaps) / columns.max(1)).clamp(1, MAX_COLUMN_WIDTH)
}

/// The panel's colors (`skin`, which-key.tsx:89-102) resolved through
/// the v2 token palette: `backgroundMenu` → `background.raised.high`
/// (`ThemeStyles::action_hovered`), `selectedListItemText` →
/// `text.action.primary.focused`, `borderSubtle` → `border`.
struct Skin {
    panel: CellColor,
    text: CellColor,
    muted: CellColor,
    subtle: CellColor,
    key: CellColor,
    accent: CellColor,
    tab: CellColor,
    tab_text: CellColor,
}

impl Skin {
    fn from_styles(styles: &ThemeStyles) -> Self {
        let color = |value: crate::style::Color| CellColor::from(value);
        Self {
            panel: color(styles.action_hovered),
            text: styles.text.fg.map_or(CellColor::Default, CellColor::from),
            muted: styles.text_muted.fg.map_or(CellColor::Default, CellColor::from),
            subtle: color(styles.border),
            key: color(styles.warning),
            accent: color(styles.accent),
            tab: color(styles.action_focused_bg),
            tab_text: color(styles.action_focused_fg),
        }
    }
}

fn truncate(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Render the panel into absolute units (surface-local) — the full
/// which-key.tsx layout: background fill, top padding, centered
/// tab/scroll header, the multi-column listing (group headers in
/// pending mode), and the footer hints.
///
/// Returns `(width, height, units)`.
#[must_use]
pub fn which_key_units(
    view: &WhichKeyView,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
) -> (usize, usize, Vec<ChromeUnit>) {
    let skin = Skin::from_styles(styles);
    let render_width = width.max(1);
    let geometry = view.geometry(render_width, height);
    let groups = view.groups();
    let pending_mode = view.pending_mode();
    let current = view.current_group(&groups);
    let rows = geometry.panel_height;
    let mut units = vec![ChromeUnit {
        key: "which-key.fill".to_owned(),
        left: 0,
        top: 0,
        body: ChromeBody::Fill {
            width: render_width,
            height: rows,
            color: skin.panel,
        },
    }];
    let mut y = PANEL_TOP_PADDING;
    // The header: group tabs (dock listing) or the scroll indicator
    // (pending mode); centered with `tabGap` (:224-232/:389-393).
    if geometry.header_visible {
        let mut header = Vec::new();
        if geometry.tabs_visible {
            header.extend(groups.iter().enumerate().map(|(index, group)| {
                HeaderItem::Tab { index, label: group.label.clone() }
            }));
        }
        if geometry.max_offset > 0 {
            header.push(HeaderItem::Scroll);
        }
        let item_width = |item: &HeaderItem| match item {
            HeaderItem::Tab { label, .. } => chars(label) + 2,
            HeaderItem::Scroll => 3,
        };
        let total: usize = header.iter().map(item_width).sum();
        let count = header.len();
        let gap = if count <= 1 {
            0
        } else {
            (geometry.content_width.saturating_sub(total) / (count - 1))
                .clamp(MIN_TAB_GAP, TAB_GAP)
        };
        let occupied = total + gap * count.saturating_sub(1);
        let mut x = 1 + geometry.content_width.saturating_sub(occupied) / 2;
        for item in &header {
            match item {
                HeaderItem::Tab { index, label } => {
                    let selected = current == Some(*index);
                    let run = format!(" {label} ");
                    let (fg, bg, attrs) = if selected {
                        (skin.tab_text, skin.tab, BOLD)
                    } else {
                        (skin.muted, CellColor::Default, 0)
                    };
                    units.extend(chrome_run_units(
                        &format!("which-key.tab.{x}"),
                        x,
                        y,
                        &run,
                        fg,
                        bg,
                        attrs,
                    ));
                    x += chars(&run) + gap;
                }
                HeaderItem::Scroll => {
                    let up = if view.offset > 0 { skin.text } else { skin.muted };
                    let down =
                        if view.offset < geometry.max_offset { skin.text } else { skin.muted };
                    units.extend(chrome_run_units(
                        "which-key.scroll.up",
                        x,
                        y,
                        "↑",
                        up,
                        CellColor::Default,
                        0,
                    ));
                    units.extend(chrome_run_units(
                        "which-key.scroll.gap",
                        x + 1,
                        y,
                        " ",
                        skin.muted,
                        CellColor::Default,
                        0,
                    ));
                    units.extend(chrome_run_units(
                        "which-key.scroll.down",
                        x + 2,
                        y,
                        "↓",
                        down,
                        CellColor::Default,
                        0,
                    ));
                    x += 3 + gap;
                }
            }
        }
        y += 1;
        if geometry.tabs_visible {
            y += TAB_CONTENT_GAP;
        }
    }
    // The listing: columns filled top-to-bottom, labels left / keys
    // right per column (`justifyContent=space-between`, :404-445).
    let shown = view.shown(&groups, &geometry);
    let column_width = column_width(geometry.content_width, geometry.columns);
    let gaps = geometry.columns.saturating_sub(1) * COLUMN_GAP;
    let total_width = column_width * geometry.columns + gaps;
    let start = 1 + geometry.content_width.saturating_sub(total_width) / 2;
    if shown.is_empty() {
        units.extend(chrome_run_units(
            "which-key.empty",
            1,
            y,
            "No reachable bindings",
            skin.muted,
            CellColor::Default,
            0,
        ));
    } else {
        for row in 0..geometry.rows {
            for (column, list) in shown.iter().enumerate() {
                let Some(item) = list.get(row) else { continue };
                let x = start + column * (column_width + COLUMN_GAP);
                match item {
                    PanelItem::Header(label) => {
                        let text = truncate(label, column_width);
                        units.extend(chrome_run_units(
                            &format!("which-key.group.{x}"),
                            x,
                            y + row,
                            &text,
                            skin.accent,
                            CellColor::Default,
                            BOLD,
                        ));
                    }
                    PanelItem::Entry(entry) => {
                        let key_len = chars(&entry.key);
                        let key_width = key_len.min(column_width);
                        let key = truncate(&entry.key, key_width);
                        let label_room = column_width.saturating_sub(key_width + 1);
                        if label_room > 0 {
                            let label = truncate(&entry.label, label_room);
                            if !label.is_empty() {
                                let fg = if entry.continues { skin.accent } else { skin.muted };
                                units.extend(chrome_run_units(
                                    &format!("which-key.label.{x}"),
                                    x,
                                    y + row,
                                    &label,
                                    fg,
                                    CellColor::Default,
                                    0,
                                ));
                            }
                        }
                        units.extend(chrome_run_units(
                            &format!("which-key.key.{x}"),
                            x + column_width.saturating_sub(key_width),
                            y + row,
                            &key,
                            skin.key,
                            CellColor::Default,
                            BOLD,
                        ));
                    }
                }
            }
        }
    }
    // The footer (`footerVisible`, :447-503): the toggle hint left, the
    // next layout's name + binding right, both in the content width.
    if geometry.footer_visible {
        let footer_y = rows.saturating_sub(FOOTER_HEIGHT);
        units.extend(chrome_run_units(
            "which-key.footer.toggle",
            1,
            footer_y,
            "toggle ",
            skin.text,
            CellColor::Default,
            0,
        ));
        units.extend(chrome_run_units(
            "which-key.footer.toggle.hint",
            1 + chars("toggle "),
            footer_y,
            &truncate(&view.toggle_hint, geometry.content_width.saturating_sub(7)),
            skin.subtle,
            CellColor::Default,
            0,
        ));
        let next = view.layout.next().as_str();
        let right_len = chars(next) + 1 + chars(&view.layout_hint);
        if right_len <= geometry.content_width {
            let right = 1 + geometry.content_width.saturating_sub(right_len);
            units.extend(chrome_run_units(
                "which-key.footer.layout",
                right,
                footer_y,
                &format!("{next} "),
                skin.text,
                CellColor::Default,
                0,
            ));
            units.extend(chrome_run_units(
                "which-key.footer.layout.hint",
                right + chars(next) + 1,
                footer_y,
                &view.layout_hint,
                skin.subtle,
                CellColor::Default,
                0,
            ));
        }
    }
    (render_width, rows, units)
}

enum HeaderItem {
    Tab { index: usize, label: String },
    Scroll,
}

/// Mount the which-key panel into the shared tree (the live path:
/// overlay seats at the screen bottom; dock seats at `dock_top`, the
/// route's reduced height).
pub fn mount_which_key(
    view: &WhichKeyView,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
    dock_top: Option<usize>,
) {
    let (w, rows, units) = which_key_units(view, styles, width, height);
    let top = match view.layout {
        WhichKeyLayout::Overlay => height.saturating_sub(rows),
        WhichKeyLayout::Dock => dock_top.unwrap_or_else(|| height.saturating_sub(rows)),
    };
    mount_overlay_surface("which-key", OVERLAYS_WHICH_KEY, 0, top, w, rows, &units, true, &[]);
}

/// The headless twin (the golden/unit harness shape): mount ONE
/// panel into a fresh hub and read it back as a standalone Frame.
#[must_use]
pub fn which_key_frame(
    view: &WhichKeyView,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
) -> Frame {
    let (w, rows, units) = which_key_units(view, styles, width, height);
    reset_hub();
    mount_overlay_surface("which-key", OVERLAYS_WHICH_KEY, 0, 0, w, rows, &units, true, &[]);
    let mut frame = blank_frame(w, rows);
    composite_chrome_tree(&mut frame);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStyles;

    fn entry(key: &str, label: &str, group: &str, continues: bool) -> WhichKeyEntry {
        WhichKeyEntry {
            key: key.to_owned(),
            label: label.to_owned(),
            group: group.to_owned(),
            continues,
        }
    }

    fn view(prefix: Option<&str>, entries: Vec<WhichKeyEntry>) -> WhichKeyView {
        WhichKeyView {
            prefix: prefix.map(str::to_owned),
            entries,
            layout: WhichKeyLayout::Overlay,
            pending_preview: true,
            pinned: false,
            offset: 0,
            active_group: None,
            toggle_hint: "ctrl+alt+k".to_owned(),
            layout_hint: "ctrl+alt+shift+k".to_owned(),
        }
    }

    fn styles() -> ThemeStyles {
        ThemeStyles::from_resolved(&crate::theme::ThemeState::v2_dark().resolved)
    }

    fn frame_text(view: &WhichKeyView, width: usize, height: usize) -> Vec<String> {
        which_key_frame(view, &styles(), width, height).text_grid()
    }

    #[test]
    fn panel_renders_tabs_entries_footer_and_columns() {
        let view = view(
            None,
            vec![
                entry("a", "List agents", "Agent", false),
                entry("b", "Toggle sidebar", "View", false),
                entry("g", "+<leader>g", "System", true),
            ],
        );
        let grid = frame_text(&view, 60, 24);
        // The standalone twin is exactly the panel rect (8 rows at 24
        // tall; 30% clamped).
        assert_eq!(grid.len(), MIN_PANEL_HEIGHT);
        assert!(grid[1].contains("Agent"), "the Agent tab: {:?}", grid[1]);
        assert!(grid[1].contains("View"), "the View tab: {:?}", grid[1]);
        assert!(grid.iter().any(|row| row.contains("List agents")), "{grid:?}");
        assert!(!grid.iter().any(|row| row.contains("Toggle sidebar")), "only the selected tab's rows: {grid:?}");
        assert!(grid[7].contains("toggle ctrl+alt+k"), "the footer: {:?}", grid[7]);
        assert!(grid[7].contains("dock ctrl+alt+shift+k"), "the next layout: {:?}", grid[7]);
        // Selecting another tab swaps the listing.
        let mut view = view;
        view.active_group = Some("View".to_owned());
        let grid = frame_text(&view, 60, 24);
        assert!(grid.iter().any(|row| row.contains("Toggle sidebar")), "{grid:?}");
        assert!(!grid.iter().any(|row| row.contains("List agents")), "{grid:?}");
    }

    #[test]
    fn groups_sort_continuations_first_then_label() {
        let view = view(
            None,
            vec![
                entry("z", "Zeta", "System", false),
                entry("x", "Alpha", "System", true),
                entry("a", "Beta", "System", false),
            ],
        );
        let groups = view.groups();
        assert_eq!(groups.len(), 1);
        let labels: Vec<&str> = groups[0].entries.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(labels, ["Alpha", "Beta", "Zeta"]);
        assert!(groups[0].entries[0].continues);
    }

    #[test]
    fn pending_mode_flattens_groups_and_hides_footer() {
        let view = view(
            Some("<leader>"),
            vec![
                entry("a", "List agents", "Agent", false),
                entry("b", "Toggle sidebar", "View", false),
            ],
        );
        assert!(view.pending_mode());
        let geometry = view.geometry(80, 24);
        assert!(!geometry.tabs_visible);
        assert!(!geometry.footer_visible);
        let grid = frame_text(&view, 80, 24);
        assert!(grid.iter().any(|row| row.contains("Agent")), "the group header: {grid:?}");
        assert!(grid.iter().any(|row| row.contains("View")), "the group header: {grid:?}");
        assert!(grid.iter().any(|row| row.contains("List agents")), "{grid:?}");
        assert!(!grid.iter().any(|row| row.contains("toggle ctrl+alt+k")), "no footer: {grid:?}");
    }

    #[test]
    fn scrolling_shifts_the_listing_and_drives_the_indicator() {
        let entries: Vec<WhichKeyEntry> = (0..40)
            .map(|index| entry(&format!("k{index:02}"), &format!("Label {index:02}"), "View", false))
            .collect();
        let mut view = view(None, entries);
        view.offset = 4;
        let geometry = view.geometry(80, 24);
        assert!(geometry.max_offset > 0);
        let grid = frame_text(&view, 80, 24);
        assert!(!grid.iter().any(|row| row.contains("Label 00")), "offset skips the first rows");
        assert!(grid.iter().any(|row| row.contains("Label 04")), "{grid:?}");
        assert!(grid.iter().any(|row| row.contains('↑')), "the scroll indicator: {grid:?}");
    }

    #[test]
    fn dock_and_overlay_share_the_panel_height() {
        assert_eq!(panel_height(24), MIN_PANEL_HEIGHT);
        assert_eq!(panel_height(80), MAX_PANEL_HEIGHT);
        assert_eq!(panel_height(40), 12);
        assert_eq!(panel_height(10), 8);
        assert_eq!(columns_for(38), 1);
        assert_eq!(columns_for(80), 1);
        assert_eq!(columns_for(138), 2);
    }
}
