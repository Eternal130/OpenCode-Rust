//! ACP-translate differential driver (rust side).
//!
//! Env-gated (zero cost when unset; nextest sees a passing no-op):
//!   ACP_DIFF_CORPUS=<dir> ACP_DIFF_OUT=<file> \
//!     cargo nextest run -p oc-cli --test acp_translate_diff_driver
//!
//! Mirrors `oc_cli::acp::translate::fold` over the same corpus the ref
//! bun driver runs (tools/diff/drivers/acp_translate_ref.ts). Output
//! records are normalized to the same shape; the python orchestrator
//! (tools/diff/acp_translate.py) canonicalizes and diffs.

use agent_client_protocol::schema::v1::SessionUpdate;
use serde_json::{json, Value};

use oc_cli::acp::child::ChildSession;
use oc_cli::acp::translate::{
    self as x, Event as XEvent, Output as XOutput, Terminal, TurnContext, TurnStart, TurnStartKind,
};

fn norm_update(update: &SessionUpdate, meta: &Option<Value>) -> Value {
    let mut value = serde_json::to_value(update).unwrap_or_else(|_| json!(null));
    if let Some(extra) = meta {
        // The service layer merges the child tag onto the serialized
        // update's `_meta` (`child.ts:39-47` `{..._meta, ...meta}`).
        if let Some(object) = value.as_object_mut() {
            let mut merged =
                object.get("_meta").and_then(Value::as_object).cloned().unwrap_or_default();
            if let Some(tag) = extra.as_object() {
                merged.extend(tag.clone());
            }
            if !merged.is_empty() {
                object.insert("_meta".to_owned(), Value::Object(merged));
            }
        }
    }
    value
}

fn norm_tool(tool: &Option<x::Tool>) -> Value {
    match tool {
        None => Value::Null,
        Some(tool) => json!({
            "sessionID": tool.session_id,
            "id": tool.id,
            "name": tool.name,
            "input": tool.input,
            "metadata": tool.metadata,
        }),
    }
}

fn norm_child(child: &Option<ChildSession>) -> Value {
    match child {
        None => Value::Null,
        Some(child) => {
            let mut value = json!({
                "id": child.id,
                "parentID": child.parent_id,
                "depth": child.depth,
            });
            if let Some(title) = &child.title {
                value["title"] = json!(title);
            }
            value
        }
    }
}

fn norm_output(output: &XOutput) -> Value {
    match output {
        XOutput::SessionUpdate { update, meta } => {
            json!({ "tag": "session_update", "update": norm_update(update, meta) })
        }
        XOutput::ChildUpdate(update) => json!({ "tag": "child_update", "update": update }),
        XOutput::PermissionAsk { data, tool, child } => json!({
            "tag": "permission_ask",
            "data": data,
            "tool": norm_tool(tool),
            "child": norm_child(child),
        }),
        XOutput::FormAsk { data, child } => json!({
            "tag": "form_ask",
            "form": data.get("form").cloned().unwrap_or(Value::Null),
            "child": norm_child(child),
        }),
        XOutput::AskSettled { id } => json!({ "tag": "ask_settled", "id": id }),
    }
}

fn terminal_str(terminal: Terminal) -> &'static str {
    match terminal {
        Terminal::Succeeded => "succeeded",
        Terminal::Failed => "failed",
        Terminal::Interrupted => "interrupted",
    }
}

#[test]
fn acp_translate_diff_driver() {
    let Some(corpus) = std::env::var_os("ACP_DIFF_CORPUS").filter(|value| !value.is_empty()) else {
        return; // no corpus staged: driver inactive
    };
    let out = std::path::PathBuf::from(std::env::var_os("ACP_DIFF_OUT").expect("ACP_DIFF_OUT"));
    let corpus = std::path::PathBuf::from(corpus);

    let mut names: Vec<String> = std::fs::read_dir(&corpus)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();

    let mut results: Vec<Value> = Vec::new();
    for name in names {
        let case: Value =
            serde_json::from_str(&std::fs::read_to_string(corpus.join(&name)).expect("read case"))
                .expect("parse case");
        let ctx = TurnContext {
            session_id: case["ctx"]["sessionID"].as_str().unwrap_or_default().to_owned(),
            cwd: case["ctx"]["cwd"].as_str().unwrap_or_default().to_owned(),
            start: TurnStart {
                kind: match case["ctx"]["start"]["type"].as_str() {
                    Some("compaction") => TurnStartKind::Compaction,
                    _ => TurnStartKind::Input,
                },
                id: case["ctx"]["start"]["id"].as_str().unwrap_or_default().to_owned(),
            },
            child_updates: case["ctx"]["childUpdates"].as_bool().unwrap_or(false),
            compaction: case["ctx"]["compaction"].as_bool().unwrap_or(true),
        };

        let mut state = x::TurnState::default();
        let mut events: Vec<Value> = Vec::new();
        for event in case["events"].as_array().cloned().unwrap_or_default() {
            let (type_, data) =
                (event["type"].as_str().unwrap_or_default().to_owned(), event["data"].clone());
            let mut xe = XEvent::new(&type_, data);
            if let Some(id) = event["id"].as_str() {
                xe = xe.with_id(id);
            }
            let folded = std::panic::catch_unwind(|| x::fold(&state, &xe, &ctx));
            let folded = match folded {
                Ok(folded) => folded,
                Err(error) => {
                    events.push(json!({
                        "outputs": [{ "tag": "driver_error",
                                      "message": error.downcast_ref::<String>()
                                        .map(String::as_str)
                                        .or_else(|| error.downcast_ref::<&str>().copied())
                                        .unwrap_or("panic").to_owned() }],
                        "terminal": Value::Null,
                    }));
                    continue;
                }
            };
            state = folded.state();
            let mut rec = json!({
                "outputs": folded.outputs.iter().map(norm_output).collect::<Vec<_>>(),
                "terminal": folded.terminal.map(terminal_str),
            });
            if let Some(terminal) = folded.terminal {
                rec["response"] =
                    serde_json::to_value(x::response(&state, &ctx.session_id, terminal))
                        .unwrap_or(Value::Null);
            }
            events.push(rec);
        }

        let mut rec = json!({ "name": case["name"].clone(), "events": events });
        if case["abandon"].as_bool().unwrap_or(false) {
            let (_, outputs) = x::abandon(&state, &ctx);
            rec["abandon_outputs"] =
                Value::Array(outputs.iter().map(norm_output).collect::<Vec<_>>());
        }
        results.push(rec);
    }

    std::fs::write(&out, serde_json::to_vec(&results).expect("serialize")).expect("write out");
    println!("rust acp driver: {} cases -> {}", results.len(), out.display());
}
