//! Port of `session/revert.ts` — stage / clear / commit around the
//! snapshot store (core-runner-map §2.6).
//!
//! - **stage**: `original = session.revert?.snapshot ?? capture()`; plan =
//!   assistant messages after the boundary → their `snapshot.start` tree per
//!   files path (first-wins); restore `{staged→original, planned→tree}`;
//!   `files = diff(original → capture())`; publish `session.revert.staged`.
//! - **clear**: restore the staged files from `original`; `session.revert.cleared`.
//! - **commit**: `session.revert.committed {to}` (the projector deletes
//!   messages at/after the boundary and clears the state).

//! Lint posture: ports of TS sources keep the mechanical-idiom allows the
//! runner established (numeric casts, exhaustive tables, ownership queries).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity,
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening,
    clippy::large_futures,
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation
)]

use std::collections::BTreeMap;

use oc_schema::id::{MessageId, SessionId, SnapshotId};
use oc_schema::session_support::Revert;
use rusqlite::OptionalExtension;

use crate::bus::Bus;
use crate::database::Database;
use crate::snapshot::SnapshotService;

/// The persisted revert state on `session_v2.revert` (JSON column).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRevertState {
    pub message_id: MessageId,
    pub snapshot: Option<SnapshotId>,
    pub files: Option<Vec<oc_schema::session_support::FileDiff>>,
}

