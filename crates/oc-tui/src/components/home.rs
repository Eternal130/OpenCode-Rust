//! The home route (`routes/home.tsx`): the OPENCODE wordmark, the
//! centered prompt composer (the same `Prompt` the session route
//! docks), and the `home.footer` plugin slot — **golden-pinned**
//! (`home.w80` / `home.w160`, text grid + canonical ANSI byte-exact;
//! the Phase-8 export chore, closed).
//!
//! ## Layout (read off the golden + `routes/home.tsx`)
//!
//! ```text
//! [ flexGrow spacer ]      ← the column centers its content
//! [ 3 fixed rows ]
//! [ Logo (4 rows, 39 cells wide, centered) ]
//! [ 1 spacer row ]
//! [ Prompt mount: paddingTop 1 + the composer (6 rows empty) ]
//! [ flexGrow spacer ]
//! [ home.footer slot (3 rows at h≥16: pad · version · pad) ]
//! ```
//!
//! * the outer column pads `2` (1 below 44 cols) each side; centered
//!   children round their offset **up** (`(free + 1) / 2`) — taffy's
//!   half-pixel rounding, pinned by both goldens (logo x21@80/x61@160,
//!   prompt x3@80/x43@160);
//! * the prompt box is `width 100% maxWidth 75` — 75 cells wide at
//!   both golden widths;
//! * the footer slot (`feature-plugins/home/footer.tsx`) shows the app
//!   version right-aligned (`paddingRight 2`); MCP/plugin status is
//!   width-gated and empty in every pinned state (the fixture server
//!   has none) — the store carries no MCP/plugin state yet, so only
//!   the version renders;
//! * the whole footer hides below 44 cols or 12 rows, the logo below
//!   12 rows (`homeFooterVisibility` / `Logo`).

use crate::components::composer::render::{
    render_prompt_frame, AutocompleteView, FooterHints, PromptView,
};
use crate::components::composer::PromptDraft;
use crate::render::{Cell, CellColor, Frame};
use crate::theme::ThemeState;
use crate::wrap::display_width;
use oc_theme::Rgba;

/// The home placeholder rotation (`routes/home.tsx` `placeholder.normal`)
/// — the reference picks its entry at RANDOM (`randomIndex`); the port
/// pins index 0 (the exporter seeds `Math.random = () => 0` for the
/// same reason).
pub const PLACEHOLDERS: &[&str] =
    &["Fix a TODO in the codebase", "What is the tech stack of this project?", "Fix broken tests"];

/// The wordmark art (`logo.ts`) — 19-cell halves joined by a 1-cell
/// gap (`gap={1}`): `_` renders as a space over the shadow, `^` as `▀`
/// over the shadow, `~` as `▀` IN the shadow, `,` as `▄` in the shadow.
const LOGO_LEFT: [&str; 4] =
    ["                   ", "█▀▀█ █▀▀█ █▀▀█ █▀▀▄", "█__█ █__█ █^^^ █__█", "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀~~▀"];
const LOGO_RIGHT: [&str; 4] =
    ["             ▄     ", "█▀▀▀ █▀▀█ █▀▀█ █▀▀█", "█___ █__█ █__█ █^^^", "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"];

/// The logo's full width: left 19 + gap 1 + right 19.
const LOGO_WIDTH: usize = 19 + 1 + 19;

/// The prompt box's `maxWidth` (`routes/home.tsx`).
const PROMPT_MAX_WIDTH: usize = 75;

/// One plugin-contributed home quick action (wave 3's home slot): a
/// label rendered on the home footer row that dispatches `command`
/// (a plugin or builtin command id) through the palette dispatcher.
/// Materialized at registration; never per-frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginQuickAction {
    pub plugin_id: String,
    pub label: String,
    pub command: String,
}

