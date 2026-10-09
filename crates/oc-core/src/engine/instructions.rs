//! `SessionInstructions` — the production AGENTS.md injection (port of
//! `session/instructions.ts`): read-tool side effects become durable
//! synthetic instruction messages, exactly once per (session, path).
//!
//! Dedup is two-layered, exactly like v2:
//!
//! 1. an **in-flight claim** (per session, per path) guards same-step
//!    parallel reads settling concurrently;
//! 2. the **lasting ledger** is the projected synthetic messages'
//!    `metadata.instruction.paths` — paths whose synthetics drop out of
//!    model-visible history (compaction, revert) are re-discovered and
//!    re-injected instead of staying silently lost.
//!
//! The synthetic publishes directly on the bus (v2 comment: a
//! Location-scoped layer cannot route through Session) with
//! `Instructions from: {path}` bodies; the human-facing description shows
//! paths relative to the project root.

#![allow(
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructors consume their parts
    clippy::too_many_lines, // the assembly table mirrors the v2 layer graph
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity, // hook/executor seams match the lib's
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown, // TS identifiers in prose stay unquoted
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening, // assembly order is deliberate
    clippy::large_futures, // boot/assemble mirror the v2 layer composition
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
    clippy::doc_lazy_continuation,
    clippy::elidable_lifetime_names,
    clippy::question_mark // the early-return shape mirrors the TS branch
)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oc_schema::id::SessionId;
use parking_lot::Mutex;

use crate::bus::{Bus, PublishOptions};
use crate::database::Database;
use crate::tool::InstructionInjection;

struct Inner {
    db: Database,
    bus: Bus,
    /// The project root `describePath` resolves against.
    root: PathBuf,
    /// In-flight claims: session → claimed paths.
    in_flight: Mutex<HashMap<String, HashSet<String>>>,
}

/// The per-location AGENTS.md injection service (clone shares the claim
/// map — the read leaf's parallel fibers all observe one another).
#[derive(Clone)]
pub struct SessionInstructions {
    inner: Arc<Inner>,
}

impl SessionInstructions {
    /// Service bound to one location's project root.
    #[must_use]
    pub fn new(db: Database, bus: Bus, root: PathBuf) -> Self {
        Self { inner: Arc::new(Inner { db, bus, root, in_flight: Mutex::new(HashMap::new()) }) }
    }

    /// The durable side of `load`: ledger filter → read files → publish
    /// `session.synthetic` exactly once per (session, path), then release
    /// the claim (`Effect.ensuring`).
    pub async fn load(&self, session_id: &SessionId, paths: &[PathBuf]) {
        let claimed = {
            let mut in_flight = self.inner.in_flight.lock();
            let existing = in_flight.entry(session_id.as_str().to_string()).or_default();
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .filter(|path| existing.insert(path.clone()))
                .collect::<Vec<String>>()
        };
        if claimed.is_empty() {
            return;
        }
        let outcome = self.load_claimed(session_id, &claimed).await;
        let _ = outcome;
        self.release(session_id, &claimed);
    }

    /// The spawned tail of the sync seam: the claim is already held.
    async fn settle(&self, session_id: &SessionId, claimed: &[String]) {
        let _ = self.load_claimed(session_id, claimed).await;
        self.release(session_id, claimed);
    }

    fn release(&self, session_id: &SessionId, claimed: &[String]) {
        let mut in_flight = self.inner.in_flight.lock();
        if let Some(existing) = in_flight.get_mut(session_id.as_str()) {
            for path in claimed {
                existing.remove(path);
            }
            if existing.is_empty() {
                in_flight.remove(session_id.as_str());
            }
        }
    }

    async fn load_claimed(&self, session_id: &SessionId, claimed: &[String]) -> Result<(), String> {
        let already = self.previously_injected(session_id).await?;
        let to_inject: Vec<&String> =
            claimed.iter().filter(|path| !already.contains(*path)).collect();
        if to_inject.is_empty() {
            return Ok(());
        }
        let readable: Vec<(&String, String)> = to_inject
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok().map(|content| (*path, content)))
            .collect();
        if readable.is_empty() {
            return Ok(());
        }
        let text = readable
            .iter()
            .map(|(path, content)| format!("Instructions from: {path}\n{content}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let description = format!(
            "Loaded {}",
            readable
                .iter()
                .map(|(path, _)| describe_path(&self.inner.root, path))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let paths: Vec<&str> = readable.iter().map(|(path, _)| path.as_str()).collect();
        let data = serde_json::json!({
            "sessionID": session_id.as_str(),
            "text": text,
            "description": description,
            "metadata": { "instruction": { "paths": paths } },
        });
        let def = crate::bus::registry::by_type_latest("session.synthetic")
            .ok_or_else(|| "no session.synthetic definition".to_string())?;
        self.inner
            .bus
            .publish(def, data, PublishOptions::default())
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// The ledger: `metadata.instruction.paths` of every projected
    /// synthetic message for the session.
    async fn previously_injected(&self, session_id: &SessionId) -> Result<HashSet<String>, String> {
        let id = session_id.as_str().to_string();
        let rows = self
            .inner
            .db
            .transaction(move |tx| {
                Ok(tx
                    .conn()
                    .prepare("SELECT type, data FROM session_message WHERE session_id = ?1 AND type = 'synthetic'")?
                    .query_map(rusqlite::params![id], |row| row.get::<_, String>(1))?
                    .collect::<rusqlite::Result<Vec<String>>>()?)
            })
            .await
            .map_err(|error| error.to_string())?;
        let mut paths = HashSet::new();
        for row in rows {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&row) else { continue };
            let Some(injected) =
                value.pointer("/metadata/instruction/paths").and_then(serde_json::Value::as_array)
            else {
                continue;
            };
            for path in injected {
                if let Some(path) = path.as_str() {
                    paths.insert(path.to_string());
                }
            }
        }
        Ok(paths)
    }
}

impl InstructionInjection for SessionInstructions {
    fn load(&self, session_id: &SessionId, paths: &[PathBuf]) {
        // The CLAIM is synchronous (v2 claims inside the read fiber, so a
        // later same-step read observes it before the read returns); the
        // durable side (ledger check + publish + claim release) runs as a
        // spawned task — the seam is sync.
        let claimed = {
            let mut in_flight = self.inner.in_flight.lock();
            let existing = in_flight.entry(session_id.as_str().to_string()).or_default();
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .filter(|path| existing.insert(path.clone()))
                .collect::<Vec<String>>()
        };
        if claimed.is_empty() {
            return;
        }
        let service = self.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            service.settle(&session_id, &claimed).await;
        });
    }
}

/// Paths are normally discovered under the project root, so the
/// description shows them relative to it; a path outside the root falls
/// back to its absolute form (instructions.ts `describePath`).
fn describe_path(root: &Path, path: &str) -> String {
    let path = Path::new(path);
    if let Ok(relative) = path.strip_prefix(root) {
        if !relative.as_os_str().is_empty() {
            return relative.to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}
