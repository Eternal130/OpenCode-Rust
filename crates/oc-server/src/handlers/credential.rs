//! Credential handlers (v2 `handlers/credential.ts`): db-backed credential
//! rows, no location wrapper. List carries the per-integration `active`
//! flag (selected-last), create 409s on explicit id collisions.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::{Path, State};
use axum::response::Response;
use serde_json::{json, Value};

use super::common::{json_data, no_content, ApiError, SharedState};

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
const ACTIVE_PICK_SQL: &str =
    "SELECT id FROM credential WHERE integration_id = ?1 ORDER BY active DESC, time_created DESC, id DESC LIMIT 1";
/// v2's replacement pick after the active credential is removed:
/// `desc(time_created), desc(id)` (credential.ts:287-293).
const REPLACEMENT_PICK_SQL: &str =
    "SELECT id FROM credential WHERE integration_id = ?1 ORDER BY time_created DESC, id DESC LIMIT 1";

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
    active: Option<bool>,
    time_created: i64,
    time_updated: i64,
}

async fn read_rows(state: &SharedState) -> Result<Vec<Row>, ApiError> {
    let db = state.engine.db.clone();
    db.transaction(move |tx| {
        let conn = tx.conn();
        let mut statement = conn.prepare(
            "SELECT id, integration_id, label, value, active, time_created, time_updated \
             FROM credential ORDER BY integration_id, time_created",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(Row {
                    id: row.get::<_, String>(0)?,
                    integration_id: row.get::<_, Option<String>>(1)?,
                    label: row.get::<_, String>(2)?,
                    value: row.get::<_, String>(3)?,
                    active: row.get::<_, Option<bool>>(4)?,
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
    let rows = read_rows(&state).await?;
    // Active flag: per-integration selected-last (§3) — the LAST row of an
    // integration with `active` set wins.
    let mut active_by_integration: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for row in &rows {
        if row.active == Some(true) {
            if let Some(integration) = &row.integration_id {
                active_by_integration.insert(integration.clone(), row.id.clone());
            }
        }
    }
    let data: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let active = row
                .integration_id
                .as_ref()
                .and_then(|integration| active_by_integration.get(integration))
                .is_some_and(|active_id| *active_id == row.id);
            json!({
                "id": row.id,
                "integrationID": row.integration_id,
                "label": row.label,
                "value": serde_json::from_str::<Value>(&row.value)
                    .unwrap_or(Value::Null),
                "active": active,
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
    let label = body["label"].as_str().unwrap_or_default().to_owned();
    let value = serde_json::to_string(&body["value"]).unwrap_or_else(|_| "null".to_owned());
    let activate = body["activate"].as_bool().unwrap_or(false);

    let db = state.engine.db.clone();
    let (id, integration_id, label, value, activate) =
        (id.clone(), integration_id.clone(), label.clone(), value.clone(), activate);
    let inserted = db
        .transaction(move |tx| {
            if let Some(explicit) = &id {
                let existing: std::result::Result<String, _> =
                    tx.conn().query_row("SELECT id FROM credential WHERE id = ?1", (explicit,), |row| {
                        row.get(0)
                    });
                if existing.is_ok() {
                    return Ok(None);
                }
            }
            let id = id.unwrap_or_else(|| format!("cred_{}", uuid::Uuid::new_v4().simple()));
            let now = oc_core::database::migrations::now_ms();
            if activate {
                tx.execute(
                    // v2's deactivate-others writes `active: false` (0),
                    // never NULL (credential.ts:124-126) — see the
                    // activate handler for the ordering rationale.
                    "UPDATE credential SET active = 0 WHERE integration_id = ?1",
                    (integration_id.clone(),),
                )?;
            }
            tx.execute(
                "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                (id.clone(), integration_id, label, value, activate, now,),
            )?;
            Ok(Some((id, now)))
        })
        .await
        .map_err(|error| { crate::errors::debug_cause("credential:unknown_ref", &error); ApiError::unknown_ref() })?;

    let Some((id, now)) = inserted else {
        let id = body["id"].as_str().unwrap_or_default();
        return Err(ApiError::conflict(format!("Credential already exists: {id}")));
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
        "label": body["label"].as_str().unwrap_or_default(),
        "value": body["value"],
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
                "UPDATE credential SET label = ?2, time_updated = ?3 WHERE id = ?1",
                (credential_id, label, oc_core::database::migrations::now_ms()),
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
                "UPDATE credential SET active = 1, time_updated = ?2 WHERE id = ?1",
                (credential_id.clone(), oc_core::database::migrations::now_ms()),
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
