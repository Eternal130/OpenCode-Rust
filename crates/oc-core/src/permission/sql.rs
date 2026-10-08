//! Permission table persistence — stub.
//!
//! The production `SavedGrantStore` lives in the `permission` sqlite table
//! (created by the schema bootstrap / migrations owned by the database lane):
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
//! TODO(database-lane): implement `SqliteSavedGrantStore` here on top of the
//! oc-core database handle — `add` inserts with conflict-do-nothing, `list`
//! filters by `project_id`, `remove` deletes by id, and `time_updated` bumps
//! on re-add. Evaluation must read rows back as `{action, resource, effect:
//! "allow"}` rules (see [`super::saved::rules`]).