/// `stage` — stage a revert boundary at `message_id`.
///
/// `files: false` skips restoring the assistant-plan paths (the
/// message-only variant).
pub async fn stage(
    db: &Database,
    bus: &Bus,
    snapshot: &dyn SnapshotService,
    session_id: &SessionId,
    message_id: &MessageId,
    include_files: bool,
) -> Result<Revert, String> {
    // session.revert?.snapshot ?? capture()
    let existing = read_revert(db, session_id).await?;
    let original = match existing.as_ref().and_then(|state| state.snapshot.clone()) {
        Some(original) => Some(original),
        None => snapshot.capture().await,
    };
    // plan: assistant messages seq > boundary.seq → snapshot.start per
    // files path (first-wins).
    let next = plan(db, session_id, message_id).await?;
    let mut restore: BTreeMap<String, SnapshotId> = BTreeMap::new();
    if let Some(original) = &original {
        if let Some(state) = &existing {
            for file in state.files.clone().unwrap_or_default() {
                restore.insert(file.file, original.clone());
            }
        }
    }
    if include_files {
        for (file, tree) in &next {
            restore.insert(file.clone(), tree.clone());
        }
    }
    if !restore.is_empty() {
        snapshot
            .restore(&restore)
            .await
            .map_err(|error| format!("revert stage failed: {error}"))?;
    }
    let paths: Vec<oc_schema::common::RelativePath> = if include_files {
        next.keys().map(|path| oc_schema::common::RelativePath(path.clone())).collect()
    } else {
        Vec::new()
    };
    let files = match &original {
        Some(original) => {
            let after = snapshot.capture().await.unwrap_or_else(|| original.clone());
            if after == *original {
                Vec::new()
            } else {
                snapshot
                    .diff(
                        original,
                        &after,
                        None,
                        if paths.is_empty() { None } else { Some(&paths) },
                    )
                    .await
                    .unwrap_or_default()
            }
        }
        None => Vec::new(),
    };
    let revert = Revert {
        message_id: message_id.clone(),
        part_id: None,
        snapshot: original,
        files: Some(files),
    };
    let data = serde_json::json!({
        "sessionID": session_id.as_str(),
        "revert": serde_json::to_value(&revert).map_err(|error| error.to_string())?,
    });
    let def = crate::bus::registry::by_type_latest("session.revert.staged")
        .ok_or_else(|| "no session.revert.staged definition".to_string())?;
    bus.publish(def, data, crate::bus::PublishOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    Ok(revert)
}

/// `clear` — restore the staged files from the original snapshot.
pub async fn clear(
    db: &Database,
    bus: &Bus,
    snapshot: &dyn SnapshotService,
    session_id: &SessionId,
) -> Result<(), String> {
    let Some(state) = read_revert(db, session_id).await? else {
        return Ok(());
    };
    if let Some(original) = &state.snapshot {
        let files: BTreeMap<String, SnapshotId> = state
            .files
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|file| (file.file, original.clone()))
            .collect();
        snapshot.restore(&files).await.map_err(|error| format!("revert clear failed: {error}"))?;
    }
    let data = serde_json::json!({ "sessionID": session_id.as_str() });
    let def = crate::bus::registry::by_type_latest("session.revert.cleared")
        .ok_or_else(|| "no session.revert.cleared definition".to_string())?;
    bus.publish(def, data, crate::bus::PublishOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// `commit` — publish the boundary (the projector deletes messages at/after
/// it and resets the instruction state).
pub async fn commit(
    bus: &Bus,
    session_id: &SessionId,
    state: &SessionRevertState,
) -> Result<(), String> {
    let data = serde_json::json!({
        "sessionID": session_id.as_str(),
        "to": state.message_id.as_str(),
    });
    let def = crate::bus::registry::by_type_latest("session.revert.committed")
        .ok_or_else(|| "no session.revert.committed definition".to_string())?;
    bus.publish(def, data, crate::bus::PublishOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Reads `session_v2.revert` for a session.
pub async fn read_revert(
    db: &Database,
    session_id: &SessionId,
) -> Result<Option<SessionRevertState>, String> {
    let id = session_id.as_str().to_string();
    let raw: Option<Option<String>> = db
        .transaction(move |tx| {
            let raw = tx
                .conn()
                .query_row(
                    "SELECT revert FROM session_v2 WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .map_err(|error| crate::database::error::DbError::from(error))?;
            Ok(raw)
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(match raw.flatten() {
        None => None,
        Some(json) => {
            let value: serde_json::Value =
                serde_json::from_str(&json).map_err(|error| error.to_string())?;
            Some(SessionRevertState {
                message_id: MessageId::parse(
                    value["messageID"].as_str().unwrap_or_default().to_string(),
                )
                .unwrap_or_else(|error| MessageId(error.value)),
                snapshot: value["snapshot"].as_str().map(SnapshotId::parse),
                files: value["files"].as_array().map(|files| {
                    files.iter().filter_map(|f| serde_json::from_value(f.clone()).ok()).collect()
                }),
            })
        }
    })
}

/// `plan` — files-per-tree from assistant snapshots after the boundary.
async fn plan(
    db: &Database,
    session_id: &SessionId,
    message_id: &MessageId,
) -> Result<BTreeMap<String, SnapshotId>, String> {
    let session = session_id.as_str().to_string();
    let boundary = message_id.as_str().to_string();
    db.transaction(move |tx| {
        let boundary_seq: Option<i64> = tx
            .conn()
            .query_row(
                "SELECT seq FROM session_message WHERE session_id = ?1 AND id = ?2",
                rusqlite::params![session, boundary],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| crate::database::error::DbError::from(error))?;
        let Some(boundary_seq) = boundary_seq else {
            return Ok(BTreeMap::new());
        };
        let mut statement = tx
            .conn()
            .prepare(
                "SELECT data FROM session_message WHERE session_id = ?1 AND type = 'assistant' AND seq > ?2 ORDER BY seq ASC",
            )
            .map_err(|error| crate::database::error::DbError::from(error))?;
        let rows = statement
            .query_map(rusqlite::params![session, boundary_seq], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| crate::database::error::DbError::from(error))?;
        let mut files: BTreeMap<String, SnapshotId> = BTreeMap::new();
        for row in rows.flatten() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&row) else { continue };
            let Some(start) = value["snapshot"]["start"].as_str() else { continue };
            let Some(paths) = value["snapshot"]["files"].as_array() else { continue };
            for path in paths {
                let Some(path) = path.as_str() else { continue };
                files.entry(path.to_string()).or_insert_with(|| SnapshotId::parse(start));
            }
        }
        Ok(files)
    })
    .await
    .map_err(|error| error.to_string())
}
