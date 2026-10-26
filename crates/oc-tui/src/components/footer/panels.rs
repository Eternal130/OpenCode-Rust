//! The five footer picker panels (`mini/footer.command.tsx`):
//! command menu, model select, agent select, variant select and
//! settings — each an entry list rendered through the shared
//! [`super::panel`] shell.
//!
//! The panels' MODEL (the settings knobs, verbosity presets, the
//! entry builders and the fuzzy filter) lives in [`super::state`] —
//! extracted painter-agnostic in lane R2 step 1; this module
//! re-exports it and keeps the five painters.

#![allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools, clippy::too_many_lines)]

use super::canvas::Canvas;
use super::panel::{
    footer_panel_layout, render_panel_shell, FooterMenuItem, FooterMenuState, PANEL_LIST_ROWS,
    RUN_COMMAND_PANEL_ROWS,
};
use super::theme::FooterTheme;
use crate::render::Frame;

pub use super::state::{
    agent_entries, apply_mini_setting_change, command_menu_entries, fuzzy_matches,
    match_mini_verbosity, model_entries, resolve_mini_settings, reveal_current, settings_entries,
    variant_entries, verbosity_change, verbosity_label, MiniSettingChange, MiniSettings, RunAgent,
    RunModel, RunProvider, WORK_SPINNERS,
};

// -- panel renders ---------------------------------------------------------------

fn panel_frame_canvas(
    width: usize,
    height: usize,
    term_height: usize,
    title: &str,
    count_visible: bool,
    hint: Option<&str>,
    entries: &[FooterMenuItem],
    menu: &FooterMenuState,
    grouped: bool,
    background: bool,
    header_color: bool,
    empty: &str,
    theme: &FooterTheme,
    with_rail: bool,
) -> Canvas {
    let mut canvas = Canvas::new(width, height);
    let layout = footer_panel_layout(term_height, PANEL_LIST_ROWS);
    render_panel_shell(
        &mut canvas,
        0,
        title,
        count_visible,
        entries.iter().filter(|item| fuzzy_matches(item, "")).count(),
        entries.len(),
        "",
        "Search",
        hint,
        entries,
        menu,
        layout,
        grouped,
        background,
        header_color.then_some(theme.muted),
        empty,
        theme,
        width,
        height,
    );
    if with_rail && !layout.compact && background {
        let rail_y = if layout.compact { 2 + layout.limit } else { 5 + layout.limit };
        if rail_y < height {
            let rail = super::panel::render_bottom_rail(width, theme);
            canvas.spans(rail_y, 0, &rail);
        }
    }
    canvas
}

/// The five pickers' dispatch: the panel frame through the family's
/// solid seam (flag OFF keeps the hand-rolled bytes).
fn panel_frame_finish(canvas: Canvas) -> Frame {
    super::solid::finish(canvas)
}

/// `footer-command-menu`: static command groups. Terminal height
/// `RUN_COMMAND_PANEL_ROWS + 1` in the corpus.
#[must_use]
pub fn render_command_menu(width: usize, height: usize, theme: &FooterTheme) -> Frame {
    panel_frame_finish(command_menu_canvas(width, height, theme))
}

/// [`render_command_menu`]'s canvas (the parity tests' op source).
#[must_use]
pub(crate) fn command_menu_canvas(width: usize, height: usize, theme: &FooterTheme) -> Canvas {
    let entries =
        command_menu_entries(0, 0, 0, &["high".to_owned(), "minimal".to_owned()], "ctrl+t");
    let menu = FooterMenuState::new();
    panel_frame_canvas(
        width,
        height,
        RUN_COMMAND_PANEL_ROWS + 1,
        "Commands",
        false,
        None,
        &entries,
        &menu,
        true,
        true,
        true,
        "No results found",
        theme,
        true,
    )
}

/// `footer-model-select`: provider-grouped model list, transparent
/// background, the current model revealed + focused.
#[must_use]
pub fn render_model_select(
    providers: &[RunProvider],
    current: Option<(&str, &str)>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    panel_frame_finish(model_select_canvas(providers, current, width, height, theme))
}

