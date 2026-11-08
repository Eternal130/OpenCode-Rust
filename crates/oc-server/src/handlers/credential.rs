//! Credential handlers (v2 `handlers/credential.ts`): db-backed credential
//! rows, no location wrapper. List carries the per-integration `active`
//! flag (selected-last), create 409s on explicit id collisions.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::{Path, State};
use axum::response::Response;
use serde_json::{Value, json};

use super::common::{ApiError, SharedState, json_data, no_content};

/// Publishes one of v2's global credential frames
/// (`credential.updated` / `credential.switched`, credential.ts:160-316 —
/// both always `{ global: true }`). Best-effort: a failed notification
/// never fails the mutation.
async fn publish_global(state: &SharedState, type_: &str, data: Value) {
    if let Some(definition) = oc_core::bus::registry::by_type(type_) {
        let options = oc_core::bus::PublishOptions { global: true, ..Default::default() };
        let _ = state.engine.bus.publish(definition, data, options).await;
    }
}

/// v2's per-integration "active" pick: top row by
/// `desc(active), desc(time_created), desc(id)` (credential.ts:186-192).
const ACTIVE_PICK_SQL: &str = "SELECT id FROM credential WHERE integration_id = ?1 ORDER BY active DESC, time_created DESC, id DESC LIMIT 1";
/// v2's replacement pick after the active credential is removed:
/// `desc(time_created), desc(id)` (credential.ts:287-293).
const REPLACEMENT_PICK_SQL: &str = "SELECT id FROM credential WHERE integration_id = ?1 ORDER BY time_created DESC, id DESC LIMIT 1";

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/credential", axum::routing::get(list))
        .route("/api/credential", axum::routing::post(create))
        .route("/api/credential/:credentialID", axum::routing::delete(remove))
        .route("/api/credential/:credentialID", axum::routing::patch(update))
        .route("/api/credential/:credentialID/activate", axum::routing::post(activate))
}

struct Row {
    id: String,
    integration_id: Option<String>,
    label: String,
    value: String,
    time_created: i64,
    time_updated: i64,
}

