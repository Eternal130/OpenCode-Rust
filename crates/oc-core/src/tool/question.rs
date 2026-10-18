//! Port of `packages/core/src/tool/plugin/question.ts` — the question tool
//! (§4.11): permission assert, one Form per ask, exact answer content.

use std::sync::Arc;

use oc_schema::form::{Field, FormValue};
use oc_schema::misc::{QuestionAnswer, QuestionPrompt};

use crate::forms::Forms;
use crate::permission::{AssertInput, PermissionError};
use crate::tool::{
    permission_cause, BuiltinTool, ToolExecuteWithContext, ToolFailure, ToolOutcome, ToolServices,
};

pub fn description() -> String {
    "Use this tool when you need to ask the user questions during execution. This allows you to:\n1. Gather user preferences or requirements\n2. Clarify ambiguous instructions\n3. Get decisions on implementation choices as you work\n4. Offer choices to the user about what direction to take.\n\nUsage notes:\n- A \"Type your own answer\" option is added automatically; don't include a separate option for free form answers\n- Set `multiple: true` to allow selecting more than one option\n- If you recommend a specific option, make that the first option in the list and add \"(Recommended)\" at the end of the label".to_string()
}

/// `QuestionTool.CancelledError` — the deliberate defect tunnel: a dismissal
/// without a message ends the step instead of becoming model-facing output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("The user dismissed this question")]
pub struct CancelledError;

/// `QuestionTool.toModelContent`.
#[must_use]
pub fn to_model_content(questions: &[QuestionPrompt], answers: &[QuestionAnswer]) -> String {
    let formatted = questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let answer = answers
                .get(index)
                .filter(|answers| !answers.is_empty())
                .map(|answers| answers.join(", "))
                .unwrap_or_else(|| "Unanswered".to_string());
            format!("\"{}\"=\"{}\"", question.question, answer)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("User has answered your questions: {formatted}. You can now continue with the user's answers in mind.")
}

/// `toField` — one question becomes one form field.
fn to_field(question: &QuestionPrompt, index: usize) -> Field {
    Field::String {
        key: format!("q{index}"),
        title: Some(question.header.clone()),
        description: Some(question.question.clone()),
        required: None,
        hidden: None,
        when: None,
        format: None,
        min_length: None,
        max_length: None,
        pattern: None,
        placeholder: None,
        default: None,
        options: Some(
            question
                .options
                .iter()
                .map(|option| oc_schema::form::Option_ {
                    value: option.label.clone(),
                    label: option.label.clone(),
                    description: Some(option.description.clone()),
                })
                .collect(),
        ),
        custom: Some(true),
    }
}

/// The question leaf executor.
pub struct QuestionExecute {
    pub forms: Forms,
}

