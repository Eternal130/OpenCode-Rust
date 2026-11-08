//! gb10-lane9 session-gap regressions through the real HTTP surface:
//!
//! - G-248 — `PATCH /api/session/:id/inbox/:inboxId {delivery:"steer"}`
//!   wakes the idle session's execution (ref session.ts:134-140
//!   `steerInbox` = mutate + `execution.wake`; `queueInbox` :141-144 has
//!   no wake). Pre-fix a queue→steer flip sat undrained until the next
//!   arbitrary wake.
//! - G-249 — `GET /api/session/:id/context` applies the native-checkpoint
//!   system filter after the completed-compaction truncation (ref
//!   store.ts:179 → history.ts:102-107: system messages strictly below
//!   the newest native checkpoint — a completed compaction carrying
//!   `providerContext` — drop; the baseline supersedes the chronological
//!   instruction updates before it).

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

mod support;

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
    _workspace: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    let dir = tempfile::Builder::new()
        .prefix("gb10-lane9-ws-")
        .tempdir_in("/tmp/opencode")
        .expect("workspace");
    let data = tempfile::Builder::new()
        .prefix("gb10-lane9-data-")
        .tempdir_in("/tmp/opencode")
        .expect("data tempdir");
    std::env::set_current_dir(dir.path()).expect("chdir to workspace");
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server");
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, addr, state, _workspace: dir, _data: data }
}

async fn create_session(client: &Client) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some("lane9".to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

/// Retries a probe until it returns `Some` or the budget runs out.
async fn wait_for<T>(
    budget: Duration,
    mut probe: impl FnMut() -> futures::future::BoxFuture<'static, Option<T>>,
) -> Option<T> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(value) = probe().await {
            return Some(value);
        }
        if Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// G-248: steer PATCH wakes the idle session
// ---------------------------------------------------------------------------

/// A queued prompt on an idle session (`resume:false` never wakes), then:
/// a `queue` PATCH leaves it parked (no wake — ref `queueInbox`), while a
/// `steer` PATCH wakes the drain and the row is delivered into the
/// message stream (proof the execution ran).
#[tokio::test]
async fn steer_inbox_patch_wakes_the_idle_session_drain() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client).await;

    // Park a queued prompt; `resume:false` means no wake on admission.
    let inbox = client
        .session()
        .prompt(
            &session_id,
            &SessionPromptBody {
                text: "parked prompt".to_owned(),
                resume: Some(false),
                delivery: Some(SessionInboxDelivery::Queue),
                ..Default::default()
            },
        )
        .await
        .expect("queued prompt");

    // Control (ref queueInbox has NO wake): the queued admission with
    // `resume:false` leaves the idle session undrained — no user message
    // row appears. (PATCHing queue→queue is a guarded-flip 409 by design,
    // so the control is the parked admission itself.)
    tokio::time::sleep(Duration::from_millis(400)).await;
    let messages = client
        .message()
        .list(
            &session_id,
            &SessionMessageListQuery { limit: Some("50".to_owned()), ..Default::default() },
        )
        .await
        .expect("message list");
    assert!(
        messages.data.iter().all(|message| !matches!(message, SessionMessageInfo::User(_))),
        "a queue PATCH does not wake the drain: {:?}",
        messages.data
    );

    // G-248: the steer PATCH wakes the execution (scope "steer"); the
    // drain promotes the row and the user message lands durably. (The
    // model turn itself fails without a configured provider — the
    // delivered user row is the wake proof either way.)
    client
        .session()
        .inbox()
        .update(
            &session_id,
            &inbox.id,
            &SessionInboxUpdateBody { delivery: SessionInboxDelivery::Steer },
        )
        .await
        .expect("steer patch");
    let user_row = wait_for(Duration::from_secs(10), || {
        let client = client.clone();
        let session_id = session_id.clone();
        async move {
            let messages = client
                .message()
                .list(
                    &session_id,
                    &SessionMessageListQuery { limit: Some("50".to_owned()), ..Default::default() },
                )
                .await
                .expect("message list");
            messages
                .data
                .into_iter()
                .find(|message| matches!(message, SessionMessageInfo::User(_)))
        }
        .boxed()
    })
    .await;
    assert!(
        user_row.is_some(),
        "the steer PATCH woke the drain and delivered the parked prompt"
    );
}

// ---------------------------------------------------------------------------
// G-249: GET context applies the native-checkpoint system filter
// ---------------------------------------------------------------------------

/// One seeded `session_message` row.
fn seeded_row(id: &str, type_: &str, seq: i64, data: serde_json::Value) -> (String, String, i64, String) {
    (id.to_owned(), type_.to_owned(), seq, data.to_string())
}

