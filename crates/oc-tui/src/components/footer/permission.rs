//! Permission body (`mini/footer.permission.tsx`,
//! `permission.shared.ts`, `util/permission.ts`): the three-stage ask
//! (permission, always, reject), the Allow once / Always allow /
//! Reject affordances, the free-text [`RejectField`], and the request
//! presentation (per-action icon/title/lines).

#![allow(clippy::too_many_lines, clippy::match_same_arms)]

use super::canvas::{Canvas, Span, FILL_FG};
use super::theme::FooterTheme;
use crate::render::Frame;
use serde::{Deserialize, Serialize};

// Pure ports of the reference state machine keep its shape (long pure
// functions, match arms that intentionally repeat); list-style doc
// comments mirror the upstream header.

/// `MiniPermissionRequest` (the subset the footer consumes; the tool
/// state indirection stays with the message-flow lane — `input` /
/// `metadata` arrive flattened).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub action: String,
    #[serde(default)]
    pub resources: Vec<String>,
    /// Save patterns: non-empty ⇒ the "Always allow" option appears.
    #[serde(default)]
    pub save: Vec<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub input: serde_json::Value,
}

/// `PermissionReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReply {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

/// `PermissionPresentation`: icon / title / lines per action
/// (`util/permission.ts`, the footer subset — diff/patch rendering
/// stays with the diff lane).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionPresentation {
    pub icon: &'static str,
    pub title: String,
    pub lines: Vec<String>,
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(serde_json::Value::as_str).unwrap_or("").to_owned()
}

/// `canonicalToolName` (the alias pairs the footer needs).
fn canonical_tool_name(action: &str) -> &str {
    match action {
        "bash" | "shell" => "shell",
        "str_replace" | "edit" => "edit",
        "read_file" | "read" => "read",
        "list" | "ls" => "list",
        "glob" => "glob",
        "grep" | "rg" => "grep",
        "subagent" | "task" | "spawn_agent" => "subagent",
        "webfetch" | "fetch" => "webfetch",
        "websearch" => "websearch",
        "lsp" => "lsp",
        "external_directory" => "external_directory",
        "doom_loop" => "doom_loop",
        other => other,
    }
}

/// `permissionPresentation` for a request.
#[must_use]
pub fn permission_presentation(request: &PermissionRequest) -> PermissionPresentation {
    let action = canonical_tool_name(&request.action);
    let input = request.input.as_object().cloned().unwrap_or_default();
    let value = |key: &str| {
        input
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
    };
    let path = value("path").or_else(|| value("filePath")).or_else(|| value("filepath"));
    let resource = request.resources.first().cloned().unwrap_or_default();

    match action {
        "edit" => {
            let file = path.clone().filter(|s| !s.is_empty()).unwrap_or(resource);
            PermissionPresentation { icon: "→", title: format!("Edit {file}"), lines: Vec::new() }
        }
        "read" | "list" => {
            let value = path.filter(|s| !s.is_empty()).unwrap_or(resource);
            let title = if action == "read" { "Read" } else { "List" };
            PermissionPresentation {
                icon: "→",
                title: format!("{title} {value}"),
                lines: if value.is_empty() { Vec::new() } else { vec![format!("Path: {value}")] },
            }
        }
        "glob" | "grep" => {
            let pattern = value("pattern").filter(|s| !s.is_empty()).unwrap_or(resource);
            let title = if action == "glob" { "Glob" } else { "Grep" };
            PermissionPresentation {
                icon: "✱",
                title: format!("{title} \"{pattern}\""),
                lines: if pattern.is_empty() {
                    Vec::new()
                } else {
                    vec![format!("Pattern: {pattern}")]
                },
            }
        }
        "shell" => {
            let command = value("command").unwrap_or_default();
            PermissionPresentation {
                icon: "#",
                title: "Shell command".into(),
                lines: if command.is_empty() {
                    request.resources.iter().map(|item| format!("- {item}")).collect()
                } else {
                    vec![format!("$ {command}")]
                },
            }
        }
        "subagent" => {
            let agent =
                value("agent").filter(|s| !s.is_empty()).unwrap_or_else(|| "general".into());
            let description = value("description").filter(|s| !s.is_empty());
            PermissionPresentation {
                icon: "#",
                title: format!("{} Subagent", titlecase(&agent)),
                lines: match description {
                    Some(description) => vec![format!("◉ {description}")],
                    None => Vec::new(),
                },
            }
        }
        "webfetch" => {
            let url = value("url")
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let url = text(&request.metadata, "url");
                    (!url.is_empty()).then_some(url)
                })
                .unwrap_or_default();
            PermissionPresentation {
                icon: "%",
                title: format!("WebFetch {url}"),
                lines: if url.is_empty() { Vec::new() } else { vec![format!("URL: {url}")] },
            }
        }
        "websearch" => {
            let query = value("query")
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let query = text(&request.metadata, "query");
                    (!query.is_empty()).then_some(query)
                })
                .unwrap_or_default();
            PermissionPresentation {
                icon: "◈",
                title: if query.is_empty() {
                    "Web search".into()
                } else {
                    format!("Web search \"{query}\"")
                },
                lines: if query.is_empty() { Vec::new() } else { vec![format!("Query: {query}")] },
            }
        }
        "lsp" => {
            let file = path.unwrap_or_default();
            let operation =
                value("operation").filter(|s| !s.is_empty()).unwrap_or_else(|| "request".into());
            PermissionPresentation {
                icon: "→",
                title: format!("LSP {operation} {file}").trim_end().to_owned(),
                lines: vec![format!("Operation: {operation}"), format!("Path: {file}")],
            }
        }
        "external_directory" => {
            let parent = text(&request.metadata, "parentDir");
            let filepath = text(&request.metadata, "filepath");
            let raw = if !parent.is_empty() {
                parent
            } else if !filepath.is_empty() {
                filepath
            } else {
                resource
            };
            PermissionPresentation {
                icon: "←",
                title: format!("Access external directory {raw}"),
                lines: request.resources.iter().map(|item| format!("- {item}")).collect(),
            }
        }
        "doom_loop" => PermissionPresentation {
            icon: "⟳",
            title: "Continue after repeated failures".into(),
            lines: vec!["This keeps the session running despite repeated failures.".into()],
        },
        other => PermissionPresentation {
            icon: "⚙",
            title: format!("Call tool {other}"),
            lines: vec![format!("Tool: {other}")],
        },
    }
}

