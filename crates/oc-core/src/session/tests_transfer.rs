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
            assert!(message.contains("ses_missing_parent"))
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