/// The GET context pipeline over a native-checkpoint layout: after the
/// G-96 truncation at the newest completed compaction, system rows
/// strictly below the newest native checkpoint (completed compaction
/// carrying `providerContext`) drop, exactly like ref history.ts:102-107.
#[tokio::test]
async fn context_drops_system_rows_below_the_native_checkpoint() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client).await;

    // The layout (a fork-like seq spread: the native checkpoint keeps a
    // high parent seq while the fork's own baseline system row lands at
    // a fresh low seq):
    //   seq 1  system  "fork baseline instruction"   ← below the native
    //   seq 2  user    "hello"                       ← below the native
    //   seq 40 compaction completed + providerContext (native checkpoint)
    //   seq 41 system  "post-checkpoint instruction" ← at/after: kept
    //   seq 42 user    "after the checkpoint"        ← kept
    let native_context = serde_json::json!({
        "version": 1,
        "provenance": {
            "providerID": "parityfake",
            "provider": "parityfake",
            "modelID": "fake-model",
            "route": "default",
            "protocol": "anthropic",
            "endpoint": "test-endpoint-hash"
        },
        "messages": []
    });
    let rows = vec![
        seeded_row(
            "msg_sys_baseline",
            "system",
            1,
            serde_json::json!({ "text": "fork baseline instruction", "time": { "created": 1 } }),
        ),
        seeded_row("msg_user_1", "user", 2, serde_json::json!({ "text": "hello", "time": { "created": 2 } })),
        seeded_row(
            "msg_native_compact",
            "compaction",
            40,
            serde_json::json!({
                "status": "completed",
                "reason": "auto",
                "summary": "checkpoint",
                "recent": "",
                "providerContext": native_context,
                "time": { "created": 40 }
            }),
        ),
        seeded_row(
            "msg_sys_after",
            "system",
            41,
            serde_json::json!({ "text": "post-checkpoint instruction", "time": { "created": 41 } }),
        ),
        seeded_row(
            "msg_user_2",
            "user",
            42,
            serde_json::json!({ "text": "after the checkpoint", "time": { "created": 42 } }),
        ),
    ];
    let sid = session_id.clone();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            for (id, type_, seq, data) in &rows {
                tx.execute(
                    "INSERT INTO session_message \
                     (id, session_id, type, seq, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![id, sid, type_, seq, 1, 1, data],
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed rows");

    let context = client.session().context(&session_id).await.expect("context");
    let ids: Vec<&str> = context
        .iter()
        .map(|message| match message {
            SessionMessageInfo::User(row) => row.id.as_str(),
            SessionMessageInfo::System(row) => row.id.as_str(),
            SessionMessageInfo::Compaction(SessionMessageCompaction::Completed(row)) => {
                row.id.as_str()
            }
            _ => "",
        })
        .collect();
    // The truncation starts at the native checkpoint; the system rows
    // below it are gone, the ones at/after it stay.
    assert_eq!(
        ids,
        vec!["msg_native_compact", "msg_sys_after", "msg_user_2"],
        "truncated at the completed compaction; pre-checkpoint rows dropped: {ids:?}"
    );
}

/// A compaction WITHOUT `providerContext` (a local summary) is a
/// truncation anchor but NOT a native checkpoint: system rows after it
/// survive (nothing advances the instruction epoch).
#[tokio::test]
async fn context_keeps_system_rows_after_a_local_summary_compaction() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client).await;

    let rows = vec![
        seeded_row("msg_user_pre", "user", 1, serde_json::json!({ "text": "old", "time": { "created": 1 } })),
        seeded_row(
            "msg_local_compact",
            "compaction",
            2,
            serde_json::json!({
                "status": "completed",
                "reason": "auto",
                "summary": "local summary",
                "recent": "",
                "time": { "created": 2 }
            }),
        ),
        seeded_row(
            "msg_sys_kept",
            "system",
            3,
            serde_json::json!({ "text": "instruction update", "time": { "created": 3 } }),
        ),
        seeded_row("msg_user_post", "user", 4, serde_json::json!({ "text": "new", "time": { "created": 4 } })),
    ];
    let sid = session_id.clone();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            for (id, type_, seq, data) in &rows {
                tx.execute(
                    "INSERT INTO session_message \
                     (id, session_id, type, seq, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![id, sid, type_, seq, 1, 1, data],
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed rows");

    let context = client.session().context(&session_id).await.expect("context");
    let ids: Vec<&str> = context
        .iter()
        .map(|message| match message {
            SessionMessageInfo::User(row) => row.id.as_str(),
            SessionMessageInfo::System(row) => row.id.as_str(),
            SessionMessageInfo::Compaction(SessionMessageCompaction::Completed(row)) => {
                row.id.as_str()
            }
            _ => "",
        })
        .collect();
    assert_eq!(ids, vec!["msg_local_compact", "msg_sys_kept", "msg_user_post"], "no native checkpoint: {ids:?}");
}
