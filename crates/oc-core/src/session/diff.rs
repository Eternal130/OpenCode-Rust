//! Turn-range session diffs (port of `session/diff.ts`).
//!
//! Diff the files changed by the turn containing a user message. A turn
//! runs from the first prompt after the session was last idle until the
//! next idle marker, so prompts steered in while it was busy belong to the
//! same turn; `to` extends the range through the turn containing a later
//! user message. The range's first recorded start snapshot is compared
//! with its last recorded end snapshot; only a step still running in the
//! active session compares against the working copy. Like VCS diffs, an
//! omitted `context` yields full-file patches.
//!
//! A session without any idle marker predates them, so its prompts span
//! until the next user message instead.
//!
//! Snapshot trees live in the repository of the location that captured
//! them, so a range spanning a location switch is rejected rather than
//! diffed wrongly. Resolution is split in two phases so the caller can
//! assemble the resolved location's snapshot service between them.

use oc_schema::location;
use oc_schema::session_support::FileDiff;

use crate::database::schema::{SessionMessageRow, SessionRow};
use crate::database::Database;

/// `vcs/patch.ts` `PATCH_CONTEXT_LINES` — an omitted `context` yields
/// full-file patches.
pub const PATCH_CONTEXT_LINES: u32 = 2_147_483_647;

/// `Session.TurnRangeError` (diff.ts:11-16).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Session turn range error: {message}")]
pub struct TurnRangeError {
    pub session_id: String,
    pub field: TurnField,
    pub message: String,
}

/// `TurnRangeError["field"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnField {
    From,
    To,
}

/// `SessionDiff` failures.
#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    /// `MessageNotFoundError`.
    #[error("Message not found: {message_id}")]
    MessageNotFound { session_id: String, message_id: String },
    #[error("{0}")]
    TurnRange(TurnRangeError),
    #[error("database error: {0}")]
    Db(String),
}

/// The resolved turn range (phase one): which snapshots to compare, in
/// which location's repository, and whether the still-running step must
/// compare against a fresh working-copy capture.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnPlan {
    /// The location whose snapshot repository holds both trees.
    pub location: location::Ref,
    /// The range's first recorded start snapshot.
    pub from: String,
    /// The range's last recorded end snapshot; `None` when only a fresh
    /// capture can close the range.
    pub recorded: Option<String>,
    /// A step is still running in the active session.
    pub capture: bool,
    /// Context lines (full-file by default).
    pub context: u32,
}

/// One `user`/`idle` anchor row.
#[derive(Debug, Clone)]
struct AnchorRow {
    id: String,
    kind: String,
    seq: i64,
}

/// One assistant step bracket.
#[derive(Debug, Clone)]
struct StepRow {
    seq: i64,
    start: Option<String>,
    end: Option<String>,
    completed: Option<i64>,
}

/// One `location-switched` row.
#[derive(Debug, Clone)]
struct SwitchRow {
    seq: i64,
    location: Option<serde_json::Value>,
    previous: Option<serde_json::Value>,
}

