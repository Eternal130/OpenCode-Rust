//! The footer family's painter-agnostic STATE MODEL (lane R2 step 1).
//!
//! Everything here is the pure form/permission/menu semantics — the
//! focus/keydown/selection state machines ported from v2's
//! `mini/footer*.tsx` + `util/{form,permission}.ts` + the select
//! controller — with ZERO painter dependencies (no `Canvas`, no
//! `Frame`, no theme). The painters in `super::{form, permission,
//! panel, panels, prompt_idle}` consume this module; the keydown
//! entry points are [`permission_key`] (the two-stage ask) and
//! [`form_key`] (the `FORM_MODE` layer set, extracted from the
//! session-route pump so both the app and the mini runtime share
//! one semantics source).
//!
//! The render-side modules re-export these items verbatim, so the
//! historical paths (`footer::form::FormField`, …) stay live.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ===========================================================================
// The footer controller (FooterMachine) — mini/mod.rs's queue/view state.
// ===========================================================================
/// A prompt-route panel (`FooterPromptRoute`, the picker subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptRoute {
    #[default]
    Composer,
    Command,
    Agent,
    Model,
    Variant,
    Settings,
    /// TUI-17 — the subagent inspector (`FooterPromptRoute`'s
    /// `subagent` arm: one child's detail transcript over the
    /// composer).
    Subagent,
}

impl PromptRoute {
    /// A panel route (anything the footer opens over the composer).
    #[must_use]
    pub fn is_panel(self) -> bool {
        !matches!(self, Self::Composer)
    }
}

/// The active footer view (`FooterView`): the prompt (route stack) or
/// one of the blockers that preempts it.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FooterView {
    #[default]
    Prompt,
    Permission {
        request: PermissionRequest,
    },
    Form {
        request: FormRequest,
    },
}

impl FooterView {
    /// The view name the footer status uses (`blockerStatus`).
    #[must_use]
    pub fn status(&self) -> &'static str {
        match self {
            Self::Permission { .. } => "awaiting permission",
            Self::Form { .. } => "awaiting form",
            Self::Prompt => "",
        }
    }
}

/// `pickBlockerView` — permission asks take precedence over forms,
/// blockers take precedence over the prompt.
#[must_use]
pub fn pick_blocker_view(
    permission: Option<&PermissionRequest>,
    form: Option<&FormRequest>,
) -> FooterView {
    if let Some(request) = permission {
        return FooterView::Permission { request: request.clone() };
    }
    if let Some(request) = form {
        return FooterView::Form { request: request.clone() };
    }
    FooterView::Prompt
}

/// The footer controller state: blocker queues, the active view and
/// the prompt route stack (`RunFooter.present` + the view-sync effect
/// in `footer.view.tsx`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FooterMachine {
    pub view: FooterView,
    pub route: PromptRoute,
    pub permissions: Vec<PermissionRequest>,
    pub forms: Vec<FormRequest>,
}

impl FooterMachine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `permission.asked` — dedup by id, then re-derive the view.
    pub fn permission_asked(&mut self, request: PermissionRequest) {
        if !self.permissions.iter().any(|item| item.id == request.id) {
            self.permissions.push(request);
        }
        self.sync_view();
    }

    /// `permission.replied` — drop the request, re-derive.
    pub fn permission_replied(&mut self, request_id: &str) {
        self.permissions.retain(|item| item.id != request_id);
        self.sync_view();
    }

    /// `form.created` — dedup by id, then re-derive.
    pub fn form_created(&mut self, request: FormRequest) {
        if !self.forms.iter().any(|item| item.id == request.id) {
            self.forms.push(request);
        }
        self.sync_view();
    }

    /// `form.replied` / `form.cancelled` — drop the form, re-derive.
    pub fn form_removed(&mut self, form_id: &str) {
        self.forms.retain(|item| item.id != form_id);
        self.sync_view();
    }

    /// Open a picker route over the composer (no-op while a blocker
    /// owns the footer).
    pub fn open_route(&mut self, route: PromptRoute) {
        if matches!(self.view, FooterView::Prompt) {
            self.route = route;
        }
    }

    /// Collapse any open panel back to the composer (`closePanel`).
    pub fn close_panel(&mut self) {
        self.route = PromptRoute::Composer;
    }

    /// The view-sync effect: blockers preempt the prompt (permission
    /// first), and a blocker arriving over an open panel closes it.
    fn sync_view(&mut self) {
        self.view = pick_blocker_view(self.permissions.first(), self.forms.first());
        if !matches!(self.view, FooterView::Prompt) && self.route.is_panel() {
            self.route = PromptRoute::Composer;
        }
    }

    /// The mode the render loop dispatches on (footer state machine
    /// surface for callers).
    #[must_use]
    pub fn mode(&self) -> FooterMode {
        match &self.view {
            FooterView::Prompt => match self.route {
                PromptRoute::Composer => FooterMode::PromptIdle,
                PromptRoute::Command => FooterMode::CommandMenu,
                PromptRoute::Agent => FooterMode::AgentSelect,
                PromptRoute::Model => FooterMode::ModelSelect,
                PromptRoute::Variant => FooterMode::Variant,
                PromptRoute::Settings => FooterMode::Settings,
                PromptRoute::Subagent => FooterMode::Subagent,
            },
            FooterView::Permission { .. } => FooterMode::Permission,
            FooterView::Form { .. } => FooterMode::Form,
        }
    }
}

/// The flattened footer mode (dispatch key for the render loop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterMode {
    PromptIdle,
    Form,
    Permission,
    CommandMenu,
    ModelSelect,
    AgentSelect,
    Variant,
    Settings,
    /// TUI-17 — the mini subagent inspector (the session-route footer
    /// never routes here; the mini pane renders it directly).
    Subagent,
}

// ===========================================================================
// The permission state machine (permission.rs's pure section).
// ===========================================================================
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
/// (`util/permission.ts`). The edit arm carries the DIFF PREVIEW
/// (TD19): `metadata.files[0].patch || .diff || metadata.diff`, else
/// `input.patchText`, else the "No diff provided" fallback
/// (util/permission.ts:32-43 + permission.tsx:77-82).
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
// Mirrors the TS presenter as one flat match chain; splitting it
// would obscure the 1:1 port.
#[allow(clippy::too_many_lines)]
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
            // The diff preview (TD19): the first file's patch/diff, a
            // top-level metadata diff, or the input's patchText — the
            // user approves file edits with the hunk lines visible.
            let metadata = request.metadata.as_object().cloned().unwrap_or_default();
            let text_of = |value: &serde_json::Value| {
                value.as_str().filter(|item| !item.is_empty()).map(str::to_owned)
            };
            let first_file = metadata
                .get("files")
                .and_then(serde_json::Value::as_array)
                .and_then(|files| files.first());
            let diff = first_file
                .and_then(|file| file.get("patch").or_else(|| file.get("diff")))
                .and_then(text_of)
                .or_else(|| metadata.get("diff").and_then(text_of));
            let lines: Vec<String> = if let Some(diff) = diff {
                diff.lines().map(str::to_owned).collect()
            } else if let Some(patch) = value("patchText") {
                patch.lines().map(str::to_owned).collect()
            } else {
                vec!["No diff provided".to_owned()]
            };
            PermissionPresentation { icon: "→", title: format!("Edit {file}"), lines }
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
    }
}