async fn read_rows(state: &SharedState) -> Result<Vec<Row>, ApiError> {
    let db = state.engine.db.clone();
    db.transaction(move |tx| {
        let conn = tx.conn();
        let mut statement = conn.prepare(
            // G-268 — CredentialStore::all's ordering (credential.ts:84-91):
            // `asc(active), asc(time_created), asc(id)` — the LAST row per
            // integration is the selected one (active rows sort last;
            // legacy NULL-active libraries degrade to newest-first, still
            // last-wins).
            "SELECT id, integration_id, label, value, active, time_created, time_updated \
             FROM credential ORDER BY active ASC, time_created ASC, id ASC",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(Row {
                    id: row.get::<_, String>(0)?,
                    integration_id: row.get::<_, Option<String>>(1)?,
                    label: row.get::<_, String>(2)?,
                    value: row.get::<_, String>(3)?,
                    time_created: row.get::<_, i64>(5)?,
                    time_updated: row.get::<_, i64>(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|error| {
        crate::errors::debug_cause("credential:unknown_ref", &error);
        ApiError::unknown_ref()
    })
}

async fn list(State(state): State<SharedState>) -> Result<Response, ApiError> {
    let mut rows = read_rows(&state).await?;
    // G-268 — mirror CredentialStore::all + the handler's `entries()`
    // (handlers/credential.ts:53-63): a row WITHOUT integration_id is
    // dropped (`stored()` — credential.ts:66-72), a value that does not
    // decode FAILS the read (never a Null-valued fake row — the G-225
    // core-store semantics this handler previously bypassed), and the
    // per-integration `active` flag is the LAST row of the ordering
    // (`new Map(...)` overwrite — selected-last).
    rows.retain(|row| row.integration_id.is_some());
    for row in &rows {
        if serde_json::from_str::<serde_json::Value>(&row.value).is_err() {
            crate::errors::debug_cause(
                "credential:decode",
                &format!("row {}: value does not decode", row.id),
            );
            return Err(ApiError::unknown_ref());
        }
    }
    let mut selected_by_integration: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for row in &rows {
        if let Some(integration) = row.integration_id.as_deref() {
            selected_by_integration.insert(integration.to_owned(), row.id.clone());
        }
    }
    let data: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let active = row
                .integration_id
                .as_deref()
                .and_then(|integration| selected_by_integration.get(integration))
                .is_some_and(|selected| selected.as_str() == row.id);
            json!({
                "id": row.id,
                "integrationID": row.integration_id,
                "label": row.label,
                "value": serde_json::from_str::<Value>(&row.value)
                    .unwrap_or(Value::Null),
                "active": active,
                // G-270⑩ (recorded wire superset): the port keeps the
                // `time` block the ref Entry omits — dropping it would
                // break existing consumers.
                "time": { "created": row.time_created, "updated": row.time_updated },
            })
        })
        .collect();
    Ok(json_data(Value::Array(data)))
}

async fn create(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let id = body["id"].as_str().map(String::from);
    let integration_id = body["integrationID"].as_str().unwrap_or_default().to_owned();
    // G-268 — `label ?? "default"` (credential.ts:109): a MISSING label
    // defaults; an explicit empty string stays empty.
    let label = body["label"].as_str().map(String::from).unwrap_or_else(|| "default".to_owned());
    let value = serde_json::to_string(&body["value"]).unwrap_or_else(|_| "null".to_owned());
    let requested_activate = body["activate"].as_bool();

    let db = state.engine.db.clone();
    let label_for_insert = label.clone();
    let (id, integration_id, label, value, requested_activate) =
        (id.clone(), integration_id.clone(), label, value.clone(), requested_activate);
    let inserted = db
        .transaction(move |tx| {
            // Conflict pre-check mirroring `credential.get(id)`
            // (credential.ts:101-104 + handlers/credential.ts:20-24): a
            // row WITHOUT integration_id is invisible to `get` (stored()
            // drops it) — no conflict here, the insert below then dies on
            // the PK (500) exactly like the ref's orDie.
            if let Some(explicit) = &id {
                let existing = tx.conn().query_row(
                    "SELECT integration_id, value FROM credential WHERE id = ?1",
                    (explicit,),
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, String>(1)?,
                        ))
                    },
                );
                if let Ok((Some(_integration), value)) = existing {
                    // `get` decodes the value before answering — a row
                    // with an undecodable value dies (500), never 409.
                    if serde_json::from_str::<serde_json::Value>(&value).is_err() {
                        return Ok(None);
                    }
                    return Ok(Some(Err(explicit.clone())));
                }
            }
            // G-268 — CredentialStore::create semantics
            // (credential.ts:105-168), replicated with local SQL (the
            // G-117 decision: keep the handler's own statements so the
            // create/list wire shapes stay byte-identical):
            //   activate = `activate !== false || !current` — default
            //   ACTIVE, and a first credential is ALWAYS active (an
            //   integration without a selected credential would be
            //   unusable after login).
            let current: Option<String> =
                tx.conn().query_row(ACTIVE_PICK_SQL, (&integration_id,), |row| row.get(0)).ok();
            let activate = requested_activate != Some(false) || current.is_none();
            if activate {
                tx.execute(
                    // v2's deactivate-others writes `active: false` (0),
                    // never NULL (credential.ts:124-126) — see the
                    // activate handler for the ordering rationale.
                    "UPDATE credential SET active = 0 WHERE integration_id = ?1",
                    (integration_id.clone(),),
                )?;
            } else if let Some(current) = &current {
                // Legacy rows have no active flag and the newest one is
                // selected: pin it before inserting a newer inactive row
                // (credential.ts:128-130).
                let pinned: Option<bool> = tx
                    .conn()
                    .query_row(
                        "SELECT active FROM credential WHERE id = ?1",
                        (current,),
                        |row| row.get(0),
                    )
                    .ok();
                if pinned.is_none() {
                    tx.execute("UPDATE credential SET active = 1 WHERE id = ?1", (current,))?;
                }
            }
            let id = id.unwrap_or_else(|| format!("cred_{}", uuid::Uuid::new_v4().simple()));
            let now = oc_core::database::migrations::now_ms();
            tx.execute(
                "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                (id.clone(), integration_id, label_for_insert, value, activate, now,),
            )?;
            Ok(Some(Ok((id, now, activate))))
        })
        .await
        .map_err(|error| { crate::errors::debug_cause("credential:unknown_ref", &error); ApiError::unknown_ref() })?;

    let (id, now, activate) = match inserted {
        // The pre-check found an existing decodable row: 409 with the
        // resource (handlers/credential.ts:21-24).
        Some(Err(existing)) => {
            return Err(ApiError::conflict_resource(
                format!("Credential already exists: {existing}"),
                &existing,
            ));
        }
        // Undecodable existing value: `get` dies before the conflict
        // answer (500 UnknownError).
        None => {
            crate::errors::debug_cause("credential:decode", "existing row value does not decode");
            return Err(ApiError::unknown_ref());
        }
        Some(Ok(created)) => created,
    };
    // credential.ts:160-168 — `credential.updated` always; plus
    // `credential.switched` when the new row was created active.
    publish_global(&state, oc_core::bus::registry::CREDENTIAL_UPDATED, json!({})).await;
    if activate {
        publish_global(
            &state,
            oc_core::bus::registry::CREDENTIAL_SWITCHED,
            json!({ "integrationID": body["integrationID"], "credentialID": id }),
        )
        .await;
    }
    Ok(json_data(json!({
        "id": id,
        "integrationID": body["integrationID"],
        "label": label,
        "value": body["value"],
        // The ref's response reads `entries(list(integrationID))` for the
        // created id (handlers/credential.ts:26-27): `active` is whether
        // the row is the integration's SELECTION — exactly the activate
        // outcome above.
        "active": activate,
        "time": { "created": now, "updated": now },
    })))
}

