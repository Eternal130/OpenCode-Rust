//! Port of `session/transfer.ts` — session import replay.
//!
//! REF `SessionTransfer.import` (transfer.ts:64-154): reject duplicate ids
//! (`ImportConflictError`), verify `parentID`, project the session via a
//! fresh `session.created` publication, then — in that event's commit —
//! insert the export's settled messages at `seq = 1..n`, reserve the
//! aggregate sequence past them (`Bus.reserveSequence`) and backfill the
//! aggregate counters (`cost`, tokens, idle/viewed/archived times) from the
//! export's `info`.
//!
//! The port performs the same steps with the same ordering: `store.create`
//! publishes `session.created` (projecting the row), then one transaction
//! inserts the message rows, reserves the sequence and updates the
//! counters. `time_updated` is stamped with the import wall clock
//! (`importedAt`), exactly like REF.

use oc_schema::id::SessionId;
use oc_schema::location;
use oc_schema::model;
use oc_schema::permission::Ruleset;
use oc_schema::session_support::SessionMetadata;
use rusqlite::OptionalExtension;
use serde_json::{Map, Value};

use crate::bus::{Bus, BusError};
use crate::database::error::DbError;
use crate::database::schema::SessionRow;
use crate::database::Database;
use crate::session::store::{CreateOutcome, ResolvedProject, SessionStore};

/// `SessionTransfer.import` input (the caller supplies the target location
/// and its resolved project, matching REF's `projects.resolve`).
#[derive(Debug, Clone)]
pub struct ImportInput {
    /// The export payload: `{ info: Session.Info, messages: SessionMessage.Info[] }`.
    pub data: Value,
    /// Target location for the imported session.
    pub location: location::Ref,
    /// Caller-resolved project for the target location.
    pub project: ResolvedProject,
    /// App version stamped on the created session.
    pub version: String,
}

/// `SessionTransfer.ImportConflictError` + the decode/defect failures REF
/// surfaces from `import`.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("session already exists: {0}")]
    Conflict(String),
    #[error("invalid transfer payload: {0}")]
    InvalidData(String),
    #[error("session not found: {0}")]
    ParentNotFound(String),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Bus(#[from] BusError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Create(#[from] crate::session::store::CreateError),
}