/// `PermissionStage` — the session route's TWO-stage machine
/// (`routes/session/permission.tsx:20`: `"permission" | "reject"`):
/// the ask, and the child-session free-text reject prompt. There is
/// NO always-confirmation stage — `always` replies on a single enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionStage {
    #[default]
    Permission,
    Reject,
}

/// `PermissionOption`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOption {
    Once,
    Always,
    Reject,
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

/// `permissionOptions(stage)` — the ask's three affordances (the
/// reject stage offers none: its own enter/esc hint row).
#[must_use]
pub fn permission_options(stage: PermissionStage) -> Vec<PermissionOption> {
    match stage {
        PermissionStage::Permission => {
            vec![PermissionOption::Once, PermissionOption::Always, PermissionOption::Reject]
        }
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

/// `permissionRun` — the session route's `onSelect`
/// (permission.tsx:243-257): `once` and `always` reply IMMEDIATELY
/// (always needs NO confirmation round), and `reject` replies
/// immediately on a TOP-LEVEL session while a CHILD session
/// (`session()?.parentID`) opens the free-text stage instead.
#[must_use]
pub fn permission_run(
    state: &PermissionBodyState,
    option: PermissionOption,
    child_session: bool,
) -> PermissionStep {
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
            PermissionOption::Always => {
                PermissionStep { state: state.clone(), reply: Some(reply("always", "")) }
            }
            PermissionOption::Reject => {
                if child_session {
                    PermissionStep {
                        state: PermissionBodyState {
                            stage: PermissionStage::Reject,
                            selected: PermissionOption::Reject,
                            ..state.clone()
                        },
                        reply: None,
                    }
                } else {
                    PermissionStep { state: state.clone(), reply: Some(reply("reject", "")) }
                }
            }
            PermissionOption::Once => {
                PermissionStep { state: state.clone(), reply: Some(reply("once", "")) }
            }
        },
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

/// The `SessionQuestion` + `RejectPrompt` keymaps
/// (permission.tsx:284-307, 440-496): left/right/tab shift the
/// selection, return runs it; `escapeKey="reject"` makes escape run
/// the REJECT path — immediate on a top-level session, the free-text
/// stage on a child; inside the reject stage printable keys edit the
/// message, enter confirms, esc cancels.
#[must_use]
pub fn permission_key(
    state: &PermissionBodyState,
    request: &PermissionRequest,
    child_session: bool,
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
        "right" | "l" | "tab" => PermissionKey::State(permission_shift(state, 1, &offered)),
        "enter" | "return" => {
            let step = permission_run(state, state.selected, child_session);
            match step.reply {
                Some(reply) => PermissionKey::Reply(reply, step.state),
                None => PermissionKey::State(step.state),
            }
        }
        // `escapeKey="reject"` (permission.tsx:241, 437-438): the ask's
        // escape runs the reject path — no expanded-fullscreen state
        // to collapse first in the docked body.
        "escape" => {
            let step = permission_run(state, PermissionOption::Reject, child_session);
            match step.reply {
                Some(reply) => PermissionKey::Reply(reply, step.state),
                None => PermissionKey::State(step.state),
            }
        }
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

// ===========================================================================
// The form state machine (form.rs's pure section).
// ===========================================================================
/// One selectable option row of a field (`formRows` — synthesized for
/// booleans as the Yes/No pair, `util/form.ts:47-61`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A rendered row's value: option strings, or the boolean pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormRowValue {
    Str(String),
    Bool(bool),
}

/// `formRows`'s row: value + label (+ description).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormRow {
    pub value: FormRowValue,
    pub label: String,
    pub description: Option<String>,
}

/// `FormField` kinds the footer renders (wire `type` tags — the v2
/// `Form.Field` family, `oc-schema/src/form.rs`: string [with
/// options], multiselect, boolean, number, integer, external).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldKind {
    #[serde(rename = "string")]
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        /// Option list — a string WITH options renders as a picker
        /// (`formRows`: `field.type === "string" ? field.options`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<FormOption>>,
        /// `"custom": true` — custom answers allowed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom: Option<bool>,
        #[serde(rename = "minLength", default, skip_serializing_if = "Option::is_none")]
        min_length: Option<u64>,
        #[serde(rename = "maxLength", default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u64>,
    },
    Multiselect {
        #[serde(default)]
        options: Vec<FormOption>,
        /// `"custom": true`-style custom entries allowed
        /// (`formCustom`: multiselect is custom only when the flag is
        /// set — util/form.ts:41-45).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom: Option<bool>,
    },
    Boolean,
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    Integer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    External {
        url: String,
    },
}

/// One form field (`FormField` subset).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(flatten)]
    pub kind: FieldKind,
}

impl FormField {
    #[must_use]
    pub fn text(key: &str, title: &str) -> Self {
        Self {
            key: key.into(),
            title: Some(title.into()),
            description: None,
            required: false,
            kind: FieldKind::Text {
                placeholder: None,
                options: None,
                custom: None,
                min_length: None,
                max_length: None,
            },
        }
    }