async fn update(
    State(state): State<SharedState>,
    Path(credential_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let label = body["label"].as_str().unwrap_or_default();
    let db = state.engine.db.clone();
    let (credential_id, label) = (credential_id, label.to_owned());
    let changed = db
        .transaction(move |tx| {
            // credential.ts:236-253 — a no-op update (same label) skips the
            // notification; a missing row logs-and-returns.
            let previous: Option<String> = tx
                .conn()
                .query_row(
                    "SELECT label FROM credential WHERE id = ?1",
                    (credential_id.clone(),),
                    |row| row.get(0),
                )
                .ok();
            let changed = previous.as_deref().is_some_and(|old| old != label);
            tx.execute(
                // G-117 — ref update's `.set({ label })` writes only the
                // label column (credential.ts:227-228): `time_updated`
                // keeps its insert-time value.
                "UPDATE credential SET label = ?2 WHERE id = ?1",
                (credential_id, label),
            )?;
            Ok(changed)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("credential:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if changed {
        // credential.ts:253 — label-visible updates notify `credential.updated`.
        publish_global(&state, oc_core::bus::registry::CREDENTIAL_UPDATED, json!({})).await;
    }
    Ok(no_content())
}

async fn activate(
    State(state): State<SharedState>,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    let db = state.engine.db.clone();
    let result = db
        .transaction(move |tx| {
            let integration: std::result::Result<String, _> = tx.conn().query_row(
                "SELECT integration_id FROM credential WHERE id = ?1",
                (credential_id.clone(),),
                |row| row.get(0),
            );
            let Ok(integration) = integration else {
                return Ok(None);
            };
            // credential.ts:186-192 — already the picked-active row: no-op
            // (and no `credential.switched` event).
            let current: Option<String> =
                tx.conn().query_row(ACTIVE_PICK_SQL, (integration.clone(),), |row| row.get(0)).ok();
            if current.as_deref() == Some(credential_id.as_str()) {
                return Ok(None);
            }
            tx.execute(
                // v2 deactivates siblings with `set({ active: false })`
                // (credential.ts:124-126) — a real 0, never NULL (NULL is
                // only ever a legacy pre-column value; the census's 3/3
                // NULLs are migrated rows). Writing NULL here would skew
                // the `desc(active)` selection order v2 relies on.
                "UPDATE credential SET active = 0 WHERE integration_id = ?1",
                (integration.clone(),),
            )?;
            tx.execute(
                // G-117 — ref activate's `.set({ active: true })` writes
                // only the active column (credential.ts:187):
                // `time_updated` keeps its insert-time value.
                "UPDATE credential SET active = 1 WHERE id = ?1",
                (&credential_id,),
            )?;
            Ok(Some((integration, credential_id)))
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("credential:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if let Some((integration, id)) = result {
        // credential.ts:200-203 — activation notifies `credential.switched`.
        publish_global(
            &state,
            oc_core::bus::registry::CREDENTIAL_SWITCHED,
            json!({ "integrationID": integration, "credentialID": id }),
        )
        .await;
    }
    Ok(no_content())
}

async fn remove(
    State(state): State<SharedState>,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    let db = state.engine.db.clone();
    let credential_id = credential_id.clone();
    let removed = db
        .transaction(move |tx| {
            // credential.ts:262-301 — capture the removed row, whether it
            // was the picked-active one, and the replacement (if any) in
            // one transaction; promotion mirrors v2 exactly.
            let row: Option<(Option<String>,)> = tx
                .conn()
                .query_row(
                    "SELECT integration_id FROM credential WHERE id = ?1",
                    (credential_id.clone(),),
                    |row| Ok((row.get(0)?,)),
                )
                .ok();
            let Some((integration_id,)) = row else {
                return Ok(None);
            };
            let mut switched = None;
            if let Some(integration) = &integration_id {
                let active: Option<String> =
                    tx.conn().query_row(ACTIVE_PICK_SQL, (integration,), |row| row.get(0)).ok();
                if active.as_deref() == Some(credential_id.as_str()) {
                    tx.execute("DELETE FROM credential WHERE id = ?1", (credential_id.clone(),))?;
                    let replacement: Option<String> = tx
                        .conn()
                        .query_row(REPLACEMENT_PICK_SQL, (integration,), |row| row.get(0))
                        .ok();
                    tx.execute(
                        "UPDATE credential SET active = 0 WHERE integration_id = ?1",
                        (integration,),
                    )?;
                    if let Some(replacement) = &replacement {
                        tx.execute(
                            "UPDATE credential SET active = 1 WHERE id = ?1",
                            (replacement,),
                        )?;
                    }
                    switched = Some((integration.clone(), replacement));
                    return Ok(Some((integration_id, switched)));
                }
            }
            tx.execute("DELETE FROM credential WHERE id = ?1", (credential_id.clone(),))?;
            Ok(Some((integration_id, switched)))
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("credential:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if let Some((integration_id, switched)) = removed {
        // credential.ts:312-320 — `credential.updated` for every removal;
        // `credential.switched` (replacement id or null) when the removed
        // row was the active one.
        publish_global(&state, oc_core::bus::registry::CREDENTIAL_UPDATED, json!({})).await;
        if let Some((integration, replacement)) = switched {
            publish_global(
                &state,
                oc_core::bus::registry::CREDENTIAL_SWITCHED,
                json!({
                    "integrationID": integration,
                    "credentialID": replacement,
                }),
            )
            .await;
        }
        let _ = integration_id;
    }
    Ok(no_content())
}
