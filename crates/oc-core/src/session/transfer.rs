//! Port of `session/transfer.ts` — session import replay.
//!
//! REF `SessionTransfer.import` (transfer.ts:64-154): reject duplicate ids
//! (`ImportConflictError`), verify `parentID`, project the session via a
//! fresh `session.created` publication whose COMMIT HOOK inserts the
//! export's settled messages at `seq = 1..n`, reserves the aggregate
//! sequence past them (`Bus.reserveSequence`) and backfills the aggregate
//! counters (`cost`, tokens, idle/viewed/archived times) from the export's
//! `info` — all in the publication's single transaction.
//!
//! The port rides the same seam: `store.create_with_commit` attaches the
//! message insert + sequence reservation + counter backfill to the
//! `session.created` commit hook (G-250 — projection, messages and
//! counters commit atomically; a duplicate message id rolls the whole
//! import back instead of silently overwriting). `time_updated` is
//! stamped with the import wall clock (`importedAt`), exactly like REF.

use oc_schema::id::SessionId;
use oc_schema::location;
use oc_schema::model;
use oc_schema::permission::Ruleset;
use oc_schema::session_support::SessionMetadata;
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
    _db: &Database,
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
    // location envelope) and — through its commit hook — inserts the
    // export's settled messages and backfills the aggregate counters in
    // the SAME transaction (G-250, ref transfer.ts:98-140: everything
    // rides `bus.publish(Created, { commit })`). A failure anywhere in
    // the hook rolls the whole import back, so a crashed or rejected
    // import never leaves a half-imported session row that a retry
    // would conflict against.
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
        // REF transfer.ts:98-140 — the Created event's `commit(seq)`
        // hook: bare `db.insert` (a duplicate message id fails the
        // session_message PK and rolls the whole import back — never a
        // silent `INSERT OR REPLACE` overwrite), `Bus.reserveSequence(
        // seq + messages.length)`, then the aggregate backfill UPDATE.
        let commit = Box::new(move |tx: &mut crate::database::Tx<'_>, seq: i64| {
            // Settled messages land at seq = 1..n (REF: `seq: index + 1`).
            for (index, row) in rows.iter().enumerate() {
                tx.execute(
                    "INSERT INTO session_message \
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
                // imported seq range. The event's own allocated `seq`
                // rides in (the outer commit's upsert is monotonic-max).
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
        });
        let outcome = store
            .create_with_commit(
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
                    // G-250 — the export's metadata/permissions replay
                    // verbatim; a missing key stays missing instead of
                    // inheriting the local parent's (ref transfer.ts:105
                    // passes the export's value directly).
                    explicit_defaults: true,
                },
                commit,
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

// =============================================================================
// Export (SessionTransfer.export, transfer.ts:60-64 + 165-330)
// =============================================================================

/// `isSettled` (transfer.ts:165-169) over a wire message value.
#[must_use]
pub fn is_settled(message: &Value) -> bool {
    let type_ = message.get("type").and_then(Value::as_str).unwrap_or_default();
    match type_ {
        "assistant" => message
            .get("time")
            .and_then(|time| time.get("completed"))
            .is_some_and(|value| !value.is_null()),
        "shell" | "compaction" => message.get("status").and_then(Value::as_str) != Some("running"),
        _ => true,
    }
}

/// `SessionTransfer.export` (transfer.ts:60-64): the wire `Data` pair —
/// `info` plus the SETTLED messages in ascending order (the settled
/// filter runs on BOTH the sanitize and non-sanitize paths); `sanitize`
/// scrubs it per `sanitize`/`sanitizeMessage` (transfer.ts:180-330).
#[must_use]
pub fn export_data(info: &Value, messages: &[Value], sanitize: bool) -> Value {
    let settled: Vec<Value> =
        messages.iter().filter(|message| is_settled(message)).cloned().collect();
    if !sanitize {
        return serde_json::json!({ "info": info, "messages": settled });
    }
    let sanitized_messages: Vec<Value> = settled.iter().map(sanitize_message).collect();
    serde_json::json!({ "info": sanitize_info(info), "messages": sanitized_messages })
}

/// `redact` (transfer.ts:172-174): empty (whitespace-only) values pass
/// through untouched, everything else becomes the placeholder.
fn redact(kind: &str, id: &str, value: &str) -> String {
    if value.trim().is_empty() {
        value.to_string()
    } else {
        format!("[redacted:{kind}:{id}]")
    }
}

/// `redact` over a wire value: strings redact, other shapes pass through
/// (REF only ever calls `redact` on strings).
fn redact_value(kind: &str, id: &str, value: &Value) -> Value {
    match value.as_str() {
        Some(text) => Value::String(redact(kind, id, text)),
        None => value.clone(),
    }
}

/// `metadata` (transfer.ts:176-179): a non-empty object collapses to
/// `{ redacted: "<kind>:<id>" }`; an empty object (or absent value)
/// passes through.
fn metadata_value(kind: &str, id: &str, value: &Value) -> Value {
    if value.as_object().is_some_and(|object| !object.is_empty()) {
        serde_json::json!({ "redacted": format!("{kind}:{id}") })
    } else {
        value.clone()
    }
}

/// Replace `key` with its redacted form when present (`redact(kind, id, …)`).
fn redact_key(map: &mut serde_json::Map<String, Value>, key: &str, kind: &str, id: &str) {
    if let Some(value) = map.get(key) {
        let redacted = redact_value(kind, id, value);
        map.insert(key.to_owned(), redacted);
    }
}

/// `sanitize` (transfer.ts:180-200): session title/metadata/directory and
/// the revert file list.
fn sanitize_info(info: &Value) -> Value {
    let id = info.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut out = info.clone();
    let Some(map) = out.as_object_mut() else { return out };
    redact_key(map, "title", "session-title", &id);
    if let Some(metadata) = map.get("metadata") {
        let sanitized = metadata_value("session-metadata", &id, metadata);
        map.insert("metadata".to_owned(), sanitized);
    }
    // `directory: AbsolutePath.make("/" + redact(...))` — the placeholder
    // becomes a synthetic absolute path.
    if let Some(directory) = map
        .get_mut("location")
        .and_then(Value::as_object_mut)
        .and_then(|location| location.get("directory"))
        .and_then(Value::as_str)
        .map(|directory| redact("session-directory", &id, directory))
    {
        if let Some(location) = map.get_mut("location").and_then(Value::as_object_mut) {
            location.insert("directory".to_owned(), Value::String(format!("/{directory}")));
        }
    }
    if let Some(files) = map.get_mut("revert").and_then(Value::as_object_mut).and_then(|revert| {
        revert.get_mut("files").and_then(Value::as_array_mut)
    }) {
        for (index, file) in files.iter_mut().enumerate() {
            let Some(file) = file.as_object_mut() else { continue };
            let index = index.to_string();
            redact_key(file, "file", "revert-file", &index);
            redact_key(file, "patch", "revert-patch", &index);
        }
    }
    out
}

/// `sanitizeMessage` (transfer.ts:202-294) + `sanitizeToolState` /
/// `sanitizeToolContent` (:296-331).
fn sanitize_message(message: &Value) -> Value {
    let id = message.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut out = message.clone();
    let Some(map) = out.as_object_mut() else { return out };
    // The common metadata scrub (every type).
    if let Some(metadata) = map.get("metadata") {
        let sanitized = metadata_value("message-metadata", &id, metadata);
        map.insert("metadata".to_owned(), sanitized);
    }
    match map.get("type").and_then(Value::as_str).unwrap_or_default() {
        "user" => {
            redact_key(map, "text", "text", &id);
            if let Some(files) = map.get_mut("files").and_then(Value::as_array_mut) {
                for (index, file) in files.iter_mut().enumerate() {
                    let Some(file) = file.as_object_mut() else { continue };
                    let index = index.to_string();
                    file.insert("data".to_owned(), Value::String(String::new()));
                    file.insert("source".to_owned(), serde_json::json!({ "type": "inline" }));
                    redact_key(file, "name", "file-name", &index);
                    redact_key(file, "description", "file-description", &index);
                    if let Some(mention) = file.get_mut("mention").and_then(Value::as_object_mut) {
                        redact_key(mention, "text", "file-mention", &index);
                    }
                }
            }
            for (key, name_kind, mention_kind) in
                [("agents", "agent-name", "agent-mention"), ("skills", "skill-name", "skill-mention")]
            {
                if let Some(items) = map.get_mut(key).and_then(Value::as_array_mut) {
                    for (index, item) in items.iter_mut().enumerate() {
                        let Some(item) = item.as_object_mut() else { continue };
                        let index = index.to_string();
                        redact_key(item, "name", name_kind, &index);
                        if key == "skills" {
                            redact_key(item, "text", "skill", &index);
                        }
                        if let Some(mention) = item.get_mut("mention").and_then(Value::as_object_mut)
                        {
                            redact_key(mention, "text", mention_kind, &index);
                        }
                    }
                }
            }
        }
        "synthetic" => {
            redact_key(map, "text", "synthetic", &id);
            redact_key(map, "description", "synthetic-description", &id);
        }
        "system" => {
            redact_key(map, "text", "system", &id);
        }
        "skill" => {
            redact_key(map, "text", "skill", &id);
        }
        "shell" => {
            redact_key(map, "command", "shell-command", &id);
            match map.get_mut("output") {
                Some(output @ Value::Object(_)) => {
                    if let Some(output_map) = output.as_object_mut() {
                        redact_key(output_map, "output", "shell-output", &id);
                    }
                }
                _ => {
                    // `output: message.output ? … : undefined` — falsy
                    // outputs drop the key.
                    map.remove("output");
                }
            }
        }
        "assistant" => {
            if let Some(content) = map.get_mut("content").and_then(Value::as_array_mut) {
                for part in content.iter_mut() {
                    sanitize_assistant_part(&id, part);
                }
            }
        }
        "compaction" => {
            // A failed compaction keeps everything but its metadata.
            if map.get("status").and_then(Value::as_str) == Some("failed") {
                return out;
            }
            redact_key(map, "summary", "compaction-summary", &id);
            redact_key(map, "recent", "compaction-recent", &id);
            if map.get("status").and_then(Value::as_str) == Some("completed") {
                match map.get("providerState") {
                    Some(provider_state) => {
                        let sanitized =
                            metadata_value("compaction-provider-state", &id, provider_state);
                        map.insert("providerState".to_owned(), sanitized);
                    }
                    None => {
                        map.remove("providerState");
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// One assistant content part (transfer.ts:264-287).
fn sanitize_assistant_part(id: &str, part: &mut Value) {
    let Some(map) = part.as_object_mut() else { return };
    let type_ = map.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
    if type_ == "text" || type_ == "reasoning" {
        let kind = type_.as_str();
        let state_kind = if kind == "text" { "text-state" } else { "reasoning-state" };
        redact_key(map, "text", kind, id);
        match map.get("state") {
            Some(state) if !state.is_null() => {
                map.insert(
                    "state".to_owned(),
                    serde_json::json!({ "redacted": format!("{state_kind}:{id}") }),
                );
            }
            _ => {
                map.remove("state");
            }
        }
        return;
    }
    // Tool parts: provider states collapse, the tool state scrubs.
    match map.get("providerState") {
        Some(value) if !value.is_null() => {
            map.insert(
                "providerState".to_owned(),
                serde_json::json!({ "redacted": format!("tool-provider-state:{id}") }),
            );
        }
        _ => {
            map.remove("providerState");
        }
    }
    match map.get("providerResultState") {
        Some(value) if !value.is_null() => {
            map.insert(
                "providerResultState".to_owned(),
                serde_json::json!({ "redacted": format!("tool-provider-result-state:{id}") }),
            );
        }
        _ => {
            map.remove("providerResultState");
        }
    }
    if let Some(state) = map.get_mut("state") {
        *state = sanitize_tool_state(id, state);
    }
}

/// `sanitizeToolState` (transfer.ts:296-326).
fn sanitize_tool_state(id: &str, state: &Value) -> Value {
    let mut out = state.clone();
    let Some(map) = out.as_object_mut() else { return out };
    let tool_input = || serde_json::json!({ "redacted": format!("tool-input:{id}") });
    let status = map.get("status").and_then(Value::as_str).unwrap_or_default().to_string();
    match status.as_str() {
        "streaming" => {
            redact_key(map, "input", "tool-input", id);
        }
        "running" => {
            map.insert("input".to_owned(), tool_input());
            map.insert(
                "metadata".to_owned(),
                serde_json::json!({ "redacted": format!("tool-metadata:{id}") }),
            );
        }
        status => {
            map.insert("input".to_owned(), tool_input());
            match map.get("metadata") {
                Some(value) if !value.is_null() => {
                    map.insert(
                        "metadata".to_owned(),
                        serde_json::json!({ "redacted": format!("tool-metadata:{id}") }),
                    );
                }
                _ => {
                    map.remove("metadata");
                }
            }
            match map.get_mut("content") {
                Some(content @ Value::Array(_)) => {
                    if let Some(items) = content.as_array_mut() {
                        for item in items.iter_mut() {
                            sanitize_tool_content(id, item);
                        }
                    }
                }
                // REF maps `state.content` unconditionally on completed;
                // the else branch keeps `content` only when present.
                _ if status == "completed" => {}
                _ => {
                    map.remove("content");
                }
            }
        }
    }
    out
}

/// `sanitizeToolContent` (transfer.ts:328-336).
fn sanitize_tool_content(id: &str, content: &mut Value) {
    let Some(map) = content.as_object_mut() else { return };
    if map.get("type").and_then(Value::as_str) == Some("text") {
        redact_key(map, "text", "tool-output", id);
        return;
    }
    redact_key(map, "uri", "tool-file-uri", id);
    redact_key(map, "name", "tool-file-name", id);
}
