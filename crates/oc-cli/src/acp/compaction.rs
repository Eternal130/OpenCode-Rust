//! Compaction tracking (port of `acp/compaction.ts`): live and replayed
//! compactions become `compaction_update` chunks for capable clients or
//! an `opencode/compaction` `_meta` marker otherwise.

use std::collections::HashMap;

use agent_client_protocol::schema::v1::{
    CompactionStatus, CompactionSummaryChunk, CompactionUpdate, ContentBlock, SessionInfoUpdate,
    SessionUpdate, TextContent,
};
use serde_json::{json, Value};

/// `MarkerMeta` (compaction.ts:6).
pub const MARKER_META: &str = "opencode/compaction";

/// A tracked in-flight compaction (`Started`, compaction.ts:8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub message_id: String,
    pub reason: String,
}

/// `usesStandardUpdates` (compaction.ts:39-44): without child updates a
/// child's standard update would read as the parent's own.
#[must_use]
pub fn uses_standard_updates(compaction: bool, child_updates: bool, child: bool) -> bool {
    compaction && (!child || child_updates)
}

/// `apply` (compaction.ts:46-75) — a lifecycle event updates the tracked
/// set and yields the updates to send.
#[must_use]
pub fn apply(
    event_type: &str,
    session_id: &str,
    data: &Value,
    event_id: &str,
    tracked: &mut HashMap<String, Started>,
    standard_updates: bool,
) -> Vec<SessionUpdate> {
    if event_type == "session.compaction.started" {
        let started = open(data, event_id);
        tracked.insert(session_id.to_owned(), started.clone());
        return vec![update(&Compaction::Started(started), standard_updates)];
    }
    let current = tracked.remove(session_id);
    if event_type == "session.compaction.ended" {
        let Some(current) = current else { return Vec::new() };
        return vec![update(
            &Compaction::Completed {
                started: current,
                reason: string(data, "reason"),
                summary: string(data, "text"),
            },
            standard_updates,
        )];
    }
    // Auto compaction can fail without a started event; open it first for
    // standard clients (compaction.ts:65-73).
    let started = current.clone().unwrap_or_else(|| open(data, event_id));
    let mut updates = Vec::new();
    if current.is_none() && standard_updates {
        updates.push(update(&Compaction::Started(started.clone()), standard_updates));
    }
    updates.push(update(
        &Compaction::Failed {
            started,
            reason: string(data, "reason"),
            error: SessionErrorField {
                r#type: string_or(data.pointer("/error/type"), "unknown"),
                message: string_or(data.pointer("/error/message"), ""),
            },
        },
        standard_updates,
    ));
    updates
}

/// `chunk` (compaction.ts:77-80): stream summary text.
#[must_use]
pub fn chunk(
    started: Option<&Started>,
    text: &str,
    standard_updates: bool,
) -> Option<SessionUpdate> {
    let started = started?;
    if !standard_updates {
        return None;
    }
    Some(SessionUpdate::CompactionSummaryChunk(CompactionSummaryChunk::new(
        agent_client_protocol::schema::v1::CompactionId::new(started.message_id.clone()),
        ContentBlock::Text(TextContent::new(text)),
    )))
}

/// `abandon` (compaction.ts:82-87): a cancelled compaction fails.
#[must_use]
pub fn abandon(started: &Started, standard_updates: bool) -> SessionUpdate {
    update(
        &Compaction::Failed {
            started: started.clone(),
            reason: started.reason.clone(),
            error: SessionErrorField {
                r#type: "aborted".to_owned(),
                message: "Compaction cancelled".to_owned(),
            },
        },
        standard_updates,
    )
}

/// A replayed compaction message (compaction.ts:90-99).
#[must_use]
pub fn replay(message: &Value, standard_updates: bool) -> Option<SessionUpdate> {
    let status = message.get("status")?.as_str()?;
    if status == "running" {
        return None;
    }
    let started = Started { message_id: string(message, "id"), reason: string(message, "reason") };
    let compaction = if status == "failed" {
        Compaction::Failed {
            started,
            reason: string(message, "reason"),
            error: SessionErrorField {
                r#type: string_or(message.pointer("/error/type"), "unknown"),
                message: string_or(message.pointer("/error/message"), ""),
            },
        }
    } else {
        Compaction::Completed {
            started,
            reason: string(message, "reason"),
            summary: string(message, "summary"),
        }
    };
    Some(update(&compaction, standard_updates))
}

/// `open` (compaction.ts:102-108): matches core's compaction message id.
fn open(data: &Value, event_id: &str) -> Started {
    Started { message_id: string_or(data.get("inputID"), event_id), reason: string(data, "reason") }
}

#[allow(dead_code)]
enum Compaction {
    Started(Started),
    Completed { started: Started, reason: String, summary: String },
    Failed { started: Started, reason: String, error: SessionErrorField },
}

struct SessionErrorField {
    r#type: String,
    message: String,
}

