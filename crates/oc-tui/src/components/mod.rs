//! TUI components. [`session_tabs`] is golden-matched (text grid);
//! [`home`], [`session`], [`palette`] and [`tool_rows`] are the 7.1
//! foundation surfaces.

pub mod home;
pub mod palette;
pub mod session;
pub mod session_tabs;
pub mod tool_rows;

pub use session_tabs::{
    adaptive_session_tab_layout, render_bar, render_rail, AdaptiveLayout, Attention, TabStatus,
    TabView,
};
pub use tool_rows::{
    fill_style, layout_tool_wrap, render_tool_wrap, row_style, tool_wrap_fixture, ToolRowFixture,
    BOX_WIDTH, ICON_WIDTH,
};