fn titlecase(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `permissionAlwaysLines`.
#[must_use]
pub fn permission_always_lines(action: &str, save: &[String]) -> Vec<String> {
    if save.len() == 1 && save[0] == "*" {
        return vec![format!("This will always allow {action} for this project.")];
    }
    let mut lines =
        vec!["This will always allow the following patterns for this project.".to_owned()];
    lines.extend(save.iter().map(|item| format!("- {item}")));
    lines
}

/// `permissionOptionLabel`.
#[must_use]
pub fn permission_option_label(option: PermissionOption) -> &'static str {
    match option {
        PermissionOption::Once => "Allow once",
        PermissionOption::Always => "Always allow",
        PermissionOption::Reject => "Reject",
        PermissionOption::Confirm => "Confirm",
        PermissionOption::Cancel => "Cancel",
    }
}

/// `PermissionStage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionStage {
    #[default]
    Permission,
    Always,
    Reject,
}

/// `PermissionOption`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOption {
    Once,
    Always,
    Reject,
    Confirm,
    Cancel,
}

/// `PermissionBodyState`.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionBodyState {
    pub request_id: String,
    pub session_id: String,
    pub stage: PermissionStage,
    pub selected: PermissionOption,
    pub message: String,
    pub submitting: bool,
}

/// `createPermissionBodyState`.
#[must_use]
pub fn create_permission_body_state(request: &PermissionRequest) -> PermissionBodyState {
    PermissionBodyState {
        request_id: request.id.clone(),
        session_id: request.session_id.clone(),
        stage: PermissionStage::Permission,
        selected: PermissionOption::Once,
        message: String::new(),
        submitting: false,
    }
}

/// `permissionOptions(stage)`.
#[must_use]
pub fn permission_options(stage: PermissionStage) -> Vec<PermissionOption> {
    match stage {
        PermissionStage::Permission => {
            vec![PermissionOption::Once, PermissionOption::Always, PermissionOption::Reject]
        }
        PermissionStage::Always => vec![PermissionOption::Confirm, PermissionOption::Cancel],
        PermissionStage::Reject => Vec::new(),
    }
}

