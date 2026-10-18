//! CONFIG cluster regressions in the TUI (ledger CONFIG-C8, CONFIG-T1,
//! CONFIG-T3, CONFIG-T4): the session-retention effect bounds
//! `by_session` and `session.deleted` clears the family cache;
//! `session.permissions` from cli.json arms autoaccept at boot; the
//! `session.*` display keys get read-side consumers; the `mini.*`
//! display keys reach the mini runtime's lifecycle and scrollback
//! filters.
//!
//! Hermetic: /tmp/opencode sandboxes for the config roots, no network.

use oc_tui::data::{BusEvent, Ingest, SessionSnapshot};
use oc_tui::reactive::Runtime;
use oc_tui::store::{Route, Store};

/// One assistant message (the cheapest cache payload).
fn assistant(id: &str, text: &str) -> oc_tui::components::message::model::Message {
    use oc_tui::components::message::model::{Message, Part, PartTime};
    Message::Assistant {
        id: id.to_owned(),
        agent: "build".to_owned(),
        model: String::new(),
        error: None,
        parts: vec![Part::Text {
            text: text.to_owned(),
            time: PartTime { created: Some(1), completed: Some(2) },
        }],
        finish: Some("stop".to_owned()),
        time: PartTime { created: Some(1), completed: Some(2) },
        tokens: None,
        usage_total: None,
    }
}

fn snapshot(id: &str) -> SessionSnapshot {
    SessionSnapshot {
        model: None,
        id: id.to_owned(),
        title: Some(format!("Session {id}")),
        parent_id: None,
        time_idle: None,
        viewed: None,
    }
}

/// Ingests a session list so retention sees the listed roots.
fn list_sessions(store: &Store, ids: &[&str]) {
    store.ingest_all([Ingest::Resync {
        sessions: ids.iter().map(|id| snapshot(id)).collect(),
        unread: Vec::new(),
        messages: Vec::new(),
        permissions: Vec::new(),
        forms: Vec::new(),
    }]);
}

// ---------------------------------------------------------------------------
// CONFIG-C8 — session retention: by_session eviction + deleted clears
// ---------------------------------------------------------------------------

/// Navigating across sessions keeps only the open tabs, the routed
/// root, and the last 3 navigated roots (context/session-retention.ts
/// with `limit: 3`): every other listed root's cached messages evict.
#[test]
fn retention_evicts() {
    let store = Store::new(Runtime::new());
    // Tabs OFF (v2 `keep: () => (enabled() ? tabs : [])` — the
    // no-tabs environment): nothing pins roots, the recency window
    // alone decides.
    store.set_tabs_enabled(false);
    list_sessions(&store, &["ses_a", "ses_b", "ses_c", "ses_d", "ses_e", "ses_f"]);
    // The realistic load pattern: a session's transcript caches when
    // it is OPENED (the message fetch rides navigation), so each root
    // seeds as it is visited.
    for id in ["ses_a", "ses_b", "ses_c", "ses_d", "ses_e"] {
        store.navigate(Route::Session { session_id: id.to_owned() });
        store.set_messages(id, vec![assistant(&format!("msg_{id}"), id)]);
    }
    // A never-visited session's cache (a blocker fetch, a family
    // prefetch) also evicts at the next pass.
    store.set_messages("ses_f", vec![assistant("msg_f", "f")]);

    // The next navigation runs the retention pass: the window keeps
    // the newest 3 navigated roots + the routed root; ses_a/ses_b
    // (5 and 4 navigations back) fall out.
    store.navigate(Route::Session { session_id: "ses_e".to_owned() });
    assert!(
        store.messages("ses_a").is_empty(),
        "the oldest navigated root evicts (retention bounds by_session)"
    );
    assert!(store.messages("ses_b").is_empty(), "second-oldest evicts too");
    assert!(store.messages("ses_f").is_empty(), "never-navigated cache evicts");
    // The re-visited root + the 2 previous roots stay cached.
    assert!(!store.messages("ses_e").is_empty(), "the routed root stays");
    assert!(!store.messages("ses_d").is_empty(), "recent root 1 stays");
    assert!(!store.messages("ses_c").is_empty(), "recent root 2 stays");

    // An open TAB pins its root regardless of recency (tabs on again:
    // the keep set wins over the window).
    store.set_tabs_enabled(true);
    store.set_messages("ses_a", vec![assistant("msg_a2", "a")]);
    store.open_tab("ses_a", None);
    store.navigate(Route::Session { session_id: "ses_e".to_owned() });
    assert!(!store.messages("ses_a").is_empty(), "tabbed roots never evict");
}

/// `session.deleted` clears the family's cache, not just the tab
/// (client/solid/data.ts:642-644 removeSession + the evict surface).
#[test]
fn session_deleted_clears_cache() {
    let store = Store::new(Runtime::new());
    store.set_messages("ses_g", vec![assistant("msg_g", "g")]);
    store.open_tab("ses_g", None);
    assert!(!store.messages("ses_g").is_empty());

    store.ingest_all([Ingest::Event(BusEvent::new(
        "session.deleted",
        serde_json::json!({ "sessionID": "ses_g" }),
    ))]);
    assert!(store.messages("ses_g").is_empty(), "deleted clears the cached messages");
    assert!(
        !store.tabs().tabs.iter().any(|tab| tab.session_id == "ses_g"),
        "the tab closes (the pre-existing surface)"
    );
}