/// The home render's inputs (the store-assembled form drives the
/// prompt draft + footer; the golden harness injects the fixture
/// location/version directly).
pub struct HomeView<'a> {
    pub draft: &'a PromptDraft,
    /// The rotated placeholder entry (`""` = none — an empty list).
    pub placeholder: String,
    /// The status row's `{directory}:{branch}` label.
    pub location: String,
    /// The RAW directory behind the label (the working-directory
    /// actions' target — `footerLocation().directory`).
    pub directory: String,
    /// The location label's hover state (#33 — the render input).
    pub location_hovered: bool,
    /// The app version (`home.footer`'s right-aligned label).
    pub version: String,
    /// The staged agent label for the composer hint row.
    pub status_agent: String,
    /// The staged model label for the composer hint row.
    pub status_model: String,
    /// Plugin footer hint segments (wave 3).
    pub plugin_hints: Vec<String>,
    /// Plugin quick-action labels (wave 3's home slot; rendered on
    /// the footer row's left, ` · `-joined, only when non-empty so
    /// every golden stays byte-exact).
    pub plugin_actions: Vec<String>,
    /// The live slash-palette popup (`None` in the golden harness —
    /// the fixture drafts never open it).
    pub autocomplete: Option<AutocompleteView<'a>>,
    /// The rail's agent color (`None` = the golden `border.base`
    /// fallback; the live route passes the resolved categorical
    /// color).
    pub rail: Option<CellColor>,
    /// The blinking block caret offset (`None` = hidden phase / golden
    /// frames).
    pub caret: Option<usize>,
}

/// Render the home route at `size` (the golden-pinned layout).
#[must_use]
pub fn render_home_view(view: &HomeView, theme: &ThemeState, size: (u16, u16)) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let tokens = &theme.resolved.tokens;
    let base = cell(tokens.background.base);
    let default_fg = CellColor::Rgb(255, 255, 255);
    let mut frame = Frame {
        cells: vec![vec![Cell { ch: ' ', attrs: 0, fg: default_fg, bg: base }; width]; height],
        width,
        height,
    };

    let padding = if width < 44 { 1 } else { 2 };
    let content = width.saturating_sub(padding * 2);
    // Centered offset (taffy rounds the half pixel up — golden-pinned).
    let centered = |item_width: usize| padding + content.saturating_sub(item_width).div_ceil(2);

    let show_logo = height >= 12;
    let show_footer = height >= 12 && width >= 44;
    // The prompt mount: paddingTop 1 + the composer's own height.
    let prompt_width = content.clamp(1, PROMPT_MAX_WIDTH);
    let mount =
        1 + crate::components::composer::render::mount_height(view.draft, prompt_width, height);
    let fixed = if show_logo { 3 + 4 + 1 } else { 0 } + mount;
    let free = height.saturating_sub(if show_footer { 3 } else { 0 } + fixed);
    let spacer_top = free.div_ceil(2);
    let mut y = spacer_top;
    if show_logo {
        y += 3; // the fixed 3-row spacer (`height={3}`)
        paint_logo(&mut frame, centered(LOGO_WIDTH), y, theme);
        y += 4 + 1; // logo + the 1-row spacer
    }
    // The prompt mount (the composer's own frame, pinned at `top` 1
    // for the wrapper's paddingTop). The slash-palette popup floats
    // ABOVE it: the prompt frame grows by `popup_rows` rows (the
    // composer at `top = 1 + popup_rows`), pasted at
    // `y - popup_rows` so the composer itself never moves (no popup ⇒
    // the golden single-mount geometry).
    let popup_rows = match view.autocomplete.as_ref() {
        Some(popup) => crate::components::composer::autocomplete::popup_height(
            popup.options.len(),
            y.saturating_sub(1),
        )
        .min(y),
        None => 0,
    };
    let prompt_view = PromptView {
        draft: view.draft,
        placeholder: view.placeholder.clone(),
        autocomplete: view.autocomplete.clone(),
        footer: FooterHints {
            location: view.location.clone(),
            agent_shortcut: "shift+tab".into(),
            command_shortcut: "ctrl+p".into(),
            status_agent: view.status_agent.clone(),
            status_model: view.status_model.clone(),
            plugin_hints: view.plugin_hints.clone(),
            interrupt: String::new(),
            location_hovered: view.location_hovered,
        },
        top: 1 + popup_rows,
        rail: view.rail,
        caret: view.caret,
    };
    let prompt_frame = render_prompt_frame(&prompt_view, prompt_width, mount + popup_rows, theme);
    paste(&mut frame, &prompt_frame, centered(prompt_width), y - popup_rows);

    // The home.footer slot: pad · version · pad at h≥16, else one row.
    if show_footer {
        let padded = height >= 16;
        let row = if padded { height - 2 } else { height - 1 };
        let muted = cell(tokens.text.muted);
        let version_width = display_width(&view.version);
        let x = width.saturating_sub(padding + version_width);
        if version_width > 0 {
            for (index, ch) in view.version.chars().enumerate() {
                if let Some(slot) =
                    frame.cells.get_mut(row).and_then(|line| line.get_mut(x + index))
                {
                    *slot = Cell { ch, attrs: 0, fg: muted, bg: base };
                }
            }
        }
        // Wave 3's home slot: plugin quick actions on the same row's
        // left (muted, ` · `-joined) — empty by default, so the
        // golden bytes never move.
        if !view.plugin_actions.is_empty() {
            let joined = view.plugin_actions.join(" · ");
            let budget = width.saturating_sub(padding * 2 + version_width + 2);
            let text = crate::wrap::take_width(&joined, budget);
            for (index, ch) in text.chars().enumerate() {
                if let Some(slot) =
                    frame.cells.get_mut(row).and_then(|line| line.get_mut(padding + index))
                {
                    *slot = Cell { ch, attrs: 0, fg: muted, bg: base };
                }
            }
        }
    }
    frame
}