    /// `formLabel` — title ?? key.
    #[must_use]
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(self.key.as_str())
    }

    /// `formTextual` — free-text input fields: number/integer, and
    /// string WITHOUT options (util/form.ts:36-39).
    #[must_use]
    pub fn textual(&self) -> bool {
        match &self.kind {
            FieldKind::Text { options, .. } => options.is_none(),
            FieldKind::Number { .. } | FieldKind::Integer { .. } => true,
            _ => false,
        }
    }

    /// `formCustom` — option lists that accept a custom answer
    /// (util/form.ts:41-45: string-with-options and multiselect are
    /// custom only when the flag is `true`).
    #[must_use]
    pub fn custom(&self) -> bool {
        match &self.kind {
            FieldKind::Text { options: Some(_), custom, .. }
            | FieldKind::Multiselect { custom, .. } => *custom == Some(true),
            _ => false,
        }
    }

    /// `formRows` — the option rows (booleans synthesize Yes/No).
    #[must_use]
    pub fn rows(&self) -> Vec<FormRow> {
        match &self.kind {
            FieldKind::Boolean => vec![
                FormRow { value: FormRowValue::Bool(true), label: "Yes".into(), description: None },
                FormRow { value: FormRowValue::Bool(false), label: "No".into(), description: None },
            ],
            FieldKind::Text { options: Some(options), .. } => options
                .iter()
                .map(|option| FormRow {
                    value: FormRowValue::Str(option.value.clone()),
                    label: option.label.clone(),
                    description: option.description.clone(),
                })
                .collect(),
            FieldKind::Multiselect { options, .. } => options
                .iter()
                .map(|option| FormRow {
                    value: FormRowValue::Str(option.value.clone()),
                    label: option.label.clone(),
                    description: option.description.clone(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `formPlaceholder` (util/form.ts + form.tsx:199-210): the
    /// configured placeholder, the number-range hint, or the default.
    #[must_use]
    pub fn placeholder(&self) -> String {
        match &self.kind {
            FieldKind::Text { placeholder: Some(value), .. } => value.clone(),
            FieldKind::Number { minimum, maximum } | FieldKind::Integer { minimum, maximum } => {
                match (minimum, maximum) {
                    (Some(min), Some(max)) => format!("{min}-{max}"),
                    (Some(min), None) => format!("at least {min}"),
                    (None, Some(max)) => format!("at most {max}"),
                    (None, None) => "Type your answer".into(),
                }
            }
            _ => "Type your answer".into(),
        }
    }

    /// `formValidateValue` (util/form.ts:72-110): requiredness, the
    /// per-kind typing rules, and the numeric bounds.
    #[must_use]
    pub fn validate(&self, answer: Option<&serde_json::Value>) -> Option<String> {
        let Some(answer) = answer else {
            return self.required.then(|| "Answer required".to_owned());
        };
        if self.required
            && (answer == &serde_json::Value::String(String::new())
                || answer.as_array().is_some_and(Vec::is_empty))
        {
            return Some(if matches!(self.kind, FieldKind::Multiselect { .. }) {
                "Select at least one option".to_owned()
            } else {
                "Answer required".to_owned()
            });
        }
        match (&self.kind, answer) {
            (
                FieldKind::Text { options, custom, min_length, max_length, .. },
                serde_json::Value::String(value),
            ) => {
                if let Some(min) = min_length {
                    if let Ok(min) = usize::try_from(*min) {
                        if value.chars().count() < min {
                            return Some(format!("Must be at least {min} characters"));
                        }
                    }
                }
                if let Some(max) = max_length {
                    if let Ok(max) = usize::try_from(*max) {
                        if value.chars().count() > max {
                            return Some(format!("Must be at most {max} characters"));
                        }
                    }
                }
                if let Some(options) = options {
                    if *custom != Some(true) && !options.iter().any(|option| option.value == *value)
                    {
                        return Some("Select an available option".to_owned());
                    }
                }
                None
            }
            (FieldKind::Text { .. }, _) => Some("Expected text".to_owned()),
            (FieldKind::Number { minimum, maximum }, value) => {
                number_validate(value, false, minimum.as_ref(), maximum.as_ref())
            }
            (FieldKind::Integer { minimum, maximum }, value) => {
                number_validate(value, true, minimum.as_ref(), maximum.as_ref())
            }
            (FieldKind::Boolean, serde_json::Value::Bool(_)) | (FieldKind::External { .. }, _) => {
                None
            }
            (FieldKind::Boolean, _) => Some("Expected yes or no".to_owned()),
            (FieldKind::Multiselect { options, custom }, serde_json::Value::Array(items)) => {
                if *custom != Some(true)
                    && items.iter().any(|item| {
                        options
                            .iter()
                            .all(|option| option.value != item.as_str().unwrap_or_default())
                    })
                {
                    return Some("Select only available options".to_owned());
                }
                None
            }
            (FieldKind::Multiselect { .. }, _) => Some("Expected selections".to_owned()),
        }
    }
}

/// The number/integer arm of `formValidateValue` (util/form.ts:97-103).
fn number_validate(
    value: &serde_json::Value,
    integer: bool,
    minimum: Option<&f64>,
    maximum: Option<&f64>,
) -> Option<String> {
    let Some(number) = value.as_f64() else {
        return Some("Expected a number".to_owned());
    };
    if !number.is_finite() {
        return Some("Expected a number".to_owned());
    }
    if integer && number.fract() != 0.0 {
        return Some("Expected an integer".to_owned());
    }
    if let Some(min) = minimum {
        if number < *min {
            return Some(format!("Must be at least {min}"));
        }
    }
    if let Some(max) = maximum {
        if number > *max {
            return Some(format!("Must be at most {max}"));
        }
    }
    None
}

/// `MiniFormRequest` (`FormInfo` subset).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: String,
    pub fields: Vec<FormField>,
}

/// `FormReply` — the submitted answer map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormReply {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub form_id: String,
    pub answer: BTreeMap<String, serde_json::Value>,
}

/// `FormCancel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormCancel {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub form_id: String,
}

/// `FormBodyState` (the editable subset).
#[derive(Debug, Clone, PartialEq)]
pub struct FormBodyState {
    pub form_id: String,
    /// Active field index; `fields.len()` = the review/confirm step.
    pub field: usize,
    pub selected: usize,
    pub editing: bool,
    /// The free-text draft (textual fields / the custom row).
    pub draft: String,
    pub answers: BTreeMap<String, serde_json::Value>,
    pub error: String,
}

impl FormBodyState {
    #[must_use]
    pub fn new(request: &FormRequest) -> Self {
        Self {
            form_id: request.id.clone(),
            field: 0,
            selected: 0,
            editing: false,
            draft: String::new(),
            answers: BTreeMap::new(),
            error: String::new(),
        }
    }

    /// `formCurrent` — the active field, or `None` at the confirm step.
    #[must_use]
    pub fn current<'a>(&self, request: &'a FormRequest) -> Option<&'a FormField> {
        request.fields.get(self.field)
    }

    /// `formConfirm` — on the review step.
    #[must_use]
    pub fn confirm(&self, request: &FormRequest) -> bool {
        self.field >= request.fields.len()
    }

    /// `formSingle` (form.tsx:128-134) — one boolean or
    /// string-with-options field submits directly (no review step).
    #[must_use]
    pub fn single(request: &FormRequest) -> bool {
        if request.fields.len() != 1 {
            return false;
        }
        matches!(
            request.fields[0].kind,
            FieldKind::Boolean | FieldKind::Text { options: Some(_), .. }
        )
    }
}

