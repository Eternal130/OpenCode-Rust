//! The credential store service (v2 `packages/core/src/credential.ts`):
//! db-backed rows over the shared `credential` table + the global
//! `credential.updated` / `credential.switched` notifications. The HTTP
//! credential handlers (oc-server) and the integration host share this
//! table; the host routes its persistence through THIS type so the OAuth /
//! command / key flows emit the same events with the same selection rules.

use super::types::{CredentialValue, StoredCredential};

use crate::bus::{registry, Bus, PublishOptions};
use crate::database::Database;
use serde_json::json;
use std::sync::Arc;

/// v2's per-integration picked-active order (credential.ts:186-192):
/// `desc(active), desc(time_created), desc(id)`.
const ACTIVE_PICK_SQL: &str =
    "SELECT id FROM credential WHERE integration_id = ?1 ORDER BY active DESC, time_created DESC, id DESC LIMIT 1";
/// v2's replacement pick after the active credential is removed
/// (credential.ts:287-293): `desc(time_created), desc(id)`.
const REPLACEMENT_PICK_SQL: &str =
    "SELECT id FROM credential WHERE integration_id = ?1 ORDER BY time_created DESC, id DESC LIMIT 1";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("credential value does not decode: {0}")]
    Decode(String),
}

pub struct CredentialStore {
    db: Arc<Database>,
    bus: Arc<Bus>,
}

impl CredentialStore {
    #[must_use]
    pub fn new(db: Arc<Database>, bus: Arc<Bus>) -> Self {
        Self { db, bus }
    }

    async fn publish(&self, type_: &str, data: serde_json::Value) {
        if let Some(definition) = registry::by_type(type_) {
            let _ = self
                .bus
                .publish(definition, data, PublishOptions { global: true, ..Default::default() })
                .await;
        }
    }