/// One logo cell: `_`/`^` paint over the shadow, `~`/`,` paint IN the
/// shadow (`Logo.renderLine`); everything else is the half's color
/// over the base. The shadow = `tint(background.base, fg, 0.25)`
/// (theme/color.ts — 0.25·fg + 0.75·base per channel, rounded).
fn paint_logo(frame: &mut Frame, origin_x: usize, origin_y: usize, theme: &ThemeState) {
    let tokens = &theme.resolved.tokens;
    let base = cell(tokens.background.base);
    let base_rgb = base_rgb_as(base);
    let muted = cell(tokens.text.muted);
    let text_base = cell(tokens.text.base);
    let shadow = |fg: CellColor| -> CellColor {
        let (red, green, blue) = base_rgb_as(fg);
        CellColor::Rgb(
            tint_channel(red, base_rgb.0),
            tint_channel(green, base_rgb.1),
            tint_channel(blue, base_rgb.2),
        )
    };
    let shadow_muted = shadow(muted);
    let shadow_base = shadow(text_base);
    for (row, (left, right)) in LOGO_LEFT.iter().zip(&LOGO_RIGHT).enumerate() {
        paint_logo_half(frame, origin_x, origin_y + row, left, muted, shadow_muted, false, base);
        // The gap cell stays untouched (the `gap={1}` box paints
        // nothing — the golden pins the frame fill there).
        paint_logo_half(
            frame,
            origin_x + 20,
            origin_y + row,
            right,
            text_base,
            shadow_base,
            true,
            base,
        );
    }
}

fn base_rgb_as(color: CellColor) -> (u8, u8, u8) {
    match color {
        CellColor::Rgb(r, g, b) => (r, g, b),
        _ => (10, 10, 10),
    }
}

fn tint_channel(channel: u8, base: u8) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (f32::from(channel) * 0.25 + f32::from(base) * 0.75).round() as u8
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_logo_half(
    frame: &mut Frame,
    x: usize,
    y: usize,
    line: &str,
    fg: CellColor,
    shadow: CellColor,
    bold: bool,
    base: CellColor,
) {
    let attrs = u16::from(bold);
    for (index, ch) in line.chars().enumerate() {
        let cell = match ch {
            '_' => Cell { ch: ' ', attrs, fg, bg: shadow },
            '^' => Cell { ch: '▀', attrs, fg, bg: shadow },
            '~' => Cell { ch: '▀', attrs, fg: shadow, bg: base },
            ',' => Cell { ch: '▄', attrs, fg: shadow, bg: base },
            other => Cell { ch: other, attrs, fg, bg: base },
        };
        if let Some(slot) = frame.cells.get_mut(y).and_then(|row| row.get_mut(x + index)) {
            *slot = cell;
        }
    }
}

