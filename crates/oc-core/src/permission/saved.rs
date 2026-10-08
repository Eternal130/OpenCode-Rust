//! Port of `packages/core/src/permission/saved.ts` (`PermissionSaved`) —
//! persisted "always" grants, keyed by project id, read back as allow rules
//! appended after the base ruleset.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use oc_schema::id::{PermissionSavedId, ProjectId};
use oc_schema::permission::{Effect, Rule, Ruleset};
use oc_util::determinism::{Clock, IdGen};
use parking_lot::Mutex;

/// `PermissionSaved.Info` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedGrant {
    pub id: PermissionSavedId,
    pub project_id: ProjectId,
    pub action: String,
    pub resource: String,
    /// Epoch milliseconds.
    pub time_created: u64,
    pub time_updated: u64,
}

/// `PermissionSaved.AddInput` — one action over many resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddSavedGrants {
    pub project_id: ProjectId,
    pub action: String,
    pub resources: Vec<String>,
}

/// `PermissionSaved` service seam. The production store is the `permission`
/// sqlite table (unique on `(project_id, action, resource)`, insert
/// conflict-do-nothing) — see [`super::sql`].
///
/// TODO(database-lane): implement the sqlite-backed store once the database
/// lane lands; wire it here as `SqliteSavedGrantStore`.
#[async_trait]
pub trait SavedGrantStore: Send + Sync {
    /// Rows, optionally filtered by project.
    async fn list(&self, project_id: Option<&ProjectId>) -> Vec<SavedGrant>;

    /// Persist grants; existing `(project_id, action, resource)` rows are
    /// kept (`onConflictDoNothing`). Empty `resources` is a no-op.
    async fn add(&self, input: &AddSavedGrants);

    /// Delete by row id.
    async fn remove(&self, id: &PermissionSavedId);
}

/// Read-back projection: grants become trailing **allow** rules
/// (`{action, resource, effect: "allow"}`).
pub fn rules(grants: &[SavedGrant]) -> Ruleset {
    grants
        .iter()
        .map(|grant| Rule {
            action: grant.action.clone(),
            resource: grant.resource.clone(),
            effect: Effect::Allow,
        })
        .collect()
}

/// In-memory [`SavedGrantStore`] keyed by project id with unique
/// `{project_id, action, resource}` rows (insert-order preserved).
pub struct MemorySavedGrantStore {
    ids: Arc<dyn IdGen>,
    clock: Arc<dyn Clock>,
    rows: Mutex<BTreeMap<(ProjectId, String, String), SavedGrant>>,
}

impl MemorySavedGrantStore {
    pub fn new(ids: Arc<dyn IdGen>, clock: Arc<dyn Clock>) -> Self {
        Self { ids, clock, rows: Mutex::new(BTreeMap::new()) }
    }
}

#[async_trait]
impl SavedGrantStore for MemorySavedGrantStore {
    async fn list(&self, project_id: Option<&ProjectId>) -> Vec<SavedGrant> {
        let rows = self.rows.lock();
        rows.values()
            .filter(|grant| project_id.is_none_or(|id| grant.project_id == *id))
            .cloned()
            .collect()
    }

    async fn add(&self, input: &AddSavedGrants) {
        if input.resources.is_empty() {
            return;
        }
        let now = u64::try_from(self.clock.unix_ms()).unwrap_or(0);
        let mut rows = self.rows.lock();
        for resource in &input.resources {
            let key = (input.project_id.clone(), input.action.clone(), resource.clone());
            rows.entry(key).or_insert_with(|| SavedGrant {
                id: PermissionSavedId(self.ids.new_id("psv")),
                project_id: input.project_id.clone(),
                action: input.action.clone(),
                resource: resource.clone(),
                time_created: now,
                time_updated: now,
            });
        }
    }

    async fn remove(&self, id: &PermissionSavedId) {
        self.rows.lock().retain(|_, grant| grant.id != *id);
    }
}
