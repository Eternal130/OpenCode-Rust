//! Probe: dump the fade-row cells (temp lane tool).
use oc_tui::components::session_tabs::{TabStatus, TabView};
use oc_tui::store::SessionTab;

fn main() {
    let theme = oc_tui::theme::ThemeState::v2_dark();
    let styles = theme.styles();
    let title = "A".repeat(39);
    let tabs = vec![TabView {
        tab: SessionTab { session_id: "only".into(), title: Some(title) },
        status: TabStatus::EMPTY,
        detail: None,
    }];
    let frame =
        oc_tui::components::session_tabs_solid::paint_solid_bar(&tabs, Some("only"), true, 40, &styles, None, None, false);
    for col in 0..40 {
        let cell = frame.cell(0, col);
        println!("col {col:2} ch {:?} fg {:?} attrs {}", cell.ch, cell.fg, cell.attrs);
    }
}
