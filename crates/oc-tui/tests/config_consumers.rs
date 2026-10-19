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

// ---------------------------------------------------------------------------
// CONFIG-T1 — cli.json session.permissions read side
// ---------------------------------------------------------------------------

/// `session.permissions: "autoaccept"` arms the boot permission mode
/// exactly like `--auto` (context/permission.tsx:14); `prompt`/absent
/// stays manual, and the flag wins over everything.
#[test]
fn cli_json_autoaccept() {
    use oc_tui::config::TuiConfig;

    // The decoded-document mapping.
    let info = oc_core::cli_config::Info {
        session: Some(oc_core::cli_config::SessionInfo {
            permissions: Some("autoaccept".to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let config = TuiConfig::from_info(&info, None);
    assert!(config.permission_autoaccept(false), "autoaccept through cli.json without the flag");
    assert!(config.permission_autoaccept(true), "the flag stays a superset");

    // The truth table (v2's `args.auto ? "autoaccept" : config...`).
    let prompt = oc_core::cli_config::Info {
        session: Some(oc_core::cli_config::SessionInfo {
            permissions: Some("prompt".to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let config = TuiConfig::from_info(&prompt, None);
    assert!(!config.permission_autoaccept(false), "prompt stays manual");
    assert!(config.permission_autoaccept(true), "the flag overrides prompt");
    let config = TuiConfig::from_info(&oc_core::cli_config::Info::default(), None);
    assert!(!config.permission_autoaccept(false), "absent key defaults to prompt");

    // The raw-document parse path (cli.json on disk).
    let config = TuiConfig::parse(r#"{ "session": { "permissions": "autoaccept" } }"#, None);
    assert!(config.permission_autoaccept(false), "the parse path reads the key");
}

// ---------------------------------------------------------------------------
// CONFIG-T3 — session.* display keys: read side + rendering consumers
// ---------------------------------------------------------------------------

mod t3 {
    use oc_tui::components::message::list::{
        render_transcript, FollowState, ThinkingMode, TranscriptOptions, Verbosity,
    };
    use oc_tui::components::message::model::{Message, Part, PartTime, ToolState, ToolStatus};
    use oc_tui::theme::ThemeState;

    fn t3_messages() -> Vec<Message> {
        vec![
            Message::User {
                id: "t3-user".into(),
                text: "plan it".into(),
                created: 0,
                files: Vec::new(),
            },
            Message::Assistant {
                id: "t3-assistant".into(),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![
                    Part::Reasoning {
                        text: "INNER-THOUGHT-XYZ planning the run".into(),
                        time: PartTime { created: Some(10), completed: Some(12) },
                    },
                    Part::Text {
                        text: "**BOLD-MARKER** answer".into(),
                        time: PartTime { created: Some(10), completed: Some(12) },
                    },
                    Part::Tool {
                        id: "tool_1".into(),
                        name: "read".into(),
                        time: PartTime { created: Some(10), completed: Some(11) },
                        state: ToolState {
                            status: ToolStatus::Completed,
                            input: [("path".to_owned(), "src/main.rs".to_owned())]
                                .into_iter()
                                .collect(),
                            ..ToolState::default()
                        },
                    },
                ],
                finish: Some("stop".into()),
                error: None,
                time: PartTime { created: Some(1000), completed: Some(2000) },
                tokens: Some(900),
                usage_total: None,
            },
        ]
    }

    fn frame_text(options: &TranscriptOptions) -> String {
        let theme = ThemeState::v2_dark().styles();
        let transcript = render_transcript(&t3_messages(), options, &theme, 80);
        let frame = transcript.paint(
            FollowState::Free { offset: 0, maximum: 200 },
            &theme,
            (80, 200),
            None,
        );
        frame.text_grid().join("\n")
    }

    /// Every `session.*` key has a read-side consumer: the config maps
    /// the key, the store carries it, and the transcript rendering
    /// changes observable output per key (thinking disclosure,
    /// verbosity expansion/wrapping, the tps span, markdown source,
    /// the grouping seed).
    #[test]
    fn session_keys_consumed() {
        // 1. Defaults (hide / medium / tps off / rendered): the
        // collapsed Thought header, no thought text, no tok/s span,
        // no raw markdown markers.
        let base = frame_text(&TranscriptOptions::default());
        assert!(base.contains("Thought"), "hide mode keeps the collapsed header");
        assert!(!base.contains("INNER-THOUGHT-XYZ"), "hide collapses the thought body");
        assert!(!base.contains("tok/s"), "tps defaults off");
        assert!(!base.contains("**BOLD-MARKER**"), "rendered markdown consumes markers");

        // 2. thinking=show: thoughts render inline.
        let options =
            TranscriptOptions { thinking_mode: ThinkingMode::Show, ..TranscriptOptions::default() };
        let shown = frame_text(&options);
        assert!(shown.contains("INNER-THOUGHT-XYZ"), "show renders the thought body");

        // 3. verbosity=high: the exploration group opens — the tool
        // caption becomes visible.
        let options =
            TranscriptOptions { verbosity: Verbosity::High, ..TranscriptOptions::default() };
        let high = frame_text(&options);
        assert!(
            high.contains("src/main.rs"),
            "high opens exploration summaries (the read's file shows)"
        );

        // 4. verbosity=low: one activity summary wraps the run —
        // neither the thought header nor the body render standalone.
        let options =
            TranscriptOptions { verbosity: Verbosity::Low, ..TranscriptOptions::default() };
        let low = frame_text(&options);
        assert!(!low.contains("Thought"), "low wraps thoughts into the activity summary");
        assert!(!low.contains("INNER-THOUGHT-XYZ"), "the body stays inside the summary");

        // 5. tps: the `· X.X tok/s` span appears only when enabled.
        let options = TranscriptOptions { tps_enabled: true, ..TranscriptOptions::default() };
        let tps = frame_text(&options);
        assert!(tps.contains("900.0 tok/s"), "the span renders with the key on");

        // 6. markdown=source: the raw markers survive.
        let options = TranscriptOptions { markdown_source: true, ..TranscriptOptions::default() };
        let source = frame_text(&options);
        assert!(source.contains("**BOLD-MARKER**"), "source keeps the raw markdown text");
    }

    /// The config side: `TuiConfig` maps every key from the decoded
    /// cli.json document, and the grouping seed restores the
    /// exploration toggle (`grouping=none` applies after startup).
    #[test]
    fn session_keys_config_mapping() {
        use oc_tui::config::TuiConfig;
        use oc_tui::reactive::Runtime;
        use oc_tui::store::Store;

        let info = oc_core::cli_config::Info {
            session: Some(oc_core::cli_config::SessionInfo {
                thinking: Some("show".to_owned()),
                verbosity: Some("low".to_owned()),
                tps: Some(true),
                markdown: Some("source".to_owned()),
                grouping: Some("none".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let config = TuiConfig::from_info(&info, None);
        assert!(config.session_thinking);
        assert_eq!(config.session_verbosity.as_deref(), Some("low"));
        assert!(config.session_tps);
        assert!(config.session_markdown_source);
        assert!(!config.session_grouping, "grouping=none seeds the toggle off");

        // The store boot restore: grouping=none survives startup.
        let store = Store::new(Runtime::new());
        assert!(store.exploration_grouping(), "default on");
        store.restore_session_grouping(config.session_grouping);
        assert!(!store.exploration_grouping(), "grouping=none applies after startup");
        store.set_session_thinking(true);
        assert!(store.session_thinking());
        store.set_session_verbosity("high");
        assert_eq!(store.session_verbosity(), "high");
        store.set_session_tps(true);
        assert!(store.session_tps());
        store.set_session_markdown_source(true);
        assert!(store.session_markdown_source());
    }
}
