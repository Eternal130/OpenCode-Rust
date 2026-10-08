//! Permission table persistence — the production
//! [`SavedGrantStore`](super::saved::SavedGrantStore) over the `permission`
//! sqlite table (created by the schema bootstrap / migrations owned by the
//! database lane):
//!
//! ```sql
//! CREATE TABLE permission (
//!   id TEXT PRIMARY KEY,
//!   project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
//!   action TEXT NOT NULL,
//!   resource TEXT NOT NULL,
//!   time_created INTEGER NOT NULL,
//!   time_updated INTEGER NOT NULL
//! );
//! CREATE UNIQUE INDEX permission_project_action_resource_idx
//!   ON permission(project_id, action, resource);
//! ```
//!
//! - `add` inserts with **conflict-do-nothing** on the
//!   `(project_id, action, resource)` unique index (drizzle
//!   `onConflictDoNothing`); empty `resources` is a no-op;
//! - `list` filters by `project_id` (ordered like
//!   [`crate::database::schema`]'s `permissions_for_project`: action,
//!   resource);
//! - `remove` deletes by row id;
//! - evaluation reads rows back as `{action, resource, effect: "allow"}`
//!   rules (see [`super::saved::rules`]).
//!
//! The [`SavedGrantStore`] trait is infallible (v2 surfaces `saved.add`
//! failures as reply defects, but the sqlite failures that survive the
//! database lane's boot are process-fatal); this store degrades silently
//! on database errors — queries return what they can.
//!
//! Note the foreign key: `project_id REFERENCES project(id)`, so grants
//! are only ever written for projects already resolved (v2 writes grants
//! from the permission engine, which runs inside an activated location —
//! the project row exists).

use std::sync::Arc;

use async_trait::async_trait;
use oc_schema::id::{PermissionSavedId, ProjectId};
use oc_util::determinism::{Clock, IdGen};

use super::saved::{AddSavedGrants, SavedGrant, SavedGrantStore};
use crate::database::Database;

/// Sqlite-backed saved-grant store (the `permission` table).
pub struct SqliteSavedGrantStore {
    db: Database,
    ids: Arc<dyn IdGen>,
    clock: Arc<dyn Clock>,
}

impl SqliteSavedGrantStore {
    /// Store over an open [`Database`]; fresh row ids come from `ids`,
    /// timestamps from `clock`.
    pub fn new(db: Database, ids: Arc<dyn IdGen>, clock: Arc<dyn Clock>) -> Self {
        Self { db, ids, clock }
    }
}

#[async_trait]
impl SavedGrantStore for SqliteSavedGrantStore {
    async fn list(&self, project_id: Option<&ProjectId>) -> Vec<SavedGrant> {
        let filter = project_id.map(|id| id.0.clone());
        let rows = self
            .db
            .transaction(move |tx| {
                let sql = match &filter {
                    Some(_) => {
                        "SELECT id, project_id, action, resource, time_created, time_updated \
                         FROM permission WHERE project_id = ?1 ORDER BY action, resource"
                    }
                    None => {
                        "SELECT id, project_id, action, resource, time_created, time_updated \
                            FROM permission ORDER BY action, resource"
                    }
                };
                let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SavedGrant> {
                    Ok(SavedGrant {
                        id: PermissionSavedId(row.get(0)?),
                        project_id: ProjectId(row.get(1)?),
                        action: row.get(2)?,
                        resource: row.get(3)?,
                        time_created: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                        time_updated: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                    })
                };
                let mut statement = tx.conn().prepare(sql)?;
                let rows = match &filter {
                    Some(project) => statement
                        .query_map(rusqlite::params![project], map_row)?
                        .collect::<Result<Vec<_>, _>>()?,
                    None => statement.query_map([], map_row)?.collect::<Result<Vec<_>, _>>()?,
                };
                Ok(rows)
            })
            .await;
        rows.unwrap_or_default()
    }

    async fn add(&self, input: &AddSavedGrants) {
        if input.resources.is_empty() {
            return;
        }
        let now = u64::try_from(self.clock.unix_ms()).unwrap_or(0);
        let now_i64 = i64::try_from(now).unwrap_or(0);
        let rows: Vec<(String, String, String, String, i64, i64)> = input
            .resources
            .iter()
            .map(|resource| {
                (
                    self.ids.new_id("psv"),
                    input.project_id.0.clone(),
                    input.action.clone(),
                    resource.clone(),
                    now_i64,
                    now_i64,
                )
            })
            .collect();
        let _ = self
            .db
            .transaction(move |tx| {
                for (id, project_id, action, resource, created, updated) in rows {
                    // onConflictDoNothing over the (project_id, action,
                    // resource) unique index: existing grants are kept
                    // as-is (id + timestamps untouched).
                    tx.execute(
                        "INSERT OR IGNORE INTO permission \
                         (id, project_id, action, resource, time_created, time_updated) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![id, project_id, action, resource, created, updated],
                    )?;
                }
                Ok(())
            })
            .await;
    }