/// `SessionTransfer.import` — replays an exported session into the local
/// store and returns the projected [`SessionRow`].
// Import mirrors the TS original as one flat replay pass; splitting it
// would obscure the 1:1 port, so the line budget is waived here.
#[allow(clippy::too_many_lines)]
pub async fn import(
    store: &SessionStore,
    bus: &Bus,
    db: &Database,
    input: ImportInput,
) -> Result<SessionRow, ImportError> {
    let info = input
        .data
        .get("info")
        .filter(|value| value.is_object())
        .ok_or_else(|| ImportError::InvalidData("missing info object".into()))?;
    let session_id_raw = info
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ImportError::InvalidData("missing session id".into()))?;
    let session_id =
        SessionId::parse(session_id_raw.to_owned()).unwrap_or_else(|error| SessionId(error.value));

    // Duplicate → ImportConflictError (§3: 409 "Session already exists").
    if store.get(session_id.as_str()).await?.is_some() {
        return Err(ImportError::Conflict(session_id.as_str().to_owned()));
    }

    // REF resolves `parentID` (NotFoundError when absent) before creating.
    let parent_id = match info.get("parentID").and_then(Value::as_str) {
        Some(parent) if !parent.is_empty() => {
            let parent = store.get(parent).await?.ok_or_else(|| {
                ImportError::ParentNotFound(format!("session not found: {parent}"))
            })?;
            Some(SessionId::parse(parent.id.clone()).unwrap_or_else(|e| SessionId(e.value)))
        }
        _ => None,
    };

    let metadata = match info.get("metadata") {
        Some(Value::Object(map)) if !map.is_empty() => {
            Some(serde_json::from_value::<SessionMetadata>(Value::Object(map.clone()))?)
        }
        _ => None,
    };
    let permissions = match info.get("permissions") {
        Some(value) if value.is_object() => Some(serde_json::from_value::<Ruleset>(value.clone())?),
        _ => None,
    };
    let model = match info.get("model") {
        Some(value) if value.is_object() => {
            Some(serde_json::from_value::<model::Ref>(value.clone())?)
        }
        _ => None,
    };

    // The fresh `session.created` publication projects the row (slug,
    // location envelope, inherited metadata/permissions).
    let outcome = store
        .create(
            bus,
            crate::session::store::CreateInput {
                id: Some(session_id.clone()),
                parent_id,
                location: Some(input.location.clone()),
                title: info.get("title").and_then(Value::as_str).map(str::to_owned),
                agent: info.get("agent").and_then(Value::as_str).map(str::to_owned),
                model,
                metadata,
                permissions,
                project: Some(input.project.clone()),
                version: input.version,
            },
        )
        .await?;
    let _ = match outcome {
        CreateOutcome::Created(row) => row,
        // Lost a concurrent projection race — the existing identity wins
        // (same mapping as REF's SessionAlreadyProjected catch).
        CreateOutcome::Existing(_) => {
            return Err(ImportError::Conflict(session_id.as_str().to_owned()))
        }
    };

    let imported_at = crate::database::migrations::now_ms();
    let messages = settled_messages(&input.data);
    let count = i64::try_from(messages.len()).unwrap_or(i64::MAX);

    {
        let id = session_id.as_str().to_owned();
        let rows = messages;
        let cost = info.get("cost").and_then(Value::as_f64).unwrap_or_default();
        let tokens = info.get("tokens").cloned().unwrap_or_else(|| Value::Object(Map::default()));
        let token = |key: &str| num_i64(tokens.get(key));
        let tokens_input = token("input");
        let tokens_output = token("output");
        let tokens_reasoning = token("reasoning");
        let cache = tokens.get("cache").cloned().unwrap_or_else(|| Value::Object(Map::default()));
        let tokens_cache_read = num_i64(cache.get("read"));
        let tokens_cache_write = num_i64(cache.get("write"));
        let time = info.get("time").cloned().unwrap_or_else(|| Value::Object(Map::default()));
        let time_created = opt_num_i64(time.get("created")).unwrap_or(imported_at);
        let time_idle = opt_num_i64(time.get("idle"));
        let time_viewed = match (time_idle, opt_num_i64(time.get("viewed"))) {
            (Some(idle), Some(viewed)) => Some(idle.min(viewed)),
            _ => None,
        };
        let idle_outcome = match time_idle {
            Some(_) => info.get("outcome").and_then(Value::as_str).map(str::to_owned),
            None => None,
        };
        let time_archived = opt_num_i64(time.get("archived"));
        db.transaction(move |tx| {
            // Settled messages land at seq = 1..n (REF: `seq: index + 1`).
            for (index, row) in rows.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO session_message \
                     (id, session_id, type, seq, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        row.id,
                        id,
                        row.type_,
                        i64::try_from(index + 1).unwrap_or(i64::MAX),
                        row.time_created,
                        imported_at,
                        row.data,
                    ],
                )?;
            }
            if !rows.is_empty() {
                // `Bus.reserveSequence(db, sessionID, seq + messages.length)`
                // — the next published event must not collide with the
                // imported seq range.
                let seq: i64 = tx
                    .conn()
                    .query_row(
                        "SELECT seq FROM event_sequence WHERE aggregate_id = ?1",
                        rusqlite::params![id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .unwrap_or_default();
                tx.event_sequence_upsert(&id, seq + count, None)?;
            }
            tx.execute(
                "UPDATE session_v2 SET \
                   cost = ?1, tokens_input = ?2, tokens_output = ?3, tokens_reasoning = ?4, \
                   tokens_cache_read = ?5, tokens_cache_write = ?6, time_created = ?7, \
                   time_updated = ?8, time_idle = ?9, time_viewed = ?10, idle_outcome = ?11, \
                   time_archived = ?12 \
                 WHERE id = ?13",
                rusqlite::params![
                    cost,
                    tokens_input,
                    tokens_output,
                    tokens_reasoning,
                    tokens_cache_read,
                    tokens_cache_write,
                    time_created,
                    imported_at,
                    time_idle,
                    time_viewed,
                    idle_outcome,
                    time_archived,
                    id,
                ],
            )?;
            Ok(())
        })
        .await?;
    }

    store
        .get(session_id.as_str())
        .await?
        .ok_or_else(|| ImportError::Db(DbError::Pragma("session missing after import".into())))
}

/// Wire times/tokens arrive as float millis (effect `DateTime` numbers);
/// accept both integer and float JSON representations.
// Float JSON numbers truncate toward zero intentionally (TS `| 0` parity).
#[allow(clippy::cast_possible_truncation)]
fn num_i64(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
        .unwrap_or_default()
}

// Float JSON numbers truncate toward zero intentionally (TS `| 0` parity).
#[allow(clippy::cast_possible_truncation)]
fn opt_num_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
}
/// A message row staged for import.
struct StagedMessage {
    id: String,
    type_: String,
    time_created: i64,
    data: String,
}

/// REF `isSettled` (transfer.ts:165-169) + the row mapping
/// (transfer.ts:77-88): only settled messages travel, each keeping its id
/// and type while the rest of the payload becomes the row `data`.
fn settled_messages(data: &Value) -> Vec<StagedMessage> {
    let Some(list) = data.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut staged = Vec::new();
    for message in list {
        let type_ = message.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
        // assistant: settled once time.completed is set; shell/compaction:
        // settled once status leaves "running"; everything else always.
        let settled = match type_.as_str() {
            "assistant" => message
                .get("time")
                .and_then(|time| time.get("completed"))
                .is_some_and(|value| !value.is_null()),
            "shell" | "compaction" => {
                message.get("status").and_then(Value::as_str) != Some("running")
            }
            _ => true,
        };
        if !settled || type_.is_empty() {
            continue;
        }
        let Some(object) = message.as_object() else { continue };
        let mut rest = object.clone();
        rest.remove("id");
        rest.remove("type");
        staged.push(StagedMessage {
            id: message.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
            // Float JSON numbers truncate toward zero intentionally (TS `| 0` parity).
            #[allow(clippy::cast_possible_truncation)]
            time_created: message
                .get("time")
                .and_then(|time| time.get("created"))
                .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
                .unwrap_or_default(),
            type_,
            data: Value::Object(rest).to_string(),
        });
    }
    staged
}
