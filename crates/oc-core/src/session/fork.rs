//! Port of `Session.fork` orchestration (ref `session/session.ts:319-359`)
//! — G-10: the boundary lookup + inherited instruction snapshot moved out
//! of the HTTP layer so the fork command half lives beside its projection
//! half (`projector::project_fork`) and is testable at core level.
//!
//! The server handler keeps only auth/validation, the durable
//! `session.forked` publication and the row readback — no SQL.
//!
//! `SessionStore::boundary` (store.rs:513) resolves the *seq* of a named
//! message (existence probe used by fork/revert internals); the fork
//! command needs the *message id*, plus the "newest message" fallback and
//! the 404/empty-session classification. Semantics diverge, so the
//! fork-specific query stays here.

//! Lint posture: ports of TS sources keep the mechanical-idiom allows the
//! runner established.
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use serde_json::Value;

use crate::database::Database;

/// Fork command failures (mapped to HTTP errors by the server shell).
#[derive(Debug)]
pub enum ForkError {
    /// Named `before` message does not exist in the session → 404
    /// `MessageNotFound`.
    MessageNotFound { session_id: String, before: String },
    /// No messages at all and no `before` named → 400 `empty_session`.
    EmptySession,
    /// Storage failure.
    Db(String),
}

/// The resolved fork inputs (the `session.forked` payload ingredients).
#[derive(Debug, Clone)]
pub struct ForkOutcome {
    /// The new session id (caller-minted, rides the event payload).
    pub fork_id: String,
    /// The fork source.
    pub parent_id: String,
    /// The payload `before` when one was named (`None` ⇒ through-newest).
    pub before: Option<String>,
    /// The boundary message id (named or newest).
    pub boundary_id: String,
    /// `"before"` when named, `"through"` otherwise.
    pub boundary_type: &'static str,
    /// The parent's newest instruction values (raw JSON string, as read
    /// from `instruction_state.current_values`).
    pub instructions: Option<String>,
    /// The parent's instruction entry snapshot, ascending by key
    /// (ref instruction-entry.ts:36).
    pub instruction_entries: Vec<Value>,
}

/// `Session.fork` (session.ts:319-355): locate the boundary — the `before`
/// message when the caller names one, else the newest message — and read
/// the inherited instruction snapshot (the parent's *newest* values, not
/// the values in effect at the boundary; ref session.ts:341-349).
///
/// Publishing `session.forked` (and thereby creating the row) stays with
/// the caller; the projector owns the write half.
pub async fn fork(
    db: &Database,
    fork_id: &str,
    parent_id: &str,
    before: Option<&str>,
) -> Result<ForkOutcome, ForkError> {
    let session = parent_id.to_owned();
    let lookup_before = before.map(String::from);
    let (boundary_id, instructions, instruction_entries) = db
        .transaction(move |tx| {
            // Boundary (ref session.ts:321-339).
            let boundary_id: Option<String> = match &lookup_before {
                Some(before) => tx
                    .conn()
                    .query_row(
                        "SELECT id FROM session_message WHERE session_id = ?1 AND id = ?2 \
                         ORDER BY seq DESC LIMIT 1",
                        rusqlite::params![session, before],
                        |row| row.get(0),
                    )
                    .ok(),
                None => tx
                    .conn()
                    .query_row(
                        "SELECT id FROM session_message WHERE session_id = ?1 \
                         ORDER BY seq DESC LIMIT 1",
                        rusqlite::params![session],
                        |row| row.get(0),
                    )
                    .ok(),
            };
            let Some(boundary_id) = boundary_id else {
                return Ok((None, None, Vec::new()));
            };
            // Inherited instruction snapshot (ref session.ts:341-349).
            let values: Option<String> = tx
                .conn()
                .query_row(
                    "SELECT current_values FROM instruction_state WHERE session_id = ?1",
                    rusqlite::params![session],
                    |row| row.get(0),
                )
                .ok();
            let mut statement = tx.conn().prepare(
                // ref instruction-entry.ts:36 — snapshot reads ascend by key
                "SELECT key, value, removed FROM instruction_entry WHERE session_id = ?1 \
                 ORDER BY key ASC",
            )?;
            let entries = statement
                .query_map(rusqlite::params![session], |row| {
                    let raw: Option<String> = row.get::<_, Option<String>>(1)?;
                    let value = raw
                        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                        .unwrap_or(Value::Null);
                    Ok(serde_json::json!({
                        "key": row.get::<_, String>(0)?,
                        "value": value,
                        "removed": row.get::<_, bool>(2)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok((Some(boundary_id), values, entries))
        })
        .await
        .map_err(|error| ForkError::Db(error.to_string()))?;
    let Some(boundary_id) = boundary_id else {
        return match before {
            Some(before) => Err(ForkError::MessageNotFound {
                session_id: parent_id.to_owned(),
                before: before.to_owned(),
            }),
            None => Err(ForkError::EmptySession),
        };
    };
    Ok(ForkOutcome {
        fork_id: fork_id.to_owned(),
        parent_id: parent_id.to_owned(),
        before: before.map(String::from),
        boundary_type: if before.is_some() { "before" } else { "through" },
        boundary_id,
        instructions,
        instruction_entries,
    })
}
