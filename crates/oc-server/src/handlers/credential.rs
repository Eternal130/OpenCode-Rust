//! Credential handlers (v2 `handlers/credential.ts`): db-backed credential
//! rows, no location wrapper. List carries the per-integration `active`
//! flag (selected-last), create 409s on explicit id collisions.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::{Path, State};
use axum::response::Response;
use serde_json::{json, Value};

use super::common::{json_data, no_content, ApiError, SharedState};

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
                    "UPDATE credential SET active = NULL WHERE integration_id = ?1",
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
    let updated = db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE credential SET label = ?2, time_updated = ?3 WHERE id = ?1",
                (credential_id, label, oc_core::database::migrations::now_ms()),
            )
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("credential:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let _ = updated;
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
                return Ok(false);
            };
            tx.execute(
                "UPDATE credential SET active = NULL WHERE integration_id = ?1",
                (integration,),
            )?;
            tx.execute(
                "UPDATE credential SET active = 1, time_updated = ?2 WHERE id = ?1",
                (credential_id.clone(), oc_core::database::migrations::now_ms()),
            )?;
            Ok(true)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("credential:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let _ = result;
    Ok(no_content())
}

async fn remove(
    State(state): State<SharedState>,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    let db = state.engine.db.clone();
    let credential_id = credential_id.clone();
    db.transaction(move |tx| {
        tx.execute("DELETE FROM credential WHERE id = ?1", (credential_id,))?;
        Ok(())
    })
    .await
    .map_err(|error| {
        crate::errors::debug_cause("credential:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    Ok(no_content())
}
