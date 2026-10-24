//! V1 → V2 content migration (DB-D12).
//!
//! Port of `packages/core/src/database/v1-migration.bun.ts` (v2.0.24): the
//! schema journal (`migrations.rs`) only evolves DDL — this module moves the
//! *content*: v1 `session`/`message`/`part` rows become `session_v2` rows +
//! `session_message` projections, with a resumable KV state machine
//! (`migration.v1-v2`), per-session transactions, batched stale-event
//! clearing, progress reporting, and an optional read-only import of a
//! previous "opencode-next.db" V2 database.
//!
//! [`transform_session`] is a pure function (the ref's export surface) so
//! the transformation is unit-testable without a database.
//!
//! Lint posture: this is a close transliteration of one 1,135-line
//! reference file; per-function line limits, index casts and the rusqlite
//! row-mapping closures are inherent to the port's shape.
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::type_complexity,
    reason = "one-for-one v1-migration.bun.ts transliteration"
)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::{Database, Result, Tx};

// ---------------------------------------------------------------------------
// Types (v1-migration.bun.ts:18-115)
// ---------------------------------------------------------------------------

/// A v1 `message` row.
#[derive(Debug, Clone)]
pub struct SourceMessage {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: String,
}

/// A v1 `part` row.
#[derive(Debug, Clone)]
pub struct SourcePart {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: String,
}

/// The v1 `session` columns the transform consumes.
#[derive(Debug, Clone, Default)]
pub struct SourceSession {
    pub id: String,
    pub agent: Option<String>,
    pub model: Option<String>,
}

/// One skipped row (v1-migration.bun.ts:41-47).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Warning {
    pub reason: &'static str,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(rename = "partID", skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(rename = "observedType", skip_serializing_if = "Option::is_none")]
    pub observed_type: Option<String>,
}

/// A projected `session_message` row.
#[derive(Debug, Clone)]
pub struct MigratedMessage {
    pub id: String,
    pub session_id: String,
    pub r#type: String,
    pub seq: i64,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: Value,
}

/// The `session_v2` aggregate columns written back after projection.
#[derive(Debug, Clone, Default)]
pub struct SessionAggregate {
    pub agent: Option<String>,
    pub model: Option<String>,
    pub cost: f64,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub tokens_reasoning: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub revert: Option<Value>,
    pub time_compacting: Option<Value>,
}

/// [`transform_session`] output (v1-migration.bun.ts:49-74).
#[derive(Debug, Clone, Default)]
pub struct TransformResult {
    pub messages: Vec<MigratedMessage>,
    pub session: SessionAggregate,
    pub watermark: i64,
    pub warnings: Vec<Warning>,
}

/// Progress label for a running migration.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Progress {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numerator: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denominator: Option<i64>,
}

/// `GET /api/experimental/migration/v1` payload (v1-migration.bun.ts:82-89).
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Required,
    Running(Progress),
    Error(String),
    Completed,
}

impl Status {
    /// The wire JSON (schema `MigrationStatus`).
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Status::Required => json!({ "status": "required" }),
            Status::Completed => json!({ "status": "completed" }),
            Status::Error(error) => json!({ "status": "error", "error": error }),
            Status::Running(progress) => json!({
                "status": "running",
                "progress": serde_json::to_value(progress).unwrap_or_else(|_| json!({})),
            }),
        }
    }
}

/// Inputs for [`run`].
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Explicit `opencode-next.db` path (tests); `None` derives from
    /// `data_root`, or skips entirely for in-memory databases.
    pub next_database_path: Option<PathBuf>,
    /// `Global.data` (engine data root).
    pub data_root: Option<PathBuf>,
    /// In-memory database: no opencode-next.db lookup (`OPENCODE_DB=:memory:`).
    pub in_memory: bool,
}

/// Durable KV state (`migration.v1-v2`).
#[derive(Debug, Clone, PartialEq)]
enum MigrationState {
    Sessions { cursor: Option<String> },
    Completed,
}

/// In-process runtime state (the ref's module-level `runtimeState`).
#[derive(Debug, Clone, Default)]
enum RuntimeState {
    #[default]
    Idle,
    Running(Progress),
    Error(String),
}

static RUNTIME: std::sync::Mutex<RuntimeState> = std::sync::Mutex::new(RuntimeState::Idle);
static RUN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Tests share one process (and therefore one [`RUNTIME`]); each test
/// resets the module-level runtime state so a prior test's error does
/// not leak into `status()` for an unrelated database.
#[cfg(test)]
pub(crate) fn reset_runtime_for_test() {
    *RUNTIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = RuntimeState::Idle;
}

const MIGRATION_STATE_KEY: &str = "migration.v1-v2";
const EVENT_DELETE_BATCH_SIZE: i64 = 1_000;