/// `formSync` — a new form id resets the state.
#[must_use]
pub fn form_sync(state: &FormBodyState, request: &FormRequest) -> FormBodyState {
    if state.form_id == request.id {
        state.clone()
    } else {
        FormBodyState::new(request)
    }
}

/// `formSetField` — move to a field (clamped), resetting selection.
#[must_use]
pub fn form_set_field(state: &FormBodyState, request: &FormRequest, field: usize) -> FormBodyState {
    let field = field.min(request.fields.len());
    FormBodyState {
        field,
        selected: 0,
        editing: false,
        draft: String::new(),
        error: String::new(),
        ..state.clone()
    }
}

/// `formMove` — selection movement inside an option list (wraps).
#[must_use]
pub fn form_move(state: &FormBodyState, request: &FormRequest, dir: i8) -> FormBodyState {
    let Some(field) = state.current(request) else { return state.clone() };
    let total = field.rows().len() + usize::from(field.custom());
    if total == 0 {
        return state.clone();
    }
    let selected = (isize::try_from(state.selected).unwrap_or(0))
        .saturating_add(isize::from(dir))
        .rem_euclid(isize::try_from(total).unwrap_or(1));
    let selected = usize::try_from(selected).unwrap_or(0);
    FormBodyState { selected, ..state.clone() }
}

/// `formCommitInput` — validate + store a textual answer (number /
/// integer fields PARSE the text, `commitInput` form.tsx:386-399);
/// advance the field (or submit when the form is single-field).
/// Returns the next state and the reply when the form completed.
#[must_use]
pub fn form_commit_input(
    state: &FormBodyState,
    request: &FormRequest,
    text: &str,
) -> (FormBodyState, Option<FormReply>) {
    let Some(field) = state.current(request) else { return (state.clone(), None) };
    // `Number(text)`: number/integer fields convert the draft; a
    // non-numeric draft fails validation ("Expected a number").
    // Whole values keep the JSON integer form (JS `Number(7)` wires
    // `7`, not `7.0`).
    let value = match &field.kind {
        FieldKind::Number { .. } | FieldKind::Integer { .. } => match text.parse::<f64>() {
            // Whole values keep the JSON integer form (JS `Number(7)`
            // wires `7`, not `7.0`); the guard bounds the cast to
            // MAX_SAFE_INTEGER, so the truncation is exact.
            Ok(number) if number.fract() == 0.0 && number.abs() <= 9.007_199_254_740_992e15 =>
            {
                #[allow(clippy::cast_possible_truncation)]
                serde_json::Value::Number(serde_json::Number::from(number as i64))
            }
            Ok(number) => serde_json::Number::from_f64(number)
                .map_or(serde_json::Value::Null, serde_json::Value::Number),
            Err(_) => serde_json::Value::String(text.to_owned()),
        },
        _ => serde_json::Value::String(text.to_owned()),
    };
    if let Some(error) = field.validate(Some(&value)) {
        return (FormBodyState { error, ..state.clone() }, None);
    }
    let mut answers = state.answers.clone();
    answers.insert(field.key.clone(), value);
    let mut next = FormBodyState {
        answers,
        draft: String::new(),
        editing: false,
        error: String::new(),
        ..state.clone()
    };
    if FormBodyState::single(request) {
        let reply = form_reply(&next, request);
        return (next, reply);
    }
    next = form_set_field(&next, request, state.field + 1);
    (next, None)
}

/// `formPick` — select an option (multiselect toggles; single-select
/// stores + advances).
#[must_use]
pub fn form_pick(
    state: &FormBodyState,
    request: &FormRequest,
    index: usize,
) -> (FormBodyState, Option<FormReply>) {
    let Some(field) = state.current(request) else { return (state.clone(), None) };
    let rows = field.rows();
    let custom = field.custom();
    if index >= rows.len() + usize::from(custom) {
        return (state.clone(), None);
    }
    let mut next = FormBodyState { selected: index, ..state.clone() };
    if index == rows.len() && custom {
        // The custom row opens the editor.
        next.editing = true;
        return (next, None);
    }
    let row = &rows[index];
    if let FieldKind::Multiselect { .. } = &field.kind {
        // Multiselect rows are option strings (the boolean pair never
        // rides a multiselect).
        let FormRowValue::Str(option_value) = &row.value else {
            return (state.clone(), None);
        };
        let mut items = state
            .answers
            .get(&field.key)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let value = serde_json::Value::String(option_value.clone());
        if let Some(position) = items.iter().position(|item| *item == value) {
            items.remove(position);
        } else {
            items.push(value);
        }
        next.answers.insert(field.key.clone(), serde_json::Value::Array(items));
        (next, None)
    } else {
        // Boolean rows carry true/false (`formRows` synthesizes the
        // Yes/No pair); every other row stores its string value.
        let value = match row.value {
            FormRowValue::Bool(flag) => serde_json::Value::Bool(flag),
            FormRowValue::Str(ref text) => serde_json::Value::String(text.clone()),
        };
        if let Some(error) = field.validate(Some(&value)) {
            return (FormBodyState { error, ..next }, None);
        }
        next.answers.insert(field.key.clone(), value);
        if FormBodyState::single(request) {
            let reply = form_reply(&next, request);
            return (next, reply);
        }
        next = form_set_field(&next, request, state.field + 1);
        (next, None)
    }
}

/// `formValidate` + `formReply` — all required fields answered.
#[must_use]
pub fn form_reply(state: &FormBodyState, request: &FormRequest) -> Option<FormReply> {
    for field in &request.fields {
        let answer = state.answers.get(&field.key);
        if answer.is_none() {
            if field.required {
                return None;
            }
            continue;
        }
        if field.validate(answer).is_some() {
            return None;
        }
    }
    Some(FormReply {
        session_id: request.session_id.clone(),
        form_id: request.id.clone(),
        answer: state.answers.clone(),
    })
}

// ===========================================================================
// The select controller + menu item data (panel.rs's pure section).
// ===========================================================================
/// `PANEL_LIST_ROWS` / `PANEL_FRAME_ROWS` / `RUN_COMMAND_PANEL_ROWS`.
pub const PANEL_LIST_ROWS: usize = 10;
pub const PANEL_FRAME_ROWS: usize = 6;
pub const RUN_COMMAND_PANEL_ROWS: usize = PANEL_LIST_ROWS + PANEL_FRAME_ROWS;

// -- select controller (`ui/select-controller.ts`) ----------------------------

#[must_use]
pub fn reconcile_selection(selected: usize, count: usize) -> usize {
    selected.min(count.saturating_sub(1))
}

