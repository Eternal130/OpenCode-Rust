//! RUNTIME-TD04 regressions — the reconnect resync chain
//! (ref `packages/tui/src/routes/session/index.tsx:313-353` +
//! `context/session-tabs.tsx:269-293`):
//!
//! 1. After a disconnect window, the pending permission/form asks
//!    that happened while offline are RE-DELIVERED (the resync
//!    re-lists `GET /session/:id/permission` + `GET /session/:id/form`
//!    for the open family — the open sessions plus their descendants
//!    — on every successful resubscribe); asks replied while offline
//!    CLEAR.
//! 2. The session family (`children: true`) and the message tails of
//!    open sessions re-sync from the fresh snapshot.
//! 3. A session deleted while offline ("Session not found") closes
//!    its tab cleanly with the v2 toast, routing to the neighbor or
//!    home.
//!
//! The data-layer test drives the REAL production pair —
//! [`oc_tui::data::Connection`] (the reconnect state machine) +
//! [`oc_tui::data::HttpResync`] (the wire fetcher) — against a
//! scripted local server that logs every request path, proving the
//! fetch chain actually hits the blocker endpoints over HTTP. The
//! store-level tests drive [`oc_tui::store::Store`] through
//! [`oc_tui::data::Ingest`] directly.
//!
//! Hermetic: 127.0.0.1:0 listener, /tmp/opencode sandboxes, no
//! external network.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_tui::components::footer::{FooterView, FormRequest, PermissionRequest};
use oc_tui::data::{
    resync_family, BusEvent, Connection, ConnectionConfig, HttpResync, Ingest, SessionSnapshot,
};
use oc_tui::reactive::Runtime;
use oc_tui::store::{Route, Store};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn snapshot(id: &str, parent: Option<&str>) -> SessionSnapshot {
    SessionSnapshot {
        model: None,
        id: id.to_owned(),
        title: Some(id.to_owned()),
        parent_id: parent.map(str::to_owned),
        time_idle: None,
        viewed: None,
    }
}

fn permission(id: &str, session: &str) -> PermissionRequest {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "sessionID": session,
        "action": "bash",
        "resources": ["/tmp"],
    }))
    .expect("permission")
}

fn form(id: &str, session: &str) -> FormRequest {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "sessionID": session,
        "title": "Confirm deploy",
        "fields": [],
    }))
    .expect("form")
}

fn resync(
    sessions: Vec<SessionSnapshot>,
    permissions: Vec<(String, Vec<PermissionRequest>)>,
    forms: Vec<(String, Vec<FormRequest>)>,
) -> Ingest {
    Ingest::Resync { sessions, unread: Vec::new(), messages: Vec::new(), permissions, forms }
}

// ---------------------------------------------------------------------------
// Store level: blockers re-deliver + stale asks clear
// ---------------------------------------------------------------------------

/// The ledger repro: while offline, ask `p_new` arrived (and `p_old`
/// was replied) server-side. The resync snapshot re-delivers `p_new`
/// and clears `p_old` — the footer shows the OFFLINE ask, not the
/// stale one.
#[test]
fn permission_form_resync() {
    let store = Store::new(Runtime::new());
    store.open_tab("ses_1", None);
    // Pre-disconnect live state: an older pending ask for ses_1.
    store.ingest_all([Ingest::Event(BusEvent::new(
        "permission.asked",
        serde_json::json!({
            "id": "p_old",
            "sessionID": "ses_1",
            "action": "bash",
            "resources": [],
        }),
    ))]);
    assert_eq!(store.footer().permissions.len(), 1, "the live ask is pending");

    // Reconnect: the fresh list carries only p_new (p_old was replied
    // while offline) plus a pending form.
    store.ingest_all([
        Ingest::Connected,
        resync(
            vec![snapshot("ses_1", None)],
            vec![("ses_1".to_owned(), vec![permission("p_new", "ses_1")])],
            vec![("ses_1".to_owned(), vec![form("f_new", "ses_1")])],
        ),
    ]);

    let footer = store.footer();
    assert_eq!(
        footer.permissions,
        vec![permission("p_new", "ses_1")],
        "the offline ask re-delivers; the replied one clears"
    );
    assert_eq!(footer.forms, vec![form("f_new", "ses_1")], "the offline form re-delivers");
    assert!(
        matches!(footer.view, FooterView::Permission { .. }),
        "the blocker owns the footer again after reconnect: {:?}",
        footer.view
    );
}

