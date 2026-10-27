//! Port of `session/context.ts` (§3.1) + `session/history.ts` +
//! `instructions/index.ts` + `session/instruction-state.ts` +
//! `instructions/builtins.ts`.
//!
//! Two phases: [`SessionContext::select`] fixes the session, agent,
//! instruction sources and tool snapshot; [`SessionContext::load`] adds the
//! model and the active history for that selection. A session whose location
//! no longer matches the graph interrupts with `Reloaded` semantics
//! upstream.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_schema::agent::Info as AgentInfo;
use oc_schema::common::SessionError;
use oc_schema::id::AgentId;
use oc_schema::permission::Ruleset;
use oc_schema::session_message::{self as message, Info as MessageInfo};
use oc_schema::session_support::ProviderContext;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use crate::bus::{Bus, PublishOptions};
use crate::database::error::{DbError, Result as DbResult};
use crate::database::schema::{SessionMessageRow, SessionRow};
use crate::database::{Database, Tx};
use crate::location::{AgentRegistry, LocationRef, ToolRegistryState};
use crate::session::runner::{ModelError, ModelResolver, ResolvedModel};

// ---------------------------------------------------------------------------
// Instructions (instructions/index.ts)
// ---------------------------------------------------------------------------

/// `Instructions.Unavailable` — the read failed temporarily; the stored
/// value stands. Typed as [`ReadOutcome::Unavailable`] (the former
/// `"\0instructions.unavailable"` string sentinel).

/// `Instructions.removed` — an observed absence (hash-bucket sentinel).
pub const REMOVED_SENTINEL: &str = "removed";

/// One read outcome (`Schema.Json | Unavailable | Removed`).
#[derive(Debug, Clone, PartialEq)]
pub enum ReadOutcome {
    Value(serde_json::Value),
    Unavailable,
    Removed,
}

/// One composable instruction source over canonical JSON
/// (`Instructions.Source`). Renderers returning `None` skip.
pub struct InstructionSource {
    pub key: String,
    pub read: Box<dyn Fn() -> ReadOutcome + Send + Sync>,
    pub initial: Box<dyn Fn(&serde_json::Value) -> Option<String> + Send + Sync>,
    pub changed:
        Box<dyn Fn(&serde_json::Value, &serde_json::Value) -> Option<String> + Send + Sync>,
    pub removed: Option<Box<dyn Fn(&serde_json::Value) -> Option<String> + Send + Sync>>,
}

impl std::fmt::Debug for InstructionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InstructionSource({})", self.key)
    }
}

/// `Instructions.List` — ordered sources.
pub type InstructionList = Vec<InstructionSource>;

/// `Instructions.read`.
pub fn instruction_read(list: &InstructionList) -> Vec<(String, ReadOutcome)> {
    list.iter().map(|source| (source.key.clone(), (source.read)())).collect()
}

/// One admission (`Instructions.Admission`).
#[derive(Debug, Clone, Default)]
pub struct Admission {
    /// key → hash, or [`REMOVED_SENTINEL`] for removals.
    pub delta: BTreeMap<String, String>,
    /// hash → blob.
    pub blobs: BTreeMap<String, serde_json::Value>,
}

/// `Instructions.diff` — initial reads fail on unavailable sources
/// (`InitializationBlocked` carries the keys).
pub fn instruction_diff(
    observed: &[(String, ReadOutcome)],
    previous: Option<&BTreeMap<String, String>>,
) -> std::result::Result<Admission, Vec<String>> {
    let blocked: Vec<String> = if previous.is_some() {
        Vec::new()
    } else {
        observed
            .iter()
            .filter(|(_, value)| matches!(value, ReadOutcome::Unavailable))
            .map(|(key, _)| key.clone())
            .collect()
    };
    if !blocked.is_empty() {
        return Err(blocked);
    }
    let mut admission = Admission::default();
    for (key, value) in observed {
        match value {
            ReadOutcome::Unavailable => continue,
            ReadOutcome::Removed => {
                if previous.is_some_and(|previous| previous.contains_key(key)) {
                    admission.delta.insert(key.clone(), REMOVED_SENTINEL.to_string());
                }
            }
            ReadOutcome::Value(value) => {
                let next = instruction_hash(value);
                if previous.is_some_and(|previous| previous.get(key) == Some(&next)) {
                    continue;
                }
                admission.delta.insert(key.clone(), next.clone());
                admission.blobs.insert(next, value.clone());
            }
        }
    }
    Ok(admission)
}

/// `Instructions.renderInitial`.
pub fn render_initial(
    list: &InstructionList,
    values: &BTreeMap<String, serde_json::Value>,
) -> String {
    let parts: Vec<String> =
        list.iter().filter_map(|source| (source.initial)(values.get(&source.key)?)).collect();
    parts.join("\n\n")
}

/// `Instructions.renderUpdate` — `delta` values `None` mean removal.
pub fn render_update(
    list: &InstructionList,
    previous: &BTreeMap<String, serde_json::Value>,
    delta: &BTreeMap<String, Option<serde_json::Value>>,
) -> String {
    let parts: Vec<String> = list
        .iter()
        .filter_map(|source| {
            let current = delta.get(&source.key)?;
            match current {
                None => {
                    let value = previous.get(&source.key)?;
                    (source.removed.as_ref()?)(value)
                }
                Some(next) => {
                    if let Some(before) = previous.get(&source.key) {
                        (source.changed)(before, next)
                    } else {
                        (source.initial)(next)
                    }
                }
            }
        })
        .collect();
    parts.join("\n\n")
}

/// `Instructions.hash` — sha256 over canonical JSON.
pub fn instruction_hash(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(value).as_bytes());
    format!("{:x}", hasher.finalize())
}

/// `Instructions.applyHashDelta`.
pub fn apply_hash_delta(
    values: &BTreeMap<String, String>,
    delta: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut result = values.clone();
    for (key, value) in delta {
        if value == REMOVED_SENTINEL {
            result.remove(key);
        } else {
            result.insert(key.clone(), value.clone());
        }
    }
    result
}