#[must_use]
pub fn move_selection_clamp(selected: usize, count: usize, delta: i8) -> usize {
    if count == 0 {
        return 0;
    }
    let next =
        (isize::try_from(selected).unwrap_or(isize::MAX)).saturating_add(isize::from(delta)).max(0);
    reconcile_selection(usize::try_from(next).unwrap_or(0), count)
}

fn max_offset(count: usize, limit: usize) -> usize {
    count.saturating_sub(limit)
}

#[must_use]
pub fn reveal_selection_offset(
    offset: usize,
    count: usize,
    limit: usize,
    selected: usize,
) -> usize {
    let max = max_offset(count, limit);
    if selected < offset {
        return selected.min(max);
    }
    if selected >= offset + limit {
        return (selected + 1).saturating_sub(limit).min(max);
    }
    offset.min(max)
}

#[must_use]
pub fn move_selection_offset(
    offset: usize,
    count: usize,
    limit: usize,
    selected: usize,
    direction: i8,
) -> usize {
    let max = max_offset(count, limit);
    let margin = 2.min((limit.saturating_sub(1)) / 2);
    if direction < 0 && selected < offset + margin {
        return selected.saturating_sub(margin).min(max);
    }
    if direction > 0 && selected > offset + limit - margin - 1 {
        return (selected + margin + 1).saturating_sub(limit).min(max);
    }
    offset.min(max)
}

/// `createFooterMenuState`: selection + window offset over a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FooterMenuState {
    pub selected: usize,
    pub offset: usize,
}

impl FooterMenuState {
    #[must_use]
    pub fn new() -> Self {
        Self { selected: 0, offset: 0 }
    }

    pub fn reset(&mut self) {
        self.selected = 0;
        self.offset = 0;
    }

    /// `reveal` — jump to an index, keeping it in the window.
    pub fn reveal(&mut self, index: usize, count: usize, limit: usize) {
        self.selected = reconcile_selection(index, count);
        self.offset = reveal_selection_offset(self.offset, count, limit, self.selected);
    }

    /// `move` — step the selection with the window following.
    pub fn r#move(&mut self, dir: i8, count: usize, limit: usize) {
        self.selected = move_selection_clamp(self.selected, count, dir);
        self.offset = move_selection_offset(self.offset, count, limit, self.selected, dir);
    }
}

impl Default for FooterMenuState {
    fn default() -> Self {
        Self::new()
    }
}

// -- menu items ---------------------------------------------------------------

/// `footerTone` — the footer's color role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FooterTone {
    #[default]
    Muted,
    Selection,
    Running,
    Error,
    Success,
}

/// `RunFooterMenuItem`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FooterMenuItem {
    pub display: String,
    pub description: Option<String>,
    pub category: String,
    pub footer: Option<String>,
    pub footer_tone: Option<FooterTone>,
    pub current: bool,
    /// A one-cell icon glyph rendered in a 2-wide column.
    pub icon: Option<char>,
}

// ===========================================================================
// The picker panels' model: settings knobs, verbosity, entry builders,
// the fuzzy filter (panels.rs's pure section).
// ===========================================================================
/// `RunProvider` (the picker subset).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunProvider {
    pub id: String,
    pub name: String,
    /// modelID → (display name, cost.input, deprecated).
    pub models: Vec<RunModel>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunModel {
    pub id: String,
    pub name: String,
    pub cost_input: f64,
    pub deprecated: bool,
}

/// `RunAgent` (the picker subset).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunAgent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// `all` | `primary` | `subagent`.
    pub mode: String,
    pub hidden: bool,
}

/// `MiniSettings` + the verbosity presets (`mini/runtime.boot.ts`,
/// `mini/verbosity.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MiniSettings {
    pub thinking: &'static str,
    pub tools: &'static str,
    pub shell_output: &'static str,
    pub turn_summary: &'static str,
    pub footer: &'static str,
    pub splash: &'static str,
    pub work_spinner: &'static str,
    pub mono: bool,
}

/// `MiniWorkSpinner.literals`.
pub const WORK_SPINNERS: &[&str] = &[
    "block-soft-slide",
    "block-soft-sweep",
    "block-low-comet",
    "block-low-duet",
    "block-shuttle",
    "block-bridge",
    "block-squeeze",
    "small-toggle",
    "square-toggle",
    "grow-shrink",
    "quadrant-orbit",
    "crosshatch",
    "density-wave",
    "seed",
];

/// `resolveMiniSettings()` defaults.
#[must_use]
pub fn resolve_mini_settings() -> MiniSettings {
    MiniSettings {
        thinking: "show",
        tools: "hide",
        shell_output: "hide",
        turn_summary: "show",
        footer: "show",
        splash: "show",
        work_spinner: "block-soft-slide",
        mono: false,
    }
}

/// `verbosityLabel` / `matchMiniVerbosity`.
#[must_use]
pub fn verbosity_label(level: &str) -> &'static str {
    match level {
        "quiet" => "Quiet",
        "default" => "Default",
        "everything" => "Everything",
        _ => "Custom",
    }
}

fn verbosity_knob_preset(level: &str) -> MiniSettings {
    let hide_show = |thinking, tools, shell, turn, footer| MiniSettings {
        thinking,
        tools,
        shell_output: shell,
        turn_summary: turn,
        footer,
        ..resolve_mini_settings()
    };
    match level {
        "quiet" => hide_show("hide", "hide", "hide", "hide", "hide"),
        "everything" => hide_show("show", "show", "show", "show", "show"),
        _ => hide_show("show", "hide", "hide", "show", "show"),
    }
}

/// `matchMiniVerbosity` — which preset the knobs equal.
#[must_use]
pub fn match_mini_verbosity(settings: &MiniSettings) -> &'static str {
    for level in ["quiet", "default", "everything"] {
        let preset = verbosity_knob_preset(level);
        if (
            settings.thinking,
            settings.tools,
            settings.shell_output,
            settings.turn_summary,
            settings.footer,
        ) == (
            preset.thinking,
            preset.tools,
            preset.shell_output,
            preset.turn_summary,
            preset.footer,
        ) {
            return level;
        }
    }
    "custom"
}