/// Blockers of sessions NOT in the fetch set survive untouched (no
/// fetch, no decision — an unfetched session's asks keep their state).
#[test]
fn blockers_for_unfetched_sessions_survive() {
    let store = Store::new(Runtime::new());
    store.open_tab("ses_1", None);
    store.ingest_all([
        Ingest::Event(BusEvent::new(
            "permission.asked",
            serde_json::json!({
                "id": "p_other",
                "sessionID": "ses_other",
                "action": "edit",
                "resources": [],
            }),
        )),
        Ingest::Event(BusEvent::new(
            "form.created",
            serde_json::json!({
                "form": {
                    "id": "f_other",
                    "sessionID": "ses_other",
                    "title": "Elsewhere",
                    "fields": [],
                }
            }),
        )),
    ]);
    store.ingest_all([resync(
        vec![snapshot("ses_1", None)],
        vec![("ses_1".to_owned(), vec![permission("p_1", "ses_1")])],
        Vec::new(),
    )]);
    let footer = store.footer();
    assert_eq!(footer.permissions.len(), 2, "ses_other's ask survives: {:?}", footer.permissions);
    assert_eq!(footer.forms.len(), 1, "ses_other's form survives");
}

/// A session that ONLY had a form pending shows the form view after
/// the resync (the form re-delivery path, independent of permissions).
#[test]
fn form_only_resync_blocks_footer() {
    let store = Store::new(Runtime::new());
    store.open_tab("ses_1", None);
    store.ingest_all([resync(
        vec![snapshot("ses_1", None)],
        Vec::new(),
        vec![("ses_1".to_owned(), vec![form("f_only", "ses_1")])],
    )]);
    assert_eq!(store.footer().forms.len(), 1);
    assert!(matches!(store.footer().view, FooterView::Form { .. }));
}

// ---------------------------------------------------------------------------
// Store level: children + message tail resync
// ---------------------------------------------------------------------------

/// The fresh snapshot's parent links seed the session registry (v2
/// `data.session.sync(sessionID, { children: true })`): the family
/// tree re-builds even when the `session.created` events happened
/// while offline.
#[test]
fn session_children_resync() {
    let store = Store::new(Runtime::new());
    store.open_tab("ses_root", None);
    // Offline: the subagent branch was created server-side; the TUI
    // never saw the events. The snapshot carries the parent links.
    store.ingest_all([resync(
        vec![
            snapshot("ses_root", None),
            snapshot("ses_child_a", Some("ses_root")),
            snapshot("ses_child_b", Some("ses_child_a")),
        ],
        Vec::new(),
        Vec::new(),
    )]);
    assert_eq!(
        store.session_info("ses_child_a").and_then(|info| info.parent_id),
        Some("ses_root".into())
    );
    assert_eq!(
        store.session_info("ses_child_b").and_then(|info| info.parent_id),
        Some("ses_child_a".into()),
        "multi-level descendants seed"
    );
    // The family picker (active filter: the children are mid-run) sees
    // the whole tree off the root.
    for child in ["ses_child_a", "ses_child_b"] {
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.execution.started",
            serde_json::json!({ "sessionID": child }),
        ))]);
    }
    let entries = store.picker_entries("ses_root");
    let ids: Vec<&str> = entries.iter().map(|entry| entry.session_id.as_str()).collect();
    assert!(ids.contains(&"ses_child_a") && ids.contains(&"ses_child_b"), "family: {ids:?}");
}

