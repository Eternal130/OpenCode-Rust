//! The wave-3 TUI-side integration: the tuislots fixture loads through
//! the real [`PluginHost`]; its registration snapshot projects into
//! the store (owned keymap chord + palette row + group rules + home
//! action); a tapped event drives the plugin to fill every runtime
//! slot (toast / sidebar section / footer hint / dialog open) which
//! the drained UI-request queue applies through
//! [`oc_tui::plugin_ui`]; the dialog provider materializes at open
//! through the delegation seam (hard-timeout-bounded on the host
//! side) and renders + submits natively; the plugin grouping rule
//! renders its custom header in the transcript.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use oc_plugin_host::{BroadcastTap, PluginDialogSpec, TapEvent, UiRequest};
use oc_tui::components::command_palette::{Dispatch, StoreDispatcher};
use oc_tui::components::message::list::{render_transcript, TranscriptOptions};
use oc_tui::components::message::model::{Message, Part, PartTime, ToolState};
use oc_tui::data::{BusEvent, Ingest};
use oc_tui::keymap::{KeyStroke, Keymap, ManualClock, Outcome, OwnedCommandDef};
use oc_tui::plugin_ui::{apply_plugin_ui, PluginUiEvent};
use oc_tui::reactive::Runtime;
use oc_tui::store::{
    EventTap, PluginCommandEntry, PluginCommandSink, PluginDialogContent, PluginDialogOption,
    PluginDialogSource, Store,
};

/// Adapts the host's tap to the store's seam (the app layer's glue).
struct HostTap {
    tap: BroadcastTap,
}

impl EventTap for HostTap {
    fn publish(&self, event: &BusEvent) {
        self.tap.publish(TapEvent {
            id: event.id.clone(),
            typ: event.type_.clone(),
            data: event.data.clone(),
            created: event.created,
        });
    }
}

/// Bridges the palette dispatch to the host's timeout-bounded execute.
struct HostSink {
    host: Arc<oc_plugin_host::PluginHost>,
    calls: AtomicUsize,
}

impl PluginCommandSink for HostSink {
    fn execute(&self, command: &str, _args: Option<&str>) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.host.execute_command(command, None, Duration::from_secs(10)).is_ok()
    }
}

/// The delegation seam's app-side wiring: materialize through the
/// host's HARD-timeout-bounded call.
struct HostDialogSource {
    host: Arc<oc_plugin_host::PluginHost>,
}

impl PluginDialogSource for HostDialogSource {
    fn content(&self, id: &str) -> Option<PluginDialogContent> {
        let spec: PluginDialogSpec =
            self.host.materialize_dialog(id, Duration::from_secs(5)).ok()?;
        Some(PluginDialogContent {
            title: spec.title,
            rows: spec.rows,
            options: spec
                .options
                .into_iter()
                .map(|option| PluginDialogOption {
                    title: option.title,
                    value: option.value,
                    description: option.description,
                })
                .collect(),
        })
    }
}

/// The host → TUI UI-request conversion (the app layer's glue).
fn to_ui_event(request: &UiRequest) -> PluginUiEvent {
    match request {
        UiRequest::Toast { title, message, variant, duration_ms, .. } => PluginUiEvent::Toast {
            title: title.clone(),
            message: message.clone(),
            variant: variant.clone(),
            duration_ms: *duration_ms,
        },
        UiRequest::SidebarSection { title, rows, clickable, .. } => PluginUiEvent::SidebarSection {
            title: title.clone(),
            rows: rows.clone(),
            clickable: clickable.clone(),
        },
        UiRequest::NavigateToSession { session_id } => {
            PluginUiEvent::NavigateToSession { session_id: session_id.clone() }
        }
        UiRequest::FooterHint { text, .. } => PluginUiEvent::FooterHint { text: text.clone() },
        // TUI-25: the deferred dialog requests open through the same
        // conversion as the production pump.
        UiRequest::Alert { request_id, title, message, .. } => PluginUiEvent::Alert {
            request_id: *request_id,
            title: title.clone(),
            message: message.clone(),
        },
        UiRequest::Confirm { request_id, title, message, confirm_label, cancel_label, .. } => {
            PluginUiEvent::Confirm {
                request_id: *request_id,
                title: title.clone(),
                message: message.clone(),
                confirm_label: confirm_label.clone(),
                cancel_label: cancel_label.clone(),
            }
        }
        UiRequest::NavigateToPluginRoute { name, data, .. } => {
            PluginUiEvent::NavigateToPluginRoute { name: name.clone(), data: data.clone() }
        }
        UiRequest::DialogOpen { spec, .. } => PluginUiEvent::DialogOpen {
            content: PluginDialogContent {
                title: spec.title.clone(),
                rows: spec.rows.clone(),
                options: spec
                    .options
                    .iter()
                    .map(|option| PluginDialogOption {
                        title: option.title.clone(),
                        value: option.value.clone(),
                        description: option.description.clone(),
                    })
                    .collect(),
            },
        },
    }
}

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-tui-slots-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../oc-plugin-host/tests/fixtures/tuislots.ts"),
        dir.join("tuislots.ts"),
    )
    .expect("fixture copy");
    dir
}

