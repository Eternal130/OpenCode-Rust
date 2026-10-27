//! Port of `session/store.ts` + creation (`session.ts:262-318`).
//!
//! [`SessionStore`] reads `session_v2` (get/list/messages), drives the
//! write-ahead claim lifecycle (`claim`/`release`/`countResume`) and
//! publishes idempotent `session.created` / `session.deleted` events.

#![allow(
    clippy::must_use_candidate,
    clippy::redundant_closure,
    clippy::field_reassign_with_default, // PublishOptions::default + location
    clippy::option_if_let_else,
    clippy::too_many_lines,
    clippy::assigning_clones,
    clippy::useless_format,
    clippy::single_match,
    clippy::map_unwrap_or,
)]

use std::collections::HashSet;

use oc_schema::id::SessionId;
use oc_schema::location;
use oc_schema::model;
use oc_schema::permission::Ruleset;
use oc_schema::session_support::SessionMetadata;
use rusqlite::OptionalExtension;

use crate::bus::{Bus, BusError, PublishOptions};
use crate::database::error::Result;
use crate::database::schema::{SessionMessageRow, SessionRow};
use crate::database::Database;

/// The project resolution [`SessionStore::create`] needs
/// (`projects.resolve(location.directory)`).
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub id: String,
    /// Canonical project worktree (absolute).
    pub directory: String,
}

/// `SessionStore.list` input (store.ts:17-39).
#[derive(Debug, Clone, Default)]
pub struct ListInput {
    pub directory: Option<String>,
    pub workspace_id: Option<String>,
    pub project: Option<String>,
    pub subpath: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub order: Option<ListOrder>,
    /// `Some(None)` filters parentless sessions; `Some(id)` children of `id`.
    pub parent_id: Option<Option<String>>,
    pub anchor: Option<ListAnchor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListOrder {
    #[default]
    Desc,
    Asc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone)]
pub struct ListAnchor {
    pub id: String,
    pub time: i64,
    pub direction: ListDirection,
}

/// `SessionStore.messages` input.
#[derive(Debug, Clone, Default)]
pub struct MessagesInput {
    pub session_id: String,
    pub limit: Option<i64>,
    pub order: Option<ListOrder>,
    pub type_: Option<String>,
    pub cursor: Option<(String, ListDirection)>,
}

/// `Session.create` input (session.ts:224-240).
#[derive(Debug, Clone, Default)]
pub struct CreateInput {
    pub id: Option<SessionId>,
    pub parent_id: Option<SessionId>,
    pub location: Option<location::Ref>,
    pub title: Option<String>,
    pub agent: Option<String>,
    pub model: Option<model::Ref>,
    pub metadata: Option<SessionMetadata>,
    pub permissions: Option<Ruleset>,
    /// Caller-resolved project for the session's location.
    pub project: Option<ResolvedProject>,
    pub version: String,
}

/// `Session.create` outcome: fresh projection vs existing identity.
#[derive(Debug, Clone)]
pub enum CreateOutcome {
    Created(SessionRow),
    Existing(SessionRow),
}

/// `Session.ID.create`-equivalent slug source (two random words).
pub fn slug_create() -> String {
    const ADJECTIVES: &[&str] = &[
        "amber", "bold", "calm", "dusty", "eager", "fuzzy", "gentle", "hidden", "ivory", "jade",
        "keen", "lucid", "muted", "noble", "olive", "proud", "quiet", "rapid", "silent", "tidy",
        "urban", "vivid", "warm", "young", "zesty", "brisk", "clever", "deep", "early", "fresh",
    ];
    const NOUNS: &[&str] = &[
        "atlas", "bay", "canyon", "delta", "ember", "fjord", "grove", "harbor", "isle", "junction",
        "knoll", "lake", "mesa", "nectar", "orbit", "prism", "quarry", "ridge", "summit", "timber",
        "umbra", "valley", "willow", "zenith", "cinder", "flint", "granite", "lotus", "mirage",
        "onyx",
    ];
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!(
        "{}-{}",
        ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())],
        NOUNS[rng.gen_range(0..NOUNS.len())]
    )
}