/// The options actually offered: "always" requires save patterns
/// (`opts` in `RunPermissionBody`).
#[must_use]
pub fn permission_offered_options(
    stage: PermissionStage,
    request: &PermissionRequest,
) -> Vec<PermissionOption> {
    permission_options(stage)
        .into_iter()
        .filter(|option| *option != PermissionOption::Always || !request.save.is_empty())
        .collect()
}

/// `permissionShift`.
#[must_use]
pub fn permission_shift(
    state: &PermissionBodyState,
    dir: i8,
    list: &[PermissionOption],
) -> PermissionBodyState {
    if list.is_empty() {
        return state.clone();
    }
    let idx = list.iter().position(|option| *option == state.selected).unwrap_or(0);
    let next = (isize::try_from(idx).unwrap_or(0))
        .saturating_add(isize::from(dir))
        .rem_euclid(isize::try_from(list.len()).unwrap_or(1));
    let next = usize::try_from(next).unwrap_or(0);
    PermissionBodyState { selected: list[next], ..state.clone() }
}

/// One resolved step: the next state and/or the reply to send
/// (`PermissionStep`).
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionStep {
    pub state: PermissionBodyState,
    pub reply: Option<PermissionReply>,
}

/// `permissionRun`.
#[must_use]
pub fn permission_run(state: &PermissionBodyState, option: PermissionOption) -> PermissionStep {
    if state.submitting {
        return PermissionStep { state: state.clone(), reply: None };
    }
    let reply = |decision: &str, message: &str| PermissionReply {
        session_id: state.session_id.clone(),
        request_id: state.request_id.clone(),
        decision: decision.to_owned(),
        message: message.trim().to_owned(),
    };
    match state.stage {
        PermissionStage::Permission => match option {
            PermissionOption::Always => PermissionStep {
                state: PermissionBodyState {
                    stage: PermissionStage::Always,
                    selected: PermissionOption::Confirm,
                    ..state.clone()
                },
                reply: None,
            },
            PermissionOption::Reject => PermissionStep {
                state: PermissionBodyState {
                    stage: PermissionStage::Reject,
                    selected: PermissionOption::Reject,
                    ..state.clone()
                },
                reply: None,
            },
            _ => PermissionStep { state: state.clone(), reply: Some(reply("once", "")) },
        },
        PermissionStage::Always => {
            if option == PermissionOption::Cancel {
                return PermissionStep {
                    state: PermissionBodyState {
                        stage: PermissionStage::Permission,
                        selected: PermissionOption::Always,
                        ..state.clone()
                    },
                    reply: None,
                };
            }
            PermissionStep { state: state.clone(), reply: Some(reply("always", "")) }
        }
        PermissionStage::Reject => PermissionStep { state: state.clone(), reply: None },
    }
}

/// `permissionReject` — the reject stage's confirm (message included).
#[must_use]
pub fn permission_reject(state: &PermissionBodyState) -> Option<PermissionReply> {
    if state.submitting {
        return None;
    }
    Some(PermissionReply {
        session_id: state.session_id.clone(),
        request_id: state.request_id.clone(),
        decision: "reject".into(),
        message: state.message.trim().to_owned(),
    })
}

/// `permissionCancel` — the reject stage's esc (back to the ask,
/// selection on Reject).
#[must_use]
pub fn permission_cancel(state: &PermissionBodyState) -> PermissionBodyState {
    PermissionBodyState {
        stage: PermissionStage::Permission,
        selected: PermissionOption::Reject,
        ..state.clone()
    }
}

/// `permissionEscape` — from "always" back to the ask (selection on
/// Always); from the ask into the reject stage.
#[must_use]
pub fn permission_escape(state: &PermissionBodyState) -> PermissionBodyState {
    if state.stage == PermissionStage::Always {
        return PermissionBodyState {
            stage: PermissionStage::Permission,
            selected: PermissionOption::Always,
            ..state.clone()
        };
    }
    PermissionBodyState {
        stage: PermissionStage::Reject,
        selected: PermissionOption::Reject,
        ..state.clone()
    }
}

/// A key event outcome for the permission body.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionKey {
    /// State changed (re-render).
    State(PermissionBodyState),
    /// A reply should be sent (state already advanced where needed).
    Reply(PermissionReply, PermissionBodyState),
    /// The reject field consumed the key as text input.
    Text(char),
    /// Key not handled.
    Ignored,
}