/// `verbosityChange` — cycle one step if the level changes.
#[must_use]
pub fn verbosity_change(
    settings: &MiniSettings,
    direction: i8,
) -> Option<(&'static str, &'static str)> {
    let levels = ["quiet", "default", "everything"];
    let knobs = [
        settings.thinking,
        settings.tools,
        settings.shell_output,
        settings.turn_summary,
        settings.footer,
    ];
    let distance = |level: &str| {
        let preset = verbosity_knob_preset(level);
        let preset_knobs = [
            preset.thinking,
            preset.tools,
            preset.shell_output,
            preset.turn_summary,
            preset.footer,
        ];
        knobs
            .iter()
            .zip(preset_knobs.iter())
            .filter(|(current, preset)| **current != **preset)
            .count()
    };
    let current = match_mini_verbosity(settings);
    let index = if current == "custom" {
        levels
            .iter()
            .enumerate()
            .min_by_key(|(index, level)| (distance(level), *index))
            .map_or(0, |(index, _)| index)
    } else {
        levels.iter().position(|level| *level == current).unwrap_or(0)
    };
    let next = (isize::try_from(index).unwrap_or(0)).saturating_add(isize::from(direction));
    let value = if next < 0 {
        levels[0]
    } else if usize::try_from(next).is_ok_and(|value| value >= levels.len()) {
        levels[levels.len() - 1]
    } else {
        levels[usize::try_from(next).unwrap_or(0)]
    };
    (match_mini_verbosity(settings) != value).then_some(("verbosity", value))
}

/// `applyMiniSettingChange`.
#[must_use]
pub fn apply_mini_setting_change(settings: &MiniSettings, key: &str, value: &str) -> MiniSettings {
    if key == "verbosity" {
        let preset = verbosity_knob_preset(value);
        return MiniSettings { ..preset };
    }
    let mut next = settings.clone();
    match key {
        "thinking" => next.thinking = leak_str(value),
        "tools" => next.tools = leak_str(value),
        "shell_output" => next.shell_output = leak_str(value),
        "turn_summary" => next.turn_summary = leak_str(value),
        "footer" => next.footer = leak_str(value),
        "splash" => next.splash = leak_str(value),
        "work_spinner" => next.work_spinner = leak_str(value),
        _ => {}
    }
    next
}

// Settings values are the fixed show/hide literals; map any dynamic
// string onto one of them (unknown values keep the current literal).
fn leak_str(value: &str) -> &'static str {
    match value {
        "hide" => "hide",
        _ => "show",
    }
}

/// `MiniSettingChange` — a settings mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiniSettingChange {
    pub key: &'static str,
    pub value: String,
}

// -- entry lists ---------------------------------------------------------------

// The panel renders keep the reference's argument surface.

/// The command-menu entries (`RunCommandMenuBody.entries`): static
/// session/agent/system commands; subagent and queued rows appear only
/// with data, the variant row only with variants.
#[must_use]
pub fn command_menu_entries(
    subagents: usize,
    active_subagents: usize,
    queued: usize,
    variants: &[String],
    variant_cycle: &str,
) -> Vec<FooterMenuItem> {
    let mut entries = vec![
        FooterMenuItem {
            display: "Open editor".into(),
            category: "Session".into(),
            footer: Some("/editor".into()),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Show status".into(),
            category: "Session".into(),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Compact session".into(),
            category: "Session".into(),
            footer: Some("/compact".into()),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "New session".into(),
            category: "Session".into(),
            footer: Some("/new".into()),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Switch agent".into(),
            category: "Agent".into(),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Switch model".into(),
            category: "Agent".into(),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Variant cycle".into(),
            category: "Agent".into(),
            footer: Some(variant_cycle.to_owned()),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Switch model variant".into(),
            category: "Agent".into(),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Clear screen".into(),
            category: "System".into(),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Settings".into(),
            category: "System".into(),
            footer: Some("/settings".into()),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Exit".into(),
            category: "System".into(),
            footer: Some("/exit".into()),
            ..FooterMenuItem::default()
        },
    ];
    // View subagents slots in after "Show status" when present.
    if subagents > 0 {
        let footer = if active_subagents > 0 {
            format!("{active_subagents} active")
        } else {
            format!("{subagents} recent")
        };
        entries.insert(
            2,
            FooterMenuItem {
                display: "View subagents".into(),
                category: "Session".into(),
                footer: Some(footer),
                ..FooterMenuItem::default()
            },
        );
    }
    if queued > 0 {
        let position = entries.iter().position(|item| item.display == "Switch model").unwrap_or(5);
        entries.insert(
            position + 1,
            FooterMenuItem {
                display: "View pending prompts".into(),
                category: "Agent".into(),
                footer: Some(format!("{queued} pending")),
                ..FooterMenuItem::default()
            },
        );
    }
    if variants.is_empty() {
        entries.retain(|item| item.display != "Switch model variant");
    }
    entries
}

