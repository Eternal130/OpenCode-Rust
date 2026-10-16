//! One-shot session generation (port of `session/generate.ts`).
//!
//! Generates text from the current session context **without mutating the
//! session**: the transcript is the same base transcript the drain would
//! send (agent system + initial instructions + settled history + the
//! instruction update when instructions changed), plus the caller's prompt
//! as a final user message. The request goes through the normal
//! `session.generate` hook path (`RequestKind::Generate`), then streams
//! once and returns the concatenated assistant text.

use crate::session::runner::context::{
    history_preview, provenance, render_initial, render_update, InstructionState, SessionContext,
};
use crate::session::runner::model_request::{
    base_transcript, RequestInput, RequestKind, SessionModelRequest,
};
use crate::session::runner::LlmClient;

use oc_ai::schema::messages::Message;

/// `SessionGenerate.Error`.
#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("Session not found: {0}")]
    NotFound(String),
    #[error("Agent not found: session {session} agent {agent}")]
    AgentNotFound { session: String, agent: String },
    #[error("Location mismatch: {0}")]
    LocationMismatch(String),
    #[error("Instruction initialization blocked by unavailable sources: {0:?}")]
    InstructionsBlocked(Vec<String>),
    #[error("{0}")]
    Model(String),
    #[error("{0}")]
    Llm(oc_ai::schema::AiError),
}

impl From<crate::session::runner::context::SelectError> for GenerateError {
    fn from(error: crate::session::runner::context::SelectError) -> Self {
        match error {
            crate::session::runner::context::SelectError::NotFound(id) => Self::NotFound(id),
            crate::session::runner::context::SelectError::AgentNotFound { session, agent } => {
                Self::AgentNotFound { session, agent }
            }
            crate::session::runner::context::SelectError::LocationMismatch(id) => {
                Self::LocationMismatch(id)
            }
            crate::session::runner::context::SelectError::InstructionsBlocked(keys) => {
                Self::InstructionsBlocked(keys)
            }
            crate::session::runner::context::SelectError::Db(error) => {
                Self::Model(error.to_string())
            }
        }
    }
}

/// The one-shot generation service (`SessionGenerate`).
pub struct SessionGenerate {
    pub(crate) db: crate::database::Database,
    pub(crate) context: std::sync::Arc<SessionContext>,
    pub(crate) requests: std::sync::Arc<SessionModelRequest>,
    pub(crate) llm: std::sync::Arc<dyn LlmClient>,
}

/// The previewed history: settled messages plus the rendered instruction
/// update, when the instructions changed since the last commit
/// (`SessionHistory.preview`).
struct PreviewedHistory {
    initial: String,
    messages: Vec<oc_schema::session_message::Info>,
    instruction_update: Option<String>,
}

impl SessionGenerate {
    #[must_use]
    pub fn new(
        db: crate::database::Database,
        context: std::sync::Arc<SessionContext>,
        requests: std::sync::Arc<SessionModelRequest>,
        llm: std::sync::Arc<dyn LlmClient>,
    ) -> Self {
        Self { db, context, requests, llm }
    }

