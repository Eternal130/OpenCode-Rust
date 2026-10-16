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

use crate::components::composer::render::AutocompleteView;
use crate::components::composer::PromptDraft;
use crate::render::{CellColor, Frame};
use crate::theme::ThemeState;
use crate::wrap::display_width;

/// The home placeholder rotation (`routes/home.tsx` `placeholder.normal`)
/// — the reference picks its entry at RANDOM per boot (`randomIndex`,
/// prompt/index.tsx:103-106; the initial pick at :364, re-rolled on
/// session change :407-415 and shell-mode entry :977).
pub const PLACEHOLDERS: &[&str] =
    &["Fix a TODO in the codebase", "What is the tech stack of this project?", "Fix broken tests"];

/// The shell-mode pool (`placeholder.shell`, home.tsx:22) — the `!`
/// composer's `Run a command… "…"` rotation source.
pub const SHELL_PLACEHOLDERS: &[&str] = &["ls -la", "git status", "pwd"];

/// `randomIndex` (prompt/index.tsx:103-106): a uniform pick over
/// `count` — `Math.floor(Math.random() * count)`; 0 for an empty
/// pool. Seeded from the clock + pid + a stack address (no rand
/// dependency; uniform enough for a 3-entry pool).
#[must_use]
pub fn random_placeholder_index(count: usize) -> usize {
    if count <= 1 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| (elapsed.as_secs() ^ u64::from(elapsed.subsec_nanos())) << 11);
    let seed = nanos
        ^ (u64::from(std::process::id()) << 32)
        // ASLR entropy — two processes launched together differ.
        ^ (std::ptr::addr_of!(count) as u64);
    // xorshift64* (Marsaglia): full period, uniform low bits.
    let mut state = seed | 1;
    state ^= state >> 12;
    state ^= state << 25;
    state ^= state >> 27;
    let rolled = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
    #[allow(clippy::cast_possible_truncation)]
    let pick = (rolled % count as u64) as usize;
    pick
}

/// The wordmark art (`logo.ts`) — 19-cell halves joined by a 1-cell
/// gap (`gap={1}`): `_` renders as a space over the shadow, `^` as `▀`
/// over the shadow, `~` as `▀` IN the shadow, `,` as `▄` in the shadow.
pub(crate) const LOGO_LEFT: [&str; 4] =
    ["                   ", "█▀▀█ █▀▀█ █▀▀█ █▀▀▄", "█__█ █__█ █^^^ █__█", "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀~~▀"];
pub(crate) const LOGO_RIGHT: [&str; 4] =
    ["             ▄     ", "█▀▀▀ █▀▀█ █▀▀█ █▀▀█", "█___ █__█ █__█ █^^^", "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"];

/// The logo's full width: left 19 + gap 1 + right 19.
pub(crate) const LOGO_WIDTH: usize = 19 + 1 + 19;

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
    /// The composer metadata row (#5): the staged agent · model line
    /// (`PromptMetadataRow`) — empty in every pinned golden state.
    pub metadata: crate::components::composer::render::MetadataLine,
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

/// The home route's geometry plan — ONE resolution shared by the old
/// painter ([`render_home_view`]'s body) and the solid bridge (lane
/// M5's [`crate::components::home_solid`]), the M3 "extract the
/// geometry, don't mirror it" rule: the footer's collapse ladder and
/// the centering half-up rule live here exactly once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomePlan {
    /// The outer column's horizontal padding (1 below 44 cols).
    pub padding: usize,
    /// The footer slot renders (height ≥ 12 && width ≥ 44).
    pub show_footer: bool,
    /// The footer content row's absolute row.
    pub footer_row: usize,
    /// The version renders (width ≥ 64 — `homeFooterVisibility`).
    pub version_visible: bool,
    /// The version's right-aligned x (padding-adjusted).
    pub version_x: usize,
    /// The pre-truncated plugin-actions label ("" = none).
    pub actions: String,
    /// The prompt box's clamped width (`maxWidth 75`).
    pub prompt_width: usize,
    /// The prompt mount's height (paddingTop 1 + the composer).
    pub mount: usize,
    /// The popup's reserved rows above the mount.
    pub popup_rows: usize,
    /// The logo block's top-left (absent ⇒ hidden).
    pub logo: Option<(usize, usize)>,
    /// The prompt frame's paste origin.
    pub paste: (usize, usize),
}