/// The model-select entries (`RunModelSelectBody.entries`): deprecated
/// models filtered, opencode provider first, footer = current / Free
/// (zero-cost opencode) / modelID.
#[must_use]
pub fn model_entries(
    providers: &[RunProvider],
    current: Option<(&str, &str)>,
) -> Vec<FooterMenuItem> {
    let mut entries: Vec<FooterMenuItem> = providers
        .iter()
        .flat_map(|provider| {
            provider.models.iter().filter(|model| !model.deprecated).map(|model| {
                let title =
                    if model.name.is_empty() { model.id.clone() } else { model.name.clone() };
                let is_current = current == Some((provider.id.as_str(), model.id.as_str()));
                let footer = if is_current {
                    Some("current".to_owned())
                } else if model.cost_input == 0.0 && provider.id == "opencode" {
                    Some("Free".to_owned())
                } else if title != model.id {
                    Some(model.id.clone())
                } else {
                    None
                };
                FooterMenuItem {
                    display: title,
                    category: provider.name.clone(),
                    footer,
                    footer_tone: is_current.then_some(FooterTone::Selection),
                    current: is_current,
                    ..FooterMenuItem::default()
                }
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        let provider = a.category.is_empty().cmp(&b.category.is_empty());
        provider.then_with(|| a.category.cmp(&b.category)).then_with(|| a.display.cmp(&b.display))
    });
    entries
}

/// The agent-select entries (`RunAgentSelectBody.entries`): non-hidden
/// non-subagent agents, display = id, description in the muted column,
/// footer "current".
#[must_use]
pub fn agent_entries(agents: &[RunAgent], current: Option<&str>) -> Vec<FooterMenuItem> {
    agents
        .iter()
        .filter(|agent| agent.mode != "subagent" && !agent.hidden)
        .map(|agent| FooterMenuItem {
            display: agent.id.clone(),
            description: agent.description.clone(),
            footer: (current == Some(agent.id.as_str())).then(|| "current".to_owned()),
            footer_tone: Some(FooterTone::Selection),
            current: current == Some(agent.id.as_str()),
            ..FooterMenuItem::default()
        })
        .collect()
}

/// The variant-select entries: "Default" (no variant) + the variants.
#[must_use]
pub fn variant_entries(variants: &[String], current: Option<&str>) -> Vec<FooterMenuItem> {
    let mut entries = vec![FooterMenuItem {
        display: "Default".into(),
        footer: current.is_none().then(|| "current".to_owned()),
        footer_tone: Some(FooterTone::Selection),
        current: current.is_none(),
        ..FooterMenuItem::default()
    }];
    entries.extend(variants.iter().map(|variant| FooterMenuItem {
        display: variant.clone(),
        footer: (current == Some(variant.as_str())).then(|| "current".to_owned()),
        footer_tone: Some(FooterTone::Selection),
        current: current == Some(variant.as_str()),
        ..FooterMenuItem::default()
    }));
    entries
}

/// The settings entries (`RunSettingsBody.entries`).
#[must_use]
pub fn settings_entries(settings: &MiniSettings) -> Vec<FooterMenuItem> {
    let spinner = settings.work_spinner.trim_start_matches("block-").replace('-', " ");
    vec![
        FooterMenuItem {
            display: "Verbosity".into(),
            category: "Transcript".into(),
            footer: Some(verbosity_label(match_mini_verbosity(settings)).into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Thinking".into(),
            category: "Transcript".into(),
            footer: Some(settings.thinking.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Tools".into(),
            category: "Transcript".into(),
            footer: Some(settings.tools.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Shell".into(),
            category: "Transcript".into(),
            footer: Some(settings.shell_output.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Turn summary".into(),
            category: "Transcript".into(),
            footer: Some(settings.turn_summary.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Footer details".into(),
            category: "Terminal".into(),
            footer: Some(settings.footer.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Splash".into(),
            category: "Terminal".into(),
            footer: Some(settings.splash.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Monochrome UI".into(),
            category: "Terminal".into(),
            footer: Some(if settings.mono { "on" } else { "off" }.into()),
            footer_tone: Some(FooterTone::Selection),
            ..FooterMenuItem::default()
        },
        FooterMenuItem {
            display: "Work spinner".into(),
            category: "Terminal".into(),
            footer: Some(spinner),
            footer_tone: Some(FooterTone::Selection),
            icon: Some('▪'),
            ..FooterMenuItem::default()
        },
    ]
}

/// Reveal the current entry in a picker (`isCurrent` effect).
#[must_use]
pub fn reveal_current(entries: &[FooterMenuItem]) -> FooterMenuState {
    let mut menu = FooterMenuState::new();
    if let Some(index) = entries.iter().position(|item| item.current) {
        menu.reveal(index, entries.len(), PANEL_LIST_ROWS);
    }
    menu
}
/// Fuzzysort-shaped filter (`match`): a subsequence over display /
/// category / description / keywords; empty query passes everything.
/// (The reference uses `fuzzysort.go` scoring; ordering for equal
/// candidates is input order there too — corpus queries are empty.)
#[must_use]
pub fn fuzzy_matches(item: &FooterMenuItem, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let hay = format!(
        "{}{}{}{}",
        item.display,
        item.category,
        item.description.as_deref().unwrap_or(""),
        item.footer.as_deref().unwrap_or(""),
    )
    .to_lowercase();
    let mut cursor = 0;
    for ch in query.to_lowercase().chars() {
        cursor = match hay[cursor..].find(ch) {
            Some(position) => cursor + position + 1,
            None => return false,
        };
    }
    true
}

// ===========================================================================
// The form keymap — the FORM_MODE layer set (form.tsx:547-720),
// extracted from the session-route pump (app.rs `stroke_in_form`) in
// lane R2 step 1 so the keydown semantics live store-side beside the
// rest of the model. Behavior-identical: the pump only applies the
// side effects this step reports.
// ===========================================================================

/// `selectTab` with wrap (form.tsx:421-431 + 606-615): step `from` by
/// `dir` modulo the tab count (`fields.len() + 1` review tab on
/// multi-field forms; single-field forms have exactly one).
#[must_use]
pub fn form_step_field(
    state: &FormBodyState,
    request: &FormRequest,
    from: usize,
    dir: i8,
) -> FormBodyState {
    let tabs = if FormBodyState::single(request) { 1 } else { request.fields.len() + 1 };
    if tabs == 0 {
        return state.clone();
    }
    let tabs_i = isize::try_from(tabs).unwrap_or(1);
    let next = (isize::try_from(from).unwrap_or(0) + isize::from(dir)).rem_euclid(tabs_i);
    form_set_field(state, request, usize::try_from(next).unwrap_or(0))
}

/// One resolved form key step: the next state plus the side effects
/// the caller applies (a fired reply settles the form server-side; a
/// cancel dismisses it). Mirrors [`PermissionStep`]'s shape.
#[derive(Debug, Clone, PartialEq)]
pub struct FormStep {
    pub state: FormBodyState,
    pub reply: Option<FormReply>,
    pub cancel: bool,
}

/// One stroke while a form holds the footer (TD16): the `FORM_MODE`
/// keymap layers (form.tsx:547-720) — the editing/textual layer
/// (enter commits, tab/shift+tab step fields, esc dismisses/closes
/// the edit, printables edit the draft), the picker layer (arrows
/// move, digits select, enter picks, left/right step fields, esc
/// dismisses), and the review step (enter submits, esc dismisses).
/// Every other stroke is consumed — the composer underneath must not
/// type.
#[allow(clippy::too_many_lines)] // one port of the FORM_MODE layer set (form.tsx:547-720)
#[must_use]
pub fn form_key(synced: &FormBodyState, request: &FormRequest, key: &str, shift: bool) -> FormStep {
    let current = synced.current(request);
    let textual = current.is_some_and(FormField::textual);
    let editing_layer = (synced.editing || textual) && !synced.confirm(request);

    // The reply/cancel side effects land after the state match — the
    // caller's local queue keeps its borrows clean.
    let mut reply: Option<FormReply> = None;
    let mut cancel = false;
    let next = if editing_layer {
        match key {
            "escape" => {
                if textual {
                    cancel = true;
                    synced.clone()
                } else {
                    FormBodyState { editing: false, ..synced.clone() }
                }
            }
            "tab" | "return" | "enter" => {
                let (next, fired) = form_commit_input(synced, request, synced.draft.trim());
                let submitted = fired.is_some();
                reply = reply.or(fired);
                if !submitted && !FormBodyState::single(request) {
                    let dir: i8 = if key == "tab" && shift { -1 } else { 1 };
                    form_step_field(&next, request, synced.field, dir)
                } else {
                    next
                }
            }
            "up" if !textual && synced.selected > 0 => {
                FormBodyState { editing: false, selected: synced.selected - 1, ..synced.clone() }
            }
            "backspace" => {
                let mut draft = synced.draft.clone();
                draft.pop();
                FormBodyState { draft, ..synced.clone() }
            }
            other => {
                let mut chars = other.chars();
                match (chars.next(), chars.next()) {
                    (Some(ch), None) if !ch.is_control() => {
                        FormBodyState { draft: format!("{}{ch}", synced.draft), ..synced.clone() }
                    }
                    _ => synced.clone(),
                }
            }
        }
    } else {
        match key {
            "escape" | "ctrl+c" => {
                cancel = true;
                synced.clone()
            }
            "left" | "h" => form_step_field(synced, request, synced.field, -1),
            "right" | "l" | "tab" => form_step_field(synced, request, synced.field, 1),
            "up" | "k" => form_move(synced, request, -1),
            "down" | "j" => form_move(synced, request, 1),
            "enter" | "return" => {
                if synced.confirm(request) {
                    // The review step's submit (`submit`, form.tsx:
                    // 515-541): every field re-validates; the
                    // reply carries only answered keys.
                    reply = reply.or(form_reply(synced, request));
                    synced.clone()
                } else if current
                    .is_some_and(|field| matches!(field.kind, FieldKind::External { .. }))
                {
                    // External steps acknowledge on enter (the URL
                    // row's `open link` → `I finished` → continue
                    // chain, form.tsx:488-509 — headless-safe: the
                    // browser open degrades to the acknowledged
                    // state without spawning anything).
                    let mut answers = synced.answers.clone();
                    if let Some(field) = current {
                        answers.insert(field.key.clone(), serde_json::Value::Bool(true));
                    }
                    let acknowledged = FormBodyState { answers, ..synced.clone() };
                    if FormBodyState::single(request) {
                        reply = reply.or(form_reply(&acknowledged, request));
                        acknowledged
                    } else {
                        form_set_field(&acknowledged, request, synced.field + 1)
                    }
                } else {
                    let (next, fired) = form_pick(synced, request, synced.selected);
                    reply = reply.or(fired);
                    next
                }
            }
            other => {
                // Digits pick the n-th row (`1..9`, form.tsx:686);
                // a printable on the CUSTOM row opens the editor
                // (the interceptor, form.tsx:275-290).
                if let Some(digit) = other.chars().next().and_then(|c| c.to_digit(10)) {
                    if (1..=9).contains(&digit) {
                        let (next, fired) =
                            form_pick(synced, request, usize::try_from(digit).unwrap_or(1) - 1);
                        reply = reply.or(fired);
                        next
                    } else {
                        synced.clone()
                    }
                } else {
                    let mut chars = other.chars();
                    match (chars.next(), chars.next()) {
                        (Some(ch), None)
                            if !ch.is_control()
                                && current.is_some_and(|field| {
                                    field.custom() && synced.selected == field.rows().len()
                                }) =>
                        {
                            FormBodyState {
                                selected: current.map_or(0, |field| field.rows().len()),
                                editing: true,
                                draft: format!("{}{ch}", synced.draft),
                                ..synced.clone()
                            }
                        }
                        _ => synced.clone(),
                    }
                }
            }
        }
    };
    FormStep { state: next, reply, cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deployment_form() -> FormRequest {
        FormRequest {
            id: "frm_key".into(),
            session_id: "ses".into(),
            title: "Deployment".into(),
            fields: vec![FormField::text("service", "Service"), FormField::text("notes", "Notes")],
        }
    }

    fn picker_form() -> FormRequest {
        FormRequest {
            id: "frm_pick".into(),
            session_id: "s".into(),
            title: "Pick".into(),
            fields: vec![FormField {
                key: "k".into(),
                title: Some("K".into()),
                description: None,
                required: false,
                kind: FieldKind::Text {
                    placeholder: None,
                    options: Some(vec![
                        FormOption { value: "a".into(), label: "A".into(), description: None },
                        FormOption { value: "b".into(), label: "B".into(), description: None },
                    ]),
                    custom: Some(true),
                    min_length: None,
                    max_length: None,
                },
            }],
        }
    }

    /// The extracted `FORM_MODE` keymap keeps the pump's semantics:
    /// printables edit the draft, enter commits + advances, tab wraps
    /// back from the review step, escape on a textual field cancels.
    #[test]
    fn form_key_edits_commits_and_cancels() {
        let request = deployment_form();
        let state = FormBodyState::new(&request);
        // Printables append to the draft.
        let typing = form_key(&state, &request, "a", false);
        assert!(!typing.cancel);
        assert_eq!(typing.state.draft, "a");
        // Enter commits and advances to field 1; the second commit
        // lands on the review step; tab there wraps back to field 0
        // (`selectTab` modulo the fields + review tab).
        let step = form_key(&typing.state, &request, "enter", false);
        assert_eq!(step.state.field, 1);
        let review = form_key(&step.state, &request, "enter", false);
        assert_eq!(review.state.field, 2, "the review tab");
        let wrap = form_key(&review.state, &request, "tab", false);
        assert_eq!(wrap.state.field, 0, "tab wraps modulo the tab count");
        // The picker/review layer ignores shift on tab (the pump's
        // verbatim port); shift+tab steps back only in the EDITING
        // layer's tab arm.
        let forward = form_key(&review.state, &request, "tab", true);
        assert_eq!(forward.state.field, 0, "review tab ignores shift");
        let back = form_key(&step.state, &request, "tab", true);
        assert_eq!(back.state.field, 0, "editing-layer shift+tab steps back");
        // Escape on a TEXTUAL field cancels the form.
        let esc = form_key(&state, &request, "escape", false);
        assert!(esc.cancel, "textual escape dismisses");
    }

    /// The picker layer: arrows move, digits pick, enter picks the
    /// selection, a printable on the custom row opens the editor.
    #[test]
    fn form_key_drives_the_picker_layer() {
        let request = picker_form();
        let state = FormBodyState::new(&request);
        let down = form_key(&state, &request, "down", false);
        assert_eq!(down.state.selected, 1);
        // A digit picks the n-th row.
        let pick = form_key(&state, &request, "2", false);
        assert!(pick.reply.is_some(), "single-field picker replies");
        // A printable on the custom row opens the editor with the
        // draft seeded.
        let on_custom =
            form_key(&form_key(&state, &request, "down", false).state, &request, "down", false)
                .state;
        assert_eq!(on_custom.selected, 2, "the custom row");
        let typing = form_key(&on_custom, &request, "x", false);
        assert!(typing.state.editing, "the interceptor opens the editor");
        assert_eq!(typing.state.draft, "x");
        // Escape inside the editor closes it without cancelling.
        let closed = form_key(&typing.state, &request, "escape", false);
        assert!(!closed.cancel);
        assert!(!closed.state.editing);
    }
}
