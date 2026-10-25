//! WIRE-cluster parity contract tests (ledger C4a): HTTP cursor semantics.
//!
//! - HTTP-H2 `cursor_absent_not_null` — v2 omits absent cursor keys
//!   (`SessionsResponse.cursor` fields are optional; absent ≠ null) and
//!   emits encoded cursors from the first/last row of every non-empty page
//!   (no has-more gate) — `packages/server/src/handlers/session.ts:79-96`
//!   and `handlers/message.ts:58-62`.
//! - HTTP-H1 `message_cursor_order_rules` — cursor+order → 400
//!   `InvalidCursorError` ("Cursor cannot be combined with order");
//!   cursor-only pages use the order encoded in the cursor
//!   (`handlers/message.ts:35-41`).
//! - HTTP-H6 `session_cursor_preserves_query` — session cursors carry
//!   search/parentID/order/subpath and the next page is driven by the
//!   cursor payload, not the (absent) query fields
//!   (`protocol/src/groups/session.ts:83-107`, `handlers/session.ts:64-96`).

mod support;

use std::fmt::Write as _;
use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::Value;

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    addr: std::net::SocketAddr,
    /// The live workspace root (the guard keeps the tempdir alive).
    workspace: std::path::PathBuf,
    #[allow(dead_code)] // directory guard for `workspace`
    _dir: tempfile::TempDir,
    #[allow(dead_code)] // engine state root guard
    _data: tempfile::TempDir,
}

impl TestServer {
    async fn raw(&self, target: &str) -> support::RawResponse {
        support::request(self.addr, "GET", target, &[], &[]).await.expect("raw request")
    }
}

async fn boot() -> TestServer {
    let dir = tempfile::tempdir().expect("tempdir");
    let data = tempfile::tempdir().expect("data tempdir");
    let workspace = dir.path().to_path_buf();
    std::env::set_current_dir(&workspace).expect("chdir to workspace");
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
    TestServer { client, state, addr, workspace, _dir: dir, _data: data }
}

async fn create_session(client: &Client, title: &str) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some(title.to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

/// Seeds typed messages directly into the projected `session_message`
/// table (the store has no test insert API; the projector is fed by the
/// engine in production).
async fn seed_message(server: &TestServer, session_id: &str, id: &str, type_: &str, time: i64) {
    let session = session_id.to_owned();
    let message_id = id.to_owned();
    let message_type = type_.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, \
                 time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                (message_id, session, message_type, time, time, r#"{"role":"user"}"#),
            )?;
            Ok(())
        })
        .await
        .expect("seed message");
}

/// Decodes a base64url cursor token into its JSON payload.
fn decode_cursor_token(token: &str) -> Value {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .expect("cursor token is base64url");
    serde_json::from_slice(&bytes).expect("cursor token payload is JSON")
}

fn cursor_string(body: &Value, key: &str) -> String {
    body["cursor"][key]
        .as_str()
        .unwrap_or_else(|| panic!("cursor.{key} must be an encoded string, got {body}"))
        .to_owned()
}

