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

fn action(id: &'static str, key: &str, label: &str, expanded: Option<&str>) -> StatusGroup {
    // `action()` (footer.width.ts:46-52): the key in `text` + the
    // label (or its expanded form) in `muted`.
    let label = expanded.unwrap_or(label);
    StatusGroup {
        id,
        parts: vec![
            StatusPart { text: key.to_owned(), tone: StatusTone::Text },
            StatusPart { text: format!(" {label}"), tone: StatusTone::Muted },
        ],
    }
}

/// One work hint (`input.work` — footer.width.ts:32 /
/// footer.view.tsx:456-472): the queued/subagents/background action
/// rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusWorkHint<'a> {
    /// `queued` | `subagents` | `background`.
    pub id: &'static str,
    pub key: &'a str,
    pub label: &'a str,
    pub expanded: Option<&'a str>,
}

/// The model group's inputs (`input.model` — footer.width.ts:33).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusModel<'a> {
    pub name: &'a str,
    pub variant: Option<&'a str>,
}

/// `footerStatuslinePolicy` inputs (footer.width.ts:27-40).
#[derive(Debug, Clone, Default)]
pub struct StatuslineInput<'a> {
    pub mono: bool,
    pub spinner: Option<&'a str>,
    /// The status text + its expanded stage form (footer.view.tsx:
    /// 485-488); EMPTY text renders nothing.
    pub status: Option<(&'a str, Option<&'a str>)>,
    pub escape: Option<(&'a str, &'a str)>,
    pub work: Vec<StatusWorkHint<'a>>,
    pub model: Option<StatusModel<'a>>,
    pub agent: Option<&'a str>,
    /// The context group's compact/full forms (footer.view.tsx:
    /// 493-499 — `N% ctx` / `formatContextUsage`).
    pub context: Option<(&'a str, &'a str)>,
    pub cost: Option<&'a str>,
    pub provider: Option<&'a str>,
    pub menu: Option<(&'a str, &'a str)>,
}

/// `place()`'s group order (footer.width.ts:66-79) — agent, model,
/// CONTEXT, PROVIDER, cost, menu (G-481: the port had provider and
/// context swapped).
const GROUP_ORDER: [&str; 12] = [
    "spinner",
    "status",
    "escape",
    "queued",
    "subagents",
    "background",
    "agent",
    "model",
    "context",
    "provider",
    "cost",
    "menu",
];

/// `identity(cells)` (footer.width.ts:53-59): the model name clipped
/// to `cells` (with the ellipsis) + the ` [variant]` suffix.
fn identity(input: &StatuslineInput<'_>, cells: usize) -> StatusGroup {
    let model = input.model.as_ref().expect("identity called with a model");
    let ellipsis = if input.mono { "..." } else { "…" };
    // `identity(Infinity)` (footer.width.ts:109) never clips.
    let text = if cells != usize::MAX && display_width(model.name) > cells + display_width(ellipsis)
    {
        format!("{}{ellipsis}", crate::wrap::take_width(model.name, cells).trim_end())
    } else {
        model.name.to_owned()
    };
    let text = match model.variant {
        Some(variant) if !variant.is_empty() => format!("{text} [{variant}]"),
        _ => text,
    };
    group("model", text, StatusTone::Text)
}

