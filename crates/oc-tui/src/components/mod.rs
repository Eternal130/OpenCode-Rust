//! TUI components. [`session_tabs`] is golden-matched (text grid);
//! [`home`], [`session`] and [`tool_rows`] are the 7.1 foundation
//! surfaces; [`dialog`] + [`dialog_select`] + [`command_palette`] are
//! the 7.2 wave-1 dialog stack (framework, fuzzy picker, palette).

pub mod command_palette;
pub mod dialog;
pub mod dialog_select;
pub mod footer;
pub mod home;
pub mod message;
pub mod session;
pub mod session_tabs;
pub mod toast;
pub mod tool_rows;

pub use session_tabs::{
    adaptive_session_tab_layout, render_bar, render_rail, AdaptiveLayout, Attention, TabStatus,
    TabView,
};
pub use tool_rows::{
    fill_style, layout_tool_wrap, render_tool_wrap, row_style, tool_wrap_fixture, ToolRowFixture,
    BOX_WIDTH, ICON_WIDTH,
};
