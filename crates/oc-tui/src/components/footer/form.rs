//! Mini footer form body (`mini/footer.form.tsx` + `form.shared.ts`):
//! multi-field question forms in the footer — title row
//! (`◆ Title n/total`), field labels, option rows (select/multiselect
//! with `[x]` marks + the custom-answer row), the free-text textarea,
//! and per-mode hints.
//!
//! The form STATE MODEL (fields, validation, the `form*` pure
//! transitions, and the [`form_key`] keymap) lives in
//! [`super::state`] — extracted painter-agnostic in lane R2 step 1;
//! this module re-exports it and keeps only the painter.

#![allow(clippy::too_many_lines)]

use super::canvas::{Canvas, Span};
use super::theme::FooterTheme;
use crate::render::Frame;

pub use super::state::{
    form_commit_input, form_key, form_move, form_pick, form_reply, form_set_field, form_step_field,
    form_sync, FieldKind, FormBodyState, FormCancel, FormField, FormOption, FormReply, FormRequest,
    FormRow, FormRowValue, FormStep,
};


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

fn answer_picked(state: &FormBodyState, field: &FormField, value: &FormRowValue) -> bool {
    match (state.answers.get(&field.key), value) {
        (Some(serde_json::Value::Array(items)), FormRowValue::Str(text)) => {
            items.iter().any(|item| item.as_str() == Some(text.as_str()))
        }
        (Some(serde_json::Value::String(answer)), FormRowValue::Str(text)) => answer == text,
        (Some(serde_json::Value::Bool(answer)), FormRowValue::Bool(flag)) => answer == flag,
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
                    kind: text_kind(None),
                },
                FormField::text("notes", "Notes"),
            ],
        }
    }

    /// A `string` field kind (bare, or with options).
    fn text_kind(options: Option<Vec<FormOption>>) -> FieldKind {
        FieldKind::Text {
            placeholder: None,
            options,
            custom: None,
            min_length: None,
            max_length: None,
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
        // Required empty text fails (the callers pass the TRIMMED
        // draft — `submitInput`, form.tsx:418).
        let (invalid, reply) = form_commit_input(&state, &request, "");
        assert!(reply.is_none());
        assert_eq!(invalid.error, "Answer required");
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
                    custom: None,
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
                kind: text_kind(Some(vec![FormOption {
                    value: "x".into(),
                    label: "X".into(),
                    description: None,
                }])),
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
                    custom: Some(true),
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

    // -- TUI-TD17: the typed field kinds ----------------------------------

    /// The full v2 field family parses off the wire (the old `FieldKind`
    /// dropped the WHOLE form on boolean/number/integer).
    #[test]
    fn typed_field_kinds_parse_off_the_wire() {
        let wire = serde_json::json!({
            "id": "frm_types",
            "sessionID": "ses_form",
            "title": "Typed fields",
            "fields": [
                { "type": "boolean", "key": "flag", "title": "Ship it" },
                { "type": "number", "key": "count", "title": "Count",
                  "minimum": 1, "maximum": 10, "required": true },
                { "type": "integer", "key": "items", "title": "Items", "required": true },
                { "type": "string", "key": "pick", "title": "Pick",
                  "options": [ { "value": "a", "label": "A" } ], "custom": true },
                { "type": "multiselect", "key": "many", "title": "Many",
                  "options": [ { "value": "x", "label": "X" } ], "custom": true }
            ]
        });
        let parsed: FormRequest = serde_json::from_value(wire).expect("the typed form parses");
        assert!(matches!(parsed.fields[0].kind, FieldKind::Boolean));
        assert!(matches!(parsed.fields[1].kind, FieldKind::Number { .. }));
        assert!(matches!(parsed.fields[2].kind, FieldKind::Integer { .. }));
        assert!(matches!(
            parsed.fields[3].kind,
            FieldKind::Text { options: Some(_), custom: Some(true), .. }
        ));
        assert!(matches!(parsed.fields[4].kind, FieldKind::Multiselect { custom: Some(true), .. }));
    }

    /// The typed helpers (`formTextual`/`formRows`/`formCustom`):
    /// booleans render the Yes/No pair; number/integer are textual.
    #[test]
    fn typed_field_helpers_match_the_reference() {
        let boolean = FormField {
            key: "flag".into(),
            title: Some("Ship it".into()),
            description: None,
            required: false,
            kind: FieldKind::Boolean,
        };
        assert!(!boolean.textual());
        assert!(!boolean.custom());
        let rows = boolean.rows();
        assert_eq!(rows.len(), 2, "the Yes/No pair");
        assert_eq!(rows[0].label, "Yes");
        assert_eq!(rows[0].value, FormRowValue::Bool(true));
        assert_eq!(rows[1].value, FormRowValue::Bool(false));

        let number = FormField {
            key: "count".into(),
            title: Some("Count".into()),
            description: None,
            required: true,
            kind: FieldKind::Number { minimum: Some(1.0), maximum: Some(10.0) },
        };
        assert!(number.textual(), "numbers type into the textarea");
        assert_eq!(number.placeholder(), "1-10", "the range placeholder");
        assert_eq!(
            number.validate(Some(&serde_json::json!(0.5))),
            Some("Must be at least 1".to_owned())
        );
        assert_eq!(
            number.validate(Some(&serde_json::json!(11.0))),
            Some("Must be at most 10".to_owned())
        );
        assert_eq!(
            number.validate(Some(&serde_json::json!("x"))),
            Some("Expected a number".to_owned())
        );

        let integer = FormField {
            key: "items".into(),
            title: Some("Items".into()),
            description: None,
            required: true,
            kind: FieldKind::Integer { minimum: None, maximum: None },
        };
        assert_eq!(
            integer.validate(Some(&serde_json::json!(2.5))),
            Some("Expected an integer".to_owned())
        );

        // Multiselect without the custom flag has NO custom row
        // (`formCustom`: multiselect is custom only when the flag is
        // set — the old port always allowed it).
        let multi = FormField {
            key: "many".into(),
            title: Some("Many".into()),
            description: None,
            required: false,
            kind: FieldKind::Multiselect {
                options: vec![FormOption {
                    value: "x".into(),
                    label: "X".into(),
                    description: None,
                }],
                custom: None,
            },
        };
        assert!(!multi.custom(), "no custom row without the flag");

        // Required checks speak the reference's messages.
        assert_eq!(number.validate(None), Some("Answer required".to_owned()));
    }

    /// Typed answers flow through pick/commit with typed JSON values:
    /// booleans reply `true`/`false`, numbers parse the draft.
    #[test]
    fn typed_answers_commit_typed_json_values() {
        // Single boolean form: picking Yes replies immediately.
        let single = FormRequest {
            id: "frm_b".into(),
            session_id: "s".into(),
            title: "Ship".into(),
            fields: vec![FormField {
                key: "flag".into(),
                title: Some("Ship it".into()),
                description: None,
                required: false,
                kind: FieldKind::Boolean,
            }],
        };
        assert!(FormBodyState::single(&single), "boolean single-submits");
        let state = FormBodyState::new(&single);
        let (next, reply) = form_pick(&state, &single, 0);
        let reply = reply.expect("picking Yes replies");
        assert_eq!(reply.answer["flag"], serde_json::json!(true));
        assert_eq!(next.selected, 0);

        // Number fields parse the draft on commit (2.5 → JSON 2.5).
        let multi = FormRequest {
            id: "frm_n".into(),
            session_id: "s".into(),
            title: "Typed".into(),
            fields: vec![
                FormField {
                    key: "count".into(),
                    title: Some("Count".into()),
                    description: None,
                    required: true,
                    kind: FieldKind::Number { minimum: Some(1.0), maximum: Some(10.0) },
                },
                FormField {
                    key: "items".into(),
                    title: Some("Items".into()),
                    description: None,
                    required: true,
                    kind: FieldKind::Integer { minimum: None, maximum: None },
                },
            ],
        };
        let state = FormBodyState::new(&multi);
        let (bad, reply) = form_commit_input(&state, &multi, "abc");
        assert!(reply.is_none());
        assert_eq!(bad.error, "Expected a number", "a non-numeric draft fails");
        let (next, _) = form_commit_input(&state, &multi, "2.5");
        assert_eq!(next.answers.get("count"), Some(&serde_json::json!(2.5)), "float parsed");
        let (done, _) = form_commit_input(&next, &multi, "7");
        assert_eq!(done.field, 2, "both commits landed on the review step");
        let reply = form_reply(&done, &multi).expect("all fields answered");
        assert_eq!(reply.answer["count"], serde_json::json!(2.5));
        assert_eq!(reply.answer["items"], serde_json::json!(7));
        // Integers reject fractional drafts.
        let (bad, _) = form_commit_input(&next, &multi, "1.5");
        assert_eq!(bad.error, "Expected an integer");
    }
}