fn cell(color: Rgba) -> CellColor {
    CellColor::Rgb(color.r, color.g, color.b)
}

/// Paste `region`'s cells at (`x`, `y`); anything left keeps the fill.
fn paste(target: &mut Frame, region: &Frame, x: usize, y: usize) {
    for (row, cells) in region.cells.iter().enumerate() {
        let Some(slot) = target.cells.get_mut(y + row) else { break };
        let count = cells.len().min(slot.len().saturating_sub(x));
        slot[x..x + count].clone_from_slice(&cells[..count]);
    }
}

/// Render the home route over the store: the home prompt's draft
/// (`HOME_PROMPT_KEY`'s composer, placeholder rotation pinned to entry
/// 0) and the crate version in the footer slot. The location label is
/// the pump's seam (unset until the server wires it — the footer then
/// renders the label muted like the session route).
#[must_use]
pub fn render_home(store: &Store, theme: &crate::theme::ThemeStyles, size: (u16, u16)) -> Frame {
    let state = store.theme();
    let composer = store.composer(crate::app::HOME_PROMPT_KEY);
    let placeholder = if composer.placeholders.is_empty() {
        format!(
            "Ask anything… \"{}\"",
            PLACEHOLDERS[composer.placeholder_index % PLACEHOLDERS.len()]
        )
    } else {
        composer.placeholder_text()
    };
    // The staged next-session selection (agent · model) rides the
    // composer's hint row (the prompt-idle statusline).
    let (status_agent, status_model) =
        crate::components::session::composer_statusline(store, crate::app::HOME_PROMPT_KEY);
    // The live slash-palette popup + rail + blinking caret (the golden
    // harness leaves all three absent — its HomeView defaults).
    let autocomplete = match composer.autocomplete.visible {
        Some(crate::components::composer::autocomplete::Mode::Command) => Some(AutocompleteView {
            options: &composer.autocomplete.options,
            selected: composer.autocomplete.selected,
            title_width: composer.autocomplete.registry.display_width(),
        }),
        _ => None,
    };
    let view = HomeView {
        draft: &composer.draft,
        placeholder,
        location: String::new(),
        directory: String::new(),
        location_hovered: store.location_hovered(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        status_agent,
        status_model,
        plugin_hints: store.plugin_footer_hints(),
        plugin_actions: store
            .plugin_home_actions()
            .into_iter()
            .map(|action| action.label)
            .collect(),
        autocomplete,
        rail: store.composer_rail_color(crate::app::HOME_PROMPT_KEY).map(CellColor::from),
        caret: crate::components::composer::render::cursor_blink_on(store.clock())
            .then_some(composer.caret()),
    };
    let _ = theme;
    let mut frame = render_home_view(&view, &state, size);

    // F9 — the tab strip leads the HOME route while tabs are open (v2
    // app.tsx:576-579 `tabsAvailable`: `sessionTabs.enabled() &&
    // tabs().length > 0` — the SessionTabs mount is app-wide, never
    // session-route-gated; home simply has no active pill). The
    // golden harness never opens tabs, so `home.w*` stays byte-exact.
    let tabs = store.tabs();
    if store.tabs_enabled() && !tabs.tabs.is_empty() {
        let (width, height) = (usize::from(size.0), usize::from(size.1));
        if height > 1 {
            let strip = crate::components::session::paint_live_tabs_bar(
                store,
                None,
                width,
                &state.styles(),
            );
            // Shift the home view down one row under the strip.
            let mut cells = vec![frame.cells[0].clone()];
            cells.extend(frame.cells.clone());
            cells.truncate(height);
            frame = Frame { cells, width, height };
            for (row, strip_row) in strip.cells.iter().enumerate() {
                if let Some(slot) = frame.cells.get_mut(row) {
                    slot.clone_from(strip_row);
                }
            }
        }
    }
    frame
}

use crate::store::Store;

/// Dispatch one plugin quick action's command through the palette
/// dispatcher (plugin commands route through the
/// [`crate::store::PluginCommandSink`]; builtin ids run natively).
pub fn run_plugin_quick_action(
    store: &Store,
    dispatcher: &mut dyn crate::components::command_palette::CommandDispatcher,
    index: usize,
) -> crate::components::command_palette::Dispatch {
    let Some(action) = store.plugin_home_actions().into_iter().nth(index) else {
        return crate::components::command_palette::Dispatch::Unknown;
    };
    dispatcher.dispatch(store, &action.command, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn home_renders_the_logo_and_hint() {
        let store = Store::new(Runtime::new());
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Ask anything"), "{grid}");
    }

    /// The data-driven mount: the store's home draft types into the
    /// composer and the mount grows.
    #[test]
    fn home_prompt_reads_the_store_draft() {
        let store = Store::new(Runtime::new());
        let mut composer = store.composer(crate::app::HOME_PROMPT_KEY);
        composer.draft.text = "hello".into();
        store.set_composer(crate::app::HOME_PROMPT_KEY, &composer);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (80, 30));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("hello"), "the draft typed into the home prompt: {grid}");
    }

    /// Wave 3: plugin quick actions paint on the footer row's left
    /// (and stay absent — byte-exact — when none registered).
    #[test]
    fn plugin_quick_actions_paint_on_the_footer_row() {
        let store = Store::new(Runtime::new());
        store.set_plugin_home_actions(vec![
            PluginQuickAction {
                plugin_id: "omos".into(),
                label: "/preset".into(),
                command: "omo.preset".into(),
            },
            PluginQuickAction {
                plugin_id: "omos".into(),
                label: "/killall".into(),
                command: "omo.killall".into(),
            },
        ]);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (80, 30));
        let footer = &frame.text_grid()[28];
        assert!(
            footer.trim_start().starts_with("/preset · /killall"),
            "quick actions on the footer row's left: {footer:?}"
        );
        assert!(footer.contains(env!("CARGO_PKG_VERSION")), "the version stays right-aligned");

        // Empty actions: the footer row keeps the golden shape.
        let bare = Store::new(Runtime::new());
        let frame = render_home(&bare, &theme.styles(), (80, 30));
        assert!(!frame.text_grid()[28].contains("/preset"));
    }

    /// F9 — the tab strip leads the HOME route while tabs are open (v2
    /// app.tsx:576-579 `tabsAvailable`: `enabled() && tabs().length > 0`
    /// — the `SessionTabs` mount is app-wide). No tabs (or tabs
    /// disabled) keeps the golden home shape byte-exact.
    #[test]
    fn home_mounts_the_tab_strip_while_tabs_are_open() {
        let theme = crate::theme::ThemeState::v2_dark();
        let bare = Store::new(Runtime::new());
        let bare_frame = render_home(&bare, &theme.styles(), (80, 30));
        assert!(!bare_frame.text_grid()[0].contains('+'), "no strip without tabs");

        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Alpha".to_owned()));
        store.navigate(crate::store::Route::Home);
        let frame = render_home(&store, &theme.styles(), (80, 30));
        let strip = &frame.text_grid()[0];
        assert!(strip.contains("Alpha"), "the open tab's title leads home: {strip}");
        assert!(strip.contains(" + "), "the add affordance: {strip}");
        // The composer still renders under the strip.
        assert!(
            frame.text_grid().iter().any(|row| row.contains("Ask anything")),
            "the home composer survives the strip row"
        );

        // tabs.enabled = false hides it again.
        store.set_tabs_enabled(false);
        let frame = render_home(&store, &theme.styles(), (80, 30));
        assert!(!frame.text_grid()[0].contains("Alpha"), "the strip follows the config gate");
    }
}