/// The message tail of an open session re-fetches from the snapshot
/// (the wholesale rebuild — the offline turn's messages appear).
#[test]
fn message_tail_resync() {
    use oc_tui::components::message::model::{Message, UserFile};
    let store = Store::new(Runtime::new());
    store.open_tab("ses_1", None);
    let offline_message = Message::User {
        id: "msg_offline".into(),
        text: "typed while the TUI was offline".into(),
        created: 1000,
        files: Vec::<UserFile>::new(),
    };
    store.ingest_all([Ingest::Resync {
        sessions: vec![snapshot("ses_1", None)],
        unread: Vec::new(),
        messages: vec![("ses_1".to_owned(), vec![offline_message])],
        permissions: Vec::new(),
        forms: Vec::new(),
    }]);
    let messages = store.messages("ses_1");
    assert_eq!(messages.len(), 1, "the offline tail rebuilds: {messages:?}");
    assert_eq!(messages[0].id(), "msg_offline");
}

// ---------------------------------------------------------------------------
// Store level: "Session not found" wrap-up
// ---------------------------------------------------------------------------

/// The open tab's session was deleted while offline: the resync (its
/// snapshot no longer carries the session) closes the tab, routes
/// home, and surfaces the v2 toast — the stale tab never lingers.
#[test]
fn session_not_found_closes_tab() {
    let store = Store::new(Runtime::new());
    store.open_tab("ses_gone", Some("Gone".into()));
    store.open_tab("ses_kept", Some("Kept".into()));
    assert_eq!(store.tabs().tabs.len(), 2);

    // The snapshot carries only the surviving session.
    store.ingest_all([resync(vec![snapshot("ses_kept", None)], Vec::new(), Vec::new())]);

    assert_eq!(store.tabs().tabs.len(), 1, "the deleted session's tab closed");
    assert_eq!(store.tabs().tabs[0].session_id, "ses_kept");
    assert_ne!(store.route(), Route::Session { session_id: "ses_gone".into() });
}

/// The ACTIVE route's deleted session additionally toasts (v2
/// `Session not found: <id>`).
#[test]
fn session_not_found_toasts_on_active_route() {
    let store = Store::new(Runtime::new());
    store.open_tab("ses_active", None);
    assert!(matches!(store.route(), Route::Session { .. }));
    store.ingest_all([resync(Vec::new(), Vec::new(), Vec::new())]);
    let toasts = store.toasts();
    let current = toasts.current().expect("the wrap-up toasted");
    assert_eq!(current.message, "Session not found: ses_active");
    assert_eq!(current.variant, "error");
    assert_eq!(current.duration_ms, 5000);
    assert_eq!(store.route(), Route::Home, "no neighbor tab: home");
}

// ---------------------------------------------------------------------------
// Data level: the production fetch chain hits the blocker endpoints
// ---------------------------------------------------------------------------

/// One SSE frame: /api/event payloads are JSON-in-string.
fn sse_frame(id: &str, type_: &str, data_json: &str) -> String {
    let event = format!("{{\"id\":\"{id}\",\"type\":\"{type_}\",\"data\":{data_json}}}");
    let escaped = event.replace('\\', "\\\\").replace('"', "\\\"");
    format!("data: \"{escaped}\"\n\n")
}

fn session_list_body() -> String {
    // ses_root + a live child + a grandchild: the family fetch set is
    // all three (open = [ses_root]).
    concat!(
        r#"{"cursor":{},"data":["#,
        r#"{"id":"ses_root","title":"Root","time":{"created":0,"updated":0},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}},"#,
        r#"{"id":"ses_child","parentID":"ses_root","title":"Child","time":{"created":0,"updated":0},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}},"#,
        r#"{"id":"ses_grand","parentID":"ses_child","title":"Grand","time":{"created":0,"updated":0},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}"#,
        r#"]}"#,
    )
    .to_owned()
}

fn permission_body(session: &str) -> String {
    format!(
        r#"{{"data":[{{"id":"p_{session}","sessionID":"{session}","action":"bash","resources":["/tmp"]}}]}}"#
    )
}

fn form_body(session: &str) -> String {
    format!(
        r#"{{"data":[{{"id":"f_{session}","sessionID":"{session}","title":"Confirm","fields":[]}}]}}"#
    )
}