/// Boot the host + store with every seam wired (the app assembly).
fn boot() -> Option<(Arc<oc_plugin_host::PluginHost>, Store, PathBuf)> {
    let sandbox = sandbox();
    let entry = sandbox.join("tuislots.ts");
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready: {:?}", host.start_error());
    let host = Arc::new(host);

    let store = Store::new(Runtime::new());
    store.set_event_tap(Arc::new(HostTap { tap: host.tap() }));
    store.set_plugin_command_sink(Rc::new(HostSink {
        host: Arc::clone(&host),
        calls: AtomicUsize::new(0),
    }));
    store.set_plugin_dialog_source(Rc::new(HostDialogSource { host: Arc::clone(&host) }));
    Some((host, store, sandbox))
}

/// The registration-time projection (the app's snapshot application).
fn project(keymap: &mut Keymap, store: &Store, snapshot: &oc_plugin_host::RegistrationSnapshot) {
    for command in &snapshot.commands {
        if !command.keybinds.is_empty() {
            keymap.register_owned(OwnedCommandDef {
                command: command.name.clone(),
                description: command.description.clone(),
                defaults: command.keybinds.clone(),
                layer: "base".to_owned(),
                palette: command.palette,
                plugin: command.plugin_id.clone(),
            });
        }
    }
    let palette: Vec<PluginCommandEntry> = snapshot
        .commands
        .iter()
        .filter(|command| command.palette)
        .map(|command| PluginCommandEntry {
            command: command.name.clone(),
            title: command.description.clone(),
            plugin_id: command.plugin_id.clone(),
        })
        .collect();
    store.set_plugin_commands(palette);
    store.set_plugin_group_rules(oc_tui::components::message::model::GroupRules {
        rules: snapshot
            .group_rules
            .iter()
            .map(|rule| oc_tui::components::message::model::GroupRule {
                tool: rule.tool.clone(),
                tool_prefix: rule.tool_prefix.clone(),
                header: rule.header.clone(),
            })
            .collect(),
    });
    store.set_plugin_home_actions(
        snapshot
            .home_actions
            .iter()
            .map(|action| oc_tui::components::home::PluginQuickAction {
                plugin_id: action.plugin_id.clone(),
                label: action.label.clone(),
                command: action.command.clone(),
            })
            .collect(),
    );
}