/// The `useKeyboard` handler (+ the reject field's textarea semantics):
/// left/right/tab shift, return runs, escape rejects/opens the reject
/// stage; inside the reject stage printable keys edit the message.
#[must_use]
pub fn permission_key(
    state: &PermissionBodyState,
    request: &PermissionRequest,
    key: &str,
) -> PermissionKey {
    let offered = permission_offered_options(state.stage, request);
    if state.stage == PermissionStage::Reject {
        if key == "escape" {
            return PermissionKey::State(permission_cancel(state));
        }
        if key == "enter" || key == "return" {
            if let Some(reply) = permission_reject(state) {
                return PermissionKey::Reply(reply, state.clone());
            }
        }
        let mut chars = key.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            if !ch.is_control() {
                return PermissionKey::Text(ch);
            }
        }
        if key == "backspace" {
            return PermissionKey::Text('\u{8}');
        }
        return PermissionKey::Ignored;
    }
    match key {
        "left" | "h" => PermissionKey::State(permission_shift(state, -1, &offered)),
        "right" | "l" => PermissionKey::State(permission_shift(state, 1, &offered)),
        "tab" => PermissionKey::State(permission_shift(state, 1, &offered)),
        "enter" | "return" => {
            let step = permission_run(state, state.selected);
            match step.reply {
                Some(reply) => PermissionKey::Reply(reply, step.state),
                None => PermissionKey::State(step.state),
            }
        }
        "escape" => PermissionKey::State(permission_escape(state)),
        _ => PermissionKey::Ignored,
    }
}

/// Apply a [`PermissionKey::Text`] edit to the reject message
/// (backspace = `\u{8}`).
#[must_use]
pub fn permission_type_message(state: &PermissionBodyState, ch: char) -> PermissionBodyState {
    let mut message = state.message.clone();
    if ch == '\u{8}' {
        message.pop();
    } else {
        message.push(ch);
    }
    PermissionBodyState { message, ..state.clone() }
}

// -- rendering ---------------------------------------------------------------

const PERMISSION_ICON: &str = "△";

fn permission_title(stage: PermissionStage, width: usize) -> &'static str {
    if stage == PermissionStage::Always {
        return "Always allow";
    }
    if stage == PermissionStage::Reject {
        return if width < 24 { "Reject" } else { "Reject permission" };
    }
    if width < 24 {
        "Permission"
    } else {
        "Permission required"
    }
}

/// Render the permission body (`footer-permission` golden): one spacer
/// row, the surface panel (header / scroll region with the █ scrollbar
/// track / pane controls row), one spacer row.
#[must_use]
pub fn render_permission_body(
    request: &PermissionRequest,
    state: &PermissionBodyState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    let mut canvas = Canvas::new(width, height);
    let compact = width < 56 || height < 12;
    if height < 2 {
        return canvas.into_frame();
    }
    // The body fills everything below the one-row spacer above it:
    // header (3) · content (viewport rows + paddingBottom) · controls (3).
    let panel_end = height - 1; // inclusive
    for y in 1..=panel_end {
        canvas.fill_bg(y, 0, width, theme.surface);
    }
    // Header: pad(1) + title + pad(1).
    let title = permission_title(state.stage, width);
    canvas.spans(
        2,
        2,
        &[
            Span::new(format!("{PERMISSION_ICON} "), theme.permission, theme.surface),
            Span::new(title, theme.text, theme.surface),
        ],
    );
    // Content region: the scroll viewport rows (with the █ track).
    let content_start = 4;
    let content_end = panel_end.saturating_sub(4); // last viewport row
                                                   // Scrollbar track along the region's right edge (paddingRight 3).
    let track_x = width.saturating_sub(4);
    for y in content_start..=content_end {
        canvas.spans(y, track_x, &[Span::new("█", theme.line, theme.surface)]);
    }
    let text_x = 2;
    match state.stage {
        PermissionStage::Permission => {
            let info = permission_presentation(request);
            canvas.spans(
                content_start,
                text_x + 1,
                &[
                    Span::new(format!("{} ", info.icon), theme.muted, theme.surface),
                    Span::new(info.title.clone(), theme.text, theme.surface),
                ],
            );
            for (index, line) in info.lines.iter().enumerate() {
                canvas.spans(
                    content_start + 2 + index,
                    text_x,
                    &[Span::new(line.clone(), theme.text, theme.surface)],
                );
            }
        }
        PermissionStage::Always => {
            for (index, line) in
                permission_always_lines(&request.action, &request.save).iter().enumerate()
            {
                canvas.spans(
                    content_start + index,
                    text_x,
                    &[Span::new(line.clone(), theme.text, theme.surface)],
                );
            }
        }
        PermissionStage::Reject => {
            canvas.spans(
                content_start,
                text_x,
                &[Span::new("Tell OpenCode what to do differently", theme.muted, theme.surface)],
            );
        }
    }
    // Controls rows on the pane background (pad(1) · actions · pad(1)).
    let controls_y = panel_end - 1;
    for y in (panel_end - 2)..=panel_end {
        canvas.fill_bg(y, 0, width, theme.pane);
    }
    render_permission_controls(&mut canvas, controls_y, request, state, width, theme, compact);
    canvas.into_frame()
}