    /// Every stored credential ordered `asc(active), asc(created), asc(id)`
    /// (credential.ts:84-91).
    pub async fn all(&self) -> Result<Vec<StoredCredential>, StoreError> {
        let db = self.db.clone();
        db.transaction(move |tx| {
            let mut statement = tx.conn().prepare(
                "SELECT id, integration_id, label, value, active FROM credential \
                 ORDER BY active ASC, time_created ASC, id ASC",
            )?;
            let rows = statement
                .query_map([], raw_credential_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| StoreError::Database(error.to_string()))
        .and_then(stored_credentials)
    }

    /// Stored credentials belonging to one integration (credential.ts:92-100).
    pub async fn list(&self, integration_id: &str) -> Result<Vec<StoredCredential>, StoreError> {
        let integration_id = integration_id.to_owned();
        let db = self.db.clone();
        db.transaction(move |tx| {
            let mut statement = tx.conn().prepare(
                "SELECT id, integration_id, label, value, active FROM credential \
                 WHERE integration_id = ?1 ORDER BY active ASC, time_created ASC, id ASC",
            )?;
            let rows = statement
                .query_map([&integration_id], raw_credential_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| StoreError::Database(error.to_string()))
        .and_then(stored_credentials)
    }

    /// One credential by id (credential.ts:101-104).
    pub async fn get(&self, id: &str) -> Result<Option<StoredCredential>, StoreError> {
        let id = id.to_owned();
        let db = self.db.clone();
        db.transaction(move |tx| {
            let row = tx.conn().query_row(
                "SELECT id, integration_id, label, value, active FROM credential WHERE id = ?1",
                [&id],
                raw_credential_row,
            );
            match row {
                Ok(raw) => Ok(Some(raw)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
        .map_err(|error| StoreError::Database(error.to_string()))
        .and_then(|row| stored_credentials(row.into_iter().collect()).map(|mut rows| rows.pop()))
    }

    /// Creates a credential; the new row becomes the integration's
    /// selection unless `activate == Some(false)` and the integration
    /// already has one (credential.ts:105-168). Returns whether it
    /// activated.
    pub async fn create(
        &self,
        input: CreateInput<'_>,
    ) -> Result<(StoredCredential, bool), StoreError> {
        let id = input.id.map_or_else(|| format!("cred_{}", uuid_v7ish()), String::from);
        let integration_id = input.integration_id.to_owned();
        let label = input.label.map_or_else(|| "default".to_owned(), String::from);
        let value = serde_json::to_string(&input.value)
            .map_err(|error| StoreError::Decode(error.to_string()))?;
        let db = self.db.clone();
        let insert =
            (integration_id.clone(), label.clone(), value.clone(), id.clone(), input.activate);
        let activate = db
            .transaction(move |tx| {
                let (integration_id, label, value, id, input_activate) = insert;
                let current: Option<String> =
                    tx.conn().query_row(ACTIVE_PICK_SQL, (&integration_id,), |row| row.get(0)).ok();
                let activate = input_activate != Some(false) || current.is_none();
                if activate {
                    tx.execute("UPDATE credential SET active = 0 WHERE integration_id = ?1", (&integration_id,))?;
                } else if let Some(current) = current.filter(|_| input_activate == Some(false)) {
                    // Legacy rows carry NULL active; pin the current row
                    // before inserting a newer inactive one (credential.ts:128-130).
                    let pinned: Option<bool> =
                        tx.conn().query_row("SELECT active FROM credential WHERE id = ?1", [&current], |row| row.get(0)).ok();
                    if pinned.is_none() {
                        tx.execute("UPDATE credential SET active = 1 WHERE id = ?1", [&current])?;
                    }
                }
                let now = crate::database::migrations::now_ms();
                tx.execute(
                    "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    (&id, &integration_id, &label, &value, activate, now),
                )?;
                Ok(activate)
            })
            .await
            .map_err(|error| StoreError::Database(error.to_string()))?;
        let stored = StoredCredential {
            id,
            integration_id,
            label,
            value: input.value.clone(),
            active: activate,
        };
        // credential.ts:160-166 — `credential.updated` always; `credential.switched`
        // when the row was created active.
        self.publish(registry::CREDENTIAL_UPDATED, json!({})).await;
        if activate {
            self.publish(
                registry::CREDENTIAL_SWITCHED,
                json!({ "integrationID": stored.integration_id, "credentialID": stored.id }),
            )
            .await;
        }
        Ok((stored, activate))
    }

    /// Selects a stored credential for its integration (credential.ts:169-204).
    pub async fn activate(&self, id: &str) -> Result<(), StoreError> {
        let id = id.to_owned();
        let db = self.db.clone();
        let moved = id.clone();
        let switched = db
            .transaction(move |tx| {
                let id = moved;
                let integration: Option<String> = tx
                    .conn()
                    .query_row(
                        "SELECT integration_id FROM credential WHERE id = ?1",
                        [&id],
                        |row| row.get(0),
                    )
                    .ok();
                let Some(integration) = integration else { return Ok(None) };
                let active: Option<String> =
                    tx.conn().query_row(ACTIVE_PICK_SQL, (&integration,), |row| row.get(0)).ok();
                if active.as_deref() == Some(id.as_str()) {
                    return Ok(None);
                }
                tx.execute(
                    "UPDATE credential SET active = 0 WHERE integration_id = ?1",
                    (&integration,),
                )?;
                tx.execute(
                    // G-117 — ref activate's `.set({ active: true })` writes
                    // ONLY active (credential.ts:187): `time_updated` keeps
                    // its insert-time value.
                    "UPDATE credential SET active = 1 WHERE id = ?1",
                    [&id],
                )?;
                Ok(Some(integration))
            })
            .await
            .map_err(|error| StoreError::Database(error.to_string()))?;
        if let Some(integration) = switched {
            self.publish(
                registry::CREDENTIAL_SWITCHED,
                json!({ "integrationID": integration, "credentialID": id }),
            )
            .await;
        }
        Ok(())
    }

    /// Updates the label and/or value of a stored credential
    /// (credential.ts:205-254). `credential.updated` only on label changes.
    pub async fn update(
        &self,
        id: &str,
        label: Option<&str>,
        value: Option<&CredentialValue>,
    ) -> Result<(), StoreError> {
        let id = id.to_owned();
        let label = label.map(String::from);
        let value = value.map(|value| {
            serde_json::to_string(value).map_err(|error| StoreError::Decode(error.to_string()))
        });
        let value = value.transpose()?;
        let db = self.db.clone();
        let changed = db
            .transaction(move |tx| {
                let previous: Option<String> = tx
                    .conn()
                    .query_row("SELECT label FROM credential WHERE id = ?1", [&id], |row| row.get(0))
                    .ok();
                let Some(previous) = previous else { return Ok(None) };
                if label.is_none() && value.is_none() {
                    return Ok(Some(false));
                }
                let label_changed = label.as_deref().is_some_and(|next| next != previous);
                // G-117 — ref update's `.set({ label, value })` writes only
                // the listed columns (credential.ts:227-228): `time_updated`
                // is never bumped here; it keeps its insert-time value.
                match (&label, &value) {
                    (Some(label), Some(value)) => {
                        tx.execute(
                            "UPDATE credential SET label = ?2, value = ?3 WHERE id = ?1",
                            (&id, label, value),
                        )?;
                    }
                    (Some(label), None) => {
                        tx.execute("UPDATE credential SET label = ?2 WHERE id = ?1", (&id, label))?;
                    }
                    (None, Some(value)) => {
                        tx.execute("UPDATE credential SET value = ?2 WHERE id = ?1", (&id, value))?;
                    }
                    (None, None) => {}
                }
                Ok(Some(label_changed))
            })
            .await
            .map_err(|error| StoreError::Database(error.to_string()))?;
        if changed == Some(true) {
            self.publish(registry::CREDENTIAL_UPDATED, json!({})).await;
        }
        Ok(())
    }

    /// Removes a stored credential, promoting the newest remaining one
    /// (credential.ts:255-320).
    pub async fn remove(&self, id: &str) -> Result<(), StoreError> {
        let id = id.to_owned();
        let db = self.db.clone();
        let removed = db
            .transaction(move |tx| {
                let integration: Option<Option<String>> = tx
                    .conn()
                    .query_row(
                        "SELECT integration_id FROM credential WHERE id = ?1",
                        [&id],
                        |row| Ok(Some(row.get::<_, Option<String>>(0)?)),
                    )
                    .ok()
                    .flatten();
                let Some(integration_id) = integration else { return Ok(None) };
                let Some(integration) = integration_id else { return Ok(Some((None, None))) };
                let active: Option<String> =
                    tx.conn().query_row(ACTIVE_PICK_SQL, (&integration,), |row| row.get(0)).ok();
                tx.execute("DELETE FROM credential WHERE id = ?1", [&id])?;
                if active.as_deref() != Some(id.as_str()) {
                    // credential.ts:268-269 — not the picked-active row:
                    // `{switched:false}` → only `credential.updated`.
                    return Ok(Some((Some(integration), None)));
                }
                let replacement: Option<String> = tx
                    .conn()
                    .query_row(REPLACEMENT_PICK_SQL, (&integration,), |row| row.get(0))
                    .ok();
                tx.execute(
                    "UPDATE credential SET active = 0 WHERE integration_id = ?1",
                    (&integration,),
                )?;
                if let Some(replacement) = &replacement {
                    tx.execute("UPDATE credential SET active = 1 WHERE id = ?1", [replacement])?;
                }
                // Active row removed → switched, replacement id or null.
                Ok(Some((Some(integration), Some(replacement))))
            })
            .await
            .map_err(|error| StoreError::Database(error.to_string()))?;
        if let Some((integration, switched)) = removed {
            self.publish(registry::CREDENTIAL_UPDATED, json!({})).await;
            // credential.ts:314-319 — `credential.switched` only when the
            // removed credential WAS active (`{switched:true}`).
            if let (Some(integration), Some(replacement)) = (integration, switched) {
                self.publish(
                    registry::CREDENTIAL_SWITCHED,
                    json!({ "integrationID": integration, "credentialID": replacement }),
                )
                .await;
            }
        }
        Ok(())
    }
}

/// G-225 — `decode(row.value)` (credential.ts:63,67): the ref decodes
/// with `Schema.decodeUnknownSync(Value)`, which THROWS on a corrupt
/// value; `stored()` has no error channel (credential.ts:66-72), so
/// `all`/`list`/`get` FAIL the whole read — a corrupted credential is
/// never silently degraded into a fake empty-key value. Mirrored here:
/// the read errors with [`StoreError::Decode`] (plus a stderr line —
/// oc-core's logging seam, cf. the MCP stdio fallback in mcp/client.rs).
fn decode_value(value_text: Option<&str>, id: &str) -> Result<CredentialValue, StoreError> {
    value_text
        .and_then(|text| serde_json::from_str::<CredentialValue>(text).ok())
        .ok_or_else(|| {
            eprintln!("[credential] row {id}: value does not decode; failing the read");
            StoreError::Decode(format!("credential {id}"))
        })
}

/// One raw `credential` table row (id, integration_id, label, value,
/// active) — decoding happens outside the rusqlite closure so a decode
/// failure can carry [`StoreError::Decode`] (G-225).
type RawCredentialRow = (String, Option<String>, String, Option<String>, Option<bool>);

fn raw_credential_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawCredentialRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

/// `storedRows` (credential.ts:66-78): a row WITHOUT integration_id is
/// skipped; a value that does not decode FAILS the read (never a fake
/// empty-key credential — G-225).
fn stored_credentials(
    rows: Vec<RawCredentialRow>,
) -> Result<Vec<StoredCredential>, StoreError> {
    rows.into_iter()
        .filter(|(_, integration_id, _, _, _)| integration_id.is_some())
        .map(|(id, integration_id, label, value_text, active)| {
            let value = decode_value(value_text.as_deref(), &id)?;
            Ok(StoredCredential {
                id,
                integration_id: integration_id.unwrap_or_default(),
                label,
                value,
                active: active.unwrap_or(false),
            })
        })
        .collect()
}

/// Input for [`CredentialStore::create`].
pub struct CreateInput<'a> {
    pub id: Option<&'a str>,
    pub integration_id: &'a str,
    pub label: Option<&'a str>,
    pub value: &'a CredentialValue,
    pub activate: Option<bool>,
}

/// Monotonic-ish unique credential id (v2: `cred_` + ascending; random hex
/// keeps uniqueness without a process counter).
fn uuid_v7ish() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut text = String::with_capacity(24);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}