/// The session store service (`SessionStore.Service`).
#[derive(Clone)]
pub struct SessionStore {
    db: Database,
}

impl SessionStore {
    /// Assembles the store.
    #[must_use]
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// `get`.
    pub async fn get(&self, session_id: &str) -> Result<Option<SessionRow>> {
        let id = session_id.to_string();
        self.db.transaction(move |tx| SessionRow::get(tx, id)).await
    }

    /// `list` (store.ts:99-139): ordered by `(time_updated, id)` with
    /// exclusive two-column anchor paging in either direction.
    pub async fn list(&self, input: ListInput) -> Result<Vec<SessionRow>> {
        let direction =
            input.anchor.as_ref().map_or(ListDirection::Next, |anchor| anchor.direction);
        let requested = input.order.unwrap_or_default();
        let order = match direction {
            ListDirection::Next => requested,
            ListDirection::Previous => match requested {
                ListOrder::Asc => ListOrder::Desc,
                ListOrder::Desc => ListOrder::Asc,
            },
        };

        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();
        if let Some(directory) = &input.directory {
            conditions.push(format!("directory = ?{}", params.len() + 1));
            params.push(Box::new(directory.clone()));
        }
        if let Some(workspace) = &input.workspace_id {
            conditions.push(format!("workspace_id = ?{}", params.len() + 1));
            params.push(Box::new(workspace.clone()));
        }
        if let Some(project) = &input.project {
            conditions.push(format!("project_id = ?{}", params.len() + 1));
            params.push(Box::new(project.clone()));
        }
        if let Some(subpath) = &input.subpath {
            conditions.push(format!("path = ?{}", params.len() + 1));
            params.push(Box::new(subpath.clone()));
        }
        if let Some(search) = &input.search {
            conditions.push(format!("title LIKE ?{}", params.len() + 1));
            params.push(Box::new(format!("%{search}%")));
        }
        if let Some(Some(parent)) = &input.parent_id {
            conditions.push(format!("parent_id = ?{}", params.len() + 1));
            params.push(Box::new(parent.clone()));
        } else if input.parent_id == Some(None) {
            conditions.push("parent_id IS NULL".to_string());
        }
        if let Some(anchor) = &input.anchor {
            let index = params.len() + 1;
            let (cmp, tie) = match order {
                ListOrder::Asc => (">", ">"),
                ListOrder::Desc => ("<", "<"),
            };
            conditions.push(format!(
                "(time_updated {cmp} ?{index} OR (time_updated = ?{index} AND id {tie} ?{})",
                index + 1
            ));
            // open paren for the OR group, closed after both binds
            conditions.last_mut().unwrap().push(')');
            params.push(Box::new(anchor.time));
            params.push(Box::new(anchor.id.clone()));
        }

        let predicate = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let ordering = match order {
            ListOrder::Asc => "time_updated ASC, id ASC",
            ListOrder::Desc => "time_updated DESC, id DESC",
        };
        let limit = input.limit.map(|limit| format!(" LIMIT {limit}")).unwrap_or_default();
        let sql = format!("SELECT * FROM session_v2 {predicate} ORDER BY {ordering}{limit}");

        let mut rows = self
            .db
            .transaction(move |tx| {
                let mut statement = tx.conn().prepare(&sql)?;
                let references: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|value| value.as_ref() as &dyn rusqlite::ToSql).collect();
                let rows = statement
                    .query_map(references.as_slice(), SessionRow::decode_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        if direction == ListDirection::Previous {
            rows.reverse();
        }
        Ok(rows)
    }

    /// `messages` (store.ts:140-178): pages by durable sequence, not
    /// timestamp or id; cursor lookup is scoped to the session.
    pub async fn messages(&self, input: MessagesInput) -> Result<Vec<SessionMessageRow>> {
        let direction =
            input.cursor.as_ref().map_or(ListDirection::Next, |(_, direction)| *direction);
        let requested = input.order.unwrap_or_default();
        let order = match direction {
            ListDirection::Next => requested,
            ListDirection::Previous => match requested {
                ListOrder::Asc => ListOrder::Desc,
                ListOrder::Desc => ListOrder::Asc,
            },
        };

        let (lookup_session, cursor_id) = (input.session_id.clone(), input.cursor.clone());
        let anchor = match cursor_id {
            Some((cursor_id, _)) => {
                let anchor: Option<i64> =
                    self.db
                        .transaction(move |tx| {
                            Ok(tx.conn()
                            .query_row(
                                "SELECT seq FROM session_message WHERE session_id = ?1 AND id = ?2",
                                rusqlite::params![lookup_session, cursor_id],
                                |row| row.get(0),
                            )
                            .optional()?)
                        })
                        .await?;
                if anchor.is_none() {
                    return Ok(Vec::new());
                }
                anchor
            }
            None => None,
        };
        let Some(anchor) = anchor else {
            // No cursor: plain ordered page.
            return self
                .messages_query(&input.session_id, None, order, input.limit, input.type_.as_deref())
                .await
                .map(|mut rows| {
                    if direction == ListDirection::Previous {
                        rows.reverse();
                    }
                    rows
                });
        };

        let mut rows = self
            .messages_query(
                &input.session_id,
                Some((anchor, order)),
                order,
                input.limit,
                input.type_.as_deref(),
            )
            .await?;
        if direction == ListDirection::Previous {
            rows.reverse();
        }
        Ok(rows)
    }

    async fn messages_query(
        &self,
        session_id: &str,
        boundary: Option<(i64, ListOrder)>,
        order: ListOrder,
        limit: Option<i64>,
        type_: Option<&str>,
    ) -> Result<Vec<SessionMessageRow>> {
        let session_id = session_id.to_string();
        let type_filter = type_.map(str::to_string);
        self.db
            .transaction(move |tx| {
                let mut conditions = vec![format!("session_id = ?1")];
                let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> =
                    vec![Box::new(session_id.clone())];
                if let Some((anchor, order)) = boundary {
                    let cmp = match order {
                        ListOrder::Asc => '>',
                        ListOrder::Desc => '<',
                    };
                    conditions.push(format!("seq {cmp} ?{}", params.len() + 1));
                    params.push(Box::new(anchor));
                }
                if let Some(type_) = type_filter {
                    conditions.push(format!("type = ?{}", params.len() + 1));
                    params.push(Box::new(type_));
                }
                let ordering = match order {
                    ListOrder::Asc => "seq ASC",
                    ListOrder::Desc => "seq DESC",
                };
                let limit = limit.map(|limit| format!(" LIMIT {limit}")).unwrap_or_default();
                let sql = format!(
                    "SELECT * FROM session_message WHERE {} ORDER BY {ordering}{limit}",
                    conditions.join(" AND ")
                );
                let mut statement = tx.conn().prepare(&sql)?;
                let references: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|value| value.as_ref() as &dyn rusqlite::ToSql).collect();
                let rows = statement
                    .query_map(references.as_slice(), SessionMessageRow::decode_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// `listSuspended`: top-level sessions holding an execution claim.
    pub async fn list_suspended(&self) -> Result<Vec<String>> {
        self.db
            .transaction(|tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT id FROM session_v2 WHERE time_suspended IS NOT NULL \
                     AND parent_id IS NULL",
                )?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// RUNTIME-SWEEP-RACE — `listSuspended` under the boot-start guard:
    /// only claims recorded strictly BEFORE `before_ms` are pre-boot
    /// state. `time_suspended` is the claim timestamp (`claim`/the
    /// `session.execution.started` commit hook), so a row claimed at or
    /// after this process started belongs to the CURRENT boot or a
    /// concurrently-live peer sharing the database — the sweep must not
    /// resume it. v2's equivalent invariant is process-level: the sweep
    /// "assumes every orphaned claim's owner is dead" and "unregistered
    /// servers sharing the database never sweep" (restart.ts:44-52 —
    /// only the managed lifecycle installs it, process.ts:104-110).
    pub async fn list_suspended_before(&self, before_ms: i64) -> Result<Vec<String>> {
        self.db
            .transaction(move |tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT id FROM session_v2 WHERE time_suspended IS NOT NULL \
                     AND parent_id IS NULL AND time_suspended < ?1",
                )?;
                let rows = statement
                    .query_map(rusqlite::params![before_ms], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// `claim` (store.ts:205-216): records the durable write-ahead intent;
    /// re-claiming a claimed session is a no-op and `time_updated` stays
    /// pinned (claim bookkeeping is not user activity).
    pub async fn claim(&self, session_id: &str) -> Result<()> {
        let id = session_id.to_string();
        self.db
            .transaction(move |tx| {
                tx.execute(
                    "UPDATE session_v2 SET time_suspended = ?2, time_updated = time_updated \
                     WHERE id = ?1 AND time_suspended IS NULL",
                    rusqlite::params![id, crate::database::migrations::now_ms()],
                )?;
                Ok(())
            })
            .await
    }

    /// `release`: releases the claim and resets resume accounting.
    pub async fn release(&self, session_id: &str) -> Result<()> {
        let id = session_id.to_string();
        self.db
            .transaction(move |tx| {
                tx.execute(
                    "UPDATE session_v2 SET time_suspended = NULL, resume_attempts = 0, \
                     time_updated = time_updated WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await
    }

    /// `releaseChildClaims`: clears orphaned child claims except children
    /// owned by recoverable background subagent jobs.
    pub async fn release_child_claims(&self, recoverable: &[String]) -> Result<()> {
        let recoverable: Vec<String> = recoverable.to_vec();
        self.db
            .transaction(move |tx| {
                if recoverable.is_empty() {
                    tx.execute(
                        "UPDATE session_v2 SET time_suspended = NULL, resume_attempts = 0, \
                         time_updated = time_updated \
                         WHERE time_suspended IS NOT NULL AND parent_id IS NOT NULL",
                        [],
                    )?;
                    return Ok(());
                }
                let marks = recoverable.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "UPDATE session_v2 SET time_suspended = NULL, resume_attempts = 0, \
                     time_updated = time_updated \
                     WHERE time_suspended IS NOT NULL AND parent_id IS NOT NULL \
                     AND id NOT IN ({marks})"
                );
                let params: Vec<&dyn rusqlite::ToSql> =
                    recoverable.iter().map(|value| value as &dyn rusqlite::ToSql).collect();
                tx.execute(&sql, params.as_slice())?;
                Ok(())
            })
            .await
    }

    /// RUNTIME-SWEEP-RACE — `releaseChildClaims` under the boot-start
    /// guard: only child claims recorded strictly BEFORE `before_ms` are
    /// orphaned pre-boot state; a child claimed at or after this process
    /// started is live work of the CURRENT boot or a peer sharing the
    /// database and must keep its claim.
    pub async fn release_child_claims_before(
        &self,
        recoverable: &[String],
        before_ms: i64,
    ) -> Result<()> {
        let recoverable: Vec<String> = recoverable.to_vec();
        self.db
            .transaction(move |tx| {
                if recoverable.is_empty() {
                    tx.execute(
                        "UPDATE session_v2 SET time_suspended = NULL, resume_attempts = 0, \
                         time_updated = time_updated \
                         WHERE time_suspended IS NOT NULL AND parent_id IS NOT NULL \
                         AND time_suspended < ?1",
                        rusqlite::params![before_ms],
                    )?;
                    return Ok(());
                }
                let marks = recoverable.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "UPDATE session_v2 SET time_suspended = NULL, resume_attempts = 0, \
                     time_updated = time_updated \
                     WHERE time_suspended IS NOT NULL AND parent_id IS NOT NULL \
                     AND id NOT IN ({marks}) AND time_suspended < ?{}",
                    recoverable.len() + 1
                );
                let mut params: Vec<&dyn rusqlite::ToSql> =
                    recoverable.iter().map(|value| value as &dyn rusqlite::ToSql).collect();
                params.push(&before_ms);
                tx.execute(&sql, params.as_slice())?;
                Ok(())
            })
            .await
    }

    /// `countResume`: durably counts one more resume of an orphaned claim.
    pub async fn count_resume(&self, session_id: &str) -> Result<Option<i64>> {
        let id = session_id.to_string();
        self.db
            .transaction(move |tx| {
                let attempts: Option<i64> = tx
                    .conn()
                    .query_row(
                        "UPDATE session_v2 SET resume_attempts = resume_attempts + 1, \
                         time_updated = time_updated WHERE id = ?1 RETURNING resume_attempts",
                        rusqlite::params![id],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(attempts)
            })
            .await
    }

    /// The `boundary(messageID)` seq lookup used by fork/revert.
    pub async fn boundary(&self, session_id: &str, message_id: &str) -> Result<Option<i64>> {
        let (session_id, message_id) = (session_id.to_string(), message_id.to_string());
        self.db
            .transaction(move |tx| {
                let seq: Option<i64> = tx
                    .conn()
                    .query_row(
                        "SELECT seq FROM session_message WHERE session_id = ?1 AND id = ?2",
                        rusqlite::params![session_id, message_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(seq)
            })
            .await
    }

    /// `Session.remove`: publish `session.deleted` (the projector deletes
    /// the row, cascading to messages/inbox/instruction state).
    pub async fn remove(&self, bus: &Bus, session_id: &str) -> std::result::Result<(), BusError> {
        let data = serde_json::json!({ "sessionID": session_id });
        let def = crate::bus::registry::by_type_latest("session.deleted")
            .unwrap_or_else(|| panic!("no session.deleted definition"));
        bus.publish(def, data, PublishOptions::default()).await.map(|_| ())
    }

    /// `Session.create` (session.ts:262-318): idempotent `session.created`
    /// publication. Concurrent creation losing the projection race re-reads
    /// the winner (`SessionAlreadyProjected` ⇒ existing identity).
    #[allow(clippy::too_many_lines)]
    pub async fn create(
        &self,
        bus: &Bus,
        input: CreateInput,
    ) -> std::result::Result<CreateOutcome, CreateError> {
        let session_id = input.id.clone().unwrap_or_default();
        if let Some(recorded) = self.get(session_id.as_str()).await? {
            return Ok(CreateOutcome::Existing(recorded));
        }

        let parent = match &input.parent_id {
            Some(parent_id) => {
                let parent = self.get(parent_id.as_str()).await?;
                let parent =
                    parent.ok_or_else(|| CreateError::ParentNotFound(parent_id.clone()))?;
                Some(parent)
            }
            None => None,
        };

        // Children inherit location the way they inherit metadata and
        // permissions; explicit input wins.
        let location = match input.location.clone() {
            Some(location) => Some(location),
            None => parent.as_ref().map(|parent| location::Ref {
                directory: oc_schema::common::AbsolutePath(parent.directory.clone()),
                workspace_id: parent.workspace_id.clone().map(|value| {
                    oc_schema::id::WorkspaceId::parse(value)
                        .unwrap_or_else(|err| oc_schema::id::WorkspaceId(err.value))
                }),
            }),
        };
        let Some(location) = location else {
            return Err(CreateError::RequiresLocation);
        };

        // Default resolution: directory-keyed identity (the non-VCS branch of
        // `projects.resolve`).
        let project = input.project.clone().unwrap_or_else(|| ResolvedProject {
            id: crate::project::hash_fast(&format!("directory:{}", location.directory.as_str())),
            directory: location.directory.as_str().to_string(),
        });
        // Resolution persists the project row before the session references
        // it (`projects.resolve` → `persist`).
        {
            let row = crate::database::schema::ProjectRow::new(
                project.id.clone(),
                &project.directory,
                None,
            )?;
            let id = row.id.clone();
            let worktree = row.worktree.clone();
            let vcs = row.vcs.clone();
            self.db
                .transaction(move |tx| {
                    tx.execute(
                        "INSERT OR IGNORE INTO project (id, worktree, vcs, time_created, time_updated, \
                         time_active, sandboxes) VALUES (?1, ?2, ?3, ?4, ?4, 0, '[]')",
                        rusqlite::params![id, worktree, vcs, crate::database::migrations::now_ms()],
                    )?;
                    Ok(())
                })
                .await?;
        }
        let subpath = relativize(&project.directory, location.directory.as_str());

        // v2's Schema serialization omits undefined fields — absent
        // agent/parentID/metadata/permissions/model stay OUT of the event
        // payload instead of serializing as null (parity goldens pin the
        // wire shape).
        let mut data = serde_json::json!({
            "sessionID": session_id.as_str(),
            "slug": slug_create(),
            "version": input.version,
            "projectID": project.id,
            "location": serde_json::to_value(&location)?,
            "subpath": subpath,
            "title": input.title,
        });
        if let Some(parent_id) = input.parent_id.as_ref() {
            data["parentID"] = serde_json::Value::String(parent_id.as_str().to_owned());
        }
        if let Some(agent) = input.agent {
            data["agent"] = serde_json::Value::String(agent);
        }
        // Children inherit metadata and permissions so host policies
        // treat the family uniformly.
        let metadata = match input.metadata {
            Some(metadata) => Some(serde_json::to_value(&metadata)?),
            None => parent
                .as_ref()
                .and_then(|parent| parent.metadata.clone())
                .map(|stored| serde_json::from_str::<SessionMetadata>(&stored))
                .transpose()?
                .map(|metadata| serde_json::to_value(&metadata).unwrap_or_default()),
        };
        if let Some(metadata) = metadata {
            data["metadata"] = metadata;
        }
        let permissions = match input.permissions {
            Some(permissions) => Some(serde_json::to_value(&permissions)?),
            None => parent
                .as_ref()
                .and_then(|parent| parent.permission.clone())
                .map(|stored| serde_json::from_str::<Ruleset>(&stored))
                .transpose()?
                .map(|permissions| serde_json::to_value(&permissions).unwrap_or_default()),
        };
        if let Some(permissions) = permissions {
            data["permissions"] = permissions;
        }
        if let Some(model) = input.model {
            data["model"] = serde_json::to_value(&model)?;
        }

        let def = crate::bus::registry::by_type_latest("session.created")
            .unwrap_or_else(|| panic!("no session.created definition"));
        let mut options = PublishOptions::default();
        options.location = Some(location.clone());
        match bus.publish(def, data, options).await {
            Ok(_) => {}
            Err(err) if crate::session::projector::is_already_projected(&err) => {
                // Concurrent creation lost the projection race; the
                // existing Session identity wins.
                let existing = self.get(session_id.as_str()).await?.ok_or(err)?;
                return Ok(CreateOutcome::Existing(existing));
            }
            Err(err) => return Err(err.into()),
        }
        let created = self.get(session_id.as_str()).await?.ok_or_else(|| {
            CreateError::Internal(format!("session {} missing after creation", session_id.as_str()))
        })?;
        Ok(CreateOutcome::Created(created))
    }
}

/// POSIX-style `path.relative(from, to)` with forward slashes.
fn relativize(from: &str, to: &str) -> String {
    let from_parts: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".to_string());
    }
    for part in &to_parts[common..] {
        parts.push((*part).to_string());
    }
    if parts.is_empty() {
        // Node's `path.relative(x, x)` is the empty string (v2 emits
        // `subpath: ""` for a session in the project root, pinned by the
        // parity goldens) — not ".".
        String::new()
    } else {
        parts.join("/")
    }
}

/// `Session.create` typed errors.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("session not found: {0}")]
    ParentNotFound(SessionId),
    #[error("Session.create requires either location or an existing parentID")]
    RequiresLocation,
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Db(#[from] crate::database::error::DbError),
    #[error(transparent)]
    Bus(#[from] BusError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Claims snapshot used by the execution tests: id → claimed?
pub async fn claims(db: &Database) -> Result<Vec<(String, bool)>> {
    db.transaction(|tx| {
        let mut statement = tx
            .conn()
            .prepare("SELECT id, time_suspended IS NOT NULL FROM session_v2 ORDER BY id")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .await
}

/// `resume_attempts` for one session.
pub async fn attempts(db: &Database, session_id: &str) -> Result<Option<i64>> {
    let id = session_id.to_string();
    db.transaction(move |tx| {
        let attempts: Option<i64> = tx
            .conn()
            .query_row(
                "SELECT resume_attempts FROM session_v2 WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(attempts)
    })
    .await
}

/// Session ids of a set of rows (test helper).
#[must_use]
pub fn ids_of(rows: &[SessionRow]) -> Vec<String> {
    rows.iter().map(|row| row.id.clone()).collect()
}

/// Dedup helper for tests.
#[must_use]
pub fn dedup(values: &[String]) -> HashSet<String> {
    values.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RC-09 parity pin: `slug_create` intentionally diverges from the
    /// reference TS wordlist (local 30×30 adjective-noun list vs upstream's
    /// larger one). IDs are opaque, so the observable contract — not the
    /// exact words — is what parity requires. This test pins that contract:
    /// exactly two lowercase words joined by a single hyphen, drawn from the
    /// local wordlist style (both sides seen across draws), bounded lengths,
    /// no invalid characters, and enough spread to avoid collisions in
    /// practice.
    #[test]
    fn slug_contract_pins_local_wordlist_style() {
        let draws: HashSet<String> = (0..5_000).map(|_| slug_create()).collect();
        assert!(!draws.is_empty());

        let mut adjectives: HashSet<String> = HashSet::new();
        let mut nouns: HashSet<String> = HashSet::new();
        for slug in &draws {
            // Format: exactly two words, single hyphen joiner.
            let parts: Vec<&str> = slug.split('-').collect();
            assert_eq!(parts.len(), 2, "slug must be two words joined by '-': {slug}");
            let (adjective, noun) = (parts[0], parts[1]);

            // Charset: lowercase ASCII letters only, no digits/unicode/spaces.
            for (word, is_adjective) in [(adjective, true), (noun, false)] {
                assert!(
                    word.chars().all(|c| c.is_ascii_lowercase()),
                    "invalid charset in slug: {slug}"
                );
                // Length bounds from the local wordlist style (adjectives
                // 4..=6 letters, nouns 3..=9 — "junction" is the longest).
                if is_adjective {
                    assert!((4..=6).contains(&word.len()), "adjective out of bounds in {slug}");
                } else {
                    assert!((3..=9).contains(&word.len()), "noun out of bounds in {slug}");
                }
            }
            adjectives.insert(adjective.to_string());
            nouns.insert(noun.to_string());
        }

        // Local wordlist style: both sides of the list are exercised (the
        // local lists have 30 entries each — see well above the mean of a
        // degenerate source).
        assert!(adjectives.len() >= 25, "adjective side under-sampled: {}", adjectives.len());
        assert!(nouns.len() >= 25, "noun side under-sampled: {}", nouns.len());

        // Uniqueness spread: with 30×30 = 900 combinations, 5_000 draws must
        // produce a large distinct set (birthday-bound, ~866 expected) — a
        // degenerate or constant source would fail this.
        assert!(draws.len() >= 800, "uniqueness spread too small: {}", draws.len());
    }
}