/// The full reconnect → resync fetch chain over the production
/// `Connection` + `HttpResync`: the resubscribe re-lists the pending
/// permissions AND forms for every OPEN session and its descendants
/// (computed from the fresh snapshot's parent links), and the asks
/// ride the `Ingest::Resync` snapshot into the store.
#[tokio::test]
#[allow(clippy::too_many_lines)] // the scripted end-to-end listener
async fn reconnect_fetches_pending_permissions_and_forms() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let request_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&request_log);
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let log = Arc::clone(&log);
            let (mut reader, _) = socket.split();
            let mut request = vec![0u8; 8192];
            let _ = reader.read(&mut request).await;
            let head = String::from_utf8_lossy(&request);
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .split('?')
                .next()
                .unwrap_or_default()
                .to_owned();
            log.lock().expect("log").push(path.clone());
            let response = if path.starts_with("/api/event") {
                let body = sse_frame("e1", "server.connected", "{}");
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
                )
            } else if path == "/api/session" {
                let body = session_list_body();
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else if path.starts_with("/api/session/") && path.ends_with("/permission") {
                let session =
                    path.trim_start_matches("/api/session/").trim_end_matches("/permission");
                let body = permission_body(session);
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else if path.starts_with("/api/session/") && path.ends_with("/form") {
                let session = path.trim_start_matches("/api/session/").trim_end_matches("/form");
                let body = form_body(session);
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else if path.starts_with("/api/session/") && path.ends_with("/message") {
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 12\r\nconnection: close\r\n\r\n{\"data\":[]}".to_owned()
            } else {
                "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_owned()
            };
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    let client = oc_client::Client::new(&format!("http://{addr}")).expect("client");
    let fetcher = HttpResync::new(client.clone());
    // One open tab: ses_root (the family derives the descendants).
    fetcher
        .open_sessions_handle()
        .lock()
        .expect("open sessions")
        .clone_from(&vec!["ses_root".to_owned()]);

    let connection = Connection::with_config(
        client,
        ConnectionConfig {
            backoff_base: Duration::from_millis(10),
            backoff_max: Duration::from_millis(10),
            max_attempts: Some(1),
        },
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let runner = tokio::spawn(connection.run(tx, fetcher));

    let mut observed = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while observed.len() < 3 {
        let Some(message) = tokio::time::timeout_at(deadline, rx.recv()).await.expect("deadline")
        else {
            break;
        };
        observed.push(message);
    }
    let _ = runner.await;
    server.abort();

    // The blocker re-lists hit the wire for the WHOLE open family.
    let log = request_log.lock().expect("log").clone();
    for expected in [
        "/api/session/ses_root/permission",
        "/api/session/ses_child/permission",
        "/api/session/ses_grand/permission",
        "/api/session/ses_root/form",
        "/api/session/ses_child/form",
        "/api/session/ses_grand/form",
    ] {
        assert!(
            log.iter().any(|path| path == expected),
            "resync must re-list {expected}; requests: {log:?}"
        );
    }
    // The snapshot carries the re-delivered asks.
    let snapshot = observed
        .iter()
        .find_map(|message| match message {
            Ingest::Resync { permissions, forms, .. } => Some((permissions, forms)),
            _ => None,
        })
        .expect("a resync snapshot");
    let (permissions, forms) = snapshot;
    assert_eq!(permissions.len(), 3, "one ask per family member: {permissions:?}");
    assert_eq!(forms.len(), 3, "one form per family member: {forms:?}");
    assert!(
        permissions.iter().any(|(id, list)| id == "ses_child"
            && list.first().is_some_and(|request| request.id == "p_ses_child")),
        "the descendant's offline ask rides the snapshot: {permissions:?}"
    );
}

// ---------------------------------------------------------------------------
// Unit: the family computation
// ---------------------------------------------------------------------------

#[test]
fn resync_family_open_plus_descendants() {
    let sessions = vec![
        snapshot("root", None),
        snapshot("child_a", Some("root")),
        snapshot("child_b", Some("root")),
        snapshot("grand", Some("child_a")),
        snapshot("unrelated", None),
        snapshot("unrelated_child", Some("unrelated")),
    ];
    let family = resync_family(&sessions, &["root".to_owned()]);
    assert_eq!(family, vec!["child_a", "child_b", "grand", "root"]);
}
