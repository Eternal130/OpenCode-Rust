//! CONFIG cluster regressions in the TUI (ledger CONFIG-C8, CONFIG-T1,
//! CONFIG-T3, CONFIG-T4): the session-retention effect bounds
//! `by_session` and `session.deleted` clears the family cache;
//! `session.permissions` from cli.json arms autoaccept at boot; the
//! `session.*` display keys get read-side consumers; the `mini.*`
//! display keys reach the mini runtime's fold gates, scrollback
//! filters, statusline details and the busy work-spinner cell.
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
        cursors: Vec::new(),
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

// ---------------------------------------------------------------------------
// CONFIG-T4 — mini display keys: MiniSettings resolve + fold gates
// ---------------------------------------------------------------------------

mod t4 {
    use oc_tui::mini::runtime::MiniSettings;
    use oc_tui::mini::scrollback::{Commit, CommitKind, Phase};

    /// Every `mini.*` display key resolves from the cli.json document
    /// with v2's per-key defaults (runtime.boot.ts:87-98).
    #[test]
    fn mini_keys_consumed() {
        // Defaults: thinking/turn_summary/footer/splash show, tools/
        // shell_output hide, mono off, block-soft-slide spinner.
        let defaults = MiniSettings::resolve(&oc_core::cli_config::Info::default());
        assert!(defaults.thinking);
        assert!(!defaults.tools, "tools default HIDE (runtime.ts:802)");
        assert!(!defaults.shell_output);
        assert!(defaults.turn_summary);
        assert!(defaults.footer);
        assert!(defaults.splash);
        assert!(!defaults.mono);
        assert_eq!(defaults.work_spinner, "block-soft-slide");

        // Every key maps from the document.
        let info = oc_core::cli_config::Info {
            mini: Some(oc_core::cli_config::MiniInfo {
                thinking: Some("hide".to_owned()),
                tools: Some("show".to_owned()),
                shell_output: Some("show".to_owned()),
                turn_summary: Some("hide".to_owned()),
                footer: Some("hide".to_owned()),
                splash: Some("hide".to_owned()),
                work_spinner: Some("seed".to_owned()),
                mono: Some(true),
                replay: None,
                replay_limit: None,
            }),
            ..Default::default()
        };
        let settings = MiniSettings::resolve(&info);
        assert!(!settings.thinking);
        assert!(settings.tools);
        assert!(settings.shell_output);
        assert!(!settings.turn_summary);
        assert!(!settings.footer);
        assert!(!settings.splash);
        assert!(settings.mono);
        assert_eq!(settings.work_spinner, "seed");
    }