/// The controls row: buttons left (padding 2), hints right-aligned
/// before paddingRight 3.
fn render_permission_controls(
    canvas: &mut Canvas,
    y: usize,
    request: &PermissionRequest,
    state: &PermissionBodyState,
    width: usize,
    theme: &FooterTheme,
    compact: bool,
) {
    let options = permission_offered_options(state.stage, request);
    let mut x = 2;
    for (index, option) in options.iter().enumerate() {
        let label = permission_option_label(*option);
        let selected = *option == state.selected;
        let bg = if selected { theme.action_focused_bg } else { theme.pane };
        if index > 0 {
            canvas.fill_bg(y, x, 1, theme.pane);
            x += 1;
        }
        canvas.fill_bg(y, x, label.chars().count() + 2, bg);
        let fg = if selected { theme.action_focused_text } else { theme.action_secondary_text };
        canvas.spans(y, x + 1, &[Span::new(label, fg, bg)]);
        x += label.chars().count() + 2;
    }
    if state.stage == PermissionStage::Reject {
        let spans = vec![
            Span::new("enter ", theme.text, theme.pane),
            Span::new("confirm", theme.muted, theme.pane),
            Span::new("  ", FILL_FG, theme.pane),
            Span::new("esc ", theme.text, theme.pane),
            Span::new("cancel", theme.muted, theme.pane),
        ];
        canvas.spans_right(y, width - 4, &spans);
        return;
    }
    let hint: Vec<Span> = if compact && width < 56 {
        vec![
            Span::new("pgup/pgdn", theme.text, theme.pane),
            Span::new(" scroll", theme.muted, theme.pane),
        ]
    } else {
        vec![
            Span::new("⇆", theme.text, theme.pane),
            Span::new(" select  ", theme.muted, theme.pane),
            Span::new("enter", theme.text, theme.pane),
            Span::new(" confirm  ", theme.muted, theme.pane),
            Span::new("esc", theme.text, theme.pane),
            Span::new(
                if state.stage == PermissionStage::Always { " cancel" } else { " reject" },
                theme.muted,
                theme.pane,
            ),
        ]
    };
    canvas.spans_right(y, width - 4, &hint);
}

// -- the pointer surface (lane/if item 3, auditA #4) -------------------------

/// The permission body's pointer targets, at the SAME body-local
/// coordinates the canvas paints (`render_permission_controls`'s
/// layout): the offered option pills (stages Permission/Always) and,
/// in the reject stage, the `enter confirm` / `esc cancel` hint
/// BUTTONS (v2 renders both as `onMouseUp` boxes — permission.tsx:
/// 375-400). The caller translates by the mount origin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionPointerHits {
    /// The controls row (body-local y).
    pub row: usize,
    /// The offered pills left-to-right: the option + its rect.
    pub pills: Vec<(PermissionOption, (usize, usize))>,
    /// The reject stage's confirm button rect (`enter confirm`) —
    /// `(x, width)`, body-local.
    pub confirm: Option<(usize, usize)>,
    /// The reject stage's cancel button rect (`esc cancel`).
    pub cancel: Option<(usize, usize)>,
}