// ---------------------------------------------------------------------------
// HTTP-H2 — absent cursor keys are omitted, never null
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cursor_absent_not_null() {
    let server = boot().await;
    for index in 0..3 {
        create_session(&server.client, &format!("h2-{index}")).await;
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // Page 1 (2 rows): v2 emits previous AND next as strings (no has-more
    // gate — session.ts:79-96 builds both from first/last).
    let page1 = server.raw("/api/session?limit=2").await;
    assert_eq!(page1.status, 200);
    let body1 = page1.json();
    assert_eq!(body1["data"].as_array().expect("data array").len(), 2);
    assert!(
        body1["cursor"]["previous"].is_string(),
        "page 1 previous must be an encoded string: {}",
        body1["cursor"]
    );
    assert!(
        body1["cursor"]["next"].is_string(),
        "page 1 next must be an encoded string: {}",
        body1["cursor"]
    );

    // Page 2 (1 row): still non-empty → both cursors present as strings.
    let next1 = cursor_string(&body1, "next");
    let page2 = server.raw(&format!("/api/session?limit=2&cursor={next1}")).await;
    assert_eq!(page2.status, 200);
    let body2 = page2.json();
    assert_eq!(body2["data"].as_array().expect("data array").len(), 1);
    assert!(body2["cursor"]["previous"].is_string(), "non-final page carries previous");
    assert!(body2["cursor"]["next"].is_string(), "non-final page carries next");

    // Page 3 (empty): v2's RUNTIME emits explicit nulls — probed 2026-10-19
    // against the live reference: `{"data":[],"cursor":{"previous":null,"next":null}}`
    // (the cursor serializer renders undefined as null, not omitted; the
    // static reading of session.ts:79-96 was corrected by this probe).
    let next2 = cursor_string(&body2, "next");
    let page3 = server.raw(&format!("/api/session?limit=2&cursor={next2}")).await;
    assert_eq!(page3.status, 200);
    let body3 = page3.json();
    assert_eq!(body3["data"].as_array().expect("data array").len(), 0);
    assert!(
        body3["cursor"].get("previous").is_some() && body3["cursor"]["previous"].is_null(),
        "empty page must emit previous:null (v2 runtime probe): {}",
        body3["cursor"]
    );
    assert!(
        body3["cursor"].get("next").is_some() && body3["cursor"]["next"].is_null(),
        "empty page must emit next:null (v2 runtime probe): {}",
        body3["cursor"]
    );

    // Message endpoint: a session with no messages returns explicit nulls too.
    let session_id = create_session(&server.client, "h2-empty").await;
    let messages = server.raw(&format!("/api/session/{session_id}/message")).await;
    assert_eq!(messages.status, 200);
    let message_body = messages.json();
    assert_eq!(message_body["data"].as_array().expect("data array").len(), 0);
    assert!(
        message_body["cursor"].get("previous").is_some()
            && message_body["cursor"]["previous"].is_null()
            && message_body["cursor"].get("next").is_some()
            && message_body["cursor"]["next"].is_null(),
        "empty message page must emit cursor nulls (v2 runtime probe): {}",
        message_body["cursor"]
    );
}

// ---------------------------------------------------------------------------
// HTTP-H1 — message cursor/order rules
// ---------------------------------------------------------------------------

#[tokio::test]
async fn message_cursor_order_rules() {
    let server = boot().await;
    let session_id = create_session(&server.client, "h1-msgs").await;
    for (index, time) in (0..4i64).map(|index| (index, 1_700_000_000_000 + index * 1_000)) {
        seed_message(&server, &session_id, &format!("msg_{index}"), "user", time).await;
    }

    // Cursor + order both given → 400 InvalidCursorError
    // (message.ts:35-36 "Cursor cannot be combined with order").
    let first = server.raw(&format!("/api/session/{session_id}/message?order=asc&limit=2")).await;
    assert_eq!(first.status, 200);
    let first_body = first.json();
    assert_eq!(first_body["data"].as_array().expect("data").len(), 2);
    let next_token = cursor_string(&first_body, "next");
    let combined = server
        .raw(&format!("/api/session/{session_id}/message?order=asc&cursor={next_token}"))
        .await;
    assert_eq!(combined.status, 400, "cursor+order must be rejected");
    assert_eq!(combined.json()["_tag"], "InvalidCursorError");
    assert!(
        combined.json()["message"]
            .as_str()
            .is_some_and(|message| message.contains("Cursor cannot be combined with order")),
        "rejection message: {}",
        combined.text()
    );

    // Ascending first page, cursor-only follow: the order comes from the
    // cursor (message.ts:40 `decoded?.order ?? ctx.query.order ?? "desc"`).
    let page1 = server.raw(&format!("/api/session/{session_id}/message?order=asc&limit=2")).await;
    assert_eq!(page1.status, 200);
    let body1 = page1.json();
    assert_eq!(body1["data"][0]["id"], "msg_0", "explicit asc order on the first page");
    let token = cursor_string(&body1, "next");
    let page2 =
        server.raw(&format!("/api/session/{session_id}/message?limit=2&cursor={token}")).await;
    assert_eq!(
        page2.status,
        200,
        "cursor-only pages must use the cursor's order: {}",
        page2.text()
    );
    let body2 = page2.json();
    assert_eq!(body2["data"][0]["id"], "msg_2", "asc order preserved through the cursor");
    assert_eq!(body2["data"][1]["id"], "msg_3");
}

// ---------------------------------------------------------------------------
// HTTP-H6 — session cursor preserves query fields
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_cursor_preserves_query() {
    let server = boot().await;
    for title in ["alpha-one", "alpha-two", "beta-one"] {
        create_session(&server.client, title).await;
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // search=alpha on the first page; the emitted cursor token carries the
    // filter and the follow-up page (query WITHOUT search) stays filtered.
    let page1 = server.raw("/api/session?search=alpha&limit=1").await;
    assert_eq!(page1.status, 200);
    let body1 = page1.json();
    assert_eq!(body1["data"].as_array().expect("data").len(), 1);
    let token = cursor_string(&body1, "next");
    let payload = decode_cursor_token(&token);
    assert_eq!(payload["search"], "alpha", "cursor token preserves search: {payload}");
    assert!(
        payload.get("all").is_none(),
        "all-scope tokens must not carry an `all` key (v2 SessionsAllQuery has none): {payload}"
    );

    let page2 = server.raw(&format!("/api/session?limit=1&cursor={token}")).await;
    assert_eq!(page2.status, 200);
    let body2 = page2.json();
    assert_eq!(body2["data"].as_array().expect("data").len(), 1);
    let title = body2["data"][0]["title"].as_str().expect("title").to_owned();
    assert!(title.starts_with("alpha-"), "search filter survives pagination: {title}");
    let token2 = cursor_string(&body2, "next");
    let payload2 = decode_cursor_token(&token2);
    assert_eq!(payload2["search"], "alpha", "follow-up cursor still carries search");

    // order=asc: following the cursor keeps ascending order without the
    // query parameter (order rides inside the token).
    let asc1 = server.raw("/api/session?search=alpha&order=asc&limit=1").await;
    let asc_body = asc1.json();
    assert_eq!(asc_body["data"][0]["title"], "alpha-one", "asc first page");
    let asc_token = cursor_string(&asc_body, "next");
    let asc_payload = decode_cursor_token(&asc_token);
    assert_eq!(asc_payload["order"], "asc", "cursor token preserves order");
    let ascending_second = server.raw(&format!("/api/session?limit=1&cursor={asc_token}")).await;
    assert_eq!(ascending_second.status, 200, "cursor-only asc page: {}", ascending_second.text());
    let ascending_second_body = ascending_second.json();
    assert_eq!(
        ascending_second_body["data"][0]["title"], "alpha-two",
        "asc order survives pagination"
    );

    // parentID filter: children only, across pages (2 children, page size 1).
    let parent = create_session(&server.client, "h6-parent").await;
    let mut children = Vec::new();
    for index in 0..2 {
        let child_body = SessionCreateBody {
            title: Some(format!("h6-child-{index}")),
            parent_id: Some(parent.clone()),
            ..Default::default()
        };
        children.push(server.client.session().create(&child_body).await.expect("child").id);
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let parent_page = server.raw(&format!("/api/session?parentID={parent}&limit=1")).await;
    assert_eq!(parent_page.status, 200);
    let parent_body = parent_page.json();
    assert_eq!(parent_body["data"].as_array().expect("data").len(), 1);
    assert_eq!(parent_body["data"][0]["id"], children[1], "newest child first (desc)");
    let parent_token = cursor_string(&parent_body, "next");
    let parent_payload = decode_cursor_token(&parent_token);
    assert_eq!(parent_payload["parentID"], parent, "cursor token preserves parentID");
    let parent_page2 = server.raw(&format!("/api/session?limit=1&cursor={parent_token}")).await;
    let parent_page2_text = parent_page2.text();
    assert_eq!(parent_page2.status, 200, "cursor-only parentID page: {parent_page2_text}");
    let parent_body2 = parent_page2.json();
    assert_eq!(
        parent_body2["data"].as_array().expect("data").len(),
        1,
        "parentID filter survives pagination: {parent_body2}"
    );
    assert_eq!(parent_body2["data"][0]["id"], children[0], "second child on the next page");
}

// ---------------------------------------------------------------------------
// HTTP-H3 — MCP resource catalog template key is `uriTemplate`
// ---------------------------------------------------------------------------

#[tokio::test]
#[allow(clippy::too_many_lines)] // boots a live stdio MCP fixture + polls the catalog
async fn mcp_uri_template_key() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping the live MCP fixture");
        return;
    }
    let server = boot().await;
    // A stdio MCP fixture server answering resources/templates lists.
    let fixture = server.workspace.join("mcp_probe_server.js");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_probe_server.js"),
        &fixture,
    )
    .expect("fixture copy");
    std::fs::write(
        server.workspace.join("opencode.json"),
        serde_json::json!({
            "mcp": {
                "servers": {
                    "fixture": {
                        "type": "local",
                        "command": [which_bun().expect("bun"), fixture.display().to_string()],
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("project config");

    let location = {
        let mut encoded = String::new();
        for character in server.workspace.to_string_lossy().chars() {
            match character {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => {
                    encoded.push(character);
                }
                other => {
                    let code = other as u32;
                    let _ = write!(encoded, "%{code:02X}");
                }
            }
        }
        format!("location%5Bdirectory%5D={encoded}")
    };

    // The catalog populates once the fixture connects (poll the route).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let body = loop {
        assert!(std::time::Instant::now() < deadline, "fixture catalog did not populate");
        let response = server.raw(&format!("/api/mcp/resource?{location}")).await;
        assert_eq!(response.status, 200, "resource catalog: {}", response.text());
        let body = response.json();
        if !body["data"]["templates"].as_array().is_some_and(Vec::is_empty) {
            break body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };

    let template = &body["data"]["templates"][0];
    assert_eq!(template["name"], "doc");
    assert!(
        template["uriTemplate"].is_string(),
        "schema key is uriTemplate (schema/src/mcp.ts:109-115): {template}"
    );
    assert_eq!(template["uriTemplate"], "fixture://doc/{name}");
    assert!(
        template.get("uri").is_none(),
        "the wrong `uri` key must not be emitted alongside: {template}"
    );
}

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

fn which_bun() -> Option<String> {
    [std::env::var("HOME").ok()?.trim_end().to_owned() + "/.bun/bin/bun"]
        .into_iter()
        .chain(std::env::var("PATH").ok().into_iter().flat_map(|path| {
            path.split(':').map(std::borrow::ToOwned::to_owned).collect::<Vec<_>>()
        }))
        .map(|directory| std::path::PathBuf::from(directory).join("bun"))
        .find(|bun| bun.is_file())
        .map(|bun| bun.display().to_string())
}

// ---------------------------------------------------------------------------
// HTTP-H4 — session.stats returns the full SessionStats.Info shape
// ---------------------------------------------------------------------------

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)] // one seeded end-to-end stats scenario
async fn session_stats_shape() {
    let server = boot().await;
    // A real project row (session_v2 has an FK on project_id).
    server
        .client
        .session()
        .create(&SessionCreateBody { title: Some("stats".to_owned()), ..Default::default() })
        .await
        .expect("create session");
    let project_id: String = server
        .state
        .engine
        .db
        .transaction(|tx| {
            Ok(tx
                .conn()
                .query_row("SELECT id FROM project ORDER BY time_created LIMIT 1", [], |row| {
                    row.get(0)
                })
                .expect("project row"))
        })
        .await
        .expect("project lookup");
    let base = 1_700_000_000_000i64;
    // Two root sessions + one subagent child; one user + one assistant
    // message with usage, plus a compaction usage event.
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            for (id, parent) in
                [("ses_root1", None), ("ses_root2", None), ("ses_child", Some("ses_root1"))]
            {
                tx.execute(
                    "INSERT INTO session_v2 (id, project_id, parent_id, slug, directory, \
                     version, time_created, time_updated) \
                     VALUES (?1, ?2, ?3, 'slug', '/w', '2.0.24', ?4, ?4)",
                    (id, project_id.as_str(), parent, base),
                )?;
            }
            tx.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, \
                 time_updated, data) VALUES ('msg_u', 'ses_root1', 'user', 1, ?1, ?1, ?2)",
                (
                    base + 1000,
                    r#"{"role":"user","text":"hi"}"#,
                ),
            )?;
            tx.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, \
                 time_updated, data) VALUES ('msg_a', 'ses_root1', 'assistant', 2, ?1, ?1, ?2)",
                (
                    base + 2000,
                    r#"{"role":"assistant","model":{"providerID":"mock","id":"m1"},"tokens":{"input":11,"output":7,"reasoning":3,"cache":{"read":5,"write":2}},"cost":0.25,"content":[]}"#,
                ),
            )?;
            tx.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, \
                 time_updated, data) VALUES ('msg_c', 'ses_child', 'user', 1, ?1, ?1, ?2)",
                (base + 3000, r#"{"role":"user","text":"child"}"#),
            )?;
            let versioned = oc_core::bus::registry::by_type_latest("session.usage.recorded")
                .and_then(oc_core::bus::registry::EventDefinition::versioned_type)
                .unwrap_or_else(|| "session.usage.recorded.1".to_owned());
            tx.execute(
                "INSERT INTO event_sequence (aggregate_id, seq) VALUES ('ses_root2', 1)",
                [],
            )?;
            tx.execute(
                "INSERT INTO event (id, aggregate_id, seq, type, data, created) \
                 VALUES ('evt_1', 'ses_root2', 1, ?1, ?2, ?3)",
                (
                    versioned,
                    r#"{"sessionID":"ses_root2","source":"compaction","cost":1.5,"tokens":{"input":100,"output":50,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
                    base + 4000,
                ),
            )?;
            Ok(())
        })
        .await
        .expect("seed stats");

    let response = server.raw("/api/experimental/session/stats").await;
    assert_eq!(response.status, 200, "stats: {}", response.text());
    let data = response.json()["data"].clone();

    // The v2 SessionStats.Info shape (schema/src/session-stats.ts:51-67):
    // every required field present with the right type.
    // ses_root2 has no user/assistant messages (only its compaction
    // event) — v2's session/subagent sets come from the message scan.
    assert_eq!(data["sessions"], 1, "roots with messages only: {data}");
    assert_eq!(data["subagents"], 1, "child sessions counted separately");
    assert_eq!(data["prompts"], 1, "root user messages");
    assert_eq!(data["steps"], 1, "assistant messages");
    let from = data["range"]["from"].as_f64().expect("range.from number");
    let to = data["range"]["to"].as_f64().expect("range.to number");
    assert_eq!(from, (base + 1000) as f64, "range.from = earliest user/assistant message");
    assert!(to >= (base + 4000) as f64, "range.to = now");
    assert_eq!(data["tokens"]["input"], 111, "11 message + 100 compaction");
    assert_eq!(data["tokens"]["output"], 57);
    assert_eq!(data["tokens"]["reasoning"], 3);
    assert_eq!(data["tokens"]["cache"]["read"], 5);
    assert_eq!(data["tokens"]["cache"]["write"], 2);
    assert_eq!(data["cost"], 1.75, "0.25 message + 1.5 compaction");
    assert_eq!(data["tools"]["mode"], "summary", "default tool mode");
    assert!(data["tools"]["totals"].is_object(), "summary carries totals: {data}");
    assert_eq!(data["activeDays"], 1);
    assert_eq!(data["streak"], 1);
    let activity = data["activity"].as_array().expect("activity array");
    assert_eq!(activity.len(), 1, "one active day");
    assert!(activity[0]["date"].is_string(), "date bucket key");
    assert_eq!(activity[0]["steps"], 1);
    let models = data["models"].as_array().expect("models array");
    assert_eq!(models.len(), 1, "one model aggregate");
    assert_eq!(models[0]["model"]["providerID"], "mock");
    assert_eq!(models[0]["model"]["id"], "m1");
    assert_eq!(models[0]["steps"], 1);
    assert_eq!(models[0]["tokens"]["input"], 11);
    assert_eq!(models[0]["cost"], 0.25);

    // tools=none folds to {mode:"none"} (no totals).
    let response = server.raw("/api/experimental/session/stats?tools=none").await;
    assert_eq!(response.status, 200);
    let none = response.json()["data"].clone();
    assert_eq!(none["tools"]["mode"], "none");
    assert!(none["tools"].get("totals").is_none(), "none carries no totals: {none}");

    // from >= to → 400 InvalidRequestError.
    let response =
        server.raw(&format!("/api/experimental/session/stats?from={to}&to={from}")).await;
    assert_eq!(response.status, 400, "inverted range");
    assert_eq!(response.json()["_tag"], "InvalidRequestError");

    // Invalid timezone → 400 (space percent-encoded — it is a query value).
    let response = server.raw("/api/experimental/session/stats?timezone=Not%2FA%20Zone").await;
    assert_eq!(response.status, 400, "invalid timezone");
    assert_eq!(response.json()["_tag"], "InvalidRequestError");
}

