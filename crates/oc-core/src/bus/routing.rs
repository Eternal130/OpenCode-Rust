//! Location routing — port of `prepareRoutes` (`bus.ts:212-261`).
//!
//! Computes, for a batch of committed events, (a) the per-session location
//! updates for the in-memory `sessions` map and (b) each event's
//! `Location.Ref` list used by location-scoped subscriptions. **Updates are
//! computed but not applied**: the caller applies them only after the
//! projection transaction commits ("A failed move must not redirect events
//! away from the Session's actual location", bus.ts:252-253).
//!
//! Rules (`core-deep-map.md` §2c):
//!
//! - `session.created` → routes to its own location (envelope location, else
//!   the payload's), and records it in the map.
//! - `session.moved` → routes to **both** the previous owner location and the
//!   new one; the map is repointed to the destination.
//! - `session.deleted` → clears the map entry.
//! - Other unlocated session events → resolve through the in-memory map with
//!   a `session_v2` database fallback (cached, including negatives).
//! - Events carrying an envelope location (except forked/moved) route via the
//!   envelope; the map is untouched.
//!
//! A route list of `Some(vec![])` means "routing decided: this event belongs
//! to no location" (delivered to nobody location-scoped); `None` means "no
//! routing information" (the subscription filter falls back to the event's
//! own `location` field).

use std::collections::HashMap;
use std::sync::Arc;

use oc_schema::common::AbsolutePath;
use oc_schema::id::WorkspaceId;
use oc_schema::location;
use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::DurableEvent;
use crate::database::error::{DbError, Result};

/// The shared in-memory `SessionID → Location.Ref` routing map.
pub type SessionRoutes = parking_lot::RwLock<HashMap<String, location::Ref>>;

/// Computed-but-not-applied routing for one publish batch.
pub(crate) struct Routing {
    /// Session map updates (`None` removes the entry).
    updates: HashMap<String, Option<location::Ref>>,
    /// Per input event (aligned by index): the resolved route list.
    routes: Vec<Option<Arc<Vec<location::Ref>>>>,
}

impl Routing {
    /// Applies the session-map updates and returns the per-event route lists.
    pub(crate) fn apply(self, sessions: &SessionRoutes) -> Vec<Option<Arc<Vec<location::Ref>>>> {
        let mut map = sessions.write();
        for (id, ref_) in self.updates {
            match ref_ {
                Some(ref_) => {
                    map.insert(id, ref_);
                }
                None => {
                    map.remove(&id);
                }
            }
        }
        self.routes
    }
}

/// Reads a `Location.Ref` from an event payload field.
fn payload_ref(data: &serde_json::Value, field: &str) -> Option<location::Ref> {
    data.get(field).and_then(|value| serde_json::from_value(value.clone()).ok())
}

/// `session_v2` fallback (`bus.ts:229-240`): directory + workspace of the
/// session currently owning `owner`.
fn session_row_ref(conn: &Connection, owner: &str) -> Result<Option<location::Ref>> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT directory, workspace_id FROM session_v2 WHERE id = ?1",
            rusqlite::params![owner],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(DbError::from)?;
    Ok(row.map(|(directory, workspace_id)| location::Ref {
        directory: AbsolutePath::parse(directory),
        // Malformed workspace ids fall back to "no workspace" rather than
        // failing the publish (v2 passes the column through unvalidated).
        workspace_id: workspace_id.and_then(|id| WorkspaceId::parse(id).ok()),
    }))
}

/// The `prepareRoutes` port. `conn` is the transaction's connection for the
/// `session_v2` fallback (ephemeral publishers pass a plain read handle).
pub(crate) fn prepare(
    sessions: &SessionRoutes,
    conn: &Connection,
    events: &[DurableEvent],
) -> Result<Routing> {
    let mut updates: HashMap<String, Option<location::Ref>> = HashMap::new();
    let mut routes = Vec::with_capacity(events.len());

    for event in events {
        if !super::registry::is_session_event(&event.type_) {
            routes.push(None);
            continue;
        }
        let Some(id) =
            event.data.get("sessionID").and_then(serde_json::Value::as_str).map(str::to_string)
        else {
            routes.push(None);
            continue;
        };
        if event.type_ == "session.created" {
            let data_location = payload_ref(&event.data, "location");
            updates.insert(id, data_location.clone());
            let route =
                event.location.clone().or(data_location).map(|r| vec![r]).unwrap_or_default();
            routes.push(Some(Arc::new(route)));
            continue;
        }
        if event.location.is_some()
            && event.type_ != "session.forked"
            && event.type_ != "session.moved"
        {
            if event.type_ == "session.deleted" {
                updates.insert(id, None);
            }
            routes.push(None);
            continue;
        }
        let owner = if event.type_ == "session.forked" {
            event.data.get("parentID").and_then(serde_json::Value::as_str).unwrap_or(id.as_str())
        } else {
            id.as_str()
        };
        let mut owner_ref: Option<location::Ref>;
        if let Some(cached) = updates.get(owner) {
            owner_ref = cached.clone();
        } else {
            owner_ref = sessions.read().get(owner).cloned();
            if owner_ref.is_none() {
                owner_ref = session_row_ref(conn, owner)?;
                updates.insert(owner.to_string(), owner_ref.clone());
            }
        }
        if event.type_ == "session.moved" {
            // Both owners need the transition, even if the producer supplied
            // an envelope location. Later events use only the destination.
            let destination = payload_ref(&event.data, "location");
            updates.insert(id.clone(), destination.clone());
            let route = match (owner_ref, destination) {
                (Some(previous), Some(destination)) => vec![previous, destination],
                (Some(previous), None) => vec![previous],
                (None, Some(destination)) => vec![destination],
                (None, None) => Vec::new(),
            };
            routes.push(Some(Arc::new(route)));
            continue;
        }
        if event.type_ == "session.forked" {
            updates.insert(id.clone(), owner_ref.clone());
        }
        let route = match &event.location {
            Some(location) => Some(vec![location.clone()]),
            None => Some(owner_ref.map(|r| vec![r]).unwrap_or_default()),
        };
        routes.push(route.map(Arc::new));
        if event.type_ == "session.deleted" {
            updates.insert(id, None);
        }
    }

    Ok(Routing { updates, routes })
}