/// The controls row's pointer layout — mirrors
/// [`render_permission_controls`]: pills from x=2, each
/// `label.chars().count() + 2` wide with a 1-column gap (index > 0);
/// the reject-stage hints right-align as buttons ending at
/// `width - 4` (the row's paddingRight 3 + the canvas's 1-col rim).
#[must_use]
pub fn permission_pointer_hits(
    request: &PermissionRequest,
    state: &PermissionBodyState,
    width: usize,
    height: usize,
) -> PermissionPointerHits {
    let mut hits = PermissionPointerHits { row: height.saturating_sub(2), ..Default::default() };
    if height < 2 {
        return PermissionPointerHits::default();
    }
    let options = permission_offered_options(state.stage, request);
    let mut x = 2usize;
    for (index, option) in options.iter().enumerate() {
        let label = permission_option_label(*option);
        let pill_width = label.chars().count() + 2;
        if index > 0 {
            x += 1; // the inter-pill gap
        }
        hits.pills.push((*option, (x, pill_width)));
        x += pill_width;
    }
    if state.stage == PermissionStage::Reject {
        // The right-aligned hint block: `enter confirm  esc cancel`
        // (25 columns) ending at width - 4; each BUTTON is its own
        // label's extent (v2's two sibling boxes).
        let block = "enter confirm".chars().count() + 2 + "esc cancel".chars().count();
        let start = (width.saturating_sub(4)).saturating_sub(block);
        let confirm_width = "enter confirm".chars().count();
        let cancel_width = "esc cancel".chars().count();
        hits.confirm = Some((start, confirm_width));
        hits.cancel = Some((start + confirm_width + 2, cancel_width));
    }
    hits
}