async fn anchor_rows(
    db: &Database,
    session_id: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<AnchorRow>, DiffError> {
    let session = session_id.to_string();
    let (from, to) = (from.map(str::to_string), to.map(str::to_string));
    db.transaction(move |tx| {
        let mut statement = tx.conn().prepare(
            "SELECT id, type, seq FROM session_message WHERE session_id = ?1 \
                 AND (type IN ('user', 'idle') OR id = ?2 OR id = ?3) ORDER BY seq ASC",
        )?;
        let rows = statement
            .query_map(
                rusqlite::params![session, from.unwrap_or_default(), to.unwrap_or_default()],
                |row| {
                    Ok(AnchorRow {
                        id: row.get::<_, String>(0)?,
                        kind: row.get::<_, String>(1)?,
                        seq: row.get::<_, i64>(2)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|error| DiffError::Db(error.to_string()))
}

async fn step_rows(
    db: &Database,
    session_id: &str,
    start: i64,
    end: Option<i64>,
) -> Result<Vec<StepRow>, DiffError> {
    let session = session_id.to_string();
    db.transaction(move |tx| {
        let (sql, params): (&str, Vec<Box<dyn rusqlite::ToSql + Send>>) = match end {
            Some(end) => (
                "SELECT seq, json_extract(data, '$.snapshot.start'), json_extract(data, '$.snapshot.end'), \
                 json_extract(data, '$.time.completed') FROM session_message \
                 WHERE session_id = ?1 AND type = 'assistant' AND seq > ?2 AND seq < ?3 ORDER BY seq ASC",
                vec![
                    Box::new(session),
                    Box::new(start),
                    Box::new(end),
                ],
            ),
            None => (
                "SELECT seq, json_extract(data, '$.snapshot.start'), json_extract(data, '$.snapshot.end'), \
                 json_extract(data, '$.time.completed') FROM session_message \
                 WHERE session_id = ?1 AND type = 'assistant' AND seq > ?2 ORDER BY seq ASC",
                vec![Box::new(session), Box::new(start)],
            ),
        };
        let references: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|value| value.as_ref() as &dyn rusqlite::ToSql).collect();
        let mut statement = tx.conn().prepare(sql)?;
        let rows = statement
            .query_map(references.as_slice(), |row| {
                Ok(StepRow {
                    seq: row.get::<_, i64>(0)?,
                    start: row.get::<_, Option<String>>(1)?,
                    end: row.get::<_, Option<String>>(2)?,
                    completed: row.get::<_, Option<i64>>(3)?,
                })
            })
            ?
            .collect::<Result<Vec<_>, _>>()
            ?;
        Ok(rows)
    })
    .await
    .map_err(|error| DiffError::Db(error.to_string()))
}

async fn switch_rows(db: &Database, session_id: &str) -> Result<Vec<SwitchRow>, DiffError> {
    let session = session_id.to_string();
    db.transaction(move |tx| {
        let mut statement = tx
            .conn()
            .prepare(
                "SELECT seq, json_extract(data, '$.location'), json_extract(data, '$.previous.location') \
                 FROM session_message WHERE session_id = ?1 AND type = 'location-switched' ORDER BY seq ASC",
            )
            ?;
        let rows = statement
            .query_map(rusqlite::params![session], |row| {
                Ok(SwitchRow {
                    seq: row.get::<_, i64>(0)?,
                    location: row
                        .get::<_, Option<String>>(1)?
                        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok()),
                    previous: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok()),
                })
            })
            ?
            .collect::<Result<Vec<_>, _>>()
            ?;
        Ok(rows)
    })
    .await
    .map_err(|error| DiffError::Db(error.to_string()))
}

fn resolve_anchor<'a>(
    rows: &'a [AnchorRow],
    session_id: &str,
    field: TurnField,
    id: &str,
) -> Result<&'a AnchorRow, DiffError> {
    let Some(row) = rows.iter().find(|row| row.id == id) else {
        return Err(DiffError::MessageNotFound {
            session_id: session_id.to_string(),
            message_id: id.to_string(),
        });
    };
    if row.kind != "user" {
        return Err(DiffError::TurnRange(TurnRangeError {
            session_id: session_id.to_string(),
            field,
            message: format!("Message {id} is not a user message"),
        }));
    }
    Ok(row)
}

fn decode_location(value: &serde_json::Value) -> Option<location::Ref> {
    serde_json::from_value::<location::Ref>(value.clone()).ok()
}

fn session_location(session: &SessionRow) -> location::Ref {
    location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(session.directory.clone()),
        workspace_id: session
            .workspace_id
            .as_ref()
            .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
    }
}

