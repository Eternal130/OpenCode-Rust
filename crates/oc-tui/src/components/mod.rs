//! TUI components. [`session_tabs`] is golden-matched (text grid);
//! [`home`], [`session`] and [`tool_rows`] are the 7.1 foundation
//! surfaces; [`dialog`] + [`dialog_select`] + [`command_palette`] are
//! the 7.2 wave-1 dialog stack (framework, fuzzy picker, palette);
//! [`message`] is the wave-1 transcript with wave-2 window/follow
//! seam; [`sidebar`] + [`session_screen`] + [`diff_viewer`] are the
//! 7.2 wave-2 assembly surfaces (sidebar pane, session route, VCS
//! diff viewer).

pub mod command_palette;
pub mod composer;
/// Lane M3: the composer + footer on opentui-solid (the
/// `OC_COMPOSER_OPENTUI` flag selects it; the hand-rolled renderer
/// stays the fallback — see the module doc + the M3 section of
/// `docs/migration-m1-tabstrip.md`).
pub mod composer_solid;
pub mod dialog;
pub mod dialog_config;
pub mod dialog_open;
pub mod dialog_select;
/// Lane M2: the dialog family on opentui-solid — the DEFAULT
/// renderer (flipped on after the rig proof; `OC_DIALOGS_OPENTUI=0`
/// is the hand-rolled fallback for one release — see the module doc
/// + the M2 section of `docs/migration-m1-tabstrip.md`).
pub mod dialog_solid;
pub mod dialog_worktrees;
pub mod diff_viewer;
pub mod diff_viewer_mount;
pub mod footer;
pub mod home;
/// Lane M5: the home route chrome on opentui-solid (the
/// `OC_HOME_OPENTUI` flag selects it; the hand-rolled painter stays
/// the fallback — see the module doc + the M5 section of
/// `docs/migration-m1-tabstrip.md`).
pub mod home_solid;
pub mod marquee;
pub mod message;
pub mod overlays;
/// Lane M6 (retired): the composer's subagent picker panel on
/// opentui-solid — the ONLY renderer since the retirement lane
/// deleted the `OC_SUBAGENT_OPENTUI` flag pair.
pub mod picker_solid;
/// PLUG-TUI-SLOT: the plugin route page — the native full-screen
/// page a plugin's registered route renders into.
pub mod plugin_page;
/// PLUG-TUI-SLOT: the plugin UI-slot router — the ref's pure slot
/// tree resolution (`packages/tui/src/plugin/structure.ts`) over the
/// TUI's native mounted paths.
pub mod plugin_slots;
pub mod session;
pub mod session_screen;
pub mod session_tabs;
/// TUI-3: the vertical session-tab rail + the compact rail controls
/// (TD-25) — `component/session-tabs.tsx`'s `VerticalSessionTabs` +
/// `session-tabs-rail.tsx`.
pub mod session_tabs_rail;
/// Lane M1: the tab strip on opentui-solid — the DEFAULT renderer
/// (the hand-rolled one is the `OC_TABS_OPENTUI=0` fallback for one
/// release; see the module doc + `docs/migration-m1-tabstrip.md`).
pub mod session_tabs_solid;
pub mod sidebar;
/// Lane M5: the session sidebar pane on opentui-solid (the
/// `OC_SIDEBAR_OPENTUI` flag selects it; the hand-rolled renderer
/// stays the fallback — see the module doc + the M5 section of
/// `docs/migration-m1-tabstrip.md`).
pub mod sidebar_solid;
pub mod subagent_picker;
pub mod syntax;
pub mod tab_pulse;
pub mod toast;
pub mod tool_rows;
/// Lane M4: the transcript (message list) on opentui-solid — the
/// DEFAULT renderer (the hand-rolled region walk is the
/// `OC_TRANSCRIPT_OPENTUI=0` fallback for one release; see the
/// module doc + `docs/migration-m1-tabstrip.md`).
pub mod transcript_solid;
pub mod update;

pub use session_screen::{
    composer_bottom_chrome, paint_session_screen, paint_tabs_bar, ComposerSlot, ScreenTab,
    SessionScreen,
};
pub use session_tabs::{
    adaptive_session_tab_layout, AdaptiveLayout, Attention, TabStatus, TabView,
};
pub use sidebar::{
    render_sidebar, sidebar_visible, SidebarMode, SidebarSections, SidebarView,
    SIDEBAR_AUTO_BREAKPOINT,
};
pub use tool_rows::{
    fill_style, layout_tool_wrap, render_tool_wrap, row_style, tool_wrap_fixture, ToolRowFixture,
    BOX_WIDTH, ICON_WIDTH,
};