/// Render the standalone [`RejectField`] (`permission-reject-field`
/// golden): a full-width focused textarea whose placeholder is the
/// muted prompt over the formfield focused background.
#[must_use]
pub fn render_reject_field(
    placeholder_visible: bool,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    let mut canvas = Canvas::new(width, height);
    if height == 0 {
        return canvas.into_frame();
    }
    canvas.fill_bg(0, 0, width, theme.formfield_focused_bg);
    if placeholder_visible {
        canvas.spans(
            0,
            0,
            &[Span::new(
                "Tell OpenCode what to do differently",
                theme.muted,
                theme.formfield_focused_bg,
            )],
        );
    }
    canvas.into_frame()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap::display_width;

    fn request(action: &str, resources: &[&str]) -> PermissionRequest {
        PermissionRequest {
            id: "per_dump".into(),
            session_id: "ses_dump".into(),
            action: action.into(),
            resources: resources.iter().map(|item| (*item).to_owned()).collect(),
            ..PermissionRequest::default()
        }
    }

    #[test]
    fn read_presentation_matches_the_golden_fixture() {
        let request = request("read", &["src/index.ts"]);
        let info = permission_presentation(&request);
        assert_eq!(info.icon, "→");
        assert_eq!(info.title, "Read src/index.ts");
        assert_eq!(info.lines, vec!["Path: src/index.ts".to_owned()]);
    }

    #[test]
    fn presentation_covers_the_action_family() {
        let grep = request("grep", &["pattern.*x"]);
        assert_eq!(permission_presentation(&grep).icon, "✱");
        assert_eq!(permission_presentation(&grep).title, "Grep \"pattern.*x\"");
        let shell = PermissionRequest {
            input: serde_json::json!({ "command": "ls" }),
            ..request("bash", &[])
        };
        assert_eq!(permission_presentation(&shell).lines, vec!["$ ls".to_owned()]);
        assert_eq!(
            permission_presentation(&request("unknown_tool", &[])).title,
            "Call tool unknown_tool"
        );
    }

    #[test]
    fn always_option_requires_save_patterns() {
        let mut request = request("edit", &["a.ts"]);
        assert_eq!(
            permission_offered_options(PermissionStage::Permission, &request),
            vec![PermissionOption::Once, PermissionOption::Reject]
        );
        request.save = vec!["*".into()];
        assert_eq!(
            permission_offered_options(PermissionStage::Permission, &request),
            vec![PermissionOption::Once, PermissionOption::Always, PermissionOption::Reject]
        );
    }

    #[test]
    fn shift_wraps_the_offered_options() {
        let request = request("read", &[]);
        let state = create_permission_body_state(&request);
        let offered = permission_offered_options(PermissionStage::Permission, &request);
        let right = permission_shift(&state, 1, &offered);
        assert_eq!(right.selected, PermissionOption::Reject);
        let wrap = permission_shift(&right, 1, &offered);
        assert_eq!(wrap.selected, PermissionOption::Once);
    }

    #[test]
    fn run_walks_the_three_stages() {
        let mut request = request("read", &[]);
        request.save = vec!["src/**".into()];
        let state = create_permission_body_state(&request);
        // once → immediate reply.
        let once = permission_run(&state, PermissionOption::Once);
        assert_eq!(once.reply.as_ref().unwrap().decision, "once");
        // always → confirmation stage.
        let always = permission_run(&state, PermissionOption::Always);
        assert_eq!(always.state.stage, PermissionStage::Always);
        assert!(always.reply.is_none());
        let confirmed = permission_run(&always.state, PermissionOption::Confirm);
        assert_eq!(confirmed.reply.as_ref().unwrap().decision, "always");
        let cancelled = permission_run(&always.state, PermissionOption::Cancel);
        assert_eq!(cancelled.state.stage, PermissionStage::Permission);
        assert_eq!(cancelled.state.selected, PermissionOption::Always);
        // escape from the ask → reject stage; message flows into the reply.
        let reject_stage = permission_escape(&state);
        assert_eq!(reject_stage.stage, PermissionStage::Reject);
        let with_message = PermissionBodyState { message: "  no way  ".into(), ..reject_stage };
        let reply = permission_reject(&with_message).unwrap();
        assert_eq!(reply.decision, "reject");
        assert_eq!(reply.message, "no way", "trimmed");
    }

    #[test]
    fn key_handler_drives_the_replay_flow() {
        let mut request = request("read", &[]);
        request.save = vec!["*".into()];
        let mut state = create_permission_body_state(&request);
        // right → "Always allow" selected.
        state = match permission_key(&state, &request, "right") {
            PermissionKey::State(next) => next,
            other => panic!("right shifts: {other:?}"),
        };
        assert_eq!(state.selected, PermissionOption::Always);
        // left + enter → replies "once" and stays on the ask.
        state = match permission_key(&state, &request, "left") {
            PermissionKey::State(next) => next,
            other => panic!("left shifts: {other:?}"),
        };
        let reply = match permission_key(&state, &request, "enter") {
            PermissionKey::Reply(reply, _) => reply,
            other => panic!("enter replies: {other:?}"),
        };
        assert_eq!(reply.decision, "once");
        // escape opens the reject stage; typing builds the message; enter confirms.
        state = match permission_key(&state, &request, "escape") {
            PermissionKey::State(next) => next,
            other => panic!("escape rejects: {other:?}"),
        };
        assert_eq!(state.stage, PermissionStage::Reject);
        for ch in ["c", "h", "o", "o", "s", "e", " "] {
            state = match permission_key(&state, &request, ch) {
                PermissionKey::Text(ch) => permission_type_message(&state, ch),
                other => panic!("{ch} types: {other:?}"),
            };
        }
        assert_eq!(state.message, "choose ");
        let reply = match permission_key(&state, &request, "enter") {
            PermissionKey::Reply(reply, _) => reply,
            other => panic!("enter confirms the reject: {other:?}"),
        };
        assert_eq!(reply.decision, "reject");
        assert_eq!(reply.message, "choose", "the reply message is trimmed");
    }

    #[test]
    fn always_lines_cover_wildcard_and_patterns() {
        assert_eq!(
            permission_always_lines("read", &["*".to_owned()]),
            vec!["This will always allow read for this project.".to_owned()]
        );
        assert_eq!(
            permission_always_lines("edit", &["a.ts".to_owned(), "b.ts".to_owned()]),
            vec![
                "This will always allow the following patterns for this project.".to_owned(),
                "- a.ts".to_owned(),
                "- b.ts".to_owned(),
            ]
        );
    }

    #[test]
    fn golden_frame_shape_at_80() {
        let theme = FooterTheme::fallback_dark();
        let request = request("read", &["src/index.ts"]);
        let state = create_permission_body_state(&request);
        let frame = render_permission_body(&request, &state, 80, 16, &theme);
        let grid = frame.text_grid();
        assert!(grid[2].trim_start().starts_with("△ Permission required"));
        assert!(grid[4].contains("→ Read src/index.ts"));
        assert!(grid[6].contains("Path: src/index.ts"));
        assert!(grid[14].contains("Allow once"));
        assert!(grid[14].contains("Reject"));
        assert!(grid[14].contains("⇆ select  enter confirm  esc reject"));
        assert_eq!(grid[15].chars().filter(|ch| *ch != ' ').count(), 0, "pane pad row");
        assert!(display_width(&grid[13]) <= 80);
        assert_eq!(frame.cell(13, 0).bg, theme.pane, "controls pad on the pane bg");
        assert_eq!(frame.cell(14, 0).bg, theme.pane, "actions row on the pane bg");
        assert_eq!(frame.cell(4, 76).ch, '█', "scrollbar track");
    }
}