/// Phase one (`SessionDiff.turn` up to the snapshot service boundary):
/// resolve the anchor range, the snapshot bracket, and the owning
/// location. `None` when the range produced no comparable snapshots —
/// v2's `[]`.
pub async fn resolve_turn(
    db: &Database,
    session: &SessionRow,
    from: Option<&str>,
    to: Option<&str>,
    context: Option<u32>,
) -> Result<Option<TurnPlan>, DiffError> {
    let session_id = session.id.as_str();
    let rows = anchor_rows(db, session_id, from, to).await?;
    let users: Vec<&AnchorRow> = rows.iter().filter(|row| row.kind == "user").collect();
    let markers: Vec<&AnchorRow> = rows.iter().filter(|row| row.kind == "idle").collect();

    let anchor = match from {
        Some(from) => Some(resolve_anchor(&rows, session_id, TurnField::From, from)?),
        None => users.last().copied(),
    };
    let Some(anchor) = anchor else {
        return Ok(None);
    };
    let last = match to {
        Some(to) => resolve_anchor(&rows, session_id, TurnField::To, to)?,
        None => anchor,
    };
    if last.seq < anchor.seq {
        return Err(DiffError::TurnRange(TurnRangeError {
            session_id: session_id.to_string(),
            field: TurnField::To,
            message: format!("Message {} precedes {}", last.id, anchor.id),
        }));
    }
    // Without any marker, history predates idle markers and a prompt's
    // turn ends at the next prompt.
    let legacy = markers.is_empty();
    // The turn opens with the first prompt after the previous idle
    // marker; the anchor itself is the latest candidate.
    let opened = markers.iter().rev().find(|row| row.seq < anchor.seq).map_or(-1, |row| row.seq);
    let start = if legacy {
        anchor.seq
    } else {
        users.iter().find(|row| row.seq > opened).map_or(anchor.seq, |row| row.seq)
    };
    let end = if legacy {
        users.iter().find(|row| row.seq > last.seq).map(|row| row.seq)
    } else {
        markers.iter().find(|row| row.seq > last.seq).map(|row| row.seq)
    };

    let steps = step_rows(db, session_id, start, end).await?;
    let (Some(first), Some(final_step)) = (steps.first(), steps.last()) else {
        return Ok(None);
    };
    let Some(from_snapshot) =
        steps.iter().find(|step| step.start.is_some()).and_then(|step| step.start.clone())
    else {
        return Ok(None);
    };

    let switches = switch_rows(db, session_id).await?;
    if switches.iter().any(|row| row.seq > first.seq && row.seq < final_step.seq) {
        return Err(DiffError::TurnRange(TurnRangeError {
            session_id: session_id.to_string(),
            field: TurnField::To,
            message: "Turn range spans a location change".to_string(),
        }));
    }
    let before = switches
        .iter()
        .rev()
        .find(|row| row.seq < first.seq)
        .and_then(|row| row.location.as_ref())
        .and_then(decode_location);
    let after = switches
        .iter()
        .find(|row| row.seq > first.seq)
        .and_then(|row| row.previous.as_ref())
        .and_then(decode_location);
    let location = before.or(after).unwrap_or_else(|| session_location(session));
    let recorded =
        steps.iter().rev().find(|step| step.end.is_some()).and_then(|step| step.end.clone());
    // Only a step still running in the active session compares against
    // the working copy (diff.ts:245).
    Ok(Some(TurnPlan {
        location,
        from: from_snapshot,
        recorded,
        capture: final_step.completed.is_none(),
        context: context.unwrap_or(PATCH_CONTEXT_LINES),
    }))
}

/// Phase two: run the resolved plan against the location's snapshot
/// service. `active` marks the process as currently executing the session
/// — only then may a still-open range compare against the working copy.
pub async fn run_turn(
    snapshot: &std::sync::Arc<dyn crate::snapshot::SnapshotService>,
    plan: &TurnPlan,
    active: bool,
) -> Result<Vec<FileDiff>, DiffError> {
    let to = if plan.capture && active {
        snapshot.capture().await.map(|id| id.to_string()).or_else(|| plan.recorded.clone())
    } else {
        plan.recorded.clone()
    };
    let Some(to) = to else {
        return Ok(Vec::new());
    };
    snapshot
        .diff(
            &oc_schema::id::SnapshotId::parse(plan.from.clone()),
            &oc_schema::id::SnapshotId::parse(to),
            Some(plan.context),
            None,
        )
        .await
        .map_err(|error| DiffError::Db(error.to_string()))
}

/// `SessionDiff.turn` — resolve + run in one call against the session's
/// own location's snapshot service.
pub async fn turn(
    db: &Database,
    snapshot: &std::sync::Arc<dyn crate::snapshot::SnapshotService>,
    session: &SessionRow,
    active: bool,
    from: Option<&str>,
    to: Option<&str>,
    context: Option<u32>,
) -> Result<Vec<FileDiff>, DiffError> {
    let Some(plan) = resolve_turn(db, session, from, to, context).await? else {
        return Ok(Vec::new());
    };
    run_turn(snapshot, &plan, active).await
}

/// Whether the message row is an assistant step still running (the
/// `capture` eligibility check mirrors `final.completed === null`).
#[must_use]
pub fn step_running(row: &SessionMessageRow) -> bool {
    row.type_ == "assistant"
        && serde_json::from_str::<serde_json::Value>(&row.data)
            .ok()
            .and_then(|data| data.pointer("/time/completed").cloned())
            .is_none_or(|value| value.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_context_lines_match_v2() {
        assert_eq!(PATCH_CONTEXT_LINES, 2_147_483_647);
    }

    #[test]
    fn step_running_reads_completion() {
        let row = |data: &str| SessionMessageRow {
            id: "m".into(),
            session_id: "s".into(),
            type_: "assistant".into(),
            seq: 1,
            time_created: 0,
            time_updated: 0,
            data: data.into(),
        };
        assert!(step_running(&row(r#"{"time":{"created":1}}"#)));
        assert!(!step_running(&row(r#"{"time":{"created":1,"completed":2}}"#)));
    }
}