/// Resolve the plan — the exact walk `render_home_view` performed
/// inline (golden-pinned): the vertical centering rounds the free
/// space HALF UP (`free.div_ceil(2)` — taffy's half-pixel rule, the
/// D finding), the footer collapses its content row when the version
/// is width-gated off and no plugin action claims it.
pub(crate) fn plan_home(view: &HomeView, width: usize, height: usize) -> HomePlan {
    let padding = if width < 44 { 1 } else { 2 };
    let content = width.saturating_sub(padding * 2);
    // Centered offset (taffy rounds the half pixel up — golden-pinned).
    let centered = |item_width: usize| padding + content.saturating_sub(item_width).div_ceil(2);

    let show_logo = height >= 12;
    let show_footer = height >= 12 && width >= 44;
    // The footer slot's height (`feature-plugins/home/footer.tsx`):
    // paddingY 1/1 at h≥16 (none below), one content row — which
    // COLLAPSES when nothing in it is visible. The version hides
    // below 64 cols (`homeFooterVisibility`, footer.tsx:6-11) and a
    // row whose only child is the flexGrow spacer has zero height,
    // so a quiet ≤63-col footer is just its two padding rows.
    // Rig-pinned at 60x20: the 2-row footer leaves free = 3 and the
    // wordmark centers a row HIGHER than a 3-row footer allows
    // (corpus #19 — our old always-3 model sat it one row low).
    let footer_content = width >= 64 || !view.plugin_actions.is_empty();
    let footer_rows = if !show_footer {
        0
    } else if height < 16 {
        usize::from(footer_content)
    } else if footer_content {
        3
    } else {
        2
    };
    // The prompt mount: paddingTop 1 + the composer's own height.
    let prompt_width = content.clamp(1, PROMPT_MAX_WIDTH);
    let mount =
        1 + crate::components::composer::render::mount_height(view.draft, prompt_width, height);
    let fixed = if show_logo { 3 + 4 + 1 } else { 0 } + mount;
    let free = height.saturating_sub(footer_rows + fixed);
    let spacer_top = free.div_ceil(2);
    let mut y = spacer_top;
    let logo = show_logo.then(|| {
        y += 3; // the fixed 3-row spacer (`height={3}`)
        let origin = (centered(LOGO_WIDTH), y);
        y += 4 + 1; // logo + the 1-row spacer
        origin
    });
    // The slash-palette popup floats ABOVE the mount: the prompt
    // frame grows by `popup_rows` rows (the composer at
    // `top = 1 + popup_rows`), pasted at `y - popup_rows` so the
    // composer itself never moves (no popup ⇒ the golden
    // single-mount geometry).
    let popup_rows = match view.autocomplete.as_ref() {
        Some(popup) => crate::components::composer::autocomplete::popup_height(
            popup.options.len(),
            y.saturating_sub(1),
        )
        .min(y),
        None => 0,
    };

    // The footer content row: pad·row·pad at h≥16, else the last row.
    let footer_padded = height >= 16;
    let footer_row =
        if footer_padded { height.saturating_sub(2) } else { height.saturating_sub(1) };
    let version_visible = width >= 64;
    let version_width = if version_visible { display_width(&view.version) } else { 0 };
    let version_x = width.saturating_sub(padding + version_width);
    let actions = if view.plugin_actions.is_empty() {
        String::new()
    } else {
        let joined = view.plugin_actions.join(" · ");
        let budget = width.saturating_sub(padding * 2 + version_width + 2);
        crate::wrap::take_width(&joined, budget)
    };

    HomePlan {
        padding,
        show_footer,
        footer_row,
        version_visible,
        version_x,
        actions,
        prompt_width,
        mount,
        popup_rows,
        logo,
        paste: (centered(prompt_width), y - popup_rows),
    }
}

/// Render the home route at `size` (the golden-pinned layout).
/// Lane M5 (retired): the opentui-solid tree is the ONLY renderer —
/// the goldens pin its bytes (byte-identical to the retired painter's
/// since the lane's parity proof); both consumed the ONE [`plan_home`]
/// geometry, which stays.
#[must_use]
pub fn render_home_view(
    view: &HomeView,
    theme: &ThemeState,
    size: (u16, u16),
) -> (Frame, Option<(usize, usize)>) {
    // Lane M5 (retired): the home route renders through its
    // opentui-solid tree — the only renderer since the retirement
    // lane deleted the hand-rolled painter (both consumed the ONE
    // `plan_home` geometry, which stays).
    crate::components::home_solid::paint_solid_home_view(view, theme, size)
}

