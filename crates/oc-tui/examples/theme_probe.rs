//! Probe: dump the strip-relevant theme tokens (temp lane tool).
fn main() {
    let theme = oc_tui::theme::ThemeState::v2_dark();
    let styles = theme.styles();
    println!("styles.background={:?} raised={:?} prompt={:?}", styles.background, styles.background_raised, styles.background_prompt);
}
