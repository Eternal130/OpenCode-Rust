//! Mini footer form body (`mini/footer.form.tsx` + `form.shared.ts`):
//! multi-field question forms in the footer — title row
//! (`◆ Title n/total`), field labels, option rows (select/multiselect
//! with `[x]` marks + the custom-answer row), the free-text textarea,
//! and per-mode hints.

#![allow(clippy::too_many_lines)]

use super::canvas::{Canvas, Span};
use super::theme::FooterTheme;
use crate::render::Frame;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// The render port keeps the reference's shape (one long body render).

/// One selectable option of a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `FormField` kinds the footer renders (wire `type` tags).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldKind {
    #[serde(rename = "string")]
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Select {
        #[serde(default)]
        options: Vec<FormOption>,
        /// `"other": true`-style custom answers allowed.
        #[serde(default)]
        custom: bool,
    },
    #[serde(rename = "multiselect")]
    Multiselect {
        #[serde(default)]
        options: Vec<FormOption>,
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
            kind: FieldKind::Text { placeholder: None },
        }
    }

    /// `formLabel` — title ?? key.
    #[must_use]
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(self.key.as_str())
    }

    /// `formTextual` — free-text input fields.
    #[must_use]
    pub fn textual(&self) -> bool {
        matches!(self.kind, FieldKind::Text { .. })
    }

    /// `formCustom` — option lists that accept a custom answer.
    #[must_use]
    pub fn custom(&self) -> bool {
        match &self.kind {
            FieldKind::Select { custom, .. } => *custom,
            FieldKind::Multiselect { .. } => true,
            _ => false,
        }
    }

    /// `formRows` — the option rows.
    #[must_use]
    pub fn rows(&self) -> &[FormOption] {
        match &self.kind {
            FieldKind::Select { options, .. } | FieldKind::Multiselect { options } => options,
            _ => &[],
        }
    }

    /// `formPlaceholder`.
    #[must_use]
    pub fn placeholder(&self) -> &str {
        match &self.kind {
            FieldKind::Text { placeholder: Some(value) } => value.as_str(),
            _ => "Type your answer",
        }
    }

    /// `formValidateValue` — required fields must produce a value.
    #[must_use]
    pub fn validate(&self, answer: Option<&serde_json::Value>) -> Option<String> {
        let answer = answer?;
        match (&self.kind, answer) {
            (FieldKind::Multiselect { .. }, serde_json::Value::Array(items)) => {
                if self.required && items.is_empty() {
                    Some(format!("{} is required", self.label()))
                } else {
                    None
                }
            }
            (_, serde_json::Value::String(value)) => {
                if self.required && value.trim().is_empty() {
                    Some(format!("{} is required", self.label()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
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

    /// `formSingle` — one non-external select/boolean field submits
    /// directly (no review step).
    #[must_use]
    pub fn single(request: &FormRequest) -> bool {
        if request.fields.len() != 1 {
            return false;
        }
        matches!(request.fields[0].kind, FieldKind::Select { .. })
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

/// `formCommitInput` — validate + store a textual answer; advance the
/// field (or submit when the form is single-field). Returns the next
/// state and the reply when the form completed.
#[must_use]
pub fn form_commit_input(
    state: &FormBodyState,
    request: &FormRequest,
    text: &str,
) -> (FormBodyState, Option<FormReply>) {
    let Some(field) = state.current(request) else { return (state.clone(), None) };
    let value = serde_json::Value::String(text.to_owned());
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
        let mut items = state
            .answers
            .get(&field.key)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let value = serde_json::Value::String(row.value.clone());
        if let Some(position) = items.iter().position(|item| *item == value) {
            items.remove(position);
        } else {
            items.push(value);
        }
        next.answers.insert(field.key.clone(), serde_json::Value::Array(items));
        (next, None)
    } else {
        let value = serde_json::Value::String(row.value.clone());
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

// -- rendering ---------------------------------------------------------------

/// Render the form body (`footer-form` golden: multi-field textual
/// form, idle phase). Surface panel between two transparent rows.
#[must_use]
pub fn render_form_body(
    request: &FormRequest,
    state: &FormBodyState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    let mut canvas = Canvas::new(width, height);
    let compact = width < 56 || height < 12;
    if height < 2 {
        return canvas.into_frame();
    }
    let panel_end = height - 1; // inclusive — the body fills below the spacer
    for y in 1..=panel_end {
        canvas.fill_bg(y, 0, width, theme.surface);
    }
    let text_x = 2;
    // Title row: `◆` + title + `n/total` (or `Review`).
    let counter = if state.confirm(request) {
        "Review".to_owned()
    } else {
        format!("{}/{}", (state.field + 1).min(request.fields.len()), request.fields.len())
    };
    canvas.spans(
        2,
        text_x,
        &[
            Span::new("◆", theme.question, theme.surface),
            Span::new(" ", super::canvas::FILL_FG, theme.surface),
            Span::new(request.title.clone(), theme.text, theme.surface),
            Span::new(" ", super::canvas::FILL_FG, theme.surface),
            Span::new(counter, theme.muted, theme.surface),
        ],
    );
    // Content: the current field.
    let mut row = 4;
    if let Some(field) = state.current(request) {
        let multiple = matches!(field.kind, FieldKind::Multiselect { .. });
        let suffix = if field.required { " (required)" } else { "" }.to_owned()
            + if multiple { " (select all that apply)" } else { "" };
        canvas.spans(
            row,
            text_x,
            &[Span::new(
                format!("{}{suffix}", field.description.as_deref().unwrap_or(field.label())),
                theme.text,
                theme.surface,
            )],
        );
        row += 2;
        if !field.textual() && !state.editing {
            let rows = field.rows();
            for (index, option) in rows.iter().enumerate() {
                let active = state.selected == index;
                let bg = if active { theme.formfield_focused_bg } else { theme.surface };
                let picked = answer_picked(state, field, &option.value);
                let label_fg =
                    if active { theme.formfield_focused_text } else { theme.formfield_text };
                canvas.fill_bg(row, 0, width, bg);
                let mut spans = vec![Span::new(format!("{}.", index + 1), label_fg, bg)];
                if multiple {
                    spans.push(Span::new(
                        format!("[{}] ", if picked { "x" } else { " " }),
                        theme.selection,
                        bg,
                    ));
                }
                spans.push(Span::new(option.label.clone(), label_fg, bg));
                if !multiple && picked {
                    spans.push(Span::new(" *", theme.selection, bg));
                }
                canvas.spans(row, text_x, &spans);
                row += 1;
            }
            if field.custom() {
                let active = state.selected == rows.len();
                let bg = if active { theme.formfield_focused_bg } else { theme.surface };
                let label_fg =
                    if active { theme.formfield_focused_text } else { theme.formfield_text };
                canvas.fill_bg(row, 0, width, bg);
                canvas.spans(
                    row,
                    text_x,
                    &[
                        Span::new(format!("{}.", rows.len() + 1), label_fg, bg),
                        Span::new(
                            if state.draft.is_empty() {
                                "Type your own answer".to_owned()
                            } else {
                                state.draft.clone()
                            },
                            label_fg,
                            bg,
                        ),
                    ],
                );
                row += 1;
            }
        }
    }
    // The free-text textarea (textual fields): focused formfield
    // background across the content region (paddingRight 3).
    if let Some(field) = state.current(request) {
        if field.textual() || state.editing {
            let textarea_width = width.saturating_sub(text_x + 3);
            canvas.fill_bg(row, text_x, textarea_width, theme.formfield_focused_bg);
            if state.draft.is_empty() {
                canvas.spans(
                    row,
                    text_x,
                    &[Span::new(field.placeholder(), theme.muted, theme.formfield_focused_bg)],
                );
            } else {
                canvas.spans(
                    row,
                    text_x,
                    &[Span::new(
                        state.draft.clone(),
                        theme.formfield_focused_text,
                        theme.formfield_focused_bg,
                    )],
                );
            }
        }
    }
    // Hints at the panel's bottom (paddingBottom row below).
    let hints_y = panel_end - 1;
    let hints: Vec<Span> = if state.confirm(request) {
        vec![Span::new("enter submit   esc dismiss", theme.muted, theme.surface)]
    } else if state.current(request).is_some_and(|field| field.textual() || state.editing) {
        vec![Span::new("enter save   esc dismiss", theme.muted, theme.surface)]
    } else {
        vec![Span::new(
            "↑↓ select   enter choose   tab next   esc dismiss",
            theme.muted,
            theme.surface,
        )]
    };
    canvas.spans(hints_y, text_x, &hints);
    let _ = compact;
    canvas.into_frame()
}

fn answer_picked(state: &FormBodyState, field: &FormField, value: &str) -> bool {
    match state.answers.get(&field.key) {
        Some(serde_json::Value::Array(items)) => {
            items.iter().any(|item| item.as_str() == Some(value))
        }
        Some(serde_json::Value::String(answer)) => answer == value,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap::display_width;

    fn deployment_form() -> FormRequest {
        FormRequest {
            id: "frm_preempted".into(),
            session_id: "ses_child".into(),
            title: "Deployment".into(),
            fields: vec![
                FormField {
                    key: "service".into(),
                    title: Some("Service".into()),
                    description: None,
                    required: true,
                    kind: FieldKind::Text { placeholder: None },
                },
                FormField::text("notes", "Notes"),
            ],
        }
    }

    #[test]
    fn golden_frame_shape_at_80() {
        let theme = FooterTheme::fallback_dark();
        let request = deployment_form();
        let state = FormBodyState::new(&request);
        let frame = render_form_body(&request, &state, 80, 16, &theme);
        let grid = frame.text_grid();
        assert!(grid[2].starts_with("  ◆ Deployment 1/2"));
        assert!(grid[4].contains("Service (required)"));
        assert!(grid[6].contains("Type your answer"));
        assert!(grid[14].contains("enter save   esc dismiss"));
        // The textarea row paints the focused formfield background.
        assert_eq!(frame.cell(6, 2).bg, theme.formfield_focused_bg);
        assert_eq!(frame.cell(6, 77).bg, theme.surface, "paddingRight 3 stays surface");
    }

    #[test]
    fn textual_commit_validates_and_advances() {
        let request = deployment_form();
        let mut state = FormBodyState::new(&request);
        // Required empty text fails.
        let (invalid, reply) = form_commit_input(&state, &request, "  ");
        assert!(reply.is_none());
        assert!(invalid.error.contains("required"));
        // Valid text advances to the next field.
        let (next, reply) = form_commit_input(&state, &request, "api");
        assert!(reply.is_none());
        assert_eq!(next.field, 1, "Service (0) → Notes (1)");
        assert_eq!(next.answers.get("service").and_then(serde_json::Value::as_str), Some("api"));
        state = next;
        // The last field's commit lands on the review step; the reply
        // fires from there.
        let (done, reply) = form_commit_input(&state, &request, "ship it");
        assert!(reply.is_none(), "multi-field forms review before replying");
        assert_eq!(done.field, 2, "the review step");
        let reply = form_reply(&done, &request).expect("both required fields answered");
        assert_eq!(reply.form_id, "frm_preempted");
    }

    #[test]
    fn multiselect_toggles_and_single_select_replies() {
        let multi = FormRequest {
            id: "m".into(),
            session_id: "s".into(),
            title: "Pick".into(),
            fields: vec![FormField {
                key: "k".into(),
                title: Some("K".into()),
                description: None,
                required: true,
                kind: FieldKind::Multiselect {
                    options: vec![
                        FormOption { value: "a".into(), label: "A".into(), description: None },
                        FormOption { value: "b".into(), label: "B".into(), description: None },
                    ],
                },
            }],
        };
        let state = FormBodyState::new(&multi);
        let (toggled, reply) = form_pick(&state, &multi, 0);
        assert!(reply.is_none(), "multiselect does not submit");
        assert_eq!(toggled.answers.get("k").unwrap(), &serde_json::json!(["a"]), "picked a");
        let (toggled, _) = form_pick(&toggled, &multi, 0);
        assert_eq!(toggled.answers.get("k").unwrap(), &serde_json::json!([]));
        // A single-select form replies immediately.
        let single = FormRequest {
            id: "s1".into(),
            session_id: "s".into(),
            title: "One".into(),
            fields: vec![FormField {
                key: "k".into(),
                title: Some("K".into()),
                description: None,
                required: false,
                kind: FieldKind::Select {
                    options: vec![FormOption {
                        value: "x".into(),
                        label: "X".into(),
                        description: None,
                    }],
                    custom: false,
                },
            }],
        };
        let state = FormBodyState::new(&single);
        let (_, reply) = form_pick(&state, &single, 0);
        assert_eq!(reply.expect("single replies").answer["k"], serde_json::json!("x"));
    }

    #[test]
    fn sync_resets_only_on_a_new_form() {
        let request = deployment_form();
        let mut state = FormBodyState::new(&request);
        state.field = 1;
        assert_eq!(form_sync(&state, &request).field, 1);
        let other = FormRequest { id: "other".into(), ..request.clone() };
        assert_eq!(form_sync(&state, &other).field, 0);
    }

    #[test]
    fn move_wraps_the_selection() {
        let request = FormRequest {
            id: "m".into(),
            session_id: "s".into(),
            title: "Pick".into(),
            fields: vec![FormField {
                key: "k".into(),
                title: Some("K".into()),
                description: None,
                required: false,
                kind: FieldKind::Select {
                    options: vec![
                        FormOption { value: "a".into(), label: "A".into(), description: None },
                        FormOption { value: "b".into(), label: "B".into(), description: None },
                    ],
                    custom: true,
                },
            }],
        };
        let state = FormBodyState::new(&request);
        let down = form_move(&state, &request, 1);
        assert_eq!(down.selected, 1);
        let wrap = form_move(&down, &request, 1);
        assert_eq!(wrap.selected, 2, "the custom row participates");
        let wrap = form_move(&wrap, &request, 1);
        assert_eq!(wrap.selected, 0);
        // Picking the custom row opens the editor.
        let (editing, _) = form_pick(&wrap, &request, 2);
        assert!(editing.editing);
    }

    #[test]
    fn option_lists_render_checkboxes_and_custom_row() {
        let theme = FooterTheme::fallback_dark();
        let request = FormRequest {
            id: "m".into(),
            session_id: "s".into(),
            title: "Pick".into(),
            fields: vec![FormField {
                key: "k".into(),
                title: Some("K".into()),
                description: None,
                required: false,
                kind: FieldKind::Multiselect {
                    options: vec![FormOption {
                        value: "a".into(),
                        label: "Alpha".into(),
                        description: Some("first".into()),
                    }],
                },
            }],
        };
        let state = FormBodyState::new(&request);
        let frame = render_form_body(&request, &state, 80, 16, &theme);
        let grid = frame.text_grid();
        assert!(grid[6].contains("[ ]"), "unchecked multiselect box");
        assert!(grid[6].contains("Alpha"));
        assert!(grid[7].contains("Type your own answer"), "the custom row");
        assert!(grid[14].contains("↑↓ select"));
        assert!(display_width(&grid[6]) <= 80);
    }
}
