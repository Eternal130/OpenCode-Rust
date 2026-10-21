//! Prompt-idle footer (`footer-prompt-idle` golden): the composer row
//! (agent-colored `┃` rail + transparent textarea region) and the
//! statusline (`footerStatuslinePolicy` — the staged agent · model ·
//! menu hint row).

use super::canvas::{Canvas, Span, FILL_BG};
use super::theme::FooterTheme;
use crate::render::Frame;
use crate::wrap::display_width;

/// One statusline part (`tone` maps to a palette role).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPart {
    pub text: String,
    pub tone: StatusTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Text,
    Muted,
    Agent,
    Status,
}

/// One statusline group (id + parts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusGroup {
    pub id: &'static str,
    pub parts: Vec<StatusPart>,
}

fn group(id: &'static str, text: impl Into<String>, tone: StatusTone) -> StatusGroup {
    StatusGroup { id, parts: vec![StatusPart { text: text.into(), tone }] }
}

fn action(id: &'static str, key: &str, label: &str) -> StatusGroup {
    StatusGroup {
        id,
        parts: vec![
            StatusPart { text: key.to_owned(), tone: StatusTone::Text },
            StatusPart { text: format!(" {label}"), tone: StatusTone::Muted },
        ],
    }
}

/// `footerStatuslinePolicy` inputs (the subset the idle footer uses).
#[derive(Debug, Clone, Default)]
pub struct StatuslineInput<'a> {
    pub agent: Option<&'a str>,
    pub model: Option<&'a str>,
    pub provider: Option<&'a str>,
    pub menu: Option<(&'a str, &'a str)>,
    pub status: Option<&'a str>,
}

/// `footerStatuslinePolicy`: ordered groups (agent · model · … · menu)
/// with the reference's staged drop-what-does-not-fit policy.
#[must_use]
pub fn footer_statusline_policy(input: &StatuslineInput<'_>, width: usize) -> Vec<StatusGroup> {
    let mut selected: Vec<StatusGroup> = Vec::new();
    if let Some(status) = input.status {
        selected.push(group("status", status, StatusTone::Status));
    }
    if let Some(agent) = input.agent {
        selected.push(group("agent", agent, StatusTone::Agent));
    }
    let model_name = input.model.map(str::to_owned);
    if let Some(model) = &model_name {
        let cells = model.chars().count();
        let text = if display_width(model) > cells + 1 {
            format!("{}…", crate::wrap::take_width(model, 8).trim_end())
        } else {
            model.clone()
        };
        selected.push(group("model", text, StatusTone::Text));
    }
    if let Some(provider) = input.provider {
        selected.push(group("provider", provider, StatusTone::Muted));
    }
    if let Some((key, label)) = input.menu {
        selected.push(action("menu", key, label));
    }
    // Required controls may wrap; optional information never crowds
    // the fallback — drop from the end while the line overflows.
    let text = |groups: &[StatusGroup]| -> String {
        groups
            .iter()
            .map(|item| item.parts.iter().map(|part| part.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    while display_width(&text(&selected)) > width && selected.len() > 1 {
        selected.pop();
    }
    selected
}

/// The prompt-idle state the golden pins: current agent, the state
/// model label, and the command-palette shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptIdleState {
    pub agent: String,
    pub model_label: String,
    pub command_shortcut: String,
    /// TUI-17 — the statusline's status detail (the `N subagents`
    /// group, footer.view.tsx:247); EMPTY renders nothing.
    pub status_detail: String,
}

impl Default for PromptIdleState {
    fn default() -> Self {
        Self {
            agent: "Build".into(),
            model_label: "gpt-5".into(),
            command_shortcut: "ctrl+p".into(),
            status_detail: String::new(),
        }
    }
}

/// Render the idle prompt footer (`footer-prompt-idle`): the composer
/// row (rail + padding 1 + transparent textarea + padding 2) and the
/// statusline row, top-aligned like the reference dump.
#[must_use]
pub fn render_prompt_idle(
    state: &PromptIdleState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    let mut canvas = Canvas::new(width, height);
    // Composer row: ┃ rail (agent color), paddingLeft 1, the
    // transparent textarea region, paddingRight 2.
    if height > 0 {
        canvas.spans(0, 0, &[Span::new("┃", theme.agent_color(0), FILL_BG)]);
        if width > 1 {
            canvas.fill_bg(0, 1, 1, FILL_BG);
        }
        if width > 4 {
            canvas.clear_bg(0, 2, width - 4);
        }
    }
    // Statusline: agent · model · shortcut menu.
    if height > 1 {
        let groups = footer_statusline_policy(
            &StatuslineInput {
                agent: Some(&state.agent),
                model: Some(&state.model_label),
                menu: Some((state.command_shortcut.as_str(), "menu")),
                status: (!state.status_detail.is_empty()).then_some(state.status_detail.as_str()),
                provider: None,
            },
            width,
        );
        let mut spans: Vec<Span> = Vec::new();
        for (index, item) in groups.iter().enumerate() {
            if index > 0 {
                spans.push(Span::new(" · ", theme.muted, FILL_BG));
            }
            for part in &item.parts {
                let fg = match part.tone {
                    StatusTone::Text | StatusTone::Status => theme.text,
                    StatusTone::Muted => theme.muted,
                    StatusTone::Agent => theme.agent_color(0),
                };
                spans.push(Span::new(part.text.clone(), fg, FILL_BG));
            }
        }
        canvas.spans(1, 0, &spans);
    }
    canvas.into_frame()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statusline_policy_orders_agent_model_menu() {
        let groups = footer_statusline_policy(
            &StatuslineInput {
                agent: Some("Build"),
                model: Some("gpt-5"),
                menu: Some(("ctrl+p", "menu")),
                ..StatuslineInput::default()
            },
            80,
        );
        let text: String = groups
            .iter()
            .map(|item| item.parts.iter().map(|part| part.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join(" · ");
        assert_eq!(text, "Build · gpt-5 · ctrl+p menu");
        let ids: Vec<&str> = groups.iter().map(|item| item.id).collect();
        assert_eq!(ids, vec!["agent", "model", "menu"]);
    }

    #[test]
    fn golden_frame_shape_at_80() {
        let theme = FooterTheme::fallback_dark();
        let frame = render_prompt_idle(&PromptIdleState::default(), 80, 12, &theme);
        let grid = frame.text_grid();
        assert!(grid[0].starts_with('┃'), "composer rail");
        assert_eq!(grid[1], format!("Build · gpt-5 · ctrl+p menu{}", " ".repeat(53)));
        // The textarea region is transparent (canonical bg 49).
        assert_eq!(frame.cell(0, 5).bg, crate::render::CellColor::Default);
        assert_eq!(frame.cell(0, 0).fg, theme.agent_color(0));
        assert_eq!(frame.cell(0, 78).bg, FILL_BG, "paddingRight 2 painted");
    }

    #[test]
    fn golden_frame_shape_at_160() {
        let theme = FooterTheme::fallback_dark();
        let frame = render_prompt_idle(&PromptIdleState::default(), 160, 12, &theme);
        let grid = frame.text_grid();
        assert!(grid[1].starts_with("Build · gpt-5 · ctrl+p menu"));
        assert_eq!(frame.cell(0, 156).bg, crate::render::CellColor::Default);
        assert_eq!(frame.cell(0, 158).bg, FILL_BG);
        assert_eq!(frame.cell(0, 0).ch, '┃');
        assert_eq!(frame.cell(11, 79).ch, ' ');
    }
}