impl ToolExecuteWithContext for QuestionExecute {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a serde_json::Value,
        context: &'a crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        let input = input.clone();
        let context = context.clone();
        let forms = self.forms.clone();
        Box::pin(async move {
            let questions: Vec<QuestionPrompt> = serde_json::from_value(
                input.get("questions").cloned().unwrap_or(serde_json::Value::Array(vec![])),
            )
            .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            if questions.is_empty() {
                return Err(ToolFailure::message(
                    "Invalid arguments for tool \"question\":\n- questions: Expected a value with a length of at least 1\n\nArguments provided:\n{\n  \"questions\": []\n}\n\nUpdate the arguments and call the tool again.",
                ));
            }
            services
                .permission
                .assert(AssertInput {
                    id: None,
                    session_id: context.session_id.clone(),
                    agent: Some(context.agent.clone()),
                    action: "question".into(),
                    resources: vec!["*".into()],
                    save: None,
                    metadata: None,
                    source: Some(oc_schema::permission::Source::Tool {
                        message_id: context.message_id.clone(),
                        id: context.call_id.clone(),
                    }),
                })
                .await
                .map_err(|error: PermissionError| ToolFailure {
                    message: "Permission denied: question".into(),
                    cause: Some(permission_cause(&error)),
                })?;
            let mut fields = Vec::with_capacity(questions.len());
            for (index, question) in questions.iter().enumerate() {
                fields.push(to_field(question, index));
            }
            let state = forms
                .ask(crate::forms::NewForm {
                    session_id: context.session_id.to_string(),
                    title: "Questions".into(),
                    metadata: Some(
                        serde_json::from_value(serde_json::json!({
                            "kind": "question",
                            "tool": { "messageID": context.message_id, "id": context.call_id }
                        }))
                        .unwrap_or_default(),
                    ),
                    fields,
                })
                .await
                .map_err(|error| ToolFailure::message(error.to_string()))?;
            match state {
                oc_schema::form::State::Answered { answer } => {
                    let answers: Vec<QuestionAnswer> = questions
                        .iter()
                        .enumerate()
                        .map(|(index, _)| {
                            let value = answer.get(&format!("q{index}"));
                            match value {
                                None | Some(FormValue::Bool(false)) => vec![],
                                Some(FormValue::String(text)) => vec![text.clone()],
                                Some(FormValue::Number(number)) => vec![number.to_string()],
                                Some(FormValue::Bool(true)) => vec![],
                                Some(FormValue::Strings(items)) => items.clone(),
                            }
                        })
                        .collect();
                    let content = to_model_content(&questions, &answers);
                    Ok(ToolOutcome {
                        content: vec![oc_schema::tool::Content::Text { text: content }],
                        metadata: Some(
                            serde_json::from_value(serde_json::json!({ "answers": answers }))
                                .unwrap_or_default(),
                        ),
                        output: Some(serde_json::json!({ "answers": answers })),
                    })
                }
                // Deliberate defect tunnel: a dismissal must never become
                // model-facing tool output; it resurfaces as a typed
                // failure at the step level. A dismissal WITH a message is
                // ordinary model-facing feedback.
                oc_schema::form::State::Cancelled { message: Some(message) } => {
                    Err(ToolFailure::message(message))
                }
                oc_schema::form::State::Cancelled { message: None } => {
                    // Defect tunnel: resurfaces as a step interrupt, never
                    // model-facing tool output.
                    Err(ToolFailure {
                        message: CancelledError.to_string(),
                        cause: Some(super::FailureCause::Declined {
                            message: CancelledError.to_string(),
                        }),
                    })
                }
                oc_schema::form::State::Pending => {
                    Err(ToolFailure::message("Form settled without an answer"))
                }
            }
        })
    }
}

/// The question builtin registration.
pub fn tool(forms: Forms) -> BuiltinTool {
    BuiltinTool {
        name: "question",
        description: description(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string" },
                            "header": { "type": "string" },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" }
                                    },
                                    "required": ["label"]
                                }
                            },
                            "multiple": { "type": "boolean" }
                        },
                        "required": ["question", "header", "options"]
                    }
                }
            },
            "required": ["questions"]
        }),
        permission: Some("question"),
        codemode: false,
        namespace: None,
        execute: Arc::new(QuestionExecute { forms }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(question: &str, header: &str) -> QuestionPrompt {
        QuestionPrompt {
            question: question.into(),
            header: header.into(),
            options: vec![oc_schema::misc::QuestionOption {
                label: "Yes".into(),
                description: "Continue".into(),
            }],
            multiple: None,
        }
    }

    #[test]
    fn tool_question_formats_answers_for_the_model() {
        let questions = vec![prompt("Continue?", "Continue"), prompt("Which?", "Options")];
        let answers = vec![vec!["Yes".into()], vec![]];
        assert_eq!(
            to_model_content(&questions, &answers),
            "User has answered your questions: \"Continue?\"=\"Yes\", \"Which?\"=\"Unanswered\". You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn tool_question_multiselect_answers_join_with_commas() {
        let questions = vec![prompt("Pick", "Pick")];
        let answers = vec![vec!["a".into(), "b".into()]];
        assert_eq!(
            to_model_content(&questions, &answers),
            "User has answered your questions: \"Pick\"=\"a, b\". You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn tool_question_field_shape() {
        let field = to_field(&prompt("Continue?", "Continue"), 0);
        match field {
            Field::String { key, custom, options, .. } => {
                assert_eq!(key, "q0");
                assert_eq!(custom, Some(true));
                assert_eq!(options.unwrap()[0].value, "Yes");
            }
            _ => panic!("expected string field"),
        }
    }
}