// ---------------------------------------------------------------------------
// HTTP-H0 — request-validation slice (generated table + middleware seam)
// ---------------------------------------------------------------------------

/// Wrong-payload-type probes for the enforced slice: every message is
/// byte-pinned against a live v2.0.24 reference boot (evidence:
/// `/tmp/opencode/parity-evidence/HTTP-H0/ref-validation-messages*.json`).
///
/// v2 ordering nuance this pins: endpoints WITH `sessionLocationMiddleware`
/// resolve the session (404) BEFORE payload decode — the middleware must
/// NOT fire for them (see the LOC-MW control at the end).
#[tokio::test]
#[allow(clippy::too_many_lines)] // one pinned probe table (slice + batches 1-2), one test
async fn all_routes_reject_wrong_payload() {
    let server = boot().await;
    let probes: &[(&str, &str, &str, &str)] = &[
        // (method, target, body, expected Effect message)
        ("POST", "/api/session", r#"{"title": 12345}"#, "Expected string | null\n  at [\"title\"]"),
        ("GET", "/api/session?limit=notanumber", "", "Expected an integer\n  at [\"limit\"]"),
        (
            "POST",
            "/api/plugin/update",
            r#"{"targets": "notanarray"}"#,
            "Expected array\n  at [\"targets\"]",
        ),
        ("POST", "/api/plugin/update", r"{}", "Missing key\n  at [\"targets\"]"),
        (
            "POST",
            "/api/session/ses_H0PROBE/move",
            r#"{"directory": 5}"#,
            "Expected string\n  at [\"directory\"]",
        ),
        (
            "PATCH",
            "/api/credential/cred_H0PROBE",
            r#"{"label": 5}"#,
            "Expected string\n  at [\"label\"]",
        ),
        ("POST", "/api/websearch", r#"{"query": 5}"#, "Expected string\n  at [\"query\"]"),
        ("POST", "/api/shell", r#"{"command": 5}"#, "Expected string\n  at [\"command\"]"),
        (
            "POST",
            "/api/shell",
            r#"{"command": "ls", "timeout": 1.5}"#,
            "Expected an integer\n  at [\"timeout\"]",
        ),
        (
            "POST",
            "/api/shell",
            r#"{"command": "ls", "timeout": -1}"#,
            "Expected a value greater than or equal to 0\n  at [\"timeout\"]",
        ),
        // pty.create (re-enabled 2026-10-20 on probe evidence: v2 is
        // RUNTIME-LENIENT here — env-less creates 200 — and the generated
        // row is all-optional, so wrong-type probes are the only class;
        // they 400 in middleware BEFORE any terminal spawns).
        ("POST", "/api/pty", r#"{"command": 5}"#, "Expected string\n  at [\"command\"]"),
        ("POST", "/api/pty", r#"{"args": [5]}"#, "Expected string\n  at [\"args\"][0]"),
        ("POST", "/api/pty", r#"{"env": 5}"#, "Expected object\n  at [\"env\"]"),
        ("PUT", "/api/pty/pty_H0PROBE", r#"{"title": 5}"#, "Expected string\n  at [\"title\"]"),
        ("PATCH", "/api/project/prj_H0PROBE", r#"{"name": 5}"#, "Expected string\n  at [\"name\"]"),
        (
            "PATCH",
            "/api/project/prj_H0PROBE",
            r#"{"commands": 5}"#,
            "Expected Project.Commands\n  at [\"commands\"]",
        ),
        // `^ses` startsWith patterns (round-4 probe,
        // ref-validation-messages-4.json): predicate message without the
        // `| null` suffix; kind mismatch keeps it.
        (
            "POST",
            "/api/session",
            r#"{"parentID": "not_ses"}"#,
            "Expected a string starting with \"ses\"\n  at [\"parentID\"]",
        ),
        (
            "POST",
            "/api/session",
            r#"{"id": "not_ses"}"#,
            "Expected a string starting with \"ses\"\n  at [\"id\"]",
        ),
        ("POST", "/api/session", r#"{"id": 5}"#, "Expected string | null\n  at [\"id\"]"),
        // HTTP-H0 batch 1 (8 ops, wrong-type + missing-key per route —
        // messages byte-pinned against live v2.0.24 before enforcement).
        (
            "POST",
            "/api/experimental/generate",
            r#"{"prompt": 5}"#,
            "Expected string\n  at [\"prompt\"]",
        ),
        ("POST", "/api/experimental/generate", r"{}", "Missing key\n  at [\"prompt\"]"),
        (
            "POST",
            "/api/experimental/integration/wellknown",
            r#"{"url": 5}"#,
            "Expected string\n  at [\"url\"]",
        ),
        ("POST", "/api/experimental/integration/wellknown", r"{}", "Missing key\n  at [\"url\"]"),
        (
            "PATCH",
            "/api/experimental/config",
            r#"{"shell": 5}"#,
            "Expected string | null\n  at [\"shell\"]",
        ),
        ("PATCH", "/api/experimental/config", r"{}", "Missing key\n  at [\"shell\"]"),
        (
            "POST",
            "/api/integration/int_H0PROBE/connect/command",
            r#"{"methodID": 5}"#,
            "Expected string\n  at [\"methodID\"]",
        ),
        (
            "POST",
            "/api/integration/int_H0PROBE/connect/command",
            r"{}",
            "Missing key\n  at [\"methodID\"]",
        ),
        (
            "PUT",
            "/api/session/ses_H0PROBE/environment",
            r#"{"variables": 5}"#,
            "Expected object\n  at [\"variables\"]",
        ),
        ("PUT", "/api/session/ses_H0PROBE/environment", r"{}", "Missing key\n  at [\"variables\"]"),
        // server.experimental.persistentPty.create (re-enabled 2026-10-20:
        // probe-pinned v2 SPEC-STRICT — env-less create 400s
        // `Missing key at ["env"]`; validate-first, so a fake session id
        // is fine and no terminal ever spawns).
        (
            "POST",
            "/api/experimental/session/ses_H0PROBE/terminal",
            r#"{"args": 5}"#,
            "Expected array\n  at [\"args\"]",
        ),
        (
            "POST",
            "/api/experimental/session/ses_H0PROBE/terminal",
            r"{}",
            "Missing key\n  at [\"args\"]",
        ),
        (
            "POST",
            "/api/experimental/session/ses_H0PROBE/terminal",
            r#"{"args": ["-c", "exit 0"], "title": "t"}"#,
            "Missing key\n  at [\"env\"]",
        ),
        (
            "POST",
            "/api/experimental/session/ses_H0PROBE/terminal",
            r#"{"args": ["-c", "exit 0"], "title": "t", "env": 5}"#,
            "Expected object\n  at [\"env\"]",
        ),
        (
            "PUT",
            "/api/experimental/persistent-pty/pty_H0PROBE",
            r#"{"size": 5}"#,
            "Expected object\n  at [\"size\"]",
        ),
        (
            "PUT",
            "/api/experimental/persistent-pty/pty_H0PROBE",
            r"{}",
            "Missing key\n  at [\"size\"]",
        ),
        ("POST", "/api/worktree", r#"{"projectID": 5}"#, "Expected string\n  at [\"projectID\"]"),
        ("POST", "/api/worktree", r"{}", "Missing key\n  at [\"projectID\"]"),
        // HTTP-H0 batch 2 (8 ops, wrong-type + missing-key per route —
        // messages byte-pinned against live v2.0.24 before enforcement;
        // `env-requiredness-probe.json`, 2026-10-20).
        (
            "POST",
            "/api/credential",
            r#"{"integrationID": 5}"#,
            "Expected string\n  at [\"integrationID\"]",
        ),
        ("POST", "/api/credential", r"{}", "Missing key\n  at [\"integrationID\"]"),
        (
            "POST",
            "/api/credential",
            r#"{"integrationID": "int_H0PROBE"}"#,
            "Missing key\n  at [\"value\"]",
        ),
        (
            "POST",
            "/api/integration/int_H0PROBE/connect/key",
            r#"{"key": 5}"#,
            "Expected string\n  at [\"key\"]",
        ),
        ("POST", "/api/integration/int_H0PROBE/connect/key", r"{}", "Missing key\n  at [\"key\"]"),
        (
            "POST",
            "/api/integration/int_H0PROBE/connect/oauth",
            r#"{"methodID": 5}"#,
            "Expected string\n  at [\"methodID\"]",
        ),
        (
            "POST",
            "/api/integration/int_H0PROBE/connect/oauth",
            r"{}",
            "Missing key\n  at [\"methodID\"]",
        ),
        (
            "POST",
            "/api/integration/int_H0PROBE/connect/oauth/att_H0PROBE/complete",
            r#"{"code": 5}"#,
            "Expected string | null\n  at [\"code\"]",
        ),
        (
            "POST",
            "/api/plugin/check",
            r#"{"target": 5}"#,
            "Expected string | null\n  at [\"target\"]",
        ),
        (
            "POST",
            "/api/session/ses_H0PROBE/view",
            r#"{"idle": "x"}"#,
            "Expected number\n  at [\"idle\"]",
        ),
        ("POST", "/api/session/ses_H0PROBE/view", r"{}", "Missing key\n  at [\"idle\"]"),
        (
            "POST",
            "/api/worktree/refresh",
            r#"{"projectID": 5}"#,
            "Expected string\n  at [\"projectID\"]",
        ),
        ("POST", "/api/worktree/refresh", r"{}", "Missing key\n  at [\"projectID\"]"),
        ("DELETE", "/api/worktree", r#"{"projectID": 5}"#, "Expected string\n  at [\"projectID\"]"),
        (
            "DELETE",
            "/api/worktree",
            r#"{"projectID": "prj_H0PROBE"}"#,
            "Missing key\n  at [\"directory\"]",
        ),
    ];
    for (method, target, body, message) in probes {
        let headers: &[(&str, &str)] =
            if body.is_empty() { &[] } else { &[("content-type", "application/json")] };
        let response = support::request(server.addr, method, target, headers, body.as_bytes())
            .await
            .expect("raw");
        let text = String::from_utf8_lossy(&response.body).into_owned();
        assert_eq!(response.status, 400, "{method} {target}: {text}");
        let envelope = response.json();
        assert_eq!(envelope["_tag"], "InvalidRequestError", "{method} {target} tag: {envelope}");
        assert_eq!(envelope["message"], *message, "{method} {target} message: {envelope}");
        let expected_kind = if *method == "GET" { "Query" } else { "Payload" };
        assert_eq!(envelope["kind"], expected_kind, "{method} {target} kind: {envelope}");
    }

    // Pass-through: valid payloads must still reach the handler (the
    // buffered body is re-inserted, not swallowed).
    let created = server
        .client
        .session()
        .create(&SessionCreateBody { title: Some("h0-slice".to_owned()), ..Default::default() })
        .await
        .expect("valid create passes the middleware");
    assert!(created.id.starts_with("ses_"));
    // Resource-first routes keep their 404-before-400 contract: the wrong
    // payload on a LOC-MW route answers the session 404, NOT a schema 400.
    let response = support::request(
        server.addr,
        "PATCH",
        "/api/session/ses_H0MISSING",
        &[("content-type", "application/json")],
        br#"{"title": 5}"#,
    )
    .await
    .expect("loc-mw control");
    assert_eq!(response.status, 404, "session resolution precedes payload decode");
    assert_eq!(response.json()["_tag"], "SessionNotFoundError");
}

// ---------------------------------------------------------------------------
// HTTP-H0 — full rollout (2026-10-21): pinned wire probes + generative
// body/query matrices over the generated table
// ---------------------------------------------------------------------------

/// Ops whose v2 endpoint resolves the session/form (404) before any
/// query/payload decode (`sessionLocationMiddleware` ×24 +
/// `formLocationMiddleware` ×5, `protocol/src/groups/{session,permission}.ts`;
/// ids are the `OpenAPI` identifiers). Mirror of
/// `request_validation::LOCATION_FIRST_OPS`; kept test-local so the
/// matrix also runs red against the pre-rollout seam (the seam's own unit
/// test pins the const to this same set via
/// `enforced_slice_is_complete`).
const LOCATION_FIRST: &[&str] = &[
    "experimental.session.instructions.entry.list",
    "experimental.session.instructions.entry.put",
    "experimental.session.instructions.entry.remove",
    "experimental.session.skill",
    "experimental.session.wait",
    "session.background",
    "session.command",
    "session.compact",
    "session.fork",
    "session.form.cancel",
    "session.form.create",
    "session.form.get",
    "session.form.list",
    "session.form.reply",
    "session.generate",
    "session.inbox.update",
    "session.interrupt",
    "session.permission.create",
    "session.permission.get",
    "session.permission.reply",
    "session.prompt",
    "session.revert.clear",
    "session.revert.commit",
    "session.revert.stage",
    "session.shell",
    "session.switchAgent",
    "session.switchModel",
    "session.synthetic",
    "session.update",
];

/// Probe-pinned `NumberFromString` integer query params (decode-time in
/// v2, invisible in the `OpenAPI`): every entry rejects `abc` with
/// `Expected an integer`. Mirror of the seam's `NUMBER_PARAMS`.
const NUMBER_QUERY_PARAMS: &[(&str, &str)] = &[
    ("fs.find", "limit"),
    ("server.experimental.persistentPty.read", "lines"),
    ("session.list", "limit"),
    ("session.message.list", "limit"),
    ("vcs.branch.list", "limit"),
    ("vcs.diff", "context"),
];

/// The route's (method, path) with `:param`/`*tail` placeholders
/// substituted by probe-safe ids.
fn route_target(operation_id: &str) -> (&'static str, String) {
    let route = oc_server::routes_gen::ROUTES
        .iter()
        .find(|route| route.operation_id == operation_id)
        .unwrap_or_else(|| panic!("{operation_id} not in routes.gen"));
    let mut path = String::new();
    let mut rest = route.path;
    while let Some(colon) = rest.find(':') {
        path.push_str(&rest[..colon]);
        rest = &rest[colon + 1..];
        let end = rest.find('/').unwrap_or(rest.len());
        let param = &rest[..end];
        let substitution: &str = match param {
            "sessionID" => "ses_H0GEN",
            "formID" => "frm_H0GEN",
            // Pattern-valid ids so the generated Params (path-param)
            // validation passes and the matrix keeps probing Query kinds
            // (v2 decode order: Params → Query → Payload).
            "messageID" | "inboxID" => "msg_H0GEN",
            "ptyID" => "pty_H0GEN",
            "requestID" => "per_H0GEN",
            "id" => "sh_H0GEN",
            _ => "h0gen_id",
        };
        path.push_str(substitution);
        rest = &rest[end..];
    }
    if let Some(star) = rest.rfind("/*") {
        path.push_str(&rest[..=star]);
        path.push_str("h0gen_tail");
    } else {
        path.push_str(rest);
    }
    (route.method.as_str(), path)
}

/// A wrong-typed JSON value for the node kind, if one can fail.
fn wrong_value_for(node: u32) -> Option<Value> {
    use oc_server::validation_gen::{SchemaNode, NODES};
    match &NODES[node as usize] {
        // Wrong KIND per node: scalars take the other scalar's JSON
        // type, containers/booleans take a number.
        SchemaNode::Int { .. } | SchemaNode::Num => Some(Value::from("notanumber")),
        SchemaNode::Literal(_) => Some(Value::from("__h0_bogus__")),
        SchemaNode::Nullable(inner) => wrong_value_for(*inner),
        SchemaNode::Ref { node, .. } => wrong_value_for(*node),
        SchemaNode::Any => None,
        SchemaNode::Bool
        | SchemaNode::Str
        | SchemaNode::Pattern { .. }
        | SchemaNode::Arr(_)
        | SchemaNode::Obj { .. }
        | SchemaNode::Record(_)
        | SchemaNode::Union(_) => Some(Value::from(5)),
    }
}

/// The `Obj` fields of a (possibly `$ref`-wrapped) root node.
fn object_fields(node: u32) -> &'static [oc_server::validation_gen::FieldSpec] {
    use oc_server::validation_gen::{SchemaNode, NODES};
    let node = match &NODES[node as usize] {
        SchemaNode::Ref { node, .. } => *node,
        _ => node,
    };
    match &NODES[node as usize] {
        SchemaNode::Obj { fields } => fields,
        other => panic!("root node {node} is not an Obj: {other:?}"),
    }
}

/// Generative BODY matrix: every validate-first JSON-body operation (28 =
/// 46 JSON bodies − 18 location-first) answers 400
/// `InvalidRequestError{kind: "Payload"}` for (a) a non-object root,
/// (b) `{}` when the root declares a required field, and (c) a
/// wrong-typed value on its first failable field. Message bytes are
/// asserted by the seam unit tests + `all_routes_reject_wrong_payload`.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn generated_body_validation_matrix() {
    use oc_server::validation_gen::{BodyValidation, ROUTES as VALIDATION};

    let server = boot().await;

    let json_ops: Vec<&oc_server::validation_gen::RouteValidation> =
        VALIDATION.iter().filter(|row| matches!(row.body, BodyValidation::Json(_))).collect();
    assert_eq!(json_ops.len(), 46, "46 JSON-body operations");

    let validate_first: Vec<_> =
        json_ops.iter().filter(|row| !LOCATION_FIRST.contains(&row.operation_id)).collect();
    assert_eq!(validate_first.len(), 28, "28 validate-first body operations");

    let mut probes_run = 0usize;
    for row in &validate_first {
        let (method, path) = route_target(row.operation_id);
        let root = row.body;
        // (a) non-object root.
        let response = support::request(
            server.addr,
            method,
            &path,
            &[("content-type", "application/json")],
            b"[1]",
        )
        .await
        .expect("raw");
        let text = response.text();
        assert_eq!(response.status, 400, "{method} {path} [1]: {text}");
        let envelope = response.json();
        assert_eq!(
            envelope["_tag"], "InvalidRequestError",
            "{method} {path} non-object root: {envelope}"
        );
        assert_eq!(envelope["kind"], "Payload", "{method} {path}: {envelope}");
        probes_run += 1;

        let fields = match root {
            BodyValidation::Json(node) => object_fields(node),
            _ => unreachable!("filtered to Json bodies"),
        };
        // (b) `{}` when a required field exists.
        if fields.iter().any(|field| field.required) {
            let response = support::request(
                server.addr,
                method,
                &path,
                &[("content-type", "application/json")],
                b"{}",
            )
            .await
            .expect("raw");
            let text = response.text();
            assert_eq!(response.status, 400, "{method} {path} {{}}: {text}");
            let envelope = response.json();
            assert_eq!(envelope["_tag"], "InvalidRequestError", "{method} {path} {{}}: {envelope}");
            assert_eq!(envelope["kind"], "Payload", "{method} {path} {{}}: {envelope}");
            assert!(
                envelope["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("Missing key")),
                "{method} {path} {{}}: {envelope}"
            );
            probes_run += 1;
        }
        // (c) wrong-typed first failable field.
        for field in fields {
            let Some(wrong) = wrong_value_for(field.ty) else { continue };
            let body = serde_json::json!({ field.name: wrong }).to_string();
            let response = support::request(
                server.addr,
                method,
                &path,
                &[("content-type", "application/json")],
                body.as_bytes(),
            )
            .await
            .expect("raw");
            let text = response.text();
            assert_eq!(response.status, 400, "{method} {path} {body}: {text}");
            let envelope = response.json();
            assert_eq!(
                envelope["_tag"], "InvalidRequestError",
                "{method} {path} {body}: {envelope}"
            );
            assert_eq!(envelope["kind"], "Payload", "{method} {path} {body}: {envelope}");
            probes_run += 1;
            break; // first failable field only — the seam walks declaration order
        }
    }
    eprintln!("HTTP-H0 body matrix: 28 ops, {probes_run} probes");
}

/// Generative QUERY matrix: every query-declaring operation except the
/// two location-first ones (69 of 71) is probed on its failable surface —
/// the deepObject `location` (`location=scalar`), each literal/pattern/
/// `NumberFromString` param (`name=__h0_bogus__`), and required params
/// (absent → `Missing key`). Ops whose params are all accept-all strings
/// (raw handshake routes) are counted and enumerated — nothing in their
/// row can fail by construction.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn generated_query_validation_matrix() {
    use oc_server::validation_gen::{QueryValidation, SchemaNode, NODES, ROUTES as VALIDATION};

    let server = boot().await;

    let query_ops: Vec<&oc_server::validation_gen::RouteValidation> =
        VALIDATION.iter().filter(|row| matches!(row.query, QueryValidation::Params(_))).collect();
    assert_eq!(query_ops.len(), oc_server::validation_gen::QUERY_PARAM_ROUTES);
    assert_eq!(query_ops.len(), 71, "71 ops declare query parameters");

    let enforced: Vec<_> =
        query_ops.iter().filter(|row| !LOCATION_FIRST.contains(&row.operation_id)).collect();
    assert_eq!(enforced.len(), 69, "71 - session.interrupt - session.form.cancel");

    let mut probed = 0usize;
    let mut accept_all: Vec<&str> = Vec::new();
    for row in &enforced {
        let (method, path) = route_target(row.operation_id);
        let fields = match row.query {
            QueryValidation::Params(node) => match &NODES[node as usize] {
                SchemaNode::Obj { fields } => *fields,
                other => panic!("{} query root not an Obj: {other:?}", row.operation_id),
            },
            QueryValidation::None => unreachable!("filtered to Params rows"),
        };

        let mut op_probed = false;
        // (a) deepObject `location` present as a scalar.
        if fields.iter().any(|field| field.name == "location") {
            let target = format!("{path}?location=scalar");
            let response =
                support::request(server.addr, method, &target, &[], b"").await.expect("raw");
            let text = response.text();
            assert_eq!(response.status, 400, "{method} {target}: {text}");
            let envelope = response.json();
            assert_eq!(envelope["_tag"], "InvalidRequestError", "{method} {target}: {envelope}");
            assert_eq!(envelope["kind"], "Query", "{method} {target}: {envelope}");
            assert_eq!(
                envelope["message"], "Expected object | undefined\n  at [\"location\"]",
                "{method} {target}: {envelope}"
            );
            op_probed = true;
        }
        // (b) failable scalar params (literal/pattern/NumberFromString).
        for field in fields {
            let failable = matches!(
                &NODES[field.ty as usize],
                SchemaNode::Literal(_) | SchemaNode::Pattern { .. }
            ) || matches!(
                &NODES[field.ty as usize],
                SchemaNode::Ref { node, .. }
                    if matches!(&NODES[*node as usize], SchemaNode::Literal(_))
            ) || NUMBER_QUERY_PARAMS.contains(&(row.operation_id, field.name));
            if !failable {
                continue;
            }
            let target = format!("{path}?{}=__h0_bogus__", field.name);
            let response =
                support::request(server.addr, method, &target, &[], b"").await.expect("raw");
            let text = response.text();
            assert_eq!(response.status, 400, "{method} {target}: {text}");
            let envelope = response.json();
            assert_eq!(envelope["_tag"], "InvalidRequestError", "{method} {target}: {envelope}");
            assert_eq!(envelope["kind"], "Query", "{method} {target}: {envelope}");
            op_probed = true;
        }
        // (c) required params absent → Missing key (only ops whose
        // remaining params cannot fail differently first: probe the bare
        // path whenever a required param exists — declaration order puts
        // optional `location` first, so the Missing key still wins).
        if fields.iter().any(|field| field.required) {
            let response =
                support::request(server.addr, method, &path, &[], b"").await.expect("raw");
            let text = response.text();
            assert_eq!(response.status, 400, "{method} {path} (required query absent): {text}");
            let envelope = response.json();
            assert_eq!(
                envelope["_tag"], "InvalidRequestError",
                "{method} {path} (required query absent): {envelope}"
            );
            assert_eq!(envelope["kind"], "Query", "{method} {path}: {envelope}");
            assert!(
                envelope["message"].as_str().is_some_and(|m| m.starts_with("Missing key")),
                "{method} {path}: {envelope}"
            );
            op_probed = true;
        }
        if op_probed {
            probed += 1;
        } else {
            accept_all.push(row.operation_id);
        }
    }
    eprintln!(
        "HTTP-H0 query matrix: 69 enforced ops, {probed} probed, {} accept-all ({})",
        accept_all.len(),
        accept_all.join(", ")
    );
}

/// Pinned wire probes for the rollout's NEW surfaces (evidence:
/// `/tmp/opencode/parity-evidence/HTTP-H0/rollout-probe{,-2,-3}.json`):
/// batch-3 body ops, the query classes, P10-N1 content types, decode
/// order, duplicate keys, and the location-first 404 controls.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn rollout_wire_probes() {
    let server = boot().await;

    // -- batch-3 body ops (byte-pinned messages) -----------------------
    let probes: &[(&str, &str, &str, &str)] = &[
        (
            "PUT",
            "/api/experimental/mcp/srv_H0WIRE",
            r#"{"config": 5}"#,
            "Expected { readonly \"type\": \"local\", ... } | { readonly \"type\": \"remote\", ... }\n  \
               at [\"config\"]",
        ),
        (
            "PUT",
            "/api/experimental/mcp/srv_H0WIRE",
            r#"{"config": {"type": "bogus"}}"#,
            "Expected { readonly \"type\": \"local\", ... } | { readonly \"type\": \"remote\", ... }\n  \
               at [\"config\"]",
        ),
        (
            "PUT",
            "/api/experimental/mcp/srv_H0WIRE",
            r#"{"config": {"type": "local"}}"#,
            "Missing key\n  at [\"config\"][\"command\"]",
        ),
        (
            "PUT",
            "/api/experimental/mcp/srv_H0WIRE",
            r#"{"config": {"type": "remote"}}"#,
            "Missing key\n  at [\"config\"][\"url\"]",
        ),
        ("PUT", "/api/experimental/mcp/srv_H0WIRE", r"{}", "Missing key\n  at [\"config\"]"),
        ("POST", "/api/experimental/session/import", r#"{"info": 5}"#, "Expected Session.Info\n  at [\"info\"]"),
        (
            "POST",
            "/api/experimental/session/import",
            r#"{"info": {}, "messages": []}"#,
            "Missing key\n  at [\"info\"][\"id\"]",
        ),
        ("POST", "/api/rpc/rpc_H0WIRE/method_H0WIRE", r"[1]", "Expected Rpc.Input"),
    ];
    for (method, target, body, message) in probes {
        let response = support::request(
            server.addr,
            method,
            target,
            &[("content-type", "application/json")],
            body.as_bytes(),
        )
        .await
        .expect("raw");
        let text = response.text();
        assert_eq!(response.status, 400, "{method} {target}: {text}");
        let envelope = response.json();
        assert_eq!(envelope["_tag"], "InvalidRequestError", "{method} {target}: {envelope}");
        assert_eq!(envelope["message"], *message, "{method} {target}: {envelope}");
        assert_eq!(envelope["kind"], "Payload", "{method} {target}: {envelope}");
    }
    // rpc.call passes through to the handler (input is Unknown; the
    // handler's own 400 RpcError — matching the v2 probe — proves the
    // middleware did not reject it).
    let response = support::request(
        server.addr,
        "POST",
        "/api/rpc/rpc_H0WIRE/method_H0WIRE",
        &[("content-type", "application/json")],
        b"{}",
    )
    .await
    .expect("raw");
    let envelope = response.json();
    assert_ne!(envelope["_tag"], "InvalidRequestError", "rpc.call {{}} must reach the handler");
    assert_eq!(envelope["_tag"], "RpcError", "rpc.call {{}} handler outcome: {envelope}");

    // -- query classes (byte-pinned) -----------------------------------
    let query_probes: &[(&str, &str, &str)] = &[
        // deepObject location
        ("GET", "/api/config?location=scalar", "Expected object | undefined\n  at [\"location\"]"),
        (
            "GET",
            "/api/websearch/provider?location=scalar",
            "Expected object | undefined\n  at [\"location\"]",
        ),
        ("POST", "/api/pty?location=scalar", "Expected object | undefined\n  at [\"location\"]"),
        // NumberFromString bounds
        ("GET", "/api/session?limit=abc", "Expected an integer\n  at [\"limit\"]"),
        ("GET", "/api/session?limit=0", "Expected a value greater than 0\n  at [\"limit\"]"),
        ("GET", "/api/session?limit=", "Expected a value greater than 0\n  at [\"limit\"]"),
        (
            "GET",
            "/api/session/ses_H0WIRE/message?limit=0",
            "Expected a value greater than or equal to 1\n  at [\"limit\"]",
        ),
        (
            "GET",
            "/api/session/ses_H0WIRE/message?limit=201",
            "Expected a value less than or equal to 200\n  at [\"limit\"]",
        ),
        // literal unions + $ref'd literal
        ("GET", "/api/session?order=bogus", "Expected \"asc\" | \"desc\"\n  at [\"order\"]"),
        (
            "GET",
            "/api/experimental/session/stats?tools=bogus",
            "Expected \"none\" | \"summary\" | \"detail\"\n  at [\"tools\"]",
        ),
        (
            "GET",
            "/api/experimental/session/ses_H0WIRE/export?sanitize=bogus",
            "Expected \"true\" | \"false\"\n  at [\"sanitize\"]",
        ),
        (
            "GET",
            "/api/experimental/session/ses_H0WIRE/log?follow=bogus",
            "Expected \"true\" | \"false\"\n  at [\"follow\"]",
        ),
        ("GET", "/api/vcs/diff?mode=bogus", "Expected Vcs.Mode\n  at [\"mode\"]"),
        (
            "GET",
            "/api/fs/find?query=x&type=bogus",
            "Expected \"file\" | \"directory\"\n  at [\"type\"]",
        ),
        // patterns
        (
            "GET",
            "/api/session?parentID=bogus",
            "Expected a string starting with \"ses\"\n  at [\"parentID\"]",
        ),
        (
            "GET",
            "/api/session/ses_H0WIRE/diff?from=notmsg",
            "Expected a string starting with \"msg_\"\n  at [\"from\"]",
        ),
        (
            "GET",
            "/api/session/ses_H0WIRE/diff?from=msg_1&to=notmsg",
            "Expected a string starting with \"msg_\"\n  at [\"to\"]",
        ),
        // required keys
        ("GET", "/api/vcs/diff", "Missing key\n  at [\"mode\"]"),
        ("GET", "/api/fs/find", "Missing key\n  at [\"query\"]"),
        ("GET", "/api/worktree", "Missing key\n  at [\"projectID\"]"),
        ("POST", "/api/experimental/fs/write", "Missing key\n  at [\"path\"]"),
        // NumberFromString elsewhere
        (
            "GET",
            "/api/vcs/diff?mode=working&context=abc",
            "Expected an integer\n  at [\"context\"]",
        ),
        (
            "GET",
            "/api/vcs/diff?mode=working&context=-1",
            "Expected a value greater than or equal to 0\n  at [\"context\"]",
        ),
        ("GET", "/api/vcs/branch?limit=abc", "Expected an integer\n  at [\"limit\"]"),
        ("GET", "/api/fs/find?query=x&limit=abc", "Expected an integer\n  at [\"limit\"]"),
        (
            "GET",
            "/api/experimental/session/ses_H0WIRE/terminal/read?lines=abc",
            "Expected an integer\n  at [\"lines\"]",
        ),
        (
            "GET",
            "/api/experimental/session/ses_H0WIRE/terminal/read?lines=0",
            "Expected a value greater than 0\n  at [\"lines\"]",
        ),
        // duplicates → array → kind mismatch + optional suffix
        ("GET", "/api/session?limit=5&limit=abc", "Expected string | undefined\n  at [\"limit\"]"),
        (
            "GET",
            "/api/session?order=asc&order=desc",
            "Expected \"asc\" | \"desc\" | undefined\n  at [\"order\"]",
        ),
        ("GET", "/api/session?search=a&search=b", "Expected string | undefined\n  at [\"search\"]"),
    ];
    for (method, target, message) in query_probes {
        let response = support::request(server.addr, method, target, &[], b"").await.expect("raw");
        let text = response.text();
        assert_eq!(response.status, 400, "{method} {target}: {text}");
        let envelope = response.json();
        assert_eq!(envelope["_tag"], "InvalidRequestError", "{method} {target}: {envelope}");
        assert_eq!(envelope["message"], *message, "{method} {target}: {envelope}");
        assert_eq!(envelope["kind"], "Query", "{method} {target}: {envelope}");
    }

    // Pass-throughs: valid query shapes must reach handlers.
    for target in [
        "/api/config?location%5Bdirectory%5D=%2Ftmp",
        "/api/config?location%5Bbogus%5D=x",
        "/api/session?parentID=null&limit=%2B5&limit_placeholder=1",
        "/api/session?bogus=1",
    ] {
        let response = server.raw(target).await;
        assert_ne!(response.status, 400, "{target} must not 400: {}", response.text());
    }
    // Duplicate deep members last-win (probe r3: no schema 400).
    let response =
        server.raw("/api/config?location%5Bdirectory%5D=a&location%5Bdirectory%5D=b").await;
    assert_ne!(response.status, 400, "duplicate deep members: {}", response.text());

    // -- P10-N1: content types ----------------------------------------
    // Non-JSON content-type on a JSON route → plain-text 415.
    for content_type in ["text/plain", "application/octet-stream"] {
        let response = support::request(
            server.addr,
            "POST",
            "/api/session",
            &[("content-type", content_type)],
            br#"{"title": 5}"#,
        )
        .await
        .expect("raw");
        assert_eq!(response.status, 415, "{content_type}: {}", response.text());
        assert_eq!(response.header("content-type"), Some("text/plain"));
        assert_eq!(response.text(), format!("Unsupported content-type: {content_type}"));
    }
    // Absent content-type → treated as JSON and validated (probe r1).
    let response = support::request(server.addr, "POST", "/api/session", &[], br#"{"title": 5}"#)
        .await
        .expect("raw");
    assert_eq!(response.status, 400, "no content-type: {}", response.text());
    let envelope = response.json();
    assert_eq!(envelope["message"], "Expected string | null\n  at [\"title\"]");
    assert_eq!(envelope["kind"], "Payload");
    // charset suffix still JSON.
    let response = support::request(
        server.addr,
        "POST",
        "/api/session",
        &[("content-type", "application/json; charset=utf-8")],
        br#"{"title": 5}"#,
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 400);
    // Empty body decodes as null → root message (probe r1).
    let response = support::request(
        server.addr,
        "POST",
        "/api/session",
        &[("content-type", "application/json")],
        b"",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 400, "empty body: {}", response.text());
    let envelope = response.json();
    assert_eq!(envelope["message"], "Expected object");
    // Unparseable JSON (probe r1).
    let response = support::request(
        server.addr,
        "POST",
        "/api/session",
        &[("content-type", "application/json")],
        br#"{"title": "#,
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 400, "unparseable: {}", response.text());
    let envelope = response.json();
    assert_eq!(envelope["message"], "Expected a valid JSON body");
    assert_eq!(envelope["kind"], "Payload");

    // Binary route: octet-stream passes the gate; text/plain and absent
    // content-type 415 (probe r2 — absent names the JSON default).
    let response = support::request(
        server.addr,
        "POST",
        "/api/experimental/fs/write?path=h0_wire_probe.txt",
        &[("content-type", "application/octet-stream")],
        b"hello",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 200, "binary write passes: {}", response.text());
    let response = support::request(
        server.addr,
        "POST",
        "/api/experimental/fs/write?path=h0_wire_probe2.txt",
        &[("content-type", "text/plain")],
        b"hello",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 415);
    assert_eq!(response.text(), "Unsupported content-type: text/plain");
    let response = support::request(
        server.addr,
        "POST",
        "/api/experimental/fs/write?path=h0_wire_probe3.txt",
        &[],
        b"hello",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 415);
    assert_eq!(response.text(), "Unsupported content-type: application/json");

    // -- decode order: query before payload (probe r1) -----------------
    let response = support::request(
        server.addr,
        "POST",
        "/api/websearch?location=scalar",
        &[("content-type", "application/json")],
        br#"{"query": 5}"#,
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 400);
    let envelope = response.json();
    assert_eq!(envelope["message"], "Expected object | undefined\n  at [\"location\"]");
    assert_eq!(envelope["kind"], "Query");

    // -- location-first controls: the 404 always wins ------------------
    let controls: &[(&str, &str, Option<&[u8]>, &str)] = &[
        (
            "POST",
            "/api/session/ses_H0MISSING/interrupt?resume=bogus",
            None,
            "session.interrupt query",
        ),
        (
            "DELETE",
            "/api/session/ses_H0MISSING/form/frm_H0MISSING?message=bogus",
            None,
            "session.form.cancel query",
        ),
        (
            "POST",
            "/api/session/ses_H0MISSING/form/frm_H0MISSING/reply",
            Some(&br#"{"answer": 5}"#[..]),
            "session.form.reply body",
        ),
        (
            "POST",
            "/api/session/ses_H0MISSING/interrupt",
            Some(&b"{}"[..]),
            "session.interrupt body",
        ),
    ];
    for (method, target, body, label) in controls {
        let headers: &[(&str, &str)] =
            if body.is_some() { &[("content-type", "application/json")] } else { &[] };
        let response =
            support::request(server.addr, method, target, headers, body.unwrap_or_default())
                .await
                .expect("raw");
        let text = response.text();
        assert_eq!(response.status, 404, "{label}: {text}");
        assert_eq!(response.json()["_tag"], "SessionNotFoundError", "{label}: {text}");
    }
}