/// Canonical JSON (instructions/index.ts:206-216): sorted keys, no
/// whitespace.
fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(items) => {
            format!("[{}]", items.iter().map(canonical_json).collect::<Vec<_>>().join(","))
        }
        serde_json::Value::Object(object) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = object.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            format!(
                "{{{}}}",
                entries
                    .iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::json!(key),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Instruction state (session/instruction-state.ts)
// ---------------------------------------------------------------------------

/// One observed instruction state (`InstructionState.Observation`).
#[derive(Debug, Clone)]
pub struct InstructionObservation {
    pub session_id: String,
    pub initial: bool,
    pub previous: BTreeMap<String, String>,
    pub current: BTreeMap<String, String>,
    pub admission: Admission,
}

#[derive(Debug, Clone)]
struct InstructionStateRowData {
    initial_values: BTreeMap<String, String>,
    current_values: BTreeMap<String, String>,
}

fn decode_hash_map(raw: &str) -> BTreeMap<String, String> {
    serde_json::from_str::<BTreeMap<String, String>>(raw).unwrap_or_default()
}

fn find_instruction_state(
    tx: &Tx<'_>,
    session_id: &str,
) -> DbResult<Option<InstructionStateRowData>> {
    let row = tx
        .conn()
        .query_row(
            "SELECT initial_values, current_values FROM instruction_state WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| {
                Ok(InstructionStateRowData {
                    initial_values: decode_hash_map(&row.get::<_, String>(0)?),
                    current_values: decode_hash_map(&row.get::<_, String>(1)?),
                })
            },
        )
        .optional()?;
    Ok(row)
}

fn load_blobs(tx: &Tx<'_>, values: &[String]) -> DbResult<BTreeMap<String, serde_json::Value>> {
    let unique: Vec<String> = {
        let mut seen = std::collections::BTreeSet::new();
        values.iter().filter(|value| seen.insert((*value).clone())).cloned().collect()
    };
    let mut blobs = BTreeMap::new();
    for batch in unique.chunks(500) {
        let marks = batch.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!("SELECT hash, value FROM instruction_blob WHERE hash IN ({marks})");
        let params: Vec<&dyn rusqlite::ToSql> =
            batch.iter().map(|value| value as &dyn rusqlite::ToSql).collect();
        let mut statement = tx.conn().prepare(&sql)?;
        let rows = statement
            .query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (hash, value) in rows {
            let value = value
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .unwrap_or(serde_json::Value::Null);
            blobs.insert(hash, value);
        }
    }
    Ok(blobs)
}

fn insert_blobs(tx: &mut Tx<'_>, blobs: &BTreeMap<String, serde_json::Value>) -> DbResult<()> {
    for (hash, value) in blobs {
        tx.execute(
            "INSERT OR IGNORE INTO instruction_blob (hash, value) VALUES (?1, ?2)",
            rusqlite::params![hash, serde_json::to_string(value).unwrap_or_default()],
        )?;
    }
    Ok(())
}

fn dereference(
    values: &BTreeMap<String, String>,
    blobs: &BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    values
        .iter()
        .map(|(key, hash)| {
            (key.clone(), blobs.get(hash).cloned().unwrap_or(serde_json::Value::Null))
        })
        .collect()
}

/// The InstructionState port.
pub struct InstructionState;

impl InstructionState {
    /// `observe`.
    pub async fn observe(
        db: &Database,
        list: &InstructionList,
        session_id: &str,
    ) -> std::result::Result<InstructionObservation, Vec<String>> {
        let observed = instruction_read(list);
        let session = session_id.to_string();
        let stored = db
            .transaction(move |tx| find_instruction_state(tx, &session))
            .await
            .map_err(|_| Vec::new())?;
        let previous = stored.map(|state| state.current_values);
        let admission = instruction_diff(&observed, previous.as_ref())?;
        let current = apply_hash_delta(&previous.clone().unwrap_or_default(), &admission.delta);
        Ok(InstructionObservation {
            session_id: session_id.to_string(),
            initial: previous.is_none(),
            previous: previous.unwrap_or_default(),
            current,
            admission,
        })
    }

