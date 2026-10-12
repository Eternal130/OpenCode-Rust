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
pub mod dialog;
pub mod dialog_select;
pub mod dialog_worktrees;
pub mod diff_viewer;
pub mod footer;
pub mod home;
pub mod message;
pub mod session;
pub mod session_screen;
pub mod session_tabs;
pub mod sidebar;
pub mod syntax;
pub mod tab_pulse;
pub mod toast;
pub mod tool_rows;

pub use session_screen::{
    composer_bottom_chrome, paint_session_screen, paint_tabs_bar, ComposerSlot, ScreenTab,
    SessionScreen,
};
pub use session_tabs::{
    adaptive_session_tab_layout, render_bar, render_rail, AdaptiveLayout, Attention, TabStatus,
    TabView,
};
pub use sidebar::{
    render_sidebar, sidebar_visible, SidebarMode, SidebarSections, SidebarView,
    SIDEBAR_AUTO_BREAKPOINT,
};
pub use tool_rows::{
    fill_style, layout_tool_wrap, render_tool_wrap, row_style, tool_wrap_fixture, ToolRowFixture,
    BOX_WIDTH, ICON_WIDTH,
};
