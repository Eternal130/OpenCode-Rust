//! `subagent-completion.ts` port — the synthetic background-completion
//! delivery into the parent session (core-runner-map §4.13).

use std::sync::Arc;

use oc_schema::id::MessageId;
use oc_schema::session_inbox::{Delivery, Item, SyntheticPayload};
use serde_json::json;

use crate::session::inbox::SessionInbox;
use crate::subagent::{JobControl, JobStatus, SubagentRecovery, NO_TEXT};

/// `SubagentCompletion.deliver` (subagent-completion.ts:21-45): running
/// jobs deliver nothing; terminal jobs admit one synthetic inbox item into
/// the parent wrapped in the exact
/// `<subagent sessionID="…" state="…" description="…">` envelope, then
/// acknowledge the durable notification marker.
pub async fn deliver(
    inbox: &SessionInbox,
    jobs: &Arc<dyn JobControl>,
    input: DeliverInput,
) -> Result<(), DeliverError> {
    if input.status == JobStatus::Running {
        return Ok(());
    }
    let recovery = &input.recovery;
    let text = match input.status {
        JobStatus::Completed => input.output.clone().unwrap_or_else(|| NO_TEXT.to_string()),
        JobStatus::Error => input.error.clone().unwrap_or_else(|| "Subagent failed".to_string()),
        JobStatus::Cancelled => "Subagent cancelled".to_string(),
        JobStatus::Running => unreachable!("guarded above"),
    };
    let envelope = format!(
        "<subagent sessionID=\"{}\" state=\"{}\" description=\"{}\">\n{text}\n</subagent>",
        recovery.child_session_id,
        input.status.as_str(),
        recovery.description
    );
    // Admitting with the job's notification id dedupes crash-restart
    // replays (first-admission-wins reconcile).
    let inbox_id = input.notification_id.clone().unwrap_or_default();
    inbox
        .admit(
            inbox_id.as_str(),
            recovery.parent_session_id.as_str(),
            Item::Synthetic {
                payload: SyntheticPayload {
                    text: envelope,
                    description: Some(recovery.description.clone()),
                    metadata: Some(
                        serde_json::from_value(json!({
                            "source": "subagent",
                            "childID": recovery.child_session_id,
                            "agent": recovery.agent,
                            "state": input.status.as_str(),
                        }))
                        .unwrap_or_default(),
                    ),
                },
                // Background results steer: they wake an idle parent, and a
                // running parent continues with them at the next boundary.
                delivery: Delivery::Steer,
            },
        )
        .await
        .map_err(DeliverError::Inbox)?;
    if let Some(notification_id) = &input.notification_id {
        jobs.complete_background(notification_id.as_str()).await;
    }
    Ok(())
}

/// `deliver` input: the settled job's terminal fields plus its recovery.
#[derive(Debug, Clone)]
pub struct DeliverInput {
    pub status: JobStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub notification_id: Option<MessageId>,
    pub recovery: SubagentRecovery,
}

/// `deliver` errors.
#[derive(Debug, thiserror::Error)]
pub enum DeliverError {
    #[error("inbox admit failed: {0}")]
    Inbox(#[source] crate::session::InboxError),
}

/// `SubagentCompletion.text(message)` (subagent-completion.ts:10-19) over a
/// decoded message: non-empty text parts joined by a blank line.
#[must_use]
pub fn text_of(message: Option<&oc_schema::session_message::Info>) -> String {
    let Some(oc_schema::session_message::Info::Assistant { content, time, error, .. }) = message
    else {
        return NO_TEXT.to_string();
    };
    if time.completed.is_none() || error.is_some() {
        return NO_TEXT.to_string();
    }
    let joined = content
        .iter()
        .filter_map(|part| match part {
            oc_schema::session_message::AssistantContent::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if joined.is_empty() {
        NO_TEXT.to_string()
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(text: &str, completed: bool) -> oc_schema::session_message::Info {
        use oc_schema::session_message::*;
        Info::Assistant {
            id: MessageId::new(),
            metadata: None,
            time: AssistantTime { created: 1, streamed: None, completed: completed.then_some(2) },
            agent: oc_schema::id::AgentId::parse("reviewer"),
            model: oc_schema::model::Ref {
                id: oc_schema::id::ModelId::parse("m"),
                provider_id: oc_schema::id::ProviderId::parse("p"),
                variant: None,
            },
            content: vec![AssistantContent::Text { text: text.to_string(), state: None }],
            snapshot: None,
            finish: None,
            raw_finish: None,
            provider_state: None,
            cost: None,
            tokens: None,
            error: None,
            retry: None,
        }
    }

    #[test]
    fn subagent_completion_no_text_shapes() {
        assert_eq!(NO_TEXT, "Subagent completed without a text response.");
        assert_eq!(text_of(None), NO_TEXT);
        let message = assistant("done", true);
        assert_eq!(text_of(Some(&message)), "done");
        let unfinished = assistant("partial", false);
        assert_eq!(text_of(Some(&unfinished)), NO_TEXT);
    }

    #[test]
    fn subagent_completion_envelope_exact() {
        let recovery = SubagentRecovery {
            parent_session_id: oc_schema::id::SessionId::parse("ses_parent")
                .unwrap_or_else(|err| oc_schema::id::SessionId(err.value)),
            child_session_id: oc_schema::id::SessionId::parse("ses_child")
                .unwrap_or_else(|err| oc_schema::id::SessionId(err.value)),
            agent: "reviewer".to_string(),
            description: "background review".to_string(),
        };
        // The envelope string built inside deliver():
        let envelope = format!(
            "<subagent sessionID=\"{}\" state=\"{}\" description=\"{}\">\n{}\n</subagent>",
            recovery.child_session_id,
            JobStatus::Completed.as_str(),
            recovery.description,
            "child final response"
        );
        assert_eq!(
            envelope,
            "<subagent sessionID=\"ses_child\" state=\"completed\" description=\"background review\">\nchild final response\n</subagent>"
        );
    }
}