/// `update` (compaction.ts:110-136).
fn update(compaction: &Compaction, standard_updates: bool) -> SessionUpdate {
    if !standard_updates {
        let marker = match compaction {
            Compaction::Started(started) => json!({
                "status": "started",
                "messageId": started.message_id,
                "reason": started.reason,
            }),
            Compaction::Completed { started, summary, .. } => json!({
                "status": "completed",
                "messageId": started.message_id,
                "reason": started.reason,
                "summary": summary,
            }),
            Compaction::Failed { started, error, .. } => json!({
                "status": "failed",
                "messageId": started.message_id,
                "reason": started.reason,
                "error": { "type": error.r#type, "message": error.message },
            }),
        };
        let mut meta = serde_json::Map::new();
        meta.insert(MARKER_META.to_owned(), marker);
        return SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().meta(meta));
    }
    let message_id: agent_client_protocol::schema::v1::CompactionId = match compaction {
        Compaction::Started(started)
        | Compaction::Completed { started, .. }
        | Compaction::Failed { started, .. } => {
            agent_client_protocol::schema::v1::CompactionId::new(started.message_id.clone())
        }
    };
    match compaction {
        Compaction::Started(_) => SessionUpdate::CompactionUpdate(CompactionUpdate::new(
            message_id,
            CompactionStatus::InProgress,
        )),
        Compaction::Completed { summary, .. } => SessionUpdate::CompactionUpdate(
            CompactionUpdate::new(message_id, CompactionStatus::Completed).summary(
                if summary.is_empty() {
                    agent_client_protocol::schema::MaybeUndefined::Null
                } else {
                    agent_client_protocol::schema::MaybeUndefined::Value(vec![ContentBlock::Text(
                        TextContent::new(summary),
                    )])
                },
            ),
        ),
        // `Cancelled` for aborts, otherwise failed with a display error
        // (compaction.ts:127-135).
        Compaction::Failed { error, .. } => {
            if error.r#type == "aborted" || error.r#type == "compaction.interrupted" {
                SessionUpdate::CompactionUpdate(
                    CompactionUpdate::new(message_id, CompactionStatus::Cancelled)
                        .summary(agent_client_protocol::schema::MaybeUndefined::Null),
                )
            } else {
                SessionUpdate::CompactionUpdate(
                    CompactionUpdate::new(message_id, CompactionStatus::Failed)
                        .summary(agent_client_protocol::schema::MaybeUndefined::Null)
                        .error(display_error(&error.r#type, &error.message)),
                )
            }
        }
    }
}

/// `displayError` (compaction.ts:139-142).
fn display_error(error_type: &str, message: &str) -> String {
    if message.is_empty() || (error_type == "compaction.failed" && message.contains('\n')) {
        "Compaction failed".to_owned()
    } else {
        message.to_owned()
    }
}

fn string(data: &Value, field: &str) -> String {
    string_or(data.get(field), "")
}

fn string_or(value: Option<&Value>, fallback: &str) -> String {
    value.and_then(Value::as_str).unwrap_or(fallback).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn started_then_ended_streams_standard_updates() {
        let mut tracked = HashMap::new();
        let updates = apply(
            "session.compaction.started",
            "ses_1",
            &json!({ "reason": "manual", "inputID": "msg_1" }),
            "evt_1",
            &mut tracked,
            true,
        );
        assert_eq!(updates.len(), 1);
        assert!(tracked.contains_key("ses_1"));
        let updates = apply(
            "session.compaction.ended",
            "ses_1",
            &json!({ "reason": "manual", "text": "the summary" }),
            "evt_2",
            &mut tracked,
            true,
        );
        let SessionUpdate::CompactionUpdate(update) = &updates[0] else {
            panic!("compaction update");
        };
        assert_eq!(update.status, CompactionStatus::Completed);
        assert!(tracked.is_empty());
    }

    #[test]
    fn without_the_capability_updates_become_meta_markers() {
        let mut tracked = HashMap::new();
        let updates = apply(
            "session.compaction.started",
            "ses_1",
            &json!({ "reason": "auto", "inputID": "msg_1" }),
            "evt_1",
            &mut tracked,
            false,
        );
        let SessionUpdate::SessionInfoUpdate(info) = &updates[0] else {
            panic!("info update");
        };
        assert_eq!(info.meta.as_ref().expect("meta")[MARKER_META]["status"], "started");
    }

    #[test]
    fn cancelled_compactions_report_cancelled_status() {
        let update =
            abandon(&Started { message_id: "msg_1".to_owned(), reason: "manual".to_owned() }, true);
        let SessionUpdate::CompactionUpdate(update) = update else {
            panic!("compaction update");
        };
        assert_eq!(update.status, CompactionStatus::Cancelled);
    }

    #[test]
    fn running_replays_are_skipped() {
        assert!(replay(&json!({ "id": "msg_1", "status": "running", "reason": "auto" }), true)
            .is_none());
        let update = replay(
            &json!({ "id": "msg_1", "status": "completed", "reason": "auto", "summary": "s" }),
            true,
        )
        .expect("replayed");
        assert!(matches!(update, SessionUpdate::CompactionUpdate(_)));
    }

    #[test]
    fn child_updates_never_stream_standard_updates_without_the_flag() {
        // compaction && (!child || childUpdates) — compaction.ts:43.
        assert!(!uses_standard_updates(true, false, true));
        assert!(uses_standard_updates(true, true, true));
        assert!(uses_standard_updates(true, false, false));
        assert!(!uses_standard_updates(false, true, true));
    }
}