/// Paste `region` into `target` at `(x, y)` (the shared paste the
/// solid bridge reuses for the prompt mount).
pub(crate) fn paste(target: &mut Frame, region: &Frame, x: usize, y: usize) {
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
    // composer's METADATA row (#5 — `PromptMetadataRow`, not the
    // footer: the footer's left slot is the location label, #4).
    let metadata =
        crate::components::session::composer_metadata(store, crate::app::HOME_PROMPT_KEY);
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
        // #4: home shows where the next session will be created —
        // the launch cwd (`currentLocation.ref ??
        // data.location.default()`, prompt/index.tsx:1605-1610).
        location: store.footer_location(),
        directory: String::new(),
        location_hovered: store.location_hovered(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        metadata,
        plugin_hints: store.plugin_footer_hints(),
        plugin_actions: store
            .plugin_home_actions()
            .into_iter()
            .map(|action| action.label)
            .collect(),
        autocomplete,
        rail: store.composer_rail_color(crate::app::HOME_PROMPT_KEY).map(CellColor::from),
        // NEW-S1a: the caret's char offset — the terminal cursor
        // positions at the computed cell (never a painted block).
        caret: Some(composer.caret()),
    };
    let _ = theme;
    // F9 — the tab strip leads the HOME route while tabs are open (v2
    // app.tsx:576-579 `tabsAvailable`: `sessionTabs.enabled() &&
    // tabs().length > 0` — the SessionTabs mount is app-wide, never
    // session-route-gated; home simply has no active pill). The
    // golden harness never opens tabs, so `home.w*` stays byte-exact.
    //
    // v2 lays the home content out INSIDE the remaining height: the
    // strip is row 0 of the same column (`app.tsx:1349-1361` seats
    // `<SessionTabs/>` directly above the route `<Switch>`), so Home's
    // own layout sees `height - 1` and the caret rides the shifted
    // subtree. The dogfood2 repro caught this port computing the
    // caret at the UNSHIFTED row (one row above the composer) and
    // clipping the bottom status row (full-height layout + truncate):
    // render the sub-frame at height-1, prepend the strip row, then
    // set the caret cell shifted by the strip's one row.
    let tabs = store.tabs();
    let strip_shows = store.tabs_enabled() && !tabs.tabs.is_empty() && usize::from(size.1) > 1;
    let sub_size = (
        size.0,
        if strip_shows { size.1 - 1 } else { size.1 },
    );
    let (mut frame, caret_screen) = render_home_view(&view, &state, sub_size);
    if strip_shows {
        let (width, height) = (usize::from(size.0), usize::from(size.1));
        let strip = crate::components::session::paint_live_tabs_bar(
            store,
            None,
            width,
            &state.styles(),
        );
        // Prepend the strip's row (height 1); the sub-frame already
        // fits height-1 beneath it — nothing truncates.
        let mut cells = strip.cells.clone();
        cells.extend(frame.cells.clone());
        cells.truncate(height);
        frame = Frame { cells, width, height };
    }
    store.set_caret_cell(caret_screen.map(|(x, y)| {
        let y = y + usize::from(strip_shows);
        (u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0))
    }));
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

    /// #6: the pool contents match v2's `routes/home.tsx:20-23`
    /// (normal + shell), and the per-boot rotation picks a uniform
    /// in-bounds entry (`randomIndex`, prompt/index.tsx:103-106).
    #[test]
    fn placeholder_pool_matches_and_rotation_stays_in_bounds() {
        assert_eq!(
            PLACEHOLDERS,
            &[
                "Fix a TODO in the codebase",
                "What is the tech stack of this project?",
                "Fix broken tests"
            ]
        );
        assert_eq!(SHELL_PLACEHOLDERS, &["ls -la", "git status", "pwd"]);
        assert_eq!(random_placeholder_index(0), 0, "empty pool");
        assert_eq!(random_placeholder_index(1), 0, "single entry");
        // The roll: every index reachable across many draws, none out
        // of bounds.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..64 {
            let pick = random_placeholder_index(PLACEHOLDERS.len());
            assert!(pick < PLACEHOLDERS.len());
            seen.insert(pick);
        }
        assert_eq!(seen.len(), PLACEHOLDERS.len(), "every entry reachable");
        // The rendered home placeholder always draws from the pool.
        for (pick, entry) in PLACEHOLDERS.iter().enumerate() {
            let store = Store::new(Runtime::new());
            let mut composer = store.composer(crate::app::HOME_PROMPT_KEY);
            composer.placeholder_index = pick;
            store.set_composer(crate::app::HOME_PROMPT_KEY, &composer);
            let theme = crate::theme::ThemeState::v2_dark();
            let frame = render_home(&store, &theme.styles(), (80, 30));
            let grid = frame.text_grid().join("\n");
            assert!(grid.contains(entry), "pick {pick}: {grid}");
        }
    }

    /// NEW-S1a: the live home route positions the terminal cursor at
    /// the composer's caret cell (never painting a block).
    #[test]
    fn home_route_positions_the_terminal_cursor() {
        let store = Store::new(Runtime::new());
        let mut composer = store.composer(crate::app::HOME_PROMPT_KEY);
        composer.draft.text = "hi".into();
        store.set_composer(crate::app::HOME_PROMPT_KEY, &composer);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (80, 30));
        let (x, y) = store.caret_cell().expect("caret cell recorded");
        let cell = frame.cell(usize::from(y), usize::from(x));
        assert_eq!(cell.ch, 'h', "the caret blinks over the char it precedes");
        // No block-painted caret anywhere on the composer row (the
        // draft text stays unpainted-under).
        let row: String = frame.text_grid()[usize::from(y)].clone();
        assert!(row.contains("hi"), "draft text at the caret row: {row}");
    }

    #[test]
    fn home_renders_the_logo_and_hint() {
        let store = Store::new(Runtime::new());
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Ask anything"), "{grid}");
    }

    /// Corpus #19 — the 60x20 wordmark row. The footer slot's version
    /// hides below 64 cols and its content row collapses (2-row
    /// footer), so the column's free space is 3 and the logo block
    /// starts at row 5 (rig-pinned: `resize` shrink step). Our old
    /// always-3 footer model mis-centered it (corpus #19: "ref block
    /// starts one row higher than ours").
    #[test]
    fn home_sixty_by_twenty_centers_the_wordmark_at_row_five() {
        let store = Store::new(Runtime::new());
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (60, 20));
        let rows = frame.text_grid();
        // The logo's top shadow cell (▄) opens the 4-row block.
        let logo_row = rows.iter().position(|row| row.contains('▄')).expect("logo rendered");
        assert_eq!(logo_row, 5, "wordmark block starts at row 5:\n{}", rows.join("\n"));
        // The collapsed footer: NO version anywhere (hidden <64 cols).
        assert!(
            !rows.iter().any(|row| row.contains(env!("CARGO_PKG_VERSION"))),
            "version hidden below 64 cols: {}",
            rows.join("\n")
        );
        // At 80x24 the version shows and the footer is 3 rows (the
        // golden shape — the collapse must be width-gated).
        let frame = render_home(&store, &theme.styles(), (80, 24));
        let rows = frame.text_grid();
        let logo_row = rows.iter().position(|row| row.contains('▄')).expect("logo rendered");
        assert_eq!(logo_row, 6, "80x24 stays at row 6");
        assert!(
            rows[22].trim_end().ends_with(env!("CARGO_PKG_VERSION")),
            "version visible at ≥64 cols: {:?}",
            rows[22]
        );
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

    /// dogfood2 bug 2 — the caret and the layout under the strip: v2
    /// seats `<SessionTabs/>` in the SAME column as the route content
    /// (app.tsx:1349-1361), so Home lays out in `height - 1`, the
    /// composer's hardware cursor rides the SHIFTED row, and the
    /// bottom status row never clips. The port once computed the caret
    /// from the pre-shift layout (one row above the input line, exactly
    /// the strip height) and rendered full-height then truncated.
    #[test]
    fn home_caret_sits_on_the_composer_row_under_the_strip() {
        let theme = crate::theme::ThemeState::v2_dark();

        // Without tabs the golden home shape: caret on the input row.
        let bare = Store::new(Runtime::new());
        let bare_frame = render_home(&bare, &theme.styles(), (80, 24));
        let bare_grid = bare_frame.text_grid();
        let bare_input = bare_grid
            .iter()
            .position(|row| row.contains("Ask anything"))
            .expect("the bare home renders the input line");
        let bare_caret = bare.caret_cell().expect("the bare home sets the caret");
        assert_eq!(usize::from(bare_caret.1), bare_input, "no strip: caret on the input row");

        // With a restored tab the strip takes row 0 and the whole view
        // (caret included) shifts down one row — still ON the input row.
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Alpha".to_owned()));
        store.navigate(crate::store::Route::Home);
        let frame = render_home(&store, &theme.styles(), (80, 24));
        let grid = frame.text_grid();
        assert!(grid[0].contains("Alpha"), "the strip leads home");
        let input = grid
            .iter()
            .position(|row| row.contains("Ask anything"))
            .expect("the shifted home renders the input line");
        assert_eq!(input, bare_input + 1, "the home content shifts by the strip's one row");
        let caret = store.caret_cell().expect("the strip home sets the caret");
        assert_eq!(
            usize::from(caret.1),
            input,
            "the hardware cursor sits ON the composer's input row (dogfood2 bug 2)"
        );
        assert!(usize::from(caret.1) > 0, "never on the strip row");
        // height-1 layout: nothing clips — the footer's version line
        // still paints on the frame's last row region.
        assert!(
            grid.iter().any(|row| row.contains(env!("CARGO_PKG_VERSION"))),
            "the bottom status row survives the strip (no full-height truncation)"
        );
    }
}
