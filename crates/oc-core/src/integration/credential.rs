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
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let integration_id: Option<String> = row.get(1)?;
                    let label: String = row.get(2)?;
                    let value_text: Option<String> = row.get(3)?;
                    let active: Option<bool> = row.get(4)?;
                    let value = decode_value(value_text.as_deref());
                    Ok(StoredCredential {
                        id,
                        integration_id: integration_id.unwrap_or_default(),
                        label,
                        value,
                        active: active.unwrap_or(false),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| StoreError::Database(error.to_string()))
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
                .query_map([&integration_id], |row| {
                    let id: String = row.get(0)?;
                    let label: String = row.get(2)?;
                    let value_text: Option<String> = row.get(3)?;
                    let active: Option<bool> = row.get(4)?;
                    let value = decode_value(value_text.as_deref());
                    Ok(StoredCredential {
                        id,
                        integration_id: integration_id.clone(),
                        label,
                        value,
                        active: active.unwrap_or(false),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| StoreError::Database(error.to_string()))
    }

    /// One credential by id (credential.ts:101-104).
    pub async fn get(&self, id: &str) -> Result<Option<StoredCredential>, StoreError> {
        let id = id.to_owned();
        let db = self.db.clone();
        db.transaction(move |tx| {
            let row = tx.conn().query_row(
                "SELECT id, integration_id, label, value, active FROM credential WHERE id = ?1",
                [&id],
                |row| {
                    let id: String = row.get(0)?;
                    let integration_id: Option<String> = row.get(1)?;
                    let label: String = row.get(2)?;
                    let value_text: Option<String> = row.get(3)?;
                    let active: Option<bool> = row.get(4)?;
                    let value = decode_value(value_text.as_deref());
                    Ok(StoredCredential {
                        id,
                        integration_id: integration_id.unwrap_or_default(),
                        label,
                        value,
                        active: active.unwrap_or(false),
                    })
                },
            );
            match row {
                Ok(stored) => Ok(Some(stored)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
        .map_err(|error| StoreError::Database(error.to_string()))
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
                    "UPDATE credential SET active = 1, time_updated = ?2 WHERE id = ?1",
                    (&id, crate::database::migrations::now_ms()),
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
                match (&label, &value) {
                    (Some(label), Some(value)) => {
                        tx.execute(
                            "UPDATE credential SET label = ?2, value = ?3, time_updated = ?4 WHERE id = ?1",
                            (&id, label, value, crate::database::migrations::now_ms()),
                        )?;
                    }
                    (Some(label), None) => {
                        tx.execute(
                            "UPDATE credential SET label = ?2, time_updated = ?3 WHERE id = ?1",
                            (&id, label, crate::database::migrations::now_ms()),
                        )?;
                    }
                    (None, Some(value)) => {
                        tx.execute(
                            "UPDATE credential SET value = ?2, time_updated = ?3 WHERE id = ?1",
                            (&id, value, crate::database::migrations::now_ms()),
                        )?;
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
                Ok(Some((Some(integration), replacement)))
            })
            .await
            .map_err(|error| StoreError::Database(error.to_string()))?;
        if let Some((integration, replacement)) = removed {
            self.publish(registry::CREDENTIAL_UPDATED, json!({})).await;
            if let Some(integration) = integration {
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

fn decode_value(value_text: Option<&str>) -> CredentialValue {
    value_text.and_then(|text| serde_json::from_str::<CredentialValue>(text).ok()).unwrap_or(
        CredentialValue::Key(super::types::KeyValue {
            key: String::new(),
            metadata: None,
            configuration: None,
        }),
    )
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