    /// The transport-fold gates (stream-v2.transport.ts:682,870):
    /// `tools: false` drops tool commits, `thinking: false` drops
    /// reasoning commits — everything else always flows (defaults:
    /// tools hidden, thinking shown).
    #[test]
    fn mini_fold_gates() {
        let defaults = MiniSettings::resolve(&oc_core::cli_config::Info::default());
        let tool = Commit { kind: CommitKind::Tool, phase: Phase::Final, ..Commit::default() };
        let reasoning =
            Commit { kind: CommitKind::Reasoning, phase: Phase::Progress, ..Commit::default() };
        let assistant =
            Commit { kind: CommitKind::Assistant, phase: Phase::Final, ..Commit::default() };
        assert!(!defaults.commit_visible(&tool), "tools default hidden");
        assert!(defaults.commit_visible(&reasoning), "thinking default shown");
        assert!(defaults.commit_visible(&assistant));

        let flipped = MiniSettings::resolve(&oc_core::cli_config::Info {
            mini: Some(oc_core::cli_config::MiniInfo {
                thinking: Some("hide".to_owned()),
                tools: Some("show".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert!(flipped.commit_visible(&tool), "tools=show flows");
        assert!(!flipped.commit_visible(&reasoning), "thinking=hide drops reasoning");
        assert!(flipped.commit_visible(&assistant), "assistant text always flows");
    }

    /// `mini.shell_output` (tool.ts:1301 + scrollback.surface.ts:457):
    /// a shell-family tool's PROGRESS rows vanish when the knob hides
    /// them; the final row and non-shell tools always render
    /// (`bash` canonicalizes to `shell`, tool-display.ts:1-5).
    #[test]
    fn mini_shell_output_gates_shell_progress_rows() {
        use oc_tui::mini::scrollback::format_commit_with;
        let bash_progress = Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            text: "$ echo hi\nhi".to_owned(),
            tool: Some("bash".to_owned()),
            ..Commit::default()
        };
        let bash_final = Commit {
            kind: CommitKind::Tool,
            phase: Phase::Final,
            text: "Bash · echo hi".to_owned(),
            tool: Some("bash".to_owned()),
            ..Commit::default()
        };
        let read_progress = Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            text: "Read · main.rs".to_owned(),
            tool: Some("read".to_owned()),
            ..Commit::default()
        };

        let hidden = format_commit_with(None, &bash_progress, 80, false);
        assert!(hidden.is_empty(), "shell live rows hidden: {hidden:?}");
        let shown = format_commit_with(None, &bash_progress, 80, true);
        assert!(
            shown.iter().any(|row| row.contains("echo hi")),
            "shell_output=show streams the live rows: {shown:?}"
        );

        // The final row survives both settings.
        let final_hidden = format_commit_with(None, &bash_final, 80, false);
        assert!(
            final_hidden.iter().any(|row| row.contains("Bash")),
            "final row survives: {final_hidden:?}"
        );
        // Non-shell tools ignore the knob.
        let read_hidden = format_commit_with(None, &read_progress, 80, false);
        assert!(
            read_hidden.iter().any(|row| row.contains("Read")),
            "non-shell tools ignore the knob: {read_hidden:?}"
        );
    }

    /// `mini.footer` (footer.view.tsx:220,416-500): `hide` blanks the
    /// statusline's informational groups — the composer row stays.
    #[test]
    fn mini_footer_hides_statusline_details() {
        use oc_tui::components::footer::FooterTheme;
        use oc_tui::mini::lifecycle::FooterPane;

        let mut pane = FooterPane::new(FooterTheme::fallback_dark());
        pane.agent = Some("Build".into());
        pane.model_label = Some("gpt-5".into());
        let frame = pane.render_frame(40, 4);
        assert!(frame.text_grid()[1].starts_with("Build · gpt-5"), "details shown by default");

        pane.footer_details = false;
        let hidden = pane.render_frame(40, 4).text_grid();
        assert_eq!(hidden[1].trim(), "", "footer=hide blanks the status groups");
        assert!(hidden[0].starts_with('┃'), "the composer rail stays");
    }

    /// `mini.work_spinner` (footer.view.tsx:978-988): a running turn
    /// mounts the one-cell busy spinner at the statusline's left edge
    /// (glyph + gap — the status text renders into the narrowed
    /// width); `mono` swaps in the ASCII animation.
    #[test]
    fn mini_work_spinner_mounts_the_busy_cell() {
        use oc_tui::components::footer::FooterTheme;
        use oc_tui::mini::lifecycle::FooterPane;

        let mut pane = FooterPane::new(FooterTheme::fallback_dark());
        pane.agent = Some("Build".into());
        pane.model_label = Some("gpt-5".into());
        let idle = pane.render_frame(40, 4).text_grid();
        assert!(idle[1].starts_with("Build"), "no spinner while idle: {:?}", idle[1]);

        pane.set_busy(true);
        pane.work_spinner = "square-toggle".into();
        let busy = pane.render_frame(40, 4);
        let grid = busy.text_grid();
        assert!(
            matches!(grid[1].chars().next(), Some('□' | '■')),
            "the square-toggle frames mount at col 0: {:?}",
            grid[1]
        );
        assert_eq!(grid[1].chars().nth(1), Some(' '), "the gap cell follows");
        assert!(grid[1].contains("Build · gpt-5"), "the status text shifts right, not away");
        assert_eq!(busy.width, 40, "the frame keeps the terminal width");
        assert!(grid[0].starts_with('┃'), "the composer row keeps the full width");

        // mono swaps in the ASCII quartet (SEED_MONO — "-\\|/").
        pane.mono = true;
        let mono = pane.render_frame(40, 4).text_grid();
        assert!(
            matches!(mono[1].chars().next(), Some('-' | '\\' | '|' | '/')),
            "mono uses the ASCII animation: {:?}",
            mono[1]
        );

        // Idle clears the cell.
        pane.set_busy(false);
        let settled = pane.render_frame(40, 4).text_grid();
        assert!(settled[1].starts_with("Build"), "idle restores the plain statusline");
    }
}