/// [`render_model_select`]'s canvas (the parity tests' op source).
#[must_use]
pub(crate) fn model_select_canvas(
    providers: &[RunProvider],
    current: Option<(&str, &str)>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Canvas {
    let entries = model_entries(providers, current);
    let menu = reveal_current(&entries);
    panel_frame_canvas(
        width,
        height,
        RUN_COMMAND_PANEL_ROWS + 1,
        "Select model",
        true,
        None,
        &entries,
        &menu,
        true,
        false,
        true,
        if providers.is_empty() { "Models loading" } else { "No results found" },
        theme,
        true,
    )
}

/// `footer-agent-select`: flat agent list (height `RUN_COMMAND_PANEL_ROWS`
/// ⇒ compact in the corpus).
#[must_use]
pub fn render_agent_select(
    agents: &[RunAgent],
    current: Option<&str>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    panel_frame_finish(agent_select_canvas(agents, current, width, height, theme))
}

/// [`render_agent_select`]'s canvas (the parity tests' op source).
#[must_use]
pub(crate) fn agent_select_canvas(
    agents: &[RunAgent],
    current: Option<&str>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Canvas {
    let entries = agent_entries(agents, current);
    let menu = reveal_current(&entries);
    panel_frame_canvas(
        width,
        height,
        RUN_COMMAND_PANEL_ROWS,
        "Select agent",
        true,
        None,
        &entries,
        &menu,
        false,
        true,
        false,
        "No agents found",
        theme,
        false,
    )
}

/// `footer-variant-select`.
#[must_use]
pub fn render_variant_select(
    variants: &[String],
    current: Option<&str>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    panel_frame_finish(variant_select_canvas(variants, current, width, height, theme))
}

/// [`render_variant_select`]'s canvas (the parity tests' op source).
#[must_use]
pub(crate) fn variant_select_canvas(
    variants: &[String],
    current: Option<&str>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Canvas {
    let entries = variant_entries(variants, current);
    let menu = reveal_current(&entries);
    panel_frame_canvas(
        width,
        height,
        RUN_COMMAND_PANEL_ROWS,
        "Select variant",
        true,
        None,
        &entries,
        &menu,
        false,
        true,
        false,
        "No results found",
        theme,
        false,
    )
}

/// `footer-settings`.
#[must_use]
pub fn render_settings(
    settings: &MiniSettings,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    panel_frame_finish(settings_canvas(settings, width, height, theme))
}

/// [`render_settings`]'s canvas (the parity tests' op source).
#[must_use]
pub(crate) fn settings_canvas(
    settings: &MiniSettings,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Canvas {
    let entries = settings_entries(settings);
    let menu = FooterMenuState::new();
    panel_frame_canvas(
        width,
        height,
        RUN_COMMAND_PANEL_ROWS,
        "Settings",
        false,
        Some("left/right change"),
        &entries,
        &menu,
        true,
        true,
        true,
        "No settings found",
        theme,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> RunProvider {
        RunProvider {
            id: "opencode".into(),
            name: "opencode".into(),
            models: vec![
                RunModel {
                    id: "gpt-5".into(),
                    name: "GPT-5".into(),
                    cost_input: 1.0,
                    deprecated: false,
                },
                RunModel {
                    id: "gpt-free".into(),
                    name: "GPT Free".into(),
                    cost_input: 0.0,
                    deprecated: false,
                },
                RunModel {
                    id: "old".into(),
                    name: "Old Model".into(),
                    cost_input: 1.0,
                    deprecated: true,
                },
            ],
        }
    }

    fn agents() -> Vec<RunAgent> {
        vec![
            RunAgent {
                id: "build".into(),
                name: "Build".into(),
                description: Some("Build software".into()),
                mode: "all".into(),
                hidden: false,
            },
            RunAgent {
                id: "review".into(),
                name: "Review".into(),
                description: Some("Review changes".into()),
                mode: "primary".into(),
                hidden: false,
            },
            RunAgent {
                id: "explore".into(),
                name: "Explore".into(),
                description: None,
                mode: "subagent".into(),
                hidden: false,
            },
            RunAgent {
                id: "secret".into(),
                name: "Secret".into(),
                description: None,
                mode: "all".into(),
                hidden: true,
            },
        ]
    }

    #[test]
    fn model_entries_sort_filter_and_footers() {
        let entries = model_entries(&[provider()], Some(("opencode", "gpt-5")));
        let displays: Vec<&str> = entries.iter().map(|item| item.display.as_str()).collect();
        assert_eq!(
            displays,
            vec!["GPT Free", "GPT-5"],
            "deprecated filtered, GPT Free sorts first"
        );
        assert_eq!(entries[0].footer.as_deref(), Some("Free"));
        assert_eq!(entries[1].footer.as_deref(), Some("current"));
        assert!(entries[1].current);
    }

    #[test]
    fn agent_entries_filter_hidden_and_subagent() {
        let entries = agent_entries(&agents(), Some("review"));
        let displays: Vec<&str> = entries.iter().map(|item| item.display.as_str()).collect();
        assert_eq!(displays, vec!["build", "review"]);
        assert_eq!(entries[1].footer.as_deref(), Some("current"));
        assert_eq!(entries[0].description.as_deref(), Some("Build software"));
    }

    #[test]
    fn variant_entries_lead_with_default() {
        let entries = variant_entries(&["high".to_owned(), "minimal".to_owned()], Some("high"));
        assert_eq!(entries[0].display, "Default");
        assert!(entries[1].current);
        assert_eq!(entries[2].footer, None);
    }

    #[test]
    fn settings_defaults_match_the_golden_fixture() {
        let settings = resolve_mini_settings();
        assert_eq!(settings.thinking, "show");
        assert_eq!(settings.tools, "hide");
        assert_eq!(settings.shell_output, "hide");
        assert_eq!(settings.turn_summary, "show");
        assert_eq!(settings.work_spinner, "block-soft-slide");
        assert!(!settings.mono);
        let entries = settings_entries(&settings);
        assert_eq!(entries.len(), 9);
        assert_eq!(entries[0].footer.as_deref(), Some("Default"), "verbosity label");
        let spinner = entries.last().unwrap();
        assert_eq!(spinner.footer.as_deref(), Some("soft slide"), "block- prefix stripped");
        assert_eq!(spinner.icon, Some('▪'));
    }

    #[test]
    fn verbosity_cycles_through_presets() {
        let mut settings = resolve_mini_settings();
        assert_eq!(match_mini_verbosity(&settings), "default");
        let (key, value) = verbosity_change(&settings, 1).expect("changes to everything");
        assert_eq!((key, value), ("verbosity", "everything"));
        settings = apply_mini_setting_change(&settings, key, value);
        assert_eq!(match_mini_verbosity(&settings), "everything");
        let down = verbosity_change(&settings, -1).expect("back to default");
        assert_eq!(down.1, "default");
    }

    #[test]
    fn command_menu_entries_shape() {
        let entries = command_menu_entries(0, 0, 0, &["high".to_owned()], "ctrl+t");
        let displays: Vec<&str> = entries.iter().map(|item| item.display.as_str()).collect();
        assert!(displays.contains(&"Open editor"));
        assert!(displays.contains(&"Variant cycle"));
        assert!(displays.contains(&"Switch model variant"));
        assert_eq!(entries[0].category, "Session");
        let cycle = entries.iter().find(|item| item.display == "Variant cycle").unwrap();
        assert_eq!(cycle.footer.as_deref(), Some("ctrl+t"));
        let empty = command_menu_entries(0, 0, 0, &[], "");
        assert!(!empty.iter().any(|item| item.display == "Switch model variant"));
        let with_subagents = command_menu_entries(2, 1, 0, &[], "");
        assert!(with_subagents
            .iter()
            .any(|item| item.display == "View subagents"
                && item.footer.as_deref() == Some("1 active")));
    }

    #[test]
    fn golden_frame_shapes_at_80() {
        let theme = FooterTheme::fallback_dark();
        let settings = render_settings(&resolve_mini_settings(), 80, 16, &theme);
        let grid = settings.text_grid();
        assert!(grid[0].contains("Settings"));
        assert!(grid[0].contains("left/right change · esc"));
        assert!(grid[1].contains("Search"));
        assert!(grid[2].contains("Verbosity"));
        assert!(grid[2].contains("Default"));
        assert!(grid[10].contains("Work spinner"));
        assert!(grid[10].contains("soft slide"));

        let command = render_command_menu(80, 17, &theme);
        let grid = command.text_grid();
        assert!(grid[1].contains("Commands"));
        assert_eq!(grid[5].trim(), "Session");
        assert!(grid[6].contains("Open editor"));
        assert!(grid[6].contains("/editor"));
        assert_eq!(grid[11].trim(), "Agent");
        assert!(grid[14].contains("Variant cycle"));
        assert!(grid[14].contains("ctrl+t"));
        assert!(grid[15].starts_with('▀'), "half-block rail");

        let models =
            render_model_select(&[provider()], Some(("opencode", "gpt-5")), 80, 17, &theme);
        let grid = models.text_grid();
        assert!(grid[1].contains("Select model 2"));
        assert_eq!(grid[5].trim(), "opencode");
        assert!(grid[6].contains("GPT Free"));
        assert!(grid[6].contains("Free"));
        assert!(grid[7].contains("GPT-5"));
        assert!(grid[7].contains("current"));

        let agent_frame = render_agent_select(&agents(), Some("review"), 80, 16, &theme);
        let grid = agent_frame.text_grid();
        assert!(grid[0].contains("Select agent 2"));
        assert!(grid[2].contains("build"));
        assert!(grid[2].contains("Build software"));
        assert!(grid[3].contains("review"));
        assert!(grid[3].contains("current"));

        let variants = render_variant_select(
            &["high".to_owned(), "minimal".to_owned()],
            Some("high"),
            80,
            16,
            &theme,
        );
        let grid = variants.text_grid();
        assert!(grid[0].contains("Select variant 3"));
        assert!(grid[2].contains("Default"));
        assert!(grid[3].contains("high"));
        assert!(grid[3].contains("current"));
        assert!(grid[4].contains("minimal"));
    }

    #[test]
    fn fuzzy_filter_passes_subsequences() {
        let item = FooterMenuItem {
            display: "Switch model".into(),
            category: "Agent".into(),
            ..FooterMenuItem::default()
        };
        assert!(fuzzy_matches(&item, ""));
        assert!(fuzzy_matches(&item, "swimo"));
        assert!(!fuzzy_matches(&item, "zzz"));
    }
}
