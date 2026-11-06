//! Unit tests for `session/transfer.rs` — REF `transfer.test.ts` shape:
//! import replays settled messages + counters, duplicate ids conflict,
//! missing parents 404, malformed payloads reject.

#![allow(clippy::uninlined_format_args)]

use serde_json::json;

use crate::session::store::{ListOrder, MessagesInput, ResolvedProject};
use crate::session::transfer::{import, ImportError, ImportInput};
use oc_schema::location;

fn project() -> ResolvedProject {
    ResolvedProject { id: "global".to_owned(), directory: "/project".to_owned() }
}

fn location_ref() -> location::Ref {
    location::Ref {
        directory: oc_schema::common::AbsolutePath("/project".to_owned()),
        workspace_id: None,
    }
}

fn payload() -> serde_json::Value {
    json!({
        "info": {
            "id": "ses_transfer1",
            "title": "transfer",
            "agent": "build",
            "cost": 1.5,
            "tokens": {
                "input": 100.0, "output": 20.0, "reasoning": 3.0,
                "cache": { "read": 7.0, "write": 9.0 },
            },
            "time": {
                "created": 1_000.0,
                "updated": 2_000.0,
                "idle": 3_000.0,
                "viewed": 4_000.0,
                "archived": 5_000.0,
            },
            "outcome": "interrupted",
        },
        "messages": [
            {
                "id": "msg_user1", "type": "user", "text": "hello",
                "time": { "created": 1_100.0 },
            },
            {
                "id": "msg_asst1", "type": "assistant", "agent": "build",
                "time": { "created": 1_200.0, "completed": 1_300.0 },
            },
            // Unsettled: no time.completed → filtered (isSettled).
            { "id": "msg_asst2", "type": "assistant", "time": { "created": 1_250.0 } },
            // Unsettled shell: status running → filtered.
            {
                "id": "msg_shell1", "type": "shell", "command": "ls",
                "status": "running", "time": { "created": 1_260.0 },
            },
            {
                "id": "msg_shell2", "type": "shell", "command": "pwd",
                "status": "completed", "time": { "created": 1_270.0 },
            },
        ],
    })
}

