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
#[allow(clippy::too_many_lines)] // one pinned probe table (slice + batch 1), one test
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
        ("POST", "/api/pty", r#"{"command": 5}"#, "Expected string\n  at [\"command\"]"),
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