#[test]
#[allow(clippy::too_many_lines)] // the slot-bridge walkthrough: one phase per seam
fn tui_slots_end_to_end() {
    let Some((host, store, sandbox)) = boot() else { return };

    // -- registration projection -------------------------------------------
    let snapshot = host.registrations();
    let mut keymap = Keymap::new();
    keymap.push_mode("base");
    project(&mut keymap, &store, &snapshot);

    // The owned chord resolves end-to-end: ctrl+x then j → slots.open
    // (`j` is deliberately free of a native leader chord — static
    // entries win over owned ones by design; wave 1 pins that
    // precedence separately).
    let clock = ManualClock::default();
    let leader = KeyStroke::parse("ctrl+x").unwrap();
    let outcome = keymap.lookup(leader, &clock);
    assert!(
        matches!(outcome, Outcome::Pending { .. }),
        "the leader arms the plugin chord: {outcome:?}"
    );
    match keymap.lookup(KeyStroke::parse("j").unwrap(), &clock) {
        Outcome::Command { command, .. } => assert_eq!(command, "slots.open"),
        other => panic!("the chord resolves the plugin command: {other:?}"),
    }

    // The palette lists it (group "Plugin").
    let props = oc_tui::components::command_palette::palette_props(&store);
    let row = props
        .options
        .iter()
        .find(|option| option.value == "slots.open")
        .expect("plugin row in the palette");
    assert_eq!(row.category.as_deref(), Some("Plugin"));

    // The home quick action renders on the footer row.
    let theme = oc_tui::theme::ThemeState::v2_dark();
    let home = oc_tui::components::home::render_home(&store, &theme.styles(), (80, 30));
    assert!(home.text_grid()[28].contains("/slots"), "quick action label on the home footer");

    // -- event → plugin → UI requests → store ------------------------------
    store.ingest_all([Ingest::Event(BusEvent::new(
        "tui.poke",
        serde_json::json!({ "payload": "go" }),
    ))]);
    assert!(
        wait_for(Duration::from_secs(10), || sandbox.join("registration.txt").exists()),
        "the plugin handler ran to completion"
    );
    // Drain-then-apply is the pump's per-frame step; loop until every
    // request landed.
    for _ in 0..50 {
        let requests = host.drain_ui_requests();
        if requests.is_empty() {
            break;
        }
        for request in requests {
            apply_plugin_ui(&store, "tuislots", &to_ui_event(&request));
        }
    }

    // The toast landed through the SAME reducer as server events.
    let toasts = store.toasts();
    let toast = toasts.current().expect("the toast shows");
    assert_eq!(toast.message, "poked by the fixture");
    assert_eq!(toast.variant, "success");
    drop(toasts);

    // The sidebar section + footer hint materialized.
    let sections = store.plugin_sidebar_sections();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].title, "Fixture");
    assert_eq!(sections[0].rows, ["preset: zhipuai".to_owned(), "sessions: 7".to_owned()]);
    assert_eq!(store.plugin_footer_hints(), ["slots on".to_owned()]);

    // The LIVE route renders the plugin's section in the docked
    // sidebar pane (the `sidebar.content` slot bridge end-to-end):
    // at 160 the pane docks at col 118 with the section under the
    // title, the transcript column reflowed to 118.
    store.open_tab("ses_slots", Some("Slots session".to_owned()));
    store.navigate(oc_tui::store::Route::Session { session_id: "ses_slots".into() });
    let frame = oc_tui::components::session::render_session(
        &store,
        "ses_slots",
        &theme.styles(),
        (160, 30),
    );
    let grid = frame.text_grid();
    let pane: Vec<String> = grid.iter().map(|row| row.chars().skip(118).collect()).collect();
    assert!(pane[2].starts_with("  Slots session"), "the live title: {:?}", pane[2]);
    assert!(pane[3].starts_with("  Fixture"), "the plugin section title: {:?}", pane[3]);
    assert!(pane[4].starts_with("  preset: zhipuai"), "the plugin row 1: {:?}", pane[4]);
    assert!(pane[5].starts_with("  sessions: 7"), "the plugin row 2: {:?}", pane[5]);

    // The fully-built dialog opened and renders its rows natively.
    let top = store.top_dialog().expect("the poked dialog opened");
    assert!(matches!(top.kind, oc_tui::store::DialogKind::Plugin { .. }));
    let mut frame = blank(80, 24);
    oc_tui::components::dialog::render_overlay(&mut frame, &store, &theme, (80, 24));
    let rows = frame.text_grid();
    assert!(rows[7].contains("Poked"), "the dialog title: {:?}", rows[7]);
    assert!(rows[9].contains("row one"), "dialog row 1: {:?}", rows[9]);
    assert_eq!(store.dialogs().len(), 1);
    store.clear_dialogs();

    // -- materialize-at-open through the delegation source ------------------
    assert!(store.open_plugin_dialog("tuislots:picker"));
    let mut frame = blank(80, 24);
    oc_tui::components::dialog::render_overlay(&mut frame, &store, &theme, (80, 24));
    let rows = frame.text_grid();
    assert!(rows[7].contains("Slot picker"), "materialized title: {:?}", rows[7]);
    assert!(rows[12].contains("First"), "materialized option 1: {:?}", rows[12]);

    // Submit selects the option's value → the palette dispatcher's
    // plugin arm executes guest-side (marker file).
    let mut keymap = Keymap::new();
    keymap.push_mode("dialog");
    let outcome = oc_tui::components::dialog::handle_dialog_key(
        &store,
        &mut keymap,
        &oc_tui::keymap::ManualClock::default(),
        KeyStroke::parse("return").unwrap(),
        (80, 24),
    );
    assert_eq!(
        outcome,
        oc_tui::components::dialog::DialogKeyOutcome::Submitted("slots.open".to_owned())
    );
    let mut dispatcher = StoreDispatcher;
    assert_eq!(
        oc_tui::components::dialog::execute_submission(&store, &mut dispatcher, "slots.open"),
        Dispatch::Handled
    );
    assert!(
        wait_for(Duration::from_secs(10), || sandbox.join("opened.txt").exists()),
        "the palette command executed guest-side"
    );

    host.shutdown();
}

#[test]
fn plugin_group_rules_claim_raw_tool_names() {
    use oc_tui::components::message::model::{GroupKind, GroupRule, GroupRules};
    let rules = GroupRules {
        rules: vec![
            GroupRule { tool: None, tool_prefix: Some("omo_".into()), header: "OMO tools".into() },
            GroupRule {
                tool: Some("specialprobe".into()),
                tool_prefix: None,
                header: "Probe".into(),
            },
        ],
    };
    assert_eq!(rules.group_for("omo_admission"), Some(GroupKind::Plugin("OMO tools".into())));
    assert_eq!(rules.group_for("specialprobe"), Some(GroupKind::Plugin("Probe".into())));
    assert_eq!(rules.group_for("bash"), None, "unclaimed tools stand alone");
    assert_eq!(rules.group_for("grep"), None, "native exploration stays native");
    assert_eq!(GroupRules::empty().group_for("omo_x"), None);
}