#[tokio::test]
async fn transfer_import_replays_messages_and_counters() {
    let stack = super::tests::stack().await;
    let row = import(
        &stack.store,
        &stack.bus,
        &stack.db,
        ImportInput {
            data: payload(),
            location: location_ref(),
            project: project(),
            version: "test".to_owned(),
        },
    )
    .await
    .expect("import");

    // Counter backfill (transfer.ts:116-139).
    assert_eq!(row.id, "ses_transfer1");
    assert_eq!(row.cost, 1.5);
    assert_eq!((row.tokens_input, row.tokens_output, row.tokens_reasoning), (100, 20, 3));
    assert_eq!((row.tokens_cache_read, row.tokens_cache_write), (7, 9));
    assert_eq!(row.time_created, 1_000);
    assert_eq!(row.time_idle, Some(3_000));
    // time_viewed = min(idle, viewed).
    assert_eq!(row.time_viewed, Some(3_000));
    assert_eq!(row.time_archived, Some(5_000));
    assert_eq!(row.idle_outcome.as_deref(), Some("interrupted"));
    assert_eq!(row.title.as_deref(), Some("transfer"));
    assert_eq!(row.agent.as_deref(), Some("build"));

    // Settled messages land in the store, seq = 1..n, readable via the
    // normal read APIs.
    let messages = stack
        .store
        .messages(MessagesInput {
            session_id: "ses_transfer1".to_owned(),
            order: Some(ListOrder::Asc),
            ..Default::default()
        })
        .await
        .expect("messages");
    let ids: Vec<&str> = messages.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["msg_user1", "msg_asst1", "msg_shell2"], "settled only, in order");
    let seqs: Vec<i64> = messages.iter().map(|row| row.seq).collect();
    assert_eq!(seqs, [1, 2, 3]);
    let data: serde_json::Value =
        serde_json::from_str(&messages[0].data).expect("message data json");
    assert_eq!(data["text"], "hello");
    assert_eq!(messages[0].time_created, 1_100);

    // The aggregate sequence is reserved past the imported range so a
    // later published event cannot collide (`Bus.reserveSequence`).
    let reserved: Option<i64> = stack
        .db
        .transaction(move |tx| {
            use rusqlite::OptionalExtension;
            Ok(tx
                .conn()
                .query_row(
                    "SELECT seq FROM event_sequence WHERE aggregate_id = 'ses_transfer1'",
                    [],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await
        .expect("seq read");
    let reserved = reserved.unwrap_or_default();
    assert!(reserved >= 3, "sequence reserved past imported messages: {reserved}");
}

#[tokio::test]
async fn transfer_import_duplicate_conflicts() {
    let stack = super::tests::stack().await;
    let input = || ImportInput {
        data: payload(),
        location: location_ref(),
        project: project(),
        version: "test".to_owned(),
    };
    import(&stack.store, &stack.bus, &stack.db, input()).await.expect("first import");
    match import(&stack.store, &stack.bus, &stack.db, input()).await {
        Err(ImportError::Conflict(id)) => assert_eq!(id, "ses_transfer1"),
        other => panic!("expected conflict, got {other:?}"),
    }
}

#[tokio::test]
async fn transfer_import_missing_parent_rejects() {
    let stack = super::tests::stack().await;
    let mut data = payload();
    data["info"]["parentID"] = json!("ses_missing_parent");
    match import(
        &stack.store,
        &stack.bus,
        &stack.db,
        ImportInput {
            data,
            location: location_ref(),
            project: project(),
            version: "test".to_owned(),
        },
    )
    .await
    {
        Err(ImportError::ParentNotFound(message)) => {
            assert!(message.contains("ses_missing_parent"));
        }
        other => panic!("expected parent-not-found, got {other:?}"),
    }
}

#[tokio::test]
async fn transfer_import_malformed_payload_rejects() {
    let stack = super::tests::stack().await;
    for bad in [json!({}), json!({ "info": {} }), json!({ "info": { "id": "" } })] {
        match import(
            &stack.store,
            &stack.bus,
            &stack.db,
            ImportInput {
                data: bad,
                location: location_ref(),
                project: project(),
                version: "test".to_owned(),
            },
        )
        .await
        {
            Err(ImportError::InvalidData(_)) => {}
            other => panic!("expected invalid-data, got {other:?}"),
        }
    }
}

// -- G-170: export (SessionTransfer.export, transfer.ts:60-64 + 165-330) ---

fn export_info() -> serde_json::Value {
    json!({
        "id": "ses_exp1",
        "title": "secret title",
        "metadata": { "user": "alice" },
        "location": { "directory": "/home/alice/repo" },
        "revert": {
            "files": [
                { "file": "src/main.rs", "patch": "+diff", "mtime": 1 },
                { "file": "src/lib.rs", "patch": "-diff", "mtime": 2 },
            ]
        },
        "cost": 0.0,
    })
}

#[test]
fn transfer_export_filters_settled_messages_on_both_paths() {
    let messages = vec![
        json!({ "id": "msg_u1", "type": "user", "text": "hi" }),
        // assistant without time.completed → unsettled
        json!({ "id": "msg_a1", "type": "assistant", "time": { "created": 1 } }),
        // assistant completed → settled
        json!({ "id": "msg_a2", "type": "assistant", "time": { "created": 1, "completed": 2 } }),
        // running shell/compaction → unsettled
        json!({ "id": "msg_s1", "type": "shell", "status": "running", "command": "ls" }),
        json!({ "id": "msg_c1", "type": "compaction", "status": "running" }),
        json!({ "id": "msg_c2", "type": "compaction", "status": "completed" }),
    ];
    for sanitize in [false, true] {
        let data = crate::session::transfer::export_data(export_info(), messages.clone(), sanitize);
        let ids: Vec<&str> = data["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["msg_u1", "msg_a2", "msg_c2"], "sanitize={sanitize}");
    }
}

#[test]
fn transfer_export_sanitize_redacts_info_fields() {
    let data =
        crate::session::transfer::export_data(export_info(), Vec::new(), true);
    let info = &data["info"];
    assert_eq!(info["title"], json!("[redacted:session-title:ses_exp1]"));
    assert_eq!(info["metadata"], json!({ "redacted": "session-metadata:ses_exp1" }));
    assert_eq!(info["location"]["directory"], json!("/[redacted:session-directory:ses_exp1]"));
    // per-file index ids
    assert_eq!(info["revert"]["files"][0]["file"], json!("[redacted:revert-file:0]"));
    assert_eq!(info["revert"]["files"][0]["patch"], json!("[redacted:revert-patch:0]"));
    assert_eq!(info["revert"]["files"][1]["file"], json!("[redacted:revert-file:1]"));
    assert_eq!(info["revert"]["files"][1]["patch"], json!("[redacted:revert-patch:1]"));
    // untouched fields stay
    assert_eq!(info["revert"]["files"][0]["mtime"], json!(1));
    assert_eq!(info["id"], json!("ses_exp1"));
}

#[test]
fn transfer_export_sanitize_redacts_each_message_type() {
    let messages = vec![
        json!({
            "id": "msg_m1", "type": "user", "text": "secret question",
            "metadata": { "k": "v" },
            "files": [{
                "data": "file-bytes", "name": "notes.txt", "description": "my notes",
                "mention": { "text": "the notes" },
            }],
            "agents": [{ "name": "helper", "mention": { "text": "the helper" } }],
            "skills": [{ "name": "review", "text": "skill body", "mention": { "text": "the skill" } }],
        }),
        json!({ "id": "msg_m2", "type": "system", "text": "system text" }),
        json!({ "id": "msg_m3", "type": "synthetic", "text": "synthetic text", "description": "desc" }),
        json!({ "id": "msg_m4", "type": "skill", "text": "skill text" }),
        json!({
            "id": "msg_m5", "type": "shell", "command": "rm -rf /",
            "output": { "output": "command output", "exitCode": 0 },
        }),
        json!({ "id": "msg_m6", "type": "shell", "command": "ls", "output": null }),
        json!({
            "id": "msg_m7", "type": "assistant", "time": { "created": 1, "completed": 2 },
            "content": [
                { "type": "text", "text": "visible reply", "state": { "pending": true } },
                { "type": "reasoning", "text": "chain of thought", "state": { "x": 1 } },
                {
                    "type": "tool-call", "toolCallID": "t1",
                    "providerState": { "raw": true },
                    "providerResultState": { "raw2": true },
                    "state": {
                        "status": "completed", "input": { "cmd": "ls" },
                        "content": [{ "type": "text", "text": "tool output" }, { "type": "file", "uri": "file:///tmp", "name": "tmp" }],
                        "metadata": { "m": 1 },
                    },
                },
                {
                    "type": "tool-call", "toolCallID": "t2",
                    "state": { "status": "streaming", "input": "{partial json" },
                },
                {
                    "type": "tool-call", "toolCallID": "t3",
                    "state": { "status": "running", "input": { "a": 1 }, "metadata": { "b": 2 } },
                },
            ],
        }),
        json!({
            "id": "msg_m8", "type": "compaction", "status": "completed",
            "summary": "the summary", "recent": "the recent window",
            "providerState": { "native": true },
        }),
        json!({
            "id": "msg_m9", "type": "compaction", "status": "failed",
            "summary": "kept summary", "recent": "kept recent",
        }),
    ];
    let data =
        crate::session::transfer::export_data(export_info(), messages, true);
    let messages = data["messages"].as_array().unwrap();
    let by_id = |id: &str| {
        messages
            .iter()
            .find(|message| message["id"] == json!(id))
            .unwrap_or_else(|| panic!("message {id} missing; have {messages:?}"))
            .clone()
    };

    let user = by_id("msg_m1");
    assert_eq!(user["text"], json!("[redacted:text:msg_m1]"));
    assert_eq!(user["metadata"], json!({ "redacted": "message-metadata:msg_m1" }));
    let file = &user["files"][0];
    assert_eq!(file["data"], json!(""));
    assert_eq!(file["source"], json!({ "type": "inline" }));
    assert_eq!(file["name"], json!("[redacted:file-name:0]"));
    assert_eq!(file["description"], json!("[redacted:file-description:0]"));
    assert_eq!(file["mention"]["text"], json!("[redacted:file-mention:0]"));
    assert_eq!(user["agents"][0]["name"], json!("[redacted:agent-name:0]"));
    assert_eq!(user["agents"][0]["mention"]["text"], json!("[redacted:agent-mention:0]"));
    assert_eq!(user["skills"][0]["name"], json!("[redacted:skill-name:0]"));
    assert_eq!(user["skills"][0]["text"], json!("[redacted:skill:0]"));
    assert_eq!(user["skills"][0]["mention"]["text"], json!("[redacted:skill-mention:0]"));

    assert_eq!(by_id("msg_m2")["text"], json!("[redacted:system:msg_m2]"));
    let synthetic = by_id("msg_m3");
    assert_eq!(synthetic["text"], json!("[redacted:synthetic:msg_m3]"));
    assert_eq!(synthetic["description"], json!("[redacted:synthetic-description:msg_m3]"));
    assert_eq!(by_id("msg_m4")["text"], json!("[redacted:skill:msg_m4]"));

    let shell = by_id("msg_m5");
    assert_eq!(shell["command"], json!("[redacted:shell-command:msg_m5]"));
    assert_eq!(shell["output"]["output"], json!("[redacted:shell-output:msg_m5]"));
    assert_eq!(shell["output"]["exitCode"], json!(0));
    let shell_null = by_id("msg_m6");
    assert!(shell_null.get("output").is_none(), "a null output drops the key");

    let assistant = by_id("msg_m7");
    let content = assistant["content"].as_array().unwrap();
    assert_eq!(content[0]["text"], json!("[redacted:text:msg_m7]"));
    assert_eq!(content[0]["state"], json!({ "redacted": "text-state:msg_m7" }));
    assert_eq!(content[1]["text"], json!("[redacted:reasoning:msg_m7]"));
    assert_eq!(content[1]["state"], json!({ "redacted": "reasoning-state:msg_m7" }));
    let tool = &content[2];
    assert_eq!(tool["providerState"], json!({ "redacted": "tool-provider-state:msg_m7" }));
    assert_eq!(
        tool["providerResultState"],
        json!({ "redacted": "tool-provider-result-state:msg_m7" })
    );
    let state = &tool["state"];
    assert_eq!(state["input"], json!({ "redacted": "tool-input:msg_m7" }));
    assert_eq!(state["metadata"], json!({ "redacted": "tool-metadata:msg_m7" }));
    assert_eq!(state["content"][0]["text"], json!("[redacted:tool-output:msg_m7]"));
    assert_eq!(state["content"][1]["uri"], json!("[redacted:tool-file-uri:msg_m7]"));
    assert_eq!(state["content"][1]["name"], json!("[redacted:tool-file-name:msg_m7]"));
    let streaming = &content[3]["state"];
    assert_eq!(streaming["input"], json!("[redacted:tool-input:msg_m7]"));
    let running = &content[4]["state"];
    assert_eq!(running["input"], json!({ "redacted": "tool-input:msg_m7" }));
    assert_eq!(running["metadata"], json!({ "redacted": "tool-metadata:msg_m7" }));

    let compaction = by_id("msg_m8");
    assert_eq!(compaction["summary"], json!("[redacted:compaction-summary:msg_m8]"));
    assert_eq!(compaction["recent"], json!("[redacted:compaction-recent:msg_m8]"));
    assert_eq!(
        compaction["providerState"],
        json!({ "redacted": "compaction-provider-state:msg_m8" })
    );
    let failed = by_id("msg_m9");
    assert_eq!(failed["summary"], json!("kept summary"), "a failed compaction keeps its text");
    assert_eq!(failed["recent"], json!("kept recent"));
}

#[test]
fn transfer_export_sanitize_keeps_empty_values_verbatim() {
    // `redact` passes whitespace-only strings through.
    let info = json!({
        "id": "ses_exp2",
        "title": "  ",
        "metadata": {},
        "location": { "directory": "  " },
    });
    let data = crate::session::transfer::export_data(info, Vec::new(), true);
    let info = &data["info"];
    assert_eq!(info["title"], json!("  "));
    assert_eq!(info["metadata"], json!({}));
    assert_eq!(info["location"]["directory"], json!("/  "));
}

#[test]
fn transfer_export_non_sanitize_path_is_unchanged() {
    let info = export_info();
    let messages = vec![json!({
        "id": "msg_plain", "type": "user", "text": "plain", "metadata": { "k": "v" }
    })];
    let data =
        crate::session::transfer::export_data(info.clone(), messages.clone(), false);
    assert_eq!(data["info"], info);
    assert_eq!(data["messages"], json!(messages));
}