/// `footerStatuslinePolicy` (footer.width.ts:27-120): the ordered
/// groups with the reference's staged widen-until-it-does-not-fit
/// policy — each stage RETAINS the earlier groups (G-481: replaces
/// the port's push-all-then-pop-from-the-end), stopping at the first
/// non-fitting stage so resizing stays stable (:97-98).
#[must_use]
pub fn footer_statusline_policy(input: &StatuslineInput<'_>, width: usize) -> Vec<StatusGroup> {
    let mut slots: Vec<Option<StatusGroup>> = vec![None; GROUP_ORDER.len()];
    let set = |slots: &mut Vec<Option<StatusGroup>>, group: StatusGroup| {
        let index = GROUP_ORDER.iter().position(|id| *id == group.id).expect("known group id");
        slots[index] = Some(group);
    };
    // The required controls (the always-selected base, :61-63) — they
    // may wrap; optional information never crowds the fallback (:94).
    if let Some(spinner) = input.spinner {
        set(&mut slots, group("spinner", spinner, StatusTone::Text));
    }
    if let Some((status, _)) = input.status {
        if !status.is_empty() {
            set(&mut slots, group("status", status, StatusTone::Status));
        }
    }
    if let Some((key, label)) = input.escape {
        set(&mut slots, action("escape", key, label, None));
    }
    // `place()` (:65-92): the ordered groups joined by ` · ` (` - `
    // mono), a single space after the spinner group.
    let place = |slots: &[Option<StatusGroup>]| -> Vec<StatusGroup> {
        slots.iter().flatten().cloned().collect()
    };
    let text_of = |groups: &[StatusGroup]| -> String {
        // The separator (:87-88): a single space after the spinner
        // group, else ` · ` (` - ` mono).
        let mut text = String::new();
        for (index, item) in groups.iter().enumerate() {
            if index > 0 {
                if groups[index - 1].id == "spinner" {
                    text.push(' ');
                } else {
                    text.push_str(if input.mono { " - " } else { " · " });
                }
            }
            for part in &item.parts {
                text.push_str(&part.text);
            }
        }
        text
    };
    let fits = |groups: &[StatusGroup]| -> bool {
        let text = text_of(groups);
        display_width(&text) <= width && !text.contains('\n')
    };
    let mut layout = place(&slots);
    if !fits(&layout) {
        // Required controls may wrap — return the base as-is (:95).
        return layout;
    }
    // The staged backfill (:99-112): each stage overwrites its own
    // group's earlier (narrower) form; stop at the first non-fitting
    // stage and KEEP the last fitting layout (:113-118).
    let mut stages: Vec<StatusGroup> = Vec::new();
    for item in &input.work {
        stages.push(action(item.id, item.key, item.label, None));
    }
    if input.model.is_some() {
        stages.push(identity(input, 8));
    }
    if let Some(agent) = input.agent {
        stages.push(group("agent", agent, StatusTone::Agent));
    }
    if let Some((compact, _)) = input.context {
        stages.push(group("context", compact, StatusTone::Muted));
    }
    if input.model.is_some() {
        stages.push(identity(input, 24));
    }
    if let Some((_, full)) = input.context {
        stages.push(group("context", full, StatusTone::Muted));
    }
    if let Some(cost) = input.cost {
        stages.push(group("cost", cost, StatusTone::Muted));
    }
    if let Some(provider) = input.provider {
        stages.push(group("provider", provider, StatusTone::Muted));
    }
    if let Some((key, label)) = input.menu {
        stages.push(action("menu", key, label, None));
    }
    if input.model.is_some() {
        stages.push(identity(input, usize::MAX));
    }
    for item in &input.work {
        if item.expanded.is_some() {
            stages.push(action(item.id, item.key, item.label, item.expanded));
        }
    }
    if let Some((_, Some(expanded))) = input.status {
        stages.push(group("status", expanded, StatusTone::Status));
    }
    for stage in stages {
        set(&mut slots, stage);
        let next = place(&slots);
        if !fits(&next) {
            break;
        }
        layout = next;
    }
    layout
}

/// One owned work hint (the mini footer's queued/subagents rows —
/// `contextHintCandidates`, footer.view.tsx:456-472).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkHintState {
    /// `queued` | `subagents` | `background`.
    pub id: &'static str,
    pub key: String,
    pub label: String,
    /// EMPTY = no expanded stage.
    pub expanded: String,
}

/// The prompt-idle state the golden pins: current agent, the state
/// model label, and the command-palette shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptIdleState {
    pub agent: String,
    pub model_label: String,
    /// G-481 — the model group's ` [variant]` suffix (EMPTY = none,
    /// footer.width.ts:58).
    pub model_variant: String,
    pub command_shortcut: String,
    /// TUI-17 — the statusline's status detail (the status group's
    /// text, footer.view.tsx:485-488); EMPTY renders nothing.
    pub status_detail: String,
    /// G-481 — the work action groups (queued/subagents/background,
    /// footer.view.tsx:456-472) replacing the merged status text.
    pub work: Vec<WorkHintState>,
    /// G-272 — the context-usage group (`18 (0%)`,
    /// `formatContextUsage`); EMPTY renders nothing.
    pub context: String,
    /// G-272 — the cost group (`money.format`, footer.view.tsx:218);
    /// EMPTY renders nothing.
    pub cost: String,
    /// G-481 — the provider group (`info?.provider`,
    /// footer.view.tsx:501); EMPTY renders nothing.
    pub provider: String,
    /// G-481 — the mono render mode (`props.mono` swaps the
    /// separator to ` - ` and the ellipsis to `...`,
    /// footer.width.ts:55/:81).
    pub mono: bool,
}