#[test]
#[allow(clippy::too_many_lines)] // a full-screen fixture assembly
fn plugin_group_rule_renders_a_custom_header() {
    // Pure TUI side: rules (as projected above) group omo_* tools.
    let store = Store::new(Runtime::new());
    store.set_plugin_group_rules(oc_tui::components::message::model::GroupRules {
        rules: vec![
            oc_tui::components::message::model::GroupRule {
                tool: None,
                tool_prefix: Some("omo_".into()),
                header: "OMO tools".into(),
            },
            oc_tui::components::message::model::GroupRule {
                tool: Some("specialprobe".into()),
                tool_prefix: None,
                header: "Probe".into(),
            },
        ],
    });
    let messages = vec![
        Message::Assistant {
            id: "a1".into(),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts: vec![
                Part::Tool {
                    id: "t1".into(),
                    name: "omo_admission".into(),
                    time: PartTime { created: Some(1), completed: Some(2) },
                    state: ToolState::default(),
                },
                Part::Tool {
                    id: "t2".into(),
                    name: "omo_scheduler".into(),
                    time: PartTime { created: Some(3), completed: Some(4) },
                    state: ToolState::default(),
                },
                Part::Tool {
                    id: "t3".into(),
                    name: "specialprobe".into(),
                    time: PartTime { created: Some(5), completed: Some(6) },
                    state: ToolState::default(),
                },
                Part::Tool {
                    id: "t4".into(),
                    name: "bash".into(),
                    time: PartTime { created: Some(7), completed: Some(8) },
                    state: ToolState::default(),
                },
            ],
            finish: None,
            error: None,
            time: PartTime { created: Some(1), completed: None },
            tokens: None,
            usage_total: None,
        },
        Message::Idle { id: "idle".into(), created: 9 },
    ];
    let theme = oc_tui::theme::ThemeState::v2_dark();
    let transcript = render_transcript(
        &messages,
        &TranscriptOptions {
            group_rules: store.plugin_group_rules(),
            ..TranscriptOptions::default()
        },
        &theme.styles(),
        80,
    );
    let frame = transcript.paint(
        oc_tui::components::message::list::FollowState::Sticky,
        &theme.styles(),
        (80, 24),
        None,
    );
    let grid = frame.text_grid().join("\n");
    assert!(grid.contains("OMO tools (2)"), "the prefix rule's header with its count:\n{grid}");
    assert!(grid.contains("Probe"), "the exact-match rule's header:\n{grid}");
    // `bash` renders no PLUGIN group (ungrouped tool parts are the
    // tool_rows surface, not transcript rows — pinned by the unit
    // test's `group_for("bash") == None`).
    // Native grouping is untouched by rules: a grep tool still groups
    // as exploration even with rules present.
    let native = vec![Message::Assistant {
        id: "a2".into(),
        agent: "build".into(),
        model: "fixture/fixture".into(),
        parts: vec![Part::Tool {
            id: "t9".into(),
            name: "grep".into(),
            time: PartTime { created: Some(1), completed: Some(2) },
            state: ToolState::default(),
        }],
        finish: None,
        error: None,
        time: PartTime { created: Some(1), completed: None },
        tokens: None,
        usage_total: None,
    }];
    let transcript = render_transcript(
        &native,
        &TranscriptOptions {
            group_rules: store.plugin_group_rules(),
            ..TranscriptOptions::default()
        },
        &theme.styles(),
        80,
    );
    let frame = transcript.paint(
        oc_tui::components::message::list::FollowState::Sticky,
        &theme.styles(),
        (80, 24),
        None,
    );
    assert!(
        frame.text_grid().join("\n").contains("Explor"),
        "the native exploration group wins over plugin rules"
    );
}

fn blank(width: u16, height: u16) -> oc_tui::render::Frame {
    let cell = oc_tui::render::Cell {
        ch: ' ',
        attrs: 0,
        fg: oc_tui::render::CellColor::Rgb(255, 255, 255),
        bg: oc_tui::render::CellColor::Rgb(10, 10, 10),
    };
    oc_tui::render::Frame {
        cells: vec![vec![cell; usize::from(width)]; usize::from(height)],
        width: usize::from(width),
        height: usize::from(height),
    }
}

/// Poll every 25 ms until `probe` holds or `timeout` elapses.
fn wait_for(timeout: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if probe() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}