// ---------------------------------------------------------------------------
// Value accessors
// ---------------------------------------------------------------------------

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn f(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn i(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn b(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn arr<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_array)
}

// ---------------------------------------------------------------------------
// V1 schema validation (SessionV1.Info / SessionV1.Part decoding)
// ---------------------------------------------------------------------------

/// `SessionV1.Part` discriminators.
const PART_TYPES: &[&str] = &[
    "text",
    "reasoning",
    "tool",
    "file",
    "agent",
    "subtask",
    "compaction",
    "step-start",
    "step-finish",
    "snapshot",
    "patch",
    "retry",
];

fn decode_message(row: &SourceMessage) -> Option<Value> {
    let mut value: Value = serde_json::from_str(&row.data).ok()?;
    let map = value.as_object_mut()?;
    map.insert("id".into(), json!(row.id));
    map.insert("sessionID".into(), json!(row.session_id));
    let role = s(&value, "role")?;
    match role {
        "user" => {
            i(value.get("time")?, "created")?;
            s(&value, "agent")?;
            let model = value.get("model")?;
            s(model, "providerID")?;
            s(model, "modelID")?;
        }
        "assistant" => {
            i(value.get("time")?, "created")?;
            s(&value, "parentID")?;
            s(&value, "modelID")?;
            s(&value, "providerID")?;
            s(&value, "agent")?;
            f(&value, "cost")?;
            let tokens = value.get("tokens")?;
            f(tokens, "input")?;
            f(tokens, "output")?;
            f(tokens, "reasoning")?;
            let cache = tokens.get("cache")?;
            f(cache, "read")?;
            f(cache, "write")?;
        }
        _ => return None,
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// transformSession (v1-migration.bun.ts:222-491)
// ---------------------------------------------------------------------------

/// The pure v1 → v2 content transform.
#[must_use]
pub fn transform_session(
    session: &SourceSession,
    messages: &[SourceMessage],
    parts: &[SourcePart],
) -> TransformResult {
    let mut warnings: Vec<Warning> = Vec::new();

    // Decode + validate messages, then order by (time_created, id).
    let mut decoded_messages: Vec<(&SourceMessage, Value)> = Vec::new();
    for row in messages {
        match decode_message(row) {
            Some(value) => decoded_messages.push((row, value)),
            None => warnings.push(Warning {
                reason: "invalid-message",
                session_id: session.id.clone(),
                message_id: Some(row.id.clone()),
                part_id: None,
                observed_type: None,
            }),
        }
    }
    decoded_messages.sort_by(|a, b| (a.0.time_created, &a.0.id).cmp(&(b.0.time_created, &b.0.id)));

    let message_ids: HashSet<&str> = messages.iter().map(|row| row.id.as_str()).collect();

    // Decode + validate parts; orphans warn before decoding.
    let mut decoded_parts: Vec<(&SourcePart, Value)> = Vec::new();
    for row in parts {
        let parsed: Option<Value> = serde_json::from_str(&row.data).ok();
        let observed_type = parsed
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            .map(String::from);
        if !message_ids.contains(row.message_id.as_str()) {
            warnings.push(Warning {
                reason: "orphan-part",
                session_id: session.id.clone(),
                message_id: Some(row.message_id.clone()),
                part_id: Some(row.id.clone()),
                observed_type,
            });
            continue;
        }
        match parsed.and_then(decode_part_with(row)) {
            Some(value) => decoded_parts.push((row, value)),
            None => warnings.push(Warning {
                reason: "invalid-part",
                session_id: session.id.clone(),
                message_id: Some(row.message_id.clone()),
                part_id: Some(row.id.clone()),
                observed_type,
            }),
        }
    }
    decoded_parts.sort_by(|a, b| a.0.id.cmp(&b.0.id));

    // Parts grouped per message (part-id order preserved).
    let mut by_message: HashMap<&str, Vec<&Value>> = HashMap::new();
    for (row, value) in &decoded_parts {
        by_message.entry(row.message_id.as_str()).or_default().push(value);
    }
    let owned_of = |id: &str| by_message.get(id).cloned().unwrap_or_default();

    let mut paired: HashSet<String> = HashSet::new();
    let mut used: HashSet<String> = messages.iter().map(|row| row.id.clone()).collect();

    // (id, source row, time_updated, data, type) — the id is the row id
    // except for split synthetic rows (deterministic derived ids).
    let mut projected: Vec<(String, &SourceMessage, i64, Value, &'static str)> = Vec::new();
    for (row, value) in &decoded_messages {
        if paired.contains(&row.id) {
            continue;
        }
        let owned = owned_of(&row.id);
        if s(value, "role") == Some("user") {
            // A compaction part pairs this user message with its summary.
            if let Some(compaction) =
                owned.iter().find(|part| s(part, "type") == Some("compaction"))
            {
                let summary = decoded_messages.iter().find(|(_candidate, decoded)| {
                    s(decoded, "role") == Some("assistant")
                        && s(decoded, "parentID") == Some(row.id.as_str())
                        && b(decoded, "summary") == Some(true)
                });
                let Some((summary_row, summary_value)) = summary else {
                    continue;
                };
                paired.insert(summary_row.id.clone());
                if summary_value.get("error").is_some()
                    || summary_value.get("time").and_then(|time| time.get("completed")).is_none()
                {
                    continue;
                }
                let summary_text = owned_of(&summary_row.id)
                    .iter()
                    .filter(|part| {
                        s(part, "type") == Some("text")
                            && s(part, "text").is_some_and(|text| !text.is_empty())
                    })
                    .filter_map(|part| s(part, "text").map(String::from))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let tail_index: i64 = compaction
                    .get("tail_start_id")
                    .and_then(Value::as_str)
                    .and_then(|tail| {
                        decoded_messages.iter().position(|(candidate, _)| candidate.id == tail)
                    })
                    .map_or(-1, |index| index as i64);
                let compaction_index: i64 = decoded_messages
                    .iter()
                    .position(|(candidate, _)| candidate.id == row.id)
                    .map_or(-1, |index| index as i64);
                let tail = if tail_index < 0 || tail_index >= compaction_index {
                    &decoded_messages[..0]
                } else {
                    &decoded_messages[tail_index as usize..compaction_index as usize]
                };
                projected.push((
                    row.id.clone(),
                    row,
                    summary_row.time_updated.max(row.time_updated),
                    json!({
                        "status": "completed",
                        "reason": if b(compaction, "auto") == Some(true) { "auto" } else { "manual" },
                        "summary": summary_text,
                        "recent": serialize_recent(tail, &by_message),
                        "time": { "created": row.time_created },
                    }),
                    "compaction",
                ));
                continue;
            }
            let subtasks = owned.iter().filter(|part| s(part, "type") == Some("subtask")).count();
            let visible: Vec<&Value> = owned
                .iter()
                .filter(|part| s(part, "type") == Some("text") && b(part, "ignored") != Some(true))
                .copied()
                .collect();
            let files: Vec<&Value> =
                owned.iter().filter(|part| s(part, "type") == Some("file")).copied().collect();
            let agents: Vec<&Value> =
                owned.iter().filter(|part| s(part, "type") == Some("agent")).copied().collect();
            if subtasks > 0 && visible.is_empty() && files.is_empty() && agents.is_empty() {
                continue;
            }
            let ordinary: Vec<&Value> =
                visible.iter().filter(|part| b(part, "synthetic") != Some(true)).copied().collect();
            let synthetic: Vec<&Value> =
                visible.iter().filter(|part| b(part, "synthetic") == Some(true)).copied().collect();
            let attachments: Vec<Value> =
                files.iter().flat_map(|part| migrate_file(part)).collect();
            let unavailable: Vec<String> = files
                .iter()
                .filter(|part| s(part, "url").is_some_and(|url| !url.starts_with("data:")))
                .map(|part| unavailable_file(part))
                .collect::<Vec<String>>();
            let mut text_parts: Vec<String> = Vec::new();
            for part in &owned {
                if s(part, "type") == Some("text")
                    && b(part, "ignored") != Some(true)
                    && b(part, "synthetic") != Some(true)
                {
                    text_parts.push(s(part, "text").unwrap_or_default().to_owned());
                } else if s(part, "type") == Some("file")
                    && s(part, "url").is_some_and(|url| !url.starts_with("data:"))
                {
                    text_parts.push(unavailable_file(part));
                }
            }
            let text = text_parts.join("\n\n");
            let agent_attachments: Vec<Value> = agents
                .iter()
                .map(|part| {
                    let mut entry = Map::new();
                    entry.insert("name".into(), json!(s(part, "name").unwrap_or_default()));
                    if let Some(source) = part.get("source") {
                        entry.insert(
                            "mention".into(),
                            json!({
                                "text": s(source, "value").unwrap_or_default(),
                                "start": i(source, "start").unwrap_or_default(),
                                "end": i(source, "end").unwrap_or_default(),
                            }),
                        );
                    }
                    Value::Object(entry)
                })
                .collect();
            if ordinary.is_empty()
                && unavailable.is_empty()
                && !synthetic.is_empty()
                && attachments.is_empty()
                && agent_attachments.is_empty()
            {
                projected.push((
                    row.id.clone(),
                    row,
                    row.time_updated,
                    json!({
                        "text": synthetic.iter().filter_map(|part| s(part, "text")).collect::<Vec<_>>().join("\n\n"),
                        "time": { "created": row.time_created },
                    }),
                    "synthetic",
                ));
                continue;
            }
            let mut data = Map::new();
            data.insert("text".into(), json!(text));
            if !attachments.is_empty() {
                data.insert("files".into(), json!(attachments));
            }
            if !agent_attachments.is_empty() {
                data.insert("agents".into(), json!(agent_attachments));
            }
            data.insert("time".into(), json!({ "created": row.time_created }));
            projected.push((row.id.clone(), row, row.time_updated, Value::Object(data), "user"));
            if !synthetic.is_empty() {
                let id = synthetic_id(&row.id, &mut used);
                projected.push((
                    id,
                    row,
                    row.time_updated,
                    json!({
                        "text": synthetic.iter().filter_map(|part| s(part, "text")).collect::<Vec<_>>().join("\n\n"),
                        "time": { "created": row.time_created },
                    }),
                    "synthetic",
                ));
            }
            continue;
        }
        // role != "assistant" is impossible (validation), keep the shape.
        if s(value, "role") != Some("assistant") {
            continue;
        }
        // Subtask pair: a user message holding the subtask + the task tool
        // result — both sides of the subagent bridge stay out of history.
        let parent = decoded_messages
            .iter()
            .find(|(candidate, _)| Some(candidate.id.as_str()) == s(value, "parentID"));
        let parent_parts = parent.map(|(row, _)| owned_of(&row.id)).unwrap_or_default();
        if parent_parts.iter().any(|part| s(part, "type") == Some("subtask"))
            && owned
                .iter()
                .any(|part| s(part, "type") == Some("tool") && s(part, "tool") == Some("task"))
        {
            continue;
        }
        let content: Vec<Value> = owned
            .iter()
            .flat_map(|part| -> Vec<Value> {
                match s(part, "type") {
                    Some("text") => {
                        let mut entry =
                            json!({ "type": "text", "text": s(part, "text").unwrap_or_default() });
                        if let Some(metadata) = part.get("metadata") {
                            entry["state"] = metadata.clone();
                        }
                        vec![entry]
                    }
                    Some("reasoning") => {
                        let mut entry =
                            json!({ "type": "reasoning", "text": s(part, "text").unwrap_or_default() });
                        if let Some(metadata) = part.get("metadata") {
                            entry["state"] = metadata.clone();
                        }
                        if let Some(time) = part.get("time") {
                            let mut time_value = json!({ "created": i(time, "start").unwrap_or_default() });
                            if let Some(end) = time.get("end") {
                                time_value["completed"] = end.clone();
                            }
                            entry["time"] = time_value;
                        }
                        vec![entry]
                    }
                    Some("tool") => vec![migrate_tool(part, row.time_created)],
                    _ => vec![],
                }
            })
            .collect();
        let start = owned
            .iter()
            .filter(|part| s(part, "type") == Some("step-start"))
            .find_map(|part| part.get("snapshot").cloned())
            .or_else(|| {
                owned
                    .iter()
                    .filter(|part| s(part, "type") == Some("snapshot"))
                    .find_map(|part| part.get("snapshot").cloned())
            })
            .or_else(|| {
                owned
                    .iter()
                    .filter(|part| s(part, "type") == Some("patch"))
                    .find_map(|part| part.get("hash").cloned())
            });
        let end = owned
            .iter()
            .filter(|part| s(part, "type") == Some("step-finish"))
            .filter_map(|part| part.get("snapshot").cloned())
            .next_back();
        let mut snapshot_files: Vec<String> = Vec::new();
        for part in &owned {
            if s(part, "type") == Some("patch") {
                for file in arr(part, "files").into_iter().flatten() {
                    if let Some(file) = file.as_str() {
                        if !snapshot_files.iter().any(|seen| seen == file) {
                            snapshot_files.push(file.to_owned());
                        }
                    }
                }
            }
        }
        let finish = s(value, "finish").and_then(normalize_finish);
        let mut data = Map::new();
        data.insert("agent".into(), json!(s(value, "agent").unwrap_or_default()));
        data.insert(
            "model".into(),
            json!({
                "providerID": s(value, "providerID").unwrap_or_default(),
                "id": s(value, "modelID").unwrap_or_default(),
                "variant": s(value, "variant").unwrap_or("default"),
            }),
        );
        data.insert("content".into(), json!(content));
        if start.is_some() || end.is_some() || !snapshot_files.is_empty() {
            let mut snapshot = Map::new();
            if let Some(start) = start {
                snapshot.insert("start".into(), start);
            }
            if let Some(end) = end {
                snapshot.insert("end".into(), end);
            }
            if !snapshot_files.is_empty() {
                snapshot.insert("files".into(), json!(snapshot_files));
            }
            data.insert("snapshot".into(), Value::Object(snapshot));
        }
        if let Some(finish) = finish {
            data.insert("finish".into(), json!(finish));
        }
        data.insert("cost".into(), json!(f(value, "cost").unwrap_or_default()));
        let tokens = value.get("tokens").cloned().unwrap_or_else(|| json!({}));
        data.insert(
            "tokens".into(),
            json!({
                "input": i(&tokens, "input").unwrap_or_default(),
                "output": i(&tokens, "output").unwrap_or_default(),
                "reasoning": i(&tokens, "reasoning").unwrap_or_default(),
                "cache": {
                    "read": i(tokens.get("cache").unwrap_or(&Value::Null), "read").unwrap_or_default(),
                    "write": i(tokens.get("cache").unwrap_or(&Value::Null), "write").unwrap_or_default(),
                },
            }),
        );
        if let Some(error) = value.get("error") {
            if let Some(migrated) = migrate_error(error) {
                data.insert("error".into(), migrated);
            }
        }
        let mut time = Map::new();
        time.insert("created".into(), json!(row.time_created));
        if value.get("time").and_then(|time| time.get("completed")).is_some() {
            time.insert("completed".into(), json!(row.time_updated));
        }
        data.insert("time".into(), Value::Object(time));
        projected.push((row.id.clone(), row, row.time_updated, Value::Object(data), "assistant"));
    }

    // Contiguous sequence numbers.
    let mut messages_out: Vec<MigratedMessage> = Vec::new();
    for (seq, (id, row, time_updated, data, ty)) in projected.into_iter().enumerate() {
        messages_out.push(MigratedMessage {
            id,
            session_id: row.session_id.clone(),
            r#type: ty.to_owned(),
            seq: seq as i64,
            time_created: row.time_created,
            time_updated,
            data,
        });
    }

    // The trailing legacy-tool notice (system row) when the still-visible
    // history called V1-only tool names.
    let notice = legacy_tool_notice(&messages_out);
    if let Some(notice) = notice {
        if let Some(last) = messages_out.last() {
            let id = synthetic_id(&last.id, &mut used);
            messages_out.push(MigratedMessage {
                id,
                session_id: last.session_id.clone(),
                r#type: "system".to_owned(),
                seq: messages_out.len() as i64,
                time_created: last.time_created,
                time_updated: last.time_updated,
                data: json!({ "text": notice, "time": { "created": last.time_created } }),
            });
        }
    }

    // Session aggregates: every assistant's usage (projected or not), with
    // agent/model falling back to the latest conversational user message.
    let mut aggregate = SessionAggregate {
        agent: session.agent.clone(),
        model: session.model.clone(),
        cost: 0.0,
        tokens_input: 0,
        tokens_output: 0,
        tokens_reasoning: 0,
        tokens_cache_read: 0,
        tokens_cache_write: 0,
        revert: None,
        time_compacting: None,
    };
    let latest_user = decoded_messages.iter().rev().find(|(row, value)| {
        if s(value, "role") != Some("user") {
            return false;
        }
        let owned = owned_of(&row.id);
        if owned.iter().any(|part| s(part, "type") == Some("compaction")) {
            return false;
        }
        let has_subtask = owned.iter().any(|part| s(part, "type") == Some("subtask"));
        !has_subtask || !owned.iter().all(|part| s(part, "type") == Some("subtask"))
    });
    if aggregate.agent.is_none() {
        if let Some((_, value)) = latest_user {
            aggregate.agent = s(value, "agent").map(String::from);
        }
    }
    if aggregate.model.is_none() {
        if let Some((_, value)) = latest_user {
            let model = value.get("model").cloned().unwrap_or(Value::Null);
            if !model.is_null() {
                aggregate.model = Some(
                    json!({
                        "id": s(&model, "modelID").unwrap_or_default(),
                        "providerID": s(&model, "providerID").unwrap_or_default(),
                        "variant": s(&model, "variant").unwrap_or("default"),
                    })
                    .to_string(),
                );
            }
        }
    }
    for (_, value) in &decoded_messages {
        if s(value, "role") != Some("assistant") {
            continue;
        }
        aggregate.cost += f(value, "cost").unwrap_or_default();
        let tokens = value.get("tokens").cloned().unwrap_or_else(|| json!({}));
        let cache = tokens.get("cache").cloned().unwrap_or(Value::Null);
        aggregate.tokens_input += i(&tokens, "input").unwrap_or_default();
        aggregate.tokens_output += i(&tokens, "output").unwrap_or_default();
        aggregate.tokens_reasoning += i(&tokens, "reasoning").unwrap_or_default();
        aggregate.tokens_cache_read += i(&cache, "read").unwrap_or_default();
        aggregate.tokens_cache_write += i(&cache, "write").unwrap_or_default();
    }

    TransformResult {
        watermark: messages_out.len() as i64 - 1,
        messages: messages_out,
        session: aggregate,
        warnings,
    }
}

/// Re-decodes against an already-parsed value (keeps the injected ids).
fn decode_part_with(row: &SourcePart) -> impl FnOnce(Value) -> Option<Value> + '_ {
    move |mut value| {
        if !value.is_object() {
            return None;
        }
        let map = value.as_object_mut()?;
        map.insert("id".into(), json!(row.id));
        map.insert("messageID".into(), json!(row.message_id));
        map.insert("sessionID".into(), json!(row.session_id));
        let ty = s(&value, "type")?;
        if !PART_TYPES.contains(&ty) {
            return None;
        }
        match ty {
            "text" => {
                s(&value, "text")?;
            }
            "tool" => {
                s(&value, "callID")?;
                s(&value, "tool")?;
                value.get("state")?.as_object()?;
            }
            "file" => {
                s(&value, "url")?;
                s(&value, "mime")?;
            }
            "compaction" => {
                b(&value, "auto")?;
            }
            _ => {}
        }
        Some(value)
    }
}

// `row()` (v1-migration.bun.ts:878-897) is inlined at the projection
// sites (the tuple carries the id + envelope timestamps directly).

// ---------------------------------------------------------------------------
// Part migration helpers (v1-migration.bun.ts:899-1108)
// ---------------------------------------------------------------------------

const RENAMED_TOOLS: &[(&str, &str)] =
    &[("bash", "shell"), ("task", "subagent"), ("apply_patch", "patch")];
const PATH_TOOLS: &[&str] = &["read", "edit", "write"];
const REMOVED_TOOLS: &[&str] = &["todowrite"];

fn legacy_tool_notice(messages: &[MigratedMessage]) -> Option<String> {
    let start = messages
        .iter()
        .rposition(|message| message.r#type == "compaction")
        .map_or(0, |index| index + 1);
    let mut called: HashSet<&str> = HashSet::new();
    for message in &messages[start.min(messages.len())..] {
        if message.r#type != "assistant" {
            continue;
        }
        for item in arr(&message.data, "content").into_iter().flatten() {
            if item.get("type").and_then(Value::as_str) == Some("tool") {
                if let Some(name) = s(item, "name") {
                    called.insert(name);
                }
            }
        }
    }
    let renamed: Vec<&(&str, &str)> =
        RENAMED_TOOLS.iter().filter(|(name, _)| called.contains(name)).collect();
    let paths: Vec<&str> =
        PATH_TOOLS.iter().copied().filter(|name| called.contains(name)).collect();
    let removed: Vec<&str> =
        REMOVED_TOOLS.iter().copied().filter(|name| called.contains(name)).collect();
    let list = |names: &[&str]| {
        names.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")
    };
    let mut parts: Vec<String> = Vec::new();
    if renamed.len() == 1 {
        parts.push(format!(
            "The `{}` tool is now `{}` and must be called by that name.",
            renamed[0].0, renamed[0].1
        ));
    } else if renamed.len() > 1 {
        parts.push(format!(
            "The following tools were renamed and must be called by their new names: {}.",
            renamed
                .iter()
                .map(|(name, to)| format!("`{name}` is now `{to}`"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if called.contains("task") {
        parts.push(
            "The `subagent` tool takes `agent` instead of `subagent_type` and `sessionID` instead of `task_id`."
                .to_owned(),
        );
    }
    if paths.len() == 1 {
        parts.push(format!("The `{}` tool now takes `path` instead of `filePath`.", paths[0]));
    } else if paths.len() > 1 {
        parts.push(format!(
            "The following tools now take `path` instead of `filePath`: {}.",
            list(&paths)
        ));
    }
    if called.contains("skill") {
        parts.push("The `skill` tool now takes `id` instead of `name`.".to_owned());
    }
    if removed.len() == 1 {
        parts.push(format!(
            "The `{}` tool is no longer available and must not be called.",
            removed[0]
        ));
    } else if removed.len() > 1 {
        parts.push(format!(
            "The following tools are no longer available and must not be called: {}.",
            list(&removed)
        ));
    }
    if parts.is_empty() {
        return None;
    }
    Some(
        ["The available tools have changed.".to_owned()]
            .into_iter()
            .chain(parts)
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}

fn migrate_tool(part: &Value, fallback: i64) -> Value {
    let state = part.get("state").cloned().unwrap_or(Value::Null);
    let mut base = Map::new();
    base.insert("type".into(), json!("tool"));
    base.insert("id".into(), json!(s(part, "callID").unwrap_or_default()));
    base.insert("name".into(), json!(s(part, "tool").unwrap_or_default()));
    if let Some(metadata) = part.get("metadata") {
        base.insert("providerState".into(), metadata.clone());
    }
    let time = state.get("time").cloned().unwrap_or(Value::Null);
    match s(&state, "status") {
        Some("completed") => {
            let content = if time.get("compacted").is_some() {
                vec![json!({ "type": "text", "text": "[Old tool result content cleared]" })]
            } else {
                let mut content = vec![
                    json!({ "type": "text", "text": s(&state, "output").unwrap_or_default() }),
                ];
                for file in arr(&state, "attachments").into_iter().flatten() {
                    let mut entry = json!({
                        "type": "file",
                        "uri": s(file, "url").unwrap_or_default(),
                        "mime": s(file, "mime").unwrap_or_default(),
                    });
                    if let Some(filename) = s(file, "filename") {
                        entry["name"] = json!(filename);
                    }
                    content.push(entry);
                }
                content
            };
            let mut state_value = json!({
                "status": "completed",
                "input": state.get("input").cloned().unwrap_or(Value::Null),
                "content": content,
            });
            if let Some(metadata) = state.get("metadata") {
                state_value["metadata"] = metadata.clone();
            }
            Value::Object({
                let mut entry = base;
                entry.insert("state".into(), state_value);
                entry.insert(
                    "time".into(),
                    json!({
                        "created": i(&time, "start").unwrap_or_default(),
                        "completed": i(&time, "end").unwrap_or_default(),
                    }),
                );
                entry
            })
        }
        Some("error") => {
            let mut state_value = json!({
                "status": "error",
                "input": state.get("input").cloned().unwrap_or(Value::Null),
                "error": {
                    "type": "tool.execution",
                    "message": s(&state, "error").unwrap_or_default(),
                },
            });
            if let Some(output) = state.get("metadata").and_then(|metadata| metadata.get("output"))
            {
                if output.is_string() {
                    state_value["content"] = json!([{ "type": "text", "text": output }]);
                }
            }
            if let Some(metadata) = state.get("metadata") {
                state_value["metadata"] = metadata.clone();
            }
            Value::Object({
                let mut entry = base;
                entry.insert("state".into(), state_value);
                entry.insert(
                    "time".into(),
                    json!({
                        "created": i(&time, "start").unwrap_or_default(),
                        "completed": i(&time, "end").unwrap_or_default(),
                    }),
                );
                entry
            })
        }
        _ => {
            let mut state_value = json!({
                "status": "error",
                "input": state.get("input").cloned().unwrap_or(Value::Null),
                "error": {
                    "type": "tool.interrupted",
                    "message": "Tool execution was interrupted before V2 migration",
                },
            });
            if s(&state, "status") == Some("running") {
                if let Some(metadata) = state.get("metadata") {
                    state_value["metadata"] = metadata.clone();
                }
            }
            Value::Object({
                let mut entry = base;
                entry.insert("state".into(), state_value);
                entry.insert(
                    "time".into(),
                    json!({ "created": if s(&state, "status") == Some("running") {
                        i(&time, "start").unwrap_or_default()
                    } else {
                        fallback
                    }}),
                );
                entry
            })
        }
    }
}

fn migrate_error(error: &Value) -> Option<Value> {
    let name = s(error, "name")?;
    let data = error.get("data").cloned().unwrap_or(Value::Null);
    let message = s(&data, "message").map(String::from).or_else(|| match name {
        "MessageOutputLengthError" => Some("The model exceeded its output limit".to_owned()),
        _ => Some(name.to_owned()),
    })?;
    let ty = match name {
        "ProviderAuthError" => "provider.auth",
        "ContentFilterError" => "provider.content-filter",
        "ContextOverflowError" => "provider.invalid-request",
        "StructuredOutputError" | "MessageOutputLengthError" => "provider.invalid-output",
        "MessageAbortedError" => "aborted",
        "APIError" => "provider.error",
        _ => "unknown",
    };
    Some(json!({ "type": ty, "message": message }))
}

fn normalize_finish(finish: &str) -> Option<&'static str> {
    (["stop", "length", "tool-calls", "content-filter", "error", "unknown"]
        .into_iter()
        .find(|value| *value == finish))
    .or(Some("unknown"))
}

fn migrate_file(part: &Value) -> Vec<Value> {
    let url = s(part, "url").unwrap_or_default();
    if !url.starts_with("data:") {
        return vec![];
    }
    let Some(comma) = url.find(',') else { return vec![] };
    let header = &url[..comma];
    let payload = &url[comma + 1..];
    let data = if header.ends_with(";base64") {
        payload.to_owned()
    } else {
        match percent_decode(payload).and_then(|bytes| String::from_utf8(bytes).ok()) {
            Some(decoded) => {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD.encode(decoded.as_bytes())
            }
            None => return vec![],
        }
    };
    let mut entry = Map::new();
    entry.insert("data".into(), json!(data));
    entry.insert("mime".into(), json!(s(part, "mime").unwrap_or_default()));
    let source = part.get("source");
    entry.insert(
        "source".into(),
        if source.and_then(|source| s(source, "type")) == Some("resource") {
            json!({ "type": "uri", "uri": source.and_then(|source| s(source, "uri")).unwrap_or_default() })
        } else {
            json!({ "type": "inline" })
        },
    );
    if let Some(filename) = s(part, "filename") {
        entry.insert("name".into(), json!(filename));
    }
    if let Some(source) = source {
        if let Some(text) = source.get("text") {
            entry.insert(
                "mention".into(),
                json!({
                    "text": s(text, "value").unwrap_or_default(),
                    "start": i(text, "start").unwrap_or_default(),
                    "end": i(text, "end").unwrap_or_default(),
                }),
            );
        }
    }
    vec![Value::Object(entry)]
}

fn unavailable_file(part: &Value) -> String {
    let label = s(part, "filename")
        .map(String::from)
        .or_else(|| {
            (part.get("source").and_then(|source| s(source, "type")) == Some("resource")).then(
                || {
                    part.get("source")
                        .and_then(|source| s(source, "uri"))
                        .unwrap_or_default()
                        .to_owned()
                },
            )
        })
        .unwrap_or_else(|| s(part, "url").unwrap_or_default().to_owned());
    format!(
        "[Attachment unavailable after migration: {label} ({})]",
        s(part, "mime").unwrap_or_default()
    )
}

fn percent_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
                let byte = u8::from_str_radix(hex, 16).ok()?;
                out.push(byte);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    Some(out)
}

/// `v1-synthetic:` id derivation (v1-migration.bun.ts:1067-1083): sha256 →
/// the 14 least-significant base-62 digits of the 256-bit digest, behind a
/// 16-char prefix of the source id; salting until unused.
fn synthetic_id(source: &str, used: &mut HashSet<String>) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let prefix_end = source.len().min(16);
    let prefix = &source[..prefix_end];
    let mut salt: u64 = 0;
    loop {
        let input = if salt == 0 {
            format!("v1-synthetic:{source}")
        } else {
            format!("v1-synthetic:{source}:{salt}")
        };
        let digest = <sha2::Sha256 as sha2::Digest>::digest(input.as_bytes());
        let mut limbs = [0u32; 8];
        for (index, byte) in digest.iter().enumerate() {
            limbs[index / 4] = (limbs[index / 4] << 8) | u32::from(*byte);
        }
        let mut digits = [0u8; 14];
        for digit in &mut digits {
            *digit = ALPHABET[divmod62(&mut limbs) as usize];
        }
        let id = format!(
            "{prefix}{}",
            digits.iter().rev().map(|byte| *byte as char).collect::<String>()
        );
        if used.contains(&id) {
            salt += 1;
            continue;
        }
        used.insert(id.clone());
        return id;
    }
}

/// `limbs /= 62`, returning `limbs % 62` (big-endian u32 limbs).
fn divmod62(limbs: &mut [u32; 8]) -> u32 {
    let mut remainder: u64 = 0;
    for limb in limbs.iter_mut() {
        let current = (remainder << 32) | u64::from(*limb);
        *limb = u32::try_from(current / 62).unwrap_or(u32::MAX);
        remainder = current % 62;
    }
    u32::try_from(remainder).unwrap_or(u32::MAX)
}

fn serialize_recent<'a>(
    messages: &[(&'a SourceMessage, Value)],
    parts: &HashMap<&str, Vec<&'a Value>>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (row, value) in messages {
        let owned = parts.get(row.id.as_str()).cloned().unwrap_or_default();
        if s(value, "role") == Some("user") {
            lines.push(format!(
                "[User]: {}",
                owned
                    .iter()
                    .filter(
                        |part| s(part, "type") == Some("text") && b(part, "ignored") != Some(true)
                    )
                    .filter_map(|part| s(part, "text"))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            ));
        } else {
            for part in &owned {
                if s(part, "type") == Some("text") {
                    lines.push(format!("[Assistant]: {}", s(part, "text").unwrap_or_default()));
                } else if s(part, "type") == Some("reasoning")
                    && s(part, "text").is_some_and(|t| !t.is_empty())
                {
                    lines.push(format!(
                        "[Assistant reasoning]: {}",
                        s(part, "text").unwrap_or_default()
                    ));
                }
            }
        }
    }
    lines.join("\n\n")
}

// ---------------------------------------------------------------------------
// Orchestration (v1-migration.bun.ts:493-700, 1109-1135)
// ---------------------------------------------------------------------------

fn parse_state(input: Option<String>) -> Option<MigrationState> {
    let raw = input?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    if !value.is_object() {
        return None;
    }
    match s(&value, "phase") {
        Some("completed") => Some(MigrationState::Completed),
        Some("sessions") => match value.get("cursor") {
            None | Some(Value::Null) => Some(MigrationState::Sessions { cursor: None }),
            Some(cursor) => cursor
                .as_str()
                .map(|cursor| MigrationState::Sessions { cursor: Some(cursor.to_owned()) }),
        },
        _ => None,
    }
}

fn has_legacy_sessions(tx: &mut Tx<'_>) -> Result<bool> {
    let present: Option<String> = tx
        .conn()
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'session'",
            [],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok::<Option<String>, rusqlite::Error>(None),
            error => Err(error),
        })?;
    Ok(present.is_some())
}

fn read_state(tx: &mut Tx<'_>) -> Result<Option<MigrationState>> {
    let raw: Option<String> = tx
        .conn()
        .query_row(
            "SELECT value FROM kv WHERE key = ?1",
            rusqlite::params![MIGRATION_STATE_KEY],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok::<Option<String>, rusqlite::Error>(None),
            error => Err(error),
        })?;
    Ok(parse_state(raw))
}

fn write_state(tx: &mut Tx<'_>, state: &Value) -> Result<()> {
    tx.conn().execute(
        "INSERT INTO kv (key, value, time_created, time_updated) VALUES (?1, ?2, ?3, ?3) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, time_updated = excluded.time_updated",
        rusqlite::params![MIGRATION_STATE_KEY, state.to_string(), super::migrations::now_ms()],
    )?;
    Ok(())
}

fn update_progress(progress: Progress) {
    let mut runtime = RUNTIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if matches!(*runtime, RuntimeState::Running(_)) {
        *runtime = RuntimeState::Running(progress);
    }
}

/// `status()` (v1-migration.bun.ts:493-503).
pub async fn status(db: &Database) -> Status {
    let legacy = db.transaction(|tx| has_legacy_sessions(tx)).await.unwrap_or(false);
    if !legacy {
        return Status::Completed;
    }
    match &*RUNTIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        RuntimeState::Running(progress) => return Status::Running(progress.clone()),
        RuntimeState::Error(error) => return Status::Error(error.clone()),
        RuntimeState::Idle => {}
    }
    let state = db.transaction(|tx| read_state(tx)).await.ok().flatten();
    match state {
        Some(MigrationState::Completed) => Status::Completed,
        _ => Status::Required,
    }
}

fn next_path(options: &RunOptions) -> Option<PathBuf> {
    if let Some(path) = &options.next_database_path {
        return Some(path.clone());
    }
    if options.in_memory {
        return None;
    }
    options.data_root.as_ref().map(|root| root.join("opencode-next.db"))
}

/// `run()` (v1-migration.bun.ts:534-700): the resumable content walk.
pub async fn run(db: &Database, options: RunOptions) -> Result<String> {
    let _permit = RUN_LOCK.lock().await;
    {
        let mut runtime = RUNTIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *runtime = RuntimeState::Running(Progress {
            label: "Clearing old events".to_owned(),
            numerator: None,
            denominator: None,
        });
    }
    let outcome = run_inner(db, &options).await;
    {
        let mut runtime = RUNTIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *runtime = match outcome {
            Ok(()) => RuntimeState::Idle,
            Err(ref error) => RuntimeState::Error(error.to_string()),
        };
    }
    outcome.map(|()| "completed".to_owned())
}

async fn run_inner(db: &Database, options: &RunOptions) -> Result<()> {
    let state = db.transaction(|tx| read_state(tx)).await?;
    if state == Some(MigrationState::Completed) {
        return Ok(());
    }
    if !db.transaction(|tx| has_legacy_sessions(tx)).await? {
        return Ok(());
    }

    // The global fallback project (worktree = the data root's filesystem
    // root — `path.parse(global.data).root`, "/" on unix).
    let now = super::migrations::now_ms();
    db.transaction(move |tx| {
        tx.conn().execute(
            "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, time_active, sandboxes) \
             VALUES ('global', '/', ?1, ?1, ?1, '[]')",
            rusqlite::params![now],
        )?;
        Ok(())
    })
    .await?;

    if state.is_none() {
        // Batched stale-event clearing + the sessions-phase checkpoint.
        db.transaction(|tx| {
            loop {
                tx.conn().execute(
                    "DELETE FROM event WHERE rowid IN (SELECT rowid FROM event LIMIT ?1)",
                    rusqlite::params![EVENT_DELETE_BATCH_SIZE],
                )?;
                if tx.conn().changes() < EVENT_DELETE_BATCH_SIZE.unsigned_abs() {
                    break;
                }
            }
            write_state(tx, &json!({ "phase": "sessions" }))?;
            Ok(())
        })
        .await?;
    }

    // Progress accounting over both sources.
    let next = next_path(options);
    let source_total = count_next_sessions(next.as_deref());
    let legacy_total: i64 = db
        .transaction(|tx| {
            Ok(tx.conn().query_row("SELECT COUNT(*) FROM session", [], |row| row.get(0))?)
        })
        .await?;
    let cursor = match state {
        Some(MigrationState::Sessions { cursor }) => cursor,
        _ => None,
    };
    let migrated: i64 = match cursor.clone() {
        Some(cursor) => {
            db.transaction(move |tx| {
                Ok(tx.conn().query_row(
                    "SELECT COUNT(*) FROM session WHERE id >= ?1",
                    rusqlite::params![cursor],
                    |row| row.get(0),
                )?)
            })
            .await?
        }
        None => 0,
    };
    let denominator = source_total + legacy_total;
    update_progress(Progress {
        label: "Migrating sessions".to_owned(),
        numerator: Some(migrated),
        denominator: Some(denominator),
    });

    import_next_database(db, next.as_deref(), |completed| {
        update_progress(Progress {
            label: "Migrating sessions".to_owned(),
            numerator: Some(migrated + completed),
            denominator: Some(denominator),
        });
    })
    .await?;
    update_progress(Progress {
        label: "Migrating sessions".to_owned(),
        numerator: Some(migrated + source_total),
        denominator: Some(denominator),
    });

    // The v1 walk (newest id first, cursor-checkpointed per session).
    let project_rows: Vec<String> = db
        .transaction(|tx| {
            let mut statement = tx.conn().prepare("SELECT id FROM project")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let projects: HashSet<String> = project_rows.into_iter().collect();
    loop {
        let cursor = match db.transaction(|tx| read_state(tx)).await? {
            Some(MigrationState::Sessions { cursor }) => cursor,
            _ => None,
        };
        let lookup_cursor = cursor.clone();
        let next_id: Option<(String, String)> = db
            .transaction(move |tx| {
                let row = match lookup_cursor {
                    Some(cursor) => tx.conn().query_row(
                        "SELECT id, project_id FROM session WHERE id < ?1 ORDER BY id DESC LIMIT 1",
                        rusqlite::params![cursor],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    ),
                    None => tx.conn().query_row(
                        "SELECT id, project_id FROM session ORDER BY id DESC LIMIT 1",
                        [],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    ),
                };
                row.map(Some).or(Ok(None))
            })
            .await?;
        let Some((session_id, session_project)) = next_id else { break };

        let projects = projects.clone();
        let walk_session_id = session_id.clone();
        db.transaction(move |tx| {
            write_state(tx, &json!({ "phase": "sessions", "cursor": walk_session_id }))?;
            let project_id =
                if projects.contains(&session_project) { session_project.clone() } else { "global".to_owned() };
            tx.conn().execute(
                "INSERT OR IGNORE INTO session_v2 (\
                   id, project_id, workspace_id, parent_id, slug, directory, path, title, version, \
                   share_url, summary_additions, summary_deletions, summary_files, summary_diffs, \
                   metadata, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, \
                   tokens_cache_write, revert, permission, agent, model, time_created, time_updated, \
                   time_compacting, time_archived\
                 ) SELECT \
                   id, ?2, workspace_id, parent_id, slug, directory, path, title, version, \
                   share_url, summary_additions, summary_deletions, summary_files, summary_diffs, \
                   metadata, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, \
                   tokens_cache_write, revert, NULL, agent, model, time_created, time_updated, \
                   time_compacting, time_archived \
                 FROM session WHERE id = ?1",
                rusqlite::params![walk_session_id, project_id],
            )?;
            let (session_agent, session_model, session_time_updated): (Option<String>, Option<String>, i64) =
                tx.conn().query_row(
                    "SELECT agent, model, time_updated FROM session_v2 WHERE id = ?1",
                    rusqlite::params![walk_session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
            let mut statement = tx.conn().prepare(
                "SELECT id, session_id, time_created, time_updated, data FROM message WHERE session_id = ?1",
            )?;
            let messages = statement
                .query_map(rusqlite::params![walk_session_id], |row| {
                    Ok(SourceMessage {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        time_created: row.get(2)?,
                        time_updated: row.get(3)?,
                        data: row.get(4)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut statement = tx.conn().prepare(
                "SELECT id, message_id, session_id, time_created, time_updated, data FROM part WHERE session_id = ?1",
            )?;
            let parts = statement
                .query_map(rusqlite::params![walk_session_id], |row| {
                    Ok(SourcePart {
                        id: row.get(0)?,
                        message_id: row.get(1)?,
                        session_id: row.get(2)?,
                        time_created: row.get(3)?,
                        time_updated: row.get(4)?,
                        data: row.get(5)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let transformed = transform_session(
                &SourceSession {
                    id: walk_session_id.clone(),
                    agent: session_agent,
                    model: session_model,
                },
                &messages,
                &parts,
            );
            tx.conn()
                .execute("DELETE FROM session_message WHERE session_id = ?1", rusqlite::params![walk_session_id])?;
            for message in &transformed.messages {
                tx.conn().execute(
                    "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        message.id,
                        message.session_id,
                        message.r#type,
                        message.seq,
                        message.time_created,
                        message.time_updated,
                        message.data.to_string(),
                    ],
                )?;
            }
            let aggregate = &transformed.session;
            tx.conn().execute(
                "UPDATE session_v2 SET agent = ?2, model = ?3, cost = ?4, tokens_input = ?5, \
                 tokens_output = ?6, tokens_reasoning = ?7, tokens_cache_read = ?8, tokens_cache_write = ?9, \
                 revert = NULL, time_compacting = NULL, time_updated = ?10 WHERE id = ?1",
                rusqlite::params![
                    walk_session_id,
                    aggregate.agent,
                    aggregate.model,
                    aggregate.cost,
                    aggregate.tokens_input,
                    aggregate.tokens_output,
                    aggregate.tokens_reasoning,
                    aggregate.tokens_cache_read,
                    aggregate.tokens_cache_write,
                    session_time_updated,
                ],
            )?;
            tx.conn().execute(
                "INSERT INTO event_sequence (aggregate_id, seq) VALUES (?1, ?2) \
                 ON CONFLICT (aggregate_id) DO UPDATE SET seq = excluded.seq, owner_id = NULL",
                rusqlite::params![walk_session_id, transformed.watermark],
            )?;
            Ok(())
        })
        .await?;
        {
            let mut runtime = RUNTIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let RuntimeState::Running(progress) = &*runtime {
                *runtime = RuntimeState::Running(Progress {
                    label: progress.label.clone(),
                    numerator: Some(progress.numerator.unwrap_or(0) + 1),
                    denominator: Some(denominator),
                });
            }
        }
    }

    db.transaction(|tx| write_state(tx, &json!({ "phase": "completed" }))).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// opencode-next.db import (v1-migration.bun.ts:708-876)
// ---------------------------------------------------------------------------

fn open_readonly(path: &Path) -> Option<rusqlite::Connection> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()?;
    Some(connection)
}

fn is_next_database(source: &rusqlite::Connection) -> bool {
    let Ok(mut statement) = source.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
    else {
        return false;
    };
    let Ok(names) = statement.query_map([], |row| row.get::<_, String>(0)) else {
        return false;
    };
    let tables: HashSet<String> = names.filter_map(std::result::Result::ok).collect();
    tables.contains("project") && tables.contains("session") && tables.contains("session_message")
}

fn count_next_sessions(path: Option<&Path>) -> i64 {
    let Some(path) = path else { return 0 };
    if !path.exists() {
        return 0;
    }
    let Some(source) = open_readonly(path) else { return 0 };
    if !is_next_database(&source) {
        return 0;
    }
    source.query_row("SELECT COUNT(*) FROM session", [], |row| row.get(0)).unwrap_or(0)
}

/// Column strategy for the compatibility projection.
enum ColumnSpec {
    Required,
    Nullable,
    Fallback(&'static str),
}

/// `selectNextRows`: required columns must exist; missing nullable columns
/// project NULL; `icon_url_override` falls back to `icon_url`.
fn next_projection(
    source: &rusqlite::Connection,
    table: &str,
    columns: &[(&'static str, ColumnSpec)],
) -> Result<Vec<String>> {
    let present: HashSet<String> = {
        let mut statement =
            source.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?;
        let names = statement.query_map([], |row| row.get::<_, String>(0))?;
        names.filter_map(std::result::Result::ok).collect()
    };
    let mut projection = Vec::new();
    for (column, spec) in columns {
        if present.contains(*column) {
            projection.push(format!("\"{column}\""));
            continue;
        }
        match spec {
            ColumnSpec::Required => {
                return Err(super::error::DbError::Sqlite(rusqlite::Error::InvalidParameterName(
                    format!(
                        "Incompatible opencode-next.db: {table} missing required column {column}"
                    ),
                )));
            }
            ColumnSpec::Nullable => projection.push(format!("NULL AS \"{column}\"")),
            ColumnSpec::Fallback(fallback) => {
                if present.contains(*fallback) {
                    projection.push(format!("\"{fallback}\" AS \"{column}\""));
                } else {
                    projection.push(format!("NULL AS \"{column}\""));
                }
            }
        }
    }
    Ok(projection)
}

/// One `opencode-next.db` project row (the compatibility projection's
/// column order).
type ProjectRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<i64>,
    String,
    Option<String>,
);

async fn import_next_database(
    db: &Database,
    path: Option<&Path>,
    mut on_progress: impl FnMut(i64),
) -> Result<()> {
    let Some(path) = path else { return Ok(()) };
    if !path.exists() {
        return Ok(());
    }
    let Some(source) = open_readonly(path) else {
        return Ok(());
    };
    if !is_next_database(&source) {
        return Ok(());
    }

    let project_columns = next_projection(
        &source,
        "project",
        &[
            ("id", ColumnSpec::Required),
            ("worktree", ColumnSpec::Required),
            ("vcs", ColumnSpec::Nullable),
            ("name", ColumnSpec::Nullable),
            ("icon_url", ColumnSpec::Nullable),
            ("icon_url_override", ColumnSpec::Fallback("icon_url")),
            ("icon_color", ColumnSpec::Nullable),
            ("time_created", ColumnSpec::Required),
            ("time_updated", ColumnSpec::Required),
            ("time_initialized", ColumnSpec::Nullable),
            ("sandboxes", ColumnSpec::Required),
            ("commands", ColumnSpec::Nullable),
        ],
    )?;
    let session_columns = next_projection(
        &source,
        "session",
        &[
            ("id", ColumnSpec::Required),
            ("project_id", ColumnSpec::Required),
            ("workspace_id", ColumnSpec::Nullable),
            ("parent_id", ColumnSpec::Nullable),
            ("fork_session_id", ColumnSpec::Nullable),
            ("fork_boundary", ColumnSpec::Nullable),
            ("slug", ColumnSpec::Required),
            ("directory", ColumnSpec::Required),
            ("path", ColumnSpec::Nullable),
            ("title", ColumnSpec::Nullable),
            ("version", ColumnSpec::Required),
            ("share_url", ColumnSpec::Nullable),
            ("summary_additions", ColumnSpec::Nullable),
            ("summary_deletions", ColumnSpec::Nullable),
            ("summary_files", ColumnSpec::Nullable),
            ("summary_diffs", ColumnSpec::Nullable),
            ("metadata", ColumnSpec::Nullable),
            ("cost", ColumnSpec::Required),
            ("tokens_input", ColumnSpec::Required),
            ("tokens_output", ColumnSpec::Required),
            ("tokens_reasoning", ColumnSpec::Required),
            ("tokens_cache_read", ColumnSpec::Required),
            ("tokens_cache_write", ColumnSpec::Required),
            ("revert", ColumnSpec::Nullable),
            ("permission", ColumnSpec::Nullable),
            ("agent", ColumnSpec::Nullable),
            ("model", ColumnSpec::Nullable),
            ("time_created", ColumnSpec::Required),
            ("time_updated", ColumnSpec::Required),
            ("time_compacting", ColumnSpec::Nullable),
            ("time_archived", ColumnSpec::Nullable),
            ("time_suspended", ColumnSpec::Nullable),
        ],
    )?;

    let projects: HashMap<String, ProjectRow> = {
        let mut statement =
            source.prepare(&format!("SELECT {} FROM \"project\"", project_columns.join(", ")))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
            ))
        })?;
        rows.filter_map(std::result::Result::ok).map(|row| (row.0.clone(), row)).collect()
    };

    let session_select =
        format!("SELECT {} FROM \"session\" ORDER BY \"id\" DESC", session_columns.join(", "));
    let sessions: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        f64,
        i64,
        i64,
        i64,
        i64,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    )> = {
        let mut statement = source.prepare(&session_select)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<i64>>(12)?,
                row.get::<_, Option<i64>>(13)?,
                row.get::<_, Option<i64>>(14)?,
                row.get::<_, Option<String>>(15)?,
                row.get::<_, Option<String>>(16)?,
                row.get::<_, f64>(17)?,
                row.get::<_, i64>(18)?,
                row.get::<_, i64>(19)?,
                row.get::<_, i64>(20)?,
                row.get::<_, i64>(21)?,
                row.get::<_, i64>(22)?,
                row.get::<_, Option<String>>(23)?,
                row.get::<_, Option<String>>(24)?,
                row.get::<_, Option<String>>(25)?,
                row.get::<_, Option<String>>(26)?,
                row.get::<_, i64>(27)?,
                row.get::<_, i64>(28)?,
                row.get::<_, Option<i64>>(29)?,
                row.get::<_, Option<i64>>(30)?,
                row.get::<_, Option<i64>>(31)?,
            ))
        })?;
        rows.filter_map(std::result::Result::ok).collect()
    };

    for (index, session) in sessions.into_iter().enumerate() {
        let project = projects.get(&session.1).cloned();
        let project_id =
            project.as_ref().map_or_else(|| "global".to_owned(), |_| session.1.clone());
        let messages: Vec<(String, String, String, i64, i64, i64, String)> = {
            let mut statement = source.prepare(
                "SELECT id, session_id, type, seq, time_created, time_updated, data FROM session_message \
                 WHERE session_id = ?1 ORDER BY seq",
            )?;
            let rows = statement.query_map(rusqlite::params![session.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?;
            rows.filter_map(std::result::Result::ok).collect()
        };
        let last_seq = messages.last().map_or(-1, |message| message.3);
        db.transaction(move |tx| {
            if let Some(project) = &project {
                tx.conn().execute(
                    "INSERT OR IGNORE INTO project (\
                       id, worktree, vcs, name, icon_url, icon_url_override, icon_color, \
                       time_created, time_updated, time_initialized, time_active, sandboxes, commands\
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?9, ?11, ?12)",
                    rusqlite::params![
                        project.0,
                        project.1,
                        project.2,
                        project.3,
                        project.4,
                        project.5,
                        project.6,
                        project.7,
                        project.8,
                        project.9,
                        project.10,
                        project.11,
                    ],
                )?;
            }
            let existing: Option<String> = match tx.conn().query_row(
                "SELECT id FROM session_v2 WHERE id = ?1",
                rusqlite::params![session.0],
                |row| row.get(0),
            ) {
                Ok(id) => Some(id),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(error.into()),
            };
            if existing.is_some() {
                return Ok(());
            }
            tx.conn().execute(
                "INSERT INTO session_v2 (\
                   id, project_id, workspace_id, parent_id, fork_session_id, fork_boundary, slug, directory, \
                   path, title, version, share_url, summary_additions, summary_deletions, summary_files, \
                   summary_diffs, metadata, cost, tokens_input, tokens_output, tokens_reasoning, \
                   tokens_cache_read, tokens_cache_write, revert, permission, agent, model, time_created, \
                   time_updated, time_compacting, time_archived, time_suspended\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                   ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32)",
                rusqlite::params![
                    session.0,
                    project_id,
                    session.2,
                    session.3,
                    session.4,
                    session.5,
                    session.6,
                    session.7,
                    session.8,
                    session.9,
                    session.10,
                    session.11,
                    session.12,
                    session.13,
                    session.14,
                    session.15,
                    session.16,
                    session.17,
                    session.18,
                    session.19,
                    session.20,
                    session.21,
                    session.22,
                    session.23,
                    session.24,
                    session.25,
                    session.26,
                    session.27,
                    session.28,
                    session.29,
                    session.30,
                    session.31,
                ],
            )?;
            for message in &messages {
                tx.conn().execute(
                    "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        message.0,
                        message.1,
                        message.2,
                        message.3,
                        message.4,
                        message.5,
                        message.6,
                    ],
                )?;
            }
            tx.conn().execute(
                "INSERT INTO event_sequence (aggregate_id, seq) VALUES (?1, ?2) \
                 ON CONFLICT (aggregate_id) DO UPDATE SET seq = excluded.seq, owner_id = NULL",
                rusqlite::params![session.0, last_seq],
            )?;
            Ok(())
        })
        .await?;
        on_progress(index as i64 + 1);
    }
    Ok(())
}