impl Default for PromptIdleState {
    fn default() -> Self {
        Self {
            agent: "Build".into(),
            model_label: "gpt-5".into(),
            model_variant: String::new(),
            command_shortcut: "ctrl+p".into(),
            status_detail: String::new(),
            work: Vec::new(),
            context: String::new(),
            cost: String::new(),
            provider: String::new(),
            mono: false,
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
    super::solid::finish(&prompt_idle_canvas(state, width, height, theme))
}

/// [`render_prompt_idle`]'s canvas (the parity tests' op source).
pub(crate) fn prompt_idle_canvas(
    state: &PromptIdleState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Canvas {
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
    // Statusline (footer.width.ts's policy): the staged group row.
    if height > 1 {
        let work: Vec<StatusWorkHint<'_>> = state
            .work
            .iter()
            .map(|hint| StatusWorkHint {
                id: hint.id,
                key: hint.key.as_str(),
                label: hint.label.as_str(),
                expanded: (!hint.expanded.is_empty()).then_some(hint.expanded.as_str()),
            })
            .collect();
        let groups = footer_statusline_policy(
            &StatuslineInput {
                agent: Some(&state.agent),
                model: Some(StatusModel {
                    name: &state.model_label,
                    variant: (!state.model_variant.is_empty())
                        .then_some(state.model_variant.as_str()),
                }),
                menu: Some((state.command_shortcut.as_str(), "menu")),
                status: Some((
                    state.status_detail.as_str(),
                    // The status group's expanded stage (the busy
                    // `esc interrupt` form) is owned by the busy
                    // render path, not the idle golden.
                    None,
                )),
                work,
                context: (!state.context.is_empty())
                    .then_some((state.context.as_str(), state.context.as_str())),
                cost: (!state.cost.is_empty()).then_some(state.cost.as_str()),
                provider: (!state.provider.is_empty()).then_some(state.provider.as_str()),
                mono: state.mono,
                ..StatuslineInput::default()
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
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(groups: &[StatusGroup]) -> String {
        groups
            .iter()
            .map(|item| item.parts.iter().map(|part| part.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    #[test]
    fn statusline_policy_orders_agent_model_menu() {
        let groups = footer_statusline_policy(
            &StatuslineInput {
                agent: Some("Build"),
                model: Some(StatusModel { name: "gpt-5", variant: None }),
                menu: Some(("ctrl+p", "menu")),
                ..StatuslineInput::default()
            },
            80,
        );
        assert_eq!(text_of(&groups), "Build · gpt-5 · ctrl+p menu");
        let ids: Vec<&str> = groups.iter().map(|item| item.id).collect();
        assert_eq!(ids, vec!["agent", "model", "menu"]);
    }

    /// G-481 — the group order carries context BEFORE provider
    /// (footer.width.ts:66-79; the port had them swapped).
    #[test]
    fn statusline_policy_orders_context_before_provider() {
        let groups = footer_statusline_policy(
            &StatuslineInput {
                agent: Some("Build"),
                model: Some(StatusModel { name: "gpt-5", variant: None }),
                context: Some(("18% ctx", "18 (18%)")),
                provider: Some("openai"),
                cost: Some("$0.42"),
                menu: Some(("ctrl+p", "menu")),
                ..StatuslineInput::default()
            },
            120,
        );
        let ids: Vec<&str> = groups.iter().map(|item| item.id).collect();
        assert_eq!(ids, vec!["agent", "model", "context", "provider", "cost", "menu"]);
        assert_eq!(text_of(&groups), "Build · gpt-5 · 18 (18%) · openai · $0.42 · ctrl+p menu");
    }

    /// G-481 — the staged fit keeps earlier groups at narrower widths
    /// (each stage retains; stop at the first non-fitting stage —
    /// footer.width.ts:97-118), replacing pop-from-the-end.
    #[test]
    fn statusline_policy_stages_widen_with_width() {
        let input = |width: usize| {
            footer_statusline_policy(
                &StatuslineInput {
                    agent: Some("Build"),
                    model: Some(StatusModel { name: "claude-opus-4-6", variant: Some("beta") }),
                    context: Some(("42% ctx", "142K (42%)")),
                    cost: Some("$1.20"),
                    menu: Some(("ctrl+p", "menu")),
                    ..StatuslineInput::default()
                },
                width,
            )
        };
        // Narrow: the early stages (model@8 + agent) fit, the later
        // ones stop at the first non-fitting stage.
        let narrow = input(24);
        let text = text_of(&narrow);
        assert!(display_width(&text) <= 24, "the fit holds: {text}");
        assert!(text.contains("claude-o…"), "the model clips to 8 cells: {text}");
        assert!(text.contains("Build"), "the agent stage survives: {text}");
        assert!(!text.contains("menu"), "the later menu stage never backfills: {text}");
        // Wide: everything fits, model at full width + ` [variant]`.
        let wide = input(120);
        let text = text_of(&wide);
        assert!(text.contains("claude-opus-4-6 [beta]"), "the identity(∞) stage: {text}");
        assert!(text.ends_with("ctrl+p menu"));
    }

    /// G-481 — the work hints render as keyed action groups
    /// (`contextHintCandidates`, footer.view.tsx:456-472) with the
    /// expanded stage at wide widths.
    #[test]
    fn statusline_policy_renders_work_hints() {
        let groups = footer_statusline_policy(
            &StatuslineInput {
                work: vec![
                    StatusWorkHint {
                        id: "queued",
                        key: "ctrl+x q",
                        label: "2 pending",
                        expanded: None,
                    },
                    StatusWorkHint {
                        id: "subagents",
                        key: "↓",
                        label: "1 sub",
                        expanded: Some("1 subagent"),
                    },
                    StatusWorkHint {
                        id: "background",
                        key: "ctrl+b",
                        label: "bg",
                        expanded: Some("background"),
                    },
                ],
                agent: Some("Build"),
                model: Some(StatusModel { name: "gpt-5", variant: None }),
                menu: Some(("ctrl+p", "menu")),
                ..StatuslineInput::default()
            },
            90,
        );
        let text = text_of(&groups);
        assert!(text.contains("ctrl+x q 2 pending"), "the queued action: {text}");
        assert!(text.contains("↓ 1 subagent"), "the expanded subagents form: {text}");
        assert!(text.contains("ctrl+b background"), "the expanded background form: {text}");
        // The order (footer.width.ts:66-79): work before agent.
        let ids: Vec<&str> = groups.iter().map(|item| item.id).collect();
        assert_eq!(ids[..3].to_vec(), vec!["queued", "subagents", "background"]);
        // G-481 — the staged stop: at 80 the background expansion
        // does not fit and the LAST FITTING layout keeps the short
        // `ctrl+b bg` form (stop at the first non-fitting stage,
        // footer.width.ts:116-117).
        let groups = footer_statusline_policy(
            &StatuslineInput {
                work: vec![
                    StatusWorkHint {
                        id: "queued",
                        key: "ctrl+x q",
                        label: "2 pending",
                        expanded: None,
                    },
                    StatusWorkHint {
                        id: "subagents",
                        key: "↓",
                        label: "1 sub",
                        expanded: Some("1 subagent"),
                    },
                    StatusWorkHint {
                        id: "background",
                        key: "ctrl+b",
                        label: "bg",
                        expanded: Some("background"),
                    },
                ],
                agent: Some("Build"),
                model: Some(StatusModel { name: "gpt-5", variant: None }),
                menu: Some(("ctrl+p", "menu")),
                ..StatuslineInput::default()
            },
            80,
        );
        let text = text_of(&groups);
        assert!(text.contains("ctrl+b bg"), "the short form survives: {text}");
        assert!(!text.contains("ctrl+b background"), "the expansion stopped: {text}");
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
