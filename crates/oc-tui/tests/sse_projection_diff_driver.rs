//! SSE store-projection differential driver (diff channel 3).
//!
//! Env-gated no-op unless `SSE_PROJECTION_CORPUS` is set: for every
//! corpus sequence (`tools/diff/corpus/sse_projection/*.json`), a fresh
//! [`Store`] is driven event-by-event through [`Ingest::Event`] (the
//! production single-entry reducer path), and after EVERY event the
//! session's message tree (`Store::messages`) is snapshotted and
//! normalized to the channel's shared row schema — mirroring the ref
//! bun driver (`tools/diff/drivers/sse_ref.ts`, ref
//! `packages/client/src/solid/data.ts` `createData`).
//!
//! Declared normalizations (identical on both sides, see the report):
//! message ids + timestamps stripped (different derivations);
//! assistant model → `providerID/id`; tool streaming-input accumulators
//! masked (`<streaming-input>` — the port keeps no streaming reducer by
//! design); tool metadata compared as raw JSON; tokens/usage excluded.
//!
//! The test only FAILS on driver errors — divergence verdicts live in
//! the python orchestrator's report (`tools/diff/sse_projection.py`).

use std::path::PathBuf;

use oc_tui::components::message::model::{Message, Part};
use oc_tui::data::{BusEvent, Ingest};
use oc_tui::reactive::Runtime;
use oc_tui::store::Store;
use serde_json::{json, Map, Value};

fn norm_tool_state_input(
    status: &str,
    input: &std::collections::BTreeMap<String, String>,
) -> Value {
    if status == "streaming" {
        return json!("<streaming-input>");
    }
    let mut out = Map::new();
    for (key, value) in input {
        out.insert(key.clone(), Value::String(value.clone()));
    }
    Value::Object(out)
}

fn norm_part(part: &Part) -> Value {
    match part {
        Part::Text { text, .. } => json!({ "type": "text", "text": text }),
        Part::Reasoning { text, time } => json!({
            "type": "reasoning",
            "text": text,
            "completed": time.completed.is_some(),
        }),
        Part::Tool { id, name, state, .. } => {
            let status = state.status.as_str();
            // G-352/progress path: the live `session.tool.progress` reducer
            // writes the STRINGIFIED map (`state.metadata`), while the
            // settled reducers keep the RAW JSON (`state.metadata_json`) —
            // mirror both into the compared metadata object.
            let metadata = state.metadata_json.clone().unwrap_or_else(|| {
                let mut map = Map::new();
                for (key, value) in &state.metadata {
                    map.insert(key.clone(), Value::String(value.clone()));
                }
                Value::Object(map)
            });
            json!({
                "type": "tool",
                "id": id,
                "name": name,
                "status": status,
                "input": norm_tool_state_input(status, &state.input),
                "metadata": metadata,
                "output": state.output,
                "error": state.error,
            })
        }
    }
}

fn norm_row(message: &Message) -> Value {
    match message {
        Message::Assistant { agent, model, parts, finish, time, .. } => {
            let model = if model.is_empty() { Value::Null } else { Value::String(model.clone()) };
            json!({
                "row": "assistant",
                "agent": agent,
                "model": model,
                "finish": finish,
                "open": time.completed.is_none(),
                "parts": parts.iter().map(norm_part).collect::<Vec<_>>(),
            })
        }
        Message::AgentSwitched { agent, previous, .. } => {
            json!({ "row": "agent-switched", "agent": agent, "previous": previous })
        }
        Message::ModelSwitched { provider_id, model_id, .. } => json!({
            "row": "model-switched",
            "model": if provider_id.is_empty() || model_id.is_empty() {
                Value::Null
            } else {
                Value::String(format!("{provider_id}/{model_id}"))
            },
        }),
        Message::LocationSwitched { directory, .. } => {
            json!({ "row": "location-switched", "directory": directory })
        }
        Message::System { description, .. } => {
            json!({ "row": "system", "description": description })
        }
        Message::Synthetic { description, .. } => {
            json!({ "row": "synthetic", "description": description })
        }
        Message::Shell { shell_id, command, status, exit, output, .. } => json!({
            "row": "shell",
            "shellID": shell_id,
            "command": command,
            "status": status,
            "exit": exit,
            "output": output,
        }),
        other => json!({ "row": other.id() }),
    }
}

#[test]
fn sse_projection_diff_driver() {
    let Ok(corpus) = std::env::var("SSE_PROJECTION_CORPUS") else { return };
    if corpus.is_empty() {
        return;
    }
    let out = std::env::var("SSE_PROJECTION_OUT").expect("SSE_PROJECTION_OUT with corpus");
    // cargo/nextest run integration tests with the PACKAGE root as cwd —
    // resolve a repo-root-relative corpus against the workspace root.
    let corpus = PathBuf::from(&corpus);
    let corpus = if corpus.is_dir() {
        corpus
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .join(&corpus)
    };

    let mut names: Vec<String> = std::fs::read_dir(&corpus)
        .expect("corpus dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().and_then(|e| e.to_str())? == "json")
                .then(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned())
        })
        .collect();
    names.sort();

    let mut results: Vec<Value> = Vec::new();
    for name in names {
        let sequence: Value = serde_json::from_str(
            &std::fs::read_to_string(corpus.join(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        let store = Store::new(Runtime::new());
        let mut snapshots: Vec<Value> = Vec::new();
        let mut session = String::new();
        for event in sequence["events"].as_array().expect("events array").iter() {
            let data = event.get("data").cloned().unwrap_or_else(|| json!({}));
            if let Some(id) = data.get("sessionID").and_then(Value::as_str) {
                session = id.to_owned();
            }
            let mut bus = BusEvent::new(event["type"].as_str().expect("type"), data);
            bus.id = event["id"].as_str().unwrap_or_default().to_owned();
            bus.created = event["created"].as_u64();
            store.ingest_all([Ingest::Event(bus)]);
            let rows: Vec<Value> = store.messages(&session).iter().map(norm_row).collect();
            snapshots.push(Value::Array(rows));
        }
        results.push(json!({
            "name": sequence["name"].as_str().unwrap_or(&name),
            "events": sequence["events"].as_array().expect("events array").len(),
            "snapshots": snapshots,
        }));
    }

    std::fs::write(&out, serde_json::to_string_pretty(&results).unwrap()).expect("write results");
}