    async fn remove(&self, id: &PermissionSavedId) {
        let id = id.0.clone();
        let _ = self
            .db
            .transaction(move |tx| {
                tx.execute("DELETE FROM permission WHERE id = ?1", rusqlite::params![id])?;
                Ok(())
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]

    use super::*;
    use oc_util::determinism::{FixedClock, SequentialIdGen};

    async fn temp_db() -> (Database, tempfile::TempDir) {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-perm-sql-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir under /tmp/opencode");
        let db = Database::open_file(dir.path().join("opencode.db")).await.expect("open database");
        (db, dir)
    }

    fn store(db: &Database) -> SqliteSavedGrantStore {
        SqliteSavedGrantStore::new(
            db.clone(),
            Arc::new(SequentialIdGen::new()),
            Arc::new(FixedClock::epoch()),
        )
    }

    /// The `permission` table carries `project_id REFERENCES project(id)`;
    /// grants are only ever written for resolved projects, so the tests
    /// seed the project rows first (production gets this for free from
    /// project activation).
    async fn seed_project(db: &Database, id: &str) {
        let id = id.to_string();
        db.transaction(move |tx| {
            let row = crate::database::schema::ProjectRow::new(id, "/proj", None)?;
            crate::database::schema::ProjectRow::insert_or_ignore(tx, &row)
        })
        .await
        .expect("seed project");
    }

    #[tokio::test]
    async fn permission_sql_roundtrip_conflict_and_remove() {
        let (db, _dir) = temp_db().await;
        let grants = store(&db);
        let p1 = ProjectId("proj_1".into());
        let p2 = ProjectId("proj_2".into());
        seed_project(&db, &p1.0).await;
        seed_project(&db, &p2.0).await;

        grants
            .add(&AddSavedGrants {
                project_id: p1.clone(),
                action: "edit".into(),
                resources: vec!["src/*".into(), "doc/*".into()],
            })
            .await;
        // Conflict-do-nothing: the duplicate (p1, edit, src/*) is kept as-is.
        grants
            .add(&AddSavedGrants {
                project_id: p1.clone(),
                action: "edit".into(),
                resources: vec!["src/*".into()],
            })
            .await;
        // Empty resources is a no-op.
        grants
            .add(&AddSavedGrants {
                project_id: p1.clone(),
                action: "edit".into(),
                resources: vec![],
            })
            .await;
        grants
            .add(&AddSavedGrants {
                project_id: p2.clone(),
                action: "read".into(),
                resources: vec!["x".into()],
            })
            .await;

        let p1_rows = grants.list(Some(&p1)).await;
        assert_eq!(p1_rows.len(), 2);
        let resources: Vec<&str> = p1_rows.iter().map(|grant| grant.resource.as_str()).collect();
        assert_eq!(resources, vec!["doc/*", "src/*"], "ordered by action, resource");
        assert!(p1_rows.iter().all(|grant| grant.project_id == p1));
        assert_eq!(grants.list(None).await.len(), 3);
        assert!(grants.list(Some(&ProjectId("proj_missing".into()))).await.is_empty());

        let id = p1_rows[0].id.clone();
        grants.remove(&id).await;
        assert_eq!(grants.list(Some(&p1)).await.len(), 1);
        grants.remove(&id).await; // idempotent
        assert_eq!(grants.list(Some(&p1)).await.len(), 1);
    }

    #[tokio::test]
    async fn permission_sql_survives_restart() {
        let (db, dir) = temp_db().await;
        let writer = store(&db);
        seed_project(&db, "proj_1").await;
        writer
            .add(&AddSavedGrants {
                project_id: ProjectId("proj_1".into()),
                action: "bash".into(),
                resources: vec!["rm *".into()],
            })
            .await;
        drop(writer);
        drop(db);

        // Reopen the same file: the grant persisted.
        let db =
            Database::open_file(dir.path().join("opencode.db")).await.expect("reopen database");
        let reopened = store(&db);
        let grants = reopened.list(Some(&ProjectId("proj_1".into()))).await;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].action, "bash");
        assert_eq!(grants[0].resource, "rm *");
        // Read-back projection keeps the allow-rule shape.
        assert_eq!(
            super::super::saved::rules(&grants),
            vec![oc_schema::permission::Rule {
                action: "bash".into(),
                resource: "rm *".into(),
                effect: oc_schema::permission::Effect::Allow,
            }]
        );
    }
}