    /// `SessionHistory.preview` (history.ts:143-165): the settled prefix
    /// (an active assistant may contain an unresolved tool call) plus the
    /// instruction state without committing it.
    async fn preview_history(
        &self,
        session_id: &str,
        instructions: &crate::session::runner::context::InstructionList,
        boundary: &crate::session::runner::context::Boundary,
    ) -> Result<PreviewedHistory, GenerateError> {
        let messages = history_preview(&self.db, session_id, boundary)
            .await
            .map_err(|error| GenerateError::Model(format!("history load failed: {error}")))?;
        // `InstructionState.preview` (instruction-state.ts:184-208): the
        // observation without commit. A session with no committed state
        // renders its initial from the observed sources; a change since
        // the last commit renders the update text the request appends as
        // a system message.
        let observation = InstructionState::observe(&self.db, instructions, session_id)
            .await
            .map_err(GenerateError::InstructionsBlocked)?;
        let (initial, instruction_update) = if observation.initial {
            let values: std::collections::BTreeMap<String, serde_json::Value> = observation
                .current
                .iter()
                .filter_map(|(key, hash)| {
                    observation.admission.blobs.get(hash).cloned().map(|value| (key.clone(), value))
                })
                .collect();
            (render_initial(instructions, &values), None)
        } else {
            let initial = InstructionState::initial(&self.db, instructions, session_id)
                .await
                .map_err(|error| GenerateError::Model(format!("history load failed: {error}")))?;
            let previous: std::collections::BTreeMap<String, serde_json::Value> = observation
                .previous
                .iter()
                .filter_map(|(key, hash)| {
                    if hash == crate::session::runner::context::REMOVED_SENTINEL {
                        return None;
                    }
                    observation.admission.blobs.get(hash).cloned().map(|value| (key.clone(), value))
                })
                .collect();
            let delta: std::collections::BTreeMap<String, Option<serde_json::Value>> = observation
                .admission
                .delta
                .iter()
                .map(|(key, hash)| {
                    let value = if hash == crate::session::runner::context::REMOVED_SENTINEL {
                        None
                    } else {
                        Some(
                            observation
                                .admission
                                .blobs
                                .get(hash)
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                        )
                    };
                    (key.clone(), value)
                })
                .collect();
            let update = if observation.admission.delta.is_empty() {
                None
            } else {
                let text = render_update(instructions, &previous, &delta);
                (!text.is_empty()).then_some(text)
            };
            (initial, update)
        };
        Ok(PreviewedHistory { initial, messages, instruction_update })
    }

    /// `SessionGenerate.generate` (generate.ts:31-63): no session side
    /// effects — nothing is admitted, published, or persisted.
    pub async fn generate(&self, session_id: &str, prompt: &str) -> Result<String, GenerateError> {
        let selection = self.context.select(session_id).await?;
        let model = self
            .context
            .resolve_model(&selection.session)
            .map_err(|error| GenerateError::Model(error.to_string()))?;
        let boundary = match provenance(&model) {
            Some(provenance) => crate::session::runner::context::Boundary::Provenance(provenance),
            None => crate::session::runner::context::Boundary::Local,
        };
        let history = self.preview_history(session_id, &selection.instructions, &boundary).await?;
        let transcript = base_transcript(
            &selection.agent,
            &model,
            &selection.tools,
            &history.initial,
            &history.messages,
        );
        let mut messages = transcript.messages;
        if let Some(update) = history.instruction_update {
            messages.push(Message::system(update));
        }
        messages.push(Message::user(prompt));
        let prepared = self
            .requests
            .prepare(
                RequestKind::Generate,
                RequestInput {
                    session: selection.session.clone(),
                    agent: selection.agent_id.clone(),
                    model: model.clone(),
                    tools: Some(selection.tools.clone()),
                    system: transcript.system,
                    messages,
                    tool_choice: None,
                    web_socket: false,
                    input_tokens: None,
                },
            )
            .await
            .map_err(|error| GenerateError::Model(error.to_string()))?;
        let mut stream = self.llm.stream(prepared.request).await.map_err(GenerateError::Llm)?;
        let mut text = String::new();
        use futures::StreamExt;
        while let Some(event) = stream.next().await {
            if let oc_ai::schema::LLMEvent::TextDelta { text: delta, .. } =
                event.map_err(GenerateError::Llm)?
            {
                text.push_str(&delta);
            }
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_error_maps_select_errors() {
        let error = GenerateError::from(crate::session::runner::context::SelectError::NotFound(
            "s1".into(),
        ));
        assert!(matches!(error, GenerateError::NotFound(id) if id == "s1"));
        let error =
            GenerateError::from(crate::session::runner::context::SelectError::AgentNotFound {
                session: "s1".into(),
                agent: "build".into(),
            });
        assert!(matches!(error, GenerateError::AgentNotFound { .. }));
    }
}