    /// `commit` — publishes `session.instructions.updated`; the initial sync
    /// establishes the baseline (empty text, initial metadata).
    pub async fn commit(
        db: &Database,
        bus: &Bus,
        list: &InstructionList,
        observation: &InstructionObservation,
    ) -> DbResult<()> {
        if !observation.initial && observation.admission.delta.is_empty() {
            return Ok(());
        }
        // The rendered text is frozen into the durable event because
        // re-rendering later would require the original location-scoped
        // sources.
        let text = if observation.initial {
            String::new()
        } else {
            let replaced: Vec<String> = observation
                .previous
                .iter()
                .filter(|(key, _)| observation.admission.delta.contains_key(*key))
                .map(|(_, hash)| hash.clone())
                .collect();
            let stored =
                db.transaction(move |tx| load_blobs(tx, &replaced)).await.unwrap_or_default();
            let previous: BTreeMap<String, serde_json::Value> = observation
                .previous
                .iter()
                .filter(|(key, _)| observation.admission.delta.contains_key(*key))
                .map(|(key, hash)| {
                    (key.clone(), stored.get(hash).cloned().unwrap_or(serde_json::Value::Null))
                })
                .collect();
            let delta: BTreeMap<String, Option<serde_json::Value>> = observation
                .admission
                .delta
                .iter()
                .map(|(key, hash)| {
                    let value = if hash == REMOVED_SENTINEL {
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
            render_update(list, &previous, &delta)
        };
        let mut delta_json = serde_json::Map::new();
        for (key, hash) in &observation.admission.delta {
            delta_json.insert(key.clone(), serde_json::json!(hash));
        }
        let mut data = serde_json::json!({
            "sessionID": observation.session_id,
            "delta": serde_json::Value::Object(delta_json),
        });
        if !text.is_empty() {
            data["text"] = serde_json::json!(text);
        }
        let mut options = PublishOptions::default();
        if observation.initial {
            options.metadata = Some(serde_json::json!({ "instructions": { "initial": true } }));
        }
        let blobs = observation.admission.blobs.clone();
        options.commit = Some(Box::new(move |tx: &mut Tx<'_>, _| insert_blobs(tx, &blobs)));
        let def = crate::bus::registry::by_type_latest("session.instructions.updated")
            .unwrap_or_else(|| panic!("no session.instructions.updated definition"));
        bus.publish(def, data, options).await.map(|_| ()).map_err(|error| {
            DbError::Pragma(format!("publish session.instructions.updated failed: {error}"))
        })
    }

    /// `prepare` — observe + commit.
    pub async fn prepare(
        db: &Database,
        bus: &Bus,
        list: &InstructionList,
        session_id: &str,
    ) -> std::result::Result<(), Vec<String>> {
        let observation = Self::observe(db, list, session_id).await?;
        Self::commit(db, bus, list, &observation).await.map_err(|_| Vec::new())?;
        Ok(())
    }

    /// `apply` (projector-side delta application).
    pub fn apply(
        tx: &mut Tx<'_>,
        session_id: &str,
        seq: i64,
        delta: &BTreeMap<String, String>,
    ) -> DbResult<()> {
        let stored = find_instruction_state(tx, session_id)?;
        let current = apply_hash_delta(
            &stored.as_ref().map_or_else(BTreeMap::new, |state| state.current_values.clone()),
            delta,
        );
        let payload = serde_json::to_string(&current).unwrap_or_default();
        if stored.is_none() {
            tx.execute(
                "INSERT INTO instruction_state (session_id, epoch_start, through_seq, initial_values, current_values) \
                 VALUES (?1, ?2, ?2, ?3, ?3)",
                rusqlite::params![session_id, seq, payload],
            )?;
        } else {
            tx.execute(
                "UPDATE instruction_state SET through_seq = ?2, current_values = ?3 WHERE session_id = ?1",
                rusqlite::params![session_id, seq, payload],
            )?;
        }
        Ok(())
    }

    /// `initial` — the epoch baseline rendered at the start of every model
    /// request.
    pub async fn initial(
        db: &Database,
        list: &InstructionList,
        session_id: &str,
    ) -> DbResult<String> {
        let session = session_id.to_string();
        let (initial_values, blobs) = db
            .transaction(move |tx| {
                let Some(state) = find_instruction_state(tx, &session)? else {
                    return Err(DbError::Pragma(format!(
                        "Instruction state not found during assembly: {session}"
                    )));
                };
                let values: Vec<String> = state.initial_values.values().cloned().collect();
                let blobs = load_blobs(tx, &values)?;
                Ok((state.initial_values, blobs))
            })
            .await?;
        Ok(render_initial(list, &dereference(&initial_values, &blobs)))
    }
}

// ---------------------------------------------------------------------------
// Instruction sources: builtins + Wave C/D stubs (context.ts §3.1 order)
// ---------------------------------------------------------------------------

/// v2's empty-catalog render — the exact text
/// [`crate::tool::codemode::render_instructions`] produces for an empty
/// summary; pinned here for the disabled-runtime path's parity note.
#[allow(dead_code)]
const CODEMODE_EMPTY: &str = "No Code Mode tools are currently available. Later Code Mode catalog updates may add or remove tools. Do not call `execute` unless there is at least one available Code Mode tool.";

/// The `# Code Mode` instruction (wave 4): the real catalog renders —
/// namespaces + selected listings under the inline budget — whenever
/// the CodeMode surface is live (an `execute` executor registered,
/// i.e. the engine booted with a code-mode runtime). Without one, the
/// v2 empty-catalog text stands: `execute` is not callable, so it is
/// never advertised.
fn codemode_source(tools: Arc<ToolRegistryState>) -> InstructionSource {
    InstructionSource {
        key: "core/codemode".into(),
        read: Box::new(move || {
            let entries = crate::tool::codemode::code_mode_entries(&tools);
            if tools.executor(crate::tool::codemode::EXECUTE_NAME).is_none() {
                return ReadOutcome::Value(serde_json::json!({ "enabled": false }));
            }
            ReadOutcome::Value(crate::tool::codemode::summary_value(
                &crate::tool::codemode::catalog_summary(&entries),
            ))
        }),
        initial: Box::new(|value| Some(crate::tool::codemode::instructions_from_value(value))),
        changed: Box::new(|_previous, current| {
            let text = crate::tool::codemode::instructions_from_value(current);
            Some(crate::tool::codemode::instructions_replacement(&text))
        }),
        removed: None,
    }
}

/// `core/mcp-guidance` (mcp/instructions.ts): connected servers render as
/// `<mcp_instructions>` entries — codemode servers add the `execute`
/// namespace line — and an empty visible set is an observed absence (the
/// key stays out of the instruction delta).
pub(crate) fn mcp_guidance_source(
    mcp: Arc<dyn Fn() -> Vec<McpServerSummary> + Send + Sync>,
) -> InstructionSource {
    fn entries(servers: &[McpServerSummary]) -> Vec<String> {
        servers
            .iter()
            .flat_map(|server| {
                let mut result = vec![format!("  <server name=\"{}\">", server.server)];
                if server.codemode {
                    result.push(format!(
                        "    Use tools from this server through `execute` under `tools[{}]`.",
                        serde_json::to_string(&crate::mcp::registry::namespace(&server.server))
                            .unwrap_or_else(|_| format!("\"{}\"", server.server)),
                    ));
                }
                result.extend(server.instructions.split('\n').map(|line| format!("    {line}")));
                result.push("  </server>".to_string());
                result
            })
            .collect()
    }
    fn render(servers: &[McpServerSummary]) -> String {
        let mut lines = vec!["<mcp_instructions>".to_string()];
        lines.extend(entries(servers));
        lines.push("</mcp_instructions>".to_string());
        lines.join("\n")
    }
    fn update(previous: &[McpServerSummary], current: &[McpServerSummary]) -> String {
        let key = |server: &McpServerSummary| server.server.clone();
        let changed = |before: &McpServerSummary, after: &McpServerSummary| {
            before.instructions != after.instructions || before.codemode != after.codemode
        };
        let previous_keys: std::collections::HashSet<String> = previous.iter().map(&key).collect();
        let added: Vec<McpServerSummary> = current
            .iter()
            .filter(|server| !previous_keys.contains(&server.server))
            .cloned()
            .collect();
        let current_keys: std::collections::HashSet<String> = current.iter().map(&key).collect();
        let removed: Vec<McpServerSummary> = previous
            .iter()
            .filter(|server| !current_keys.contains(&server.server))
            .cloned()
            .collect();
        let same_changed = current.iter().any(|after| {
            previous
                .iter()
                .find(|before| before.server == after.server)
                .is_some_and(|before| changed(before, after))
        });
        if same_changed || (added.is_empty() && removed.is_empty()) {
            return [
                "The available MCP server instructions have changed. This list supersedes the previous one."
                    .to_string(),
                render(current),
            ]
            .join("\n");
        }
        let mut parts = Vec::new();
        if !added.is_empty() {
            parts.push(
                "New MCP server instructions are available in addition to those previously listed:"
                    .to_string(),
            );
            parts.extend(entries(&added));
        }
        if !removed.is_empty() {
            parts.push(format!(
                "Instructions for the following MCP servers are no longer available: {}.",
                removed.iter().map(|server| server.server.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        parts.join("\n")
    }
    InstructionSource {
        key: "core/mcp-guidance".into(),
        read: Box::new(move || {
            let servers = (mcp)();
            if servers.is_empty() {
                ReadOutcome::Removed
            } else {
                ReadOutcome::Value(serde_json::to_value(&servers).unwrap_or_default())
            }
        }),
        initial: Box::new(|value| {
            let servers: Vec<McpServerSummary> =
                serde_json::from_value(value.clone()).unwrap_or_default();
            Some(render(&servers))
        }),
        changed: Box::new(|previous, current| {
            let previous: Vec<McpServerSummary> =
                serde_json::from_value(previous.clone()).unwrap_or_default();
            let current: Vec<McpServerSummary> =
                serde_json::from_value(current.clone()).unwrap_or_default();
            Some(update(&previous, &current))
        }),
        removed: Some(Box::new(|_| {
            Some("MCP server instructions are no longer available.".to_string())
        })),
    }
}

/// `core/reference-guidance` — TODO(wave-C): reference-file guidance.
fn reference_guidance_source() -> InstructionSource {
    InstructionSource {
        key: "core/reference-guidance".into(),
        // TODO(wave-C): read the loaded reference-file summary; empty is an
        // observed absence.
        read: Box::new(|| ReadOutcome::Removed),
        initial: Box::new(|_| {
            Some(
                "No reference files are currently loaded. Reference files add context when relevant work is detected."
                    .to_string(),
            )
        }),
        changed: Box::new(|_, _| Some("The loaded reference files changed.".to_string())),
        removed: None,
    }
}

/// `core/skill-guidance` (skill/instructions.ts): the available list
/// (`description` present, autoinvoke enabled), sorted by id; an empty
/// list is an observed absence.
fn skill_guidance_source(
    skills: Arc<dyn Fn() -> Vec<oc_schema::skill::Skill> + Send + Sync>,
) -> InstructionSource {
    let summaries = |skills: &[oc_schema::skill::Skill]| -> Vec<serde_json::Value> {
        let mut visible: Vec<serde_json::Value> = skills
            .iter()
            .filter(|skill| skill.description.is_some() && skill.auto_invoke != Some(false))
            .map(|skill| {
                serde_json::json!({
                    "id": skill.id.as_str(),
                    "name": skill.name.0.clone(),
                    "description": skill.description.clone().unwrap_or_default(),
                })
            })
            .collect();
        // v2 sorts with `id.localeCompare` (ICU tertiary: case-insensitive
        // primary, lowercase before uppercase on ties). Approximation: ASCII
        // case-fold then lowercase-first tie-break — identical to ICU for
        // the ASCII corpus of skill ids; non-ASCII falls back to code
        // points (documented divergence).
        visible.sort_by(|a, b| {
            let (x, y) =
                (a["id"].as_str().unwrap_or_default(), b["id"].as_str().unwrap_or_default());
            x.to_lowercase().cmp(&y.to_lowercase()).then_with(|| y.cmp(x))
        });
        visible
    };
    let render = |skills: &[serde_json::Value]| -> String {
        let mut lines = vec![
            "Skills provide specialized instructions and workflows for specific tasks.".to_string(),
            "Use the skill tool to load a skill when a task matches its description.".to_string(),
            "The user may also invoke a skill directly. When that happens, its instructions appear in the conversation as a <skill_content> block, the same shape the skill tool returns. A skill that is already present this way does not need to be invoked again.".to_string(),
        ];
        if skills.is_empty() {
            lines.push("No skills are currently available.".to_string());
        } else {
            lines.push("<available_skills>".to_string());
            for skill in skills {
                lines.push("  <skill>".to_string());
                lines.push(format!("    <id>{}</id>", skill["id"].as_str().unwrap_or_default()));
                lines.push(format!(
                    "    <name>{}</name>",
                    skill["name"].as_str().unwrap_or_default()
                ));
                lines.push(format!(
                    "    <description>{}</description>",
                    skill["description"].as_str().unwrap_or_default()
                ));
                lines.push("  </skill>".to_string());
            }
            lines.push("</available_skills>".to_string());
        }
        lines.join("\n")
    };
    InstructionSource {
        key: "core/skill-guidance".into(),
        read: Box::new(move || {
            let visible = summaries(&(skills)());
            if visible.is_empty() {
                ReadOutcome::Removed
            } else {
                ReadOutcome::Value(serde_json::Value::Array(visible))
            }
        }),
        initial: Box::new(move |value| {
            let skills = value.as_array().cloned().unwrap_or_default();
            Some(render(&skills))
        }),
        changed: Box::new(move |_previous, current| {
            let skills = current.as_array().cloned().unwrap_or_default();
            Some(render(&skills))
        }),
        removed: Some(Box::new(|_| {
            Some(
                "Skill guidance is no longer available. Do not use any previously listed skill."
                    .to_string(),
            )
        })),
    }
}

/// `core/instructions` (instruction-discovery.ts): global first, then
/// nearest → farthest project files, rendered as one block.
fn discovery_source(
    discovery: Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>,
) -> InstructionSource {
    InstructionSource {
        key: "core/instructions".into(),
        read: Box::new(move || {
            let files = (discovery)();
            if files.is_empty() {
                // An empty instruction set is an observed absence (v2's
                // discovery source reads `removed` with no files).
                ReadOutcome::Removed
            } else {
                ReadOutcome::Value(serde_json::json!(files))
            }
        }),
        initial: Box::new(|value| {
            let files = files_from(value);
            if files.is_empty() {
                return None;
            }
            Some(render_discovery(&files, false))
        }),
        changed: Box::new(|previous, current| {
            let before = files_from(previous);
            let after = files_from(current);
            if before == after {
                return None;
            }
            Some(render_discovery(&after, before.is_empty()))
        }),
        removed: None,
    }
}

fn files_from(value: &serde_json::Value) -> Vec<(String, String)> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get(0).and_then(serde_json::Value::as_str)?.to_string(),
                        item.get(1).and_then(serde_json::Value::as_str)?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn render_discovery(files: &[(String, String)], initial: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !initial {
        parts.push(
            "Project instructions changed. The following supersedes previously loaded instructions."
                .to_string(),
        );
    }
    for (path, content) in files {
        parts.push(format!("Instructions from: {path}\n{content}"));
    }
    parts.join("\n\n")
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Date#toDateString()` (e.g. `Fri Oct 09 2026`) from epoch milliseconds.
#[must_use]
pub fn date_string(unix_ms: i64) -> String {
    let days = unix_ms.div_euclid(86_400_000);
    // 1970-01-01 was a Thursday (index 4).
    let weekday = ((days.rem_euclid(7) + 4) % 7) as usize;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{} {} {:02} {}",
        WEEKDAYS[weekday],
        MONTHS[usize::try_from(month - 1).unwrap_or(0)],
        day,
        year
    )
}

fn date_source(clock: Arc<dyn oc_util::determinism::Clock>) -> InstructionSource {
    InstructionSource {
        key: "core/date".into(),
        read: Box::new(move || ReadOutcome::Value(serde_json::json!(date_string(clock.unix_ms())))),
        initial: Box::new(|value| {
            Some(format!("Today's date: {}", value.as_str().unwrap_or_default()))
        }),
        changed: Box::new(|_previous, current| {
            Some(format!("Today's date is now: {}", current.as_str().unwrap_or_default()))
        }),
        removed: None,
    }
}

fn environment_source(env: Arc<dyn Fn() -> String + Send + Sync>) -> InstructionSource {
    InstructionSource {
        key: "core/environment".into(),
        read: Box::new(move || ReadOutcome::Value(serde_json::json!((env)()))),
        initial: Box::new(|value| {
            Some(format!(
                "Here is some useful information about the environment you are running in:\n{}",
                value.as_str().unwrap_or_default()
            ))
        }),
        changed: Box::new(|_previous, current| {
            Some(format!(
                "The environment you are running in is now:\n{}",
                current.as_str().unwrap_or_default()
            ))
        }),
        removed: None,
    }
}

/// The environment block (builtins.ts:34-46).
#[must_use]
pub fn environment_block(
    directory: &str,
    project_directory: &str,
    is_git: bool,
    tmp: &str,
) -> String {
    [
        "<env>".to_string(),
        format!("  Working directory: {directory}"),
        format!("  Workspace root folder: {project_directory}"),
        format!("  Is directory a git repo: {}", if is_git { "yes" } else { "no" }),
        format!("  Platform: {}", std::env::consts::OS),
        format!("  Prefer {tmp} over generic system temporary directories such as /tmp; it is pre-created and approved for external access."),
        "</env>".to_string(),
    ]
    .join("\n")
}

/// Per-session `api/<key>` entries (instruction-entry.ts `load`).
async fn entry_sources(db: &Database, session_id: &str) -> InstructionList {
    let session = session_id.to_string();
    let rows: Vec<(String, Option<String>, bool)> = db
        .transaction(move |tx| {
            let mut statement = tx.conn().prepare(
                "SELECT key, value, removed FROM instruction_entry WHERE session_id = ?1 \
                 ORDER BY key",
            )?;
            let rows = statement
                .query_map(rusqlite::params![session], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    rows.into_iter()
        .map(|(key, value, removed)| {
            let value = value
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .unwrap_or(serde_json::Value::Null);
            let render_block = {
                let key = key.clone();
                move |value: &serde_json::Value| -> String {
                    let rendered = match value {
                        serde_json::Value::String(text) => text.clone(),
                        other => serde_json::to_string_pretty(other).unwrap_or_default(),
                    };
                    format!("<context key=\"{key}\">\n{rendered}\n</context>")
                }
            };
            let removed_text =
                format!("The context under \"{key}\" no longer applies. Disregard it.");
            let changed_text =
                format!("The context under \"{key}\" changed and supersedes the previous value:");
            InstructionSource {
                key: format!("api/{key}"),
                read: Box::new(move || {
                    if removed {
                        ReadOutcome::Removed
                    } else {
                        ReadOutcome::Value(value.clone())
                    }
                }),
                initial: {
                    let render_block = render_block.clone();
                    Box::new(move |value| Some(render_block(value)))
                },
                changed: Box::new(move |_previous, current| {
                    Some(format!("{changed_text}\n{}", render_block(current)))
                }),
                removed: Some(Box::new(move |_previous| Some(removed_text.clone()))),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// History (session/history.ts)
// ---------------------------------------------------------------------------

/// `Boundary` — which completed compactions bound a history read.
#[derive(Debug, Clone, PartialEq)]
pub enum Boundary {
    /// Model-neutral readers take the newest checkpoint.
    Latest,
    /// The original transcript skips native windows entirely.
    Local,
    /// Only windows the target model can replay (a provenance match).
    Provenance(oc_schema::session_support::Provenance),
}

/// `SessionProviderContext.isCheckpoint`.
pub fn is_checkpoint(message: &MessageInfo) -> bool {
    matches!(
        message,
        MessageInfo::Compaction(message::Compaction::Completed { provider_context: Some(_), .. })
    )
}

fn replayable(message: &MessageInfo, boundary: &Boundary) -> bool {
    if !is_checkpoint(message) {
        return true;
    }
    match boundary {
        Boundary::Latest => true,
        Boundary::Local => false,
        Boundary::Provenance(provenance) => {
            let MessageInfo::Compaction(message::Compaction::Completed {
                provider_context: Some(context),
                ..
            }) = message
            else {
                return false;
            };
            context.provenance == *provenance
        }
    }
}

fn decode_history_row(row: &SessionMessageRow) -> DbResult<MessageInfo> {
    crate::session::inbox::decode_message_row(row).map_err(|err| match err {
        crate::session::inbox::InboxError::Decode(error) => DbError::Json(error),
        crate::session::inbox::InboxError::Db(error) => error,
        other => DbError::Pragma(other.to_string()),
    })
}
/// `latestCompaction` — seq of the newest completed compaction bounding the
/// read. Candidates decode in Rust (the provenance match is structural).
async fn latest_compaction(
    db: &Database,
    session_id: &str,
    boundary: &Boundary,
) -> DbResult<Option<i64>> {
    let session = session_id.to_string();
    let boundary = boundary.clone();
    db.transaction(move |tx| {
        let mut statement = tx.conn().prepare(
            "SELECT seq, data FROM session_message WHERE session_id = ?1 AND type = 'compaction' \
             AND json_extract(data, '$.status') = 'completed' ORDER BY seq DESC",
        )?;
        let rows = statement
            .query_map(rusqlite::params![session], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (seq, data) in rows {
            let value: serde_json::Value =
                serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
            let context = value.get("providerContext").filter(|context| !context.is_null());
            match (&boundary, context) {
                (Boundary::Latest, _) => return Ok(Some(seq)),
                (_, None) => return Ok(Some(seq)),
                (Boundary::Local, Some(_)) => continue,
                (Boundary::Provenance(provenance), Some(context)) => {
                    let matches = context
                        .get("provenance")
                        .and_then(|value| serde_json::to_value(value).ok())
                        .and_then(|value| {
                            serde_json::from_value::<oc_schema::session_support::Provenance>(value)
                                .ok()
                        })
                        .is_some_and(|candidate| candidate == *provenance);
                    if matches {
                        return Ok(Some(seq));
                    }
                }
            }
        }
        Ok(None)
    })
    .await
}

/// `messageEntries` — entries since the boundary compaction, filtered for
/// replayability and pre-checkpoint system messages.
async fn message_entries(
    db: &Database,
    session_id: &str,
    boundary: &Boundary,
) -> DbResult<Vec<(i64, MessageInfo)>> {
    let compaction = latest_compaction(db, session_id, boundary).await?;
    let session = session_id.to_string();
    let boundary = boundary.clone();
    db.transaction(move |tx| {
        let mut sql = "SELECT * FROM session_message WHERE session_id = ?1".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = vec![Box::new(session.clone())];
        if let Some(seq) = compaction {
            sql.push_str(&format!(" AND seq >= ?{}", params.len() + 1));
            params.push(Box::new(seq));
        }
        sql.push_str(" ORDER BY seq ASC");
        let references: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|value| value.as_ref() as &dyn rusqlite::ToSql).collect();
        let mut statement = tx.conn().prepare(&sql)?;
        let rows = statement
            .query_map(references.as_slice(), SessionMessageRow::decode_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            entries.push((row.seq, decode_history_row(&row)?));
        }
        // Re-expansion may cross a native checkpoint whose completion
        // already advanced the instruction epoch: the baseline supersedes
        // the chronological updates before it.
        let native_seq =
            entries.iter().rev().find(|(_, message)| is_checkpoint(message)).map(|(seq, _)| *seq);
        Ok(entries
            .into_iter()
            .filter(|(seq, message)| {
                !(matches!(message, MessageInfo::System { .. })
                    && native_seq.is_some_and(|native| *seq < native))
            })
            .filter(|(_, message)| replayable(message, &boundary))
            .collect())
    })
    .await
}

/// `SessionHistory.load`.
pub async fn history_load(
    db: &Database,
    session_id: &str,
    boundary: &Boundary,
) -> DbResult<Vec<MessageInfo>> {
    Ok(message_entries(db, session_id, boundary)
        .await?
        .into_iter()
        .map(|(_, message)| message)
        .collect())
}

/// `SessionHistory.entriesForRunner` — messages + initial instruction.
pub async fn entries_for_runner(
    db: &Database,
    session_id: &str,
    list: &InstructionList,
    boundary: &Boundary,
) -> DbResult<(String, Vec<MessageInfo>)> {
    let entries = message_entries(db, session_id, boundary).await?;
    let initial = InstructionState::initial(db, list, session_id).await?;
    Ok((initial, entries.into_iter().map(|(_, message)| message).collect()))
}

/// `SessionHistory.preview` — the settled prefix.
pub async fn history_preview(
    db: &Database,
    session_id: &str,
    boundary: &Boundary,
) -> DbResult<Vec<MessageInfo>> {
    let entries = message_entries(db, session_id, boundary).await?;
    // An active assistant may contain an unresolved tool call, so only
    // preview the settled prefix.
    let unsettled = entries.iter().position(|(_, message)| {
        matches!(message, MessageInfo::Assistant { time, .. } if time.completed.is_none())
    });
    let settled: Vec<MessageInfo> = match unsettled {
        Some(index) => entries[..index].iter().map(|(_, message)| message.clone()).collect(),
        None => entries.into_iter().map(|(_, message)| message).collect(),
    };
    Ok(settled)
}

/// `SessionHistory.firstUserMessage`.
pub async fn first_user_message(db: &Database, session_id: &str) -> Option<MessageInfo> {
    let session = session_id.to_string();
    let row = db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row(
                    "SELECT * FROM session_message WHERE session_id = ?1 AND type = 'user' \
                     ORDER BY seq ASC LIMIT 1",
                    rusqlite::params![session],
                    SessionMessageRow::decode_row,
                )
                .optional()?)
        })
        .await
        .ok()??;
    match decode_history_row(&row) {
        Ok(message @ MessageInfo::User { .. }) => Some(message),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Provider context provenance (provider-context.ts)
// ---------------------------------------------------------------------------

/// `SessionProviderContext.provenance` — no guessed endpoints.
pub fn provenance(resolved: &ResolvedModel) -> Option<oc_schema::session_support::Provenance> {
    let endpoint = &resolved.model.route.endpoint;
    let base_url = endpoint.base_url.as_ref()?;
    // No guessed endpoints: dynamic URL builders cannot establish a durable
    // deployment identity.
    let oc_ai::route::EndpointPart::Static(path) = &endpoint.path else {
        return None;
    };
    let query: BTreeMap<String, String> = endpoint.query.clone().unwrap_or_default();
    let payload = serde_json::json!([
        base_url,
        path,
        query.iter().map(|(key, value)| serde_json::json!([key, value])).collect::<Vec<_>>(),
    ]);
    let mut hasher = Sha256::new();
    hasher.update(payload.to_string().as_bytes());
    Some(oc_schema::session_support::Provenance {
        provider_id: resolved.reference.provider_id.clone(),
        provider: resolved.model.provider.0.clone(),
        model_id: resolved.model.id.0.clone(),
        route: resolved.model.route.id.clone(),
        protocol: resolved.model.route.protocol.id().to_string(),
        endpoint: format!("{:x}", hasher.finalize()),
    })
}

/// `SessionProviderContext.compatible`.
pub fn provenance_matches(
    source: &oc_schema::session_support::Provenance,
    target: Option<&oc_schema::session_support::Provenance>,
) -> bool {
    target.is_some_and(|target| source == target)
}

/// `SessionProviderContext.encode`.
pub fn encode_provider_context(
    provenance: oc_schema::session_support::Provenance,
    replacement: &[oc_ai::schema::messages::Message],
) -> ProviderContext {
    ProviderContext {
        version: (),
        provenance,
        messages: serde_json::to_value(replacement).unwrap_or(serde_json::Value::Array(Vec::new())),
    }
}

// ---------------------------------------------------------------------------
// SessionContext (context.ts)
// ---------------------------------------------------------------------------

/// `SessionContext.Selection`.
pub struct Selection {
    pub session: SessionRow,
    pub agent_id: AgentId,
    pub agent: AgentInfo,
    pub instructions: InstructionList,
    pub tools: Vec<crate::location::ToolRegistration>,
    /// The merged ruleset the snapshot was filtered with.
    pub permissions: Ruleset,
}

/// `SessionContext.Loaded`.
#[derive(Clone)]
pub struct Loaded {
    pub session: SessionRow,
    pub agent_id: AgentId,
    pub agent: AgentInfo,
    pub model: ResolvedModel,
    pub initial: String,
    pub messages: Vec<MessageInfo>,
    pub tools: Vec<crate::location::ToolRegistration>,
}

/// Why `select` failed to produce a boundary.
#[derive(Debug, thiserror::Error)]
pub enum SelectError {
    #[error("Session not found: {0}")]
    NotFound(String),
    #[error("Agent not found: session {session} agent {agent}")]
    AgentNotFound { session: String, agent: String },
    #[error("Location mismatch: {0}")]
    LocationMismatch(String),
    #[error(transparent)]
    Db(#[from] crate::database::error::DbError),
    #[error("Instruction initialization blocked by unavailable sources: {0:?}")]
    InstructionsBlocked(Vec<String>),
}

impl SelectError {
    /// `toSessionError`-style classification for the drain channel.
    #[must_use]
    pub fn to_session_error(&self) -> SessionError {
        SessionError {
            error_type: "unknown".into(),
            message: self.to_string(),
            status: None,
            response: None,
        }
    }
}

/// One connected MCP server's instruction summary (`McpInstructions`
/// Summary shape; `codemode: false` servers skip the `execute` line).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpServerSummary {
    pub server: String,
    pub instructions: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub codemode: bool,
}

/// Aux dependency handles the graph supplies (Wave B builtins + discovery
/// seam; the composition root binds the catalog, MCP summaries, and the
/// skill table).
#[derive(Clone)]
pub struct ContextDeps {
    pub clock: Arc<dyn oc_util::determinism::Clock>,
    /// AGENTS.md-style discovery: `(path, content)`, global first, then
    /// nearest → farthest.
    pub discovery: Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>,
    /// The environment block renderer (overridable for tests).
    pub environment: Arc<dyn Fn() -> String + Send + Sync>,
    /// The provider catalog (`model.small` for the title fallback; an
    /// empty catalog keeps Wave B's "reuse the primary" behavior).
    pub catalog: Arc<crate::provider_catalog::Catalog>,
    /// Connected MCP server summaries; empty = observed absence (the
    /// guidance key stays out of the instruction delta).
    pub mcp: Arc<dyn Fn() -> Vec<McpServerSummary> + Send + Sync>,
    /// Available skills (`description` present, autoinvoke enabled); empty
    /// = observed absence.
    pub skills: Arc<dyn Fn() -> Vec<oc_schema::skill::Skill> + Send + Sync>,
}

impl ContextDeps {
    /// The Wave B shape: empty catalog, no MCP servers, no config — but
    /// the two BUILT-IN skills (the internal skills plugin registers them
    /// unconditionally in v2, so every session context sees them).
    #[must_use]
    pub fn minimal(
        clock: Arc<dyn oc_util::determinism::Clock>,
        discovery: Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>,
        environment: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        let empty = crate::config::Loaded {
            sources: crate::config::discovery::Sources::default(),
            entries: Vec::new(),
            diagnostics: Vec::new(),
        };
        let builtins = crate::engine::skills::builtin::skills(&empty);
        Self {
            clock,
            discovery,
            environment,
            catalog: Arc::new(crate::provider_catalog::Catalog::empty()),
            mcp: Arc::new(Vec::new),
            skills: Arc::new(move || builtins.clone()),
        }
    }
}

impl std::fmt::Debug for ContextDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextDeps").finish_non_exhaustive()
    }
}

/// The two-phase context service.
pub struct SessionContext {
    db: Database,
    bus: Bus,
    location: LocationRef,
    agents: Arc<AgentRegistry>,
    tools: Arc<ToolRegistryState>,
    resolver: Arc<dyn ModelResolver>,
    deps: ContextDeps,
}

/// `Model.small(providerID)` (model.ts:266-274): the provider's active
/// text-in/text-out models, first match in the family preference order.
#[must_use]
pub fn small_model<'a>(
    catalog: &'a crate::provider_catalog::Catalog,
    provider_id: &str,
) -> Option<&'a oc_schema::model::Info> {
    let models: Vec<&oc_schema::model::Info> = catalog
        .provider(provider_id)?
        .models
        .iter()
        .filter(|model| {
            model.provider_id.as_str() == provider_id
                && model.status == oc_schema::model::Status::Active
                && model.capabilities.input.iter().any(|item| item.starts_with("text"))
                && model.capabilities.output.iter().any(|item| item.starts_with("text"))
        })
        .collect();
    ["gpt-luna", "gemini-flash-lite", "gemini-flash", "claude-haiku"]
        .iter()
        .find_map(|family| models.iter().find(|model| model.family.as_deref() == Some(*family)))
        .copied()
}

impl SessionContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Database,
        bus: Bus,
        location: LocationRef,
        agents: Arc<AgentRegistry>,
        tools: Arc<ToolRegistryState>,
        resolver: Arc<dyn ModelResolver>,
        deps: ContextDeps,
    ) -> Self {
        Self { db, bus, location, agents, tools, resolver, deps }
    }

    /// `resolveModel`.
    pub fn resolve_model(&self, session: &SessionRow) -> Result<ResolvedModel, ModelError> {
        self.resolver.resolve(session)
    }

    /// `selectTitle` (context.ts:95-119) — aux title capabilities without
    /// instruction or tool preflight.
    pub fn select_title(
        &self,
        session: &SessionRow,
    ) -> Option<(AgentInfo, Option<ResolvedModel>, ResolvedModel)> {
        let agent = self.agents.get(&AgentId::parse("title"))?;
        let primary = self.resolve_model(session).ok();
        // The title model: the agent's own model, else the provider's
        // small family member (model.ts `small`); neither exists ⇒ the
        // primary carries the title request.
        let info = match &agent.model {
            Some(reference) => self.deps.catalog.model(&reference.provider_id.0, &reference.id.0),
            None => {
                let primary = primary.as_ref()?;
                small_model(&self.deps.catalog, &primary.reference.provider_id.0)
            }
        };
        let preferred = info.and_then(|info| {
            // agent.model.variant ?? first minimal-reasoning variant the
            // model carries (context.ts MINIMAL_REASONING_VARIANTS).
            let variant =
                agent.model.as_ref().and_then(|reference| reference.variant.clone()).or_else(
                    || {
                        ["none", "minimal", "low"].iter().find_map(|id| {
                            info.variants
                                .iter()
                                .any(|variant| variant.id.as_str() == *id)
                                .then(|| oc_schema::id::ModelVariantId::parse(*id))
                        })
                    },
                );
            let mut reference = oc_schema::model::Ref {
                provider_id: info.provider_id.clone(),
                id: info.id.clone(),
                variant,
            };
            if reference.variant.is_none() {
                // A catalog resolution picks the default variant when none
                // is pinned (the resolver's default selection).
                reference.variant = Some(oc_schema::id::ModelVariantId::parse("default"));
            }
            self.resolver.resolve_selected(session, &reference).ok()
        });
        let selected = preferred.or_else(|| primary.clone())?;
        Some((agent, primary, selected))
    }

    /// The merged ruleset (`Permission.merge(agent, session ?? [])`).
    fn merged_permissions(agent: &AgentInfo, session: Option<&Ruleset>) -> Ruleset {
        let mut merged = agent.permissions.clone();
        if let Some(session) = session {
            merged.extend(session.iter().cloned());
        }
        merged
    }

    /// The instruction assembly in the cache-friendly order (context.ts
    /// :145-159): codemode → mcp → references → skills → discovery →
    /// builtins (date + env) → entries.
    async fn instruction_list(&self, session_id: &str) -> InstructionList {
        let mut list: InstructionList = vec![
            codemode_source(Arc::clone(&self.tools)),
            mcp_guidance_source(Arc::clone(&self.deps.mcp)),
            reference_guidance_source(),
            skill_guidance_source(Arc::clone(&self.deps.skills)),
            discovery_source(Arc::clone(&self.deps.discovery)),
            date_source(Arc::clone(&self.deps.clock)),
            environment_source(Arc::clone(&self.deps.environment)),
        ];
        list.extend(entry_sources(&self.db, session_id).await);
        list
    }

    /// `select` (context.ts:121-161).
    pub async fn select(&self, session_id: &str) -> Result<Selection, SelectError> {
        let session = self
            .db
            .transaction({
                let id = session_id.to_string();
                move |tx| SessionRow::get(tx, id)
            })
            .await?
            .ok_or_else(|| SelectError::NotFound(session_id.to_string()))?;
        // Location mismatch ⇒ interrupt (Reloaded semantics upstream).
        let location_matches = session.directory == self.location.directory.to_string_lossy()
            && session.workspace_id.as_deref() == self.location.workspace_id.as_deref();
        if !location_matches {
            return Err(SelectError::LocationMismatch(session_id.to_string()));
        }
        let selection = self.agents.select(session.agent.as_deref());
        let agent = selection.info.ok_or_else(|| SelectError::AgentNotFound {
            session: session_id.to_string(),
            agent: session.agent.clone().unwrap_or_else(|| selection.id.0.clone()),
        })?;
        let session_rules: Option<Ruleset> =
            session.permission.as_ref().and_then(|raw| serde_json::from_str::<Ruleset>(raw).ok());
        let permissions = Self::merged_permissions(&agent, session_rules.as_ref());
        let code_mode = self.tools.executor(crate::tool::codemode::EXECUTE_NAME).is_some();
        let tools = crate::tool::codemode::shape_request_tools(
            self.tools.snapshot(Some(&permissions)),
            Some(&permissions),
            code_mode,
        );
        let instructions = self.instruction_list(session_id).await;
        Ok(Selection { session, agent_id: selection.id, agent, instructions, tools, permissions })
    }

    /// `load` (context.ts:163-179) — model + history for a selection.
    pub async fn load(&self, selection: &Selection) -> Result<Loaded, ModelError> {
        let model = self.resolve_model(&selection.session)?;
        let boundary = match provenance(&model) {
            Some(provenance) => Boundary::Provenance(provenance),
            None => Boundary::Local,
        };
        let (initial, messages) = entries_for_runner(
            &self.db,
            selection.session.id.as_str(),
            &selection.instructions,
            &boundary,
        )
        .await
        .map_err(|err| ModelError::Resolver(format!("history load failed: {err}")))?;
        Ok(Loaded {
            session: selection.session.clone(),
            agent_id: selection.agent_id.clone(),
            agent: selection.agent.clone(),
            model,
            initial,
            messages,
            tools: selection.tools.clone(),
        })
    }

    /// `prepareContext` (llm.ts:198-203): select +
    /// `InstructionState.prepare`; a blocked initial instruction baseline
    /// leaves admitted input pending.
    pub async fn prepare_context(&self, session_id: &str) -> Result<Selection, SelectError> {
        let selected = self.select(session_id).await?;
        InstructionState::prepare(&self.db, &self.bus, &selected.instructions, session_id)
            .await
            .map_err(SelectError::InstructionsBlocked)?;
        Ok(selected)
    }
}

/// Convenience: tool names for the system prompt.
#[must_use]
pub fn tool_names(tools: &[crate::location::ToolRegistration]) -> Vec<String> {
    tools.iter().map(|tool| tool.name.clone()).collect()
}

/// `hasMeasuredPrompt` (compaction.ts:918-923): only the current provider's
/// counts are trusted.
#[must_use]
pub fn has_measured_prompt(message: &MessageInfo, model: &oc_schema::model::Ref) -> bool {
    match message {
        MessageInfo::Assistant { model: message_model, tokens, error, .. } => {
            message_model.provider_id == model.provider_id
                && error.is_none()
                && tokens.as_ref().is_some_and(|tokens| {
                    tokens.input.0 + tokens.cache.read.0 + tokens.cache.write.0 > 0.0
                })
        }
        _ => false,
    }
}
