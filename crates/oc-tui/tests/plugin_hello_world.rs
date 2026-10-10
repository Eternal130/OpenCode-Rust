//! The wave-1 TUI-side integration (oracle-gate11 §4): the hello-world
//! plugin loads through the real [`PluginHost`] (bun spawned from the
//! fixture's sandbox copy); its registration snapshot projects into
//! the store (palette data + owned keymap entries + theme), a
//! synthetic event ingested through the store reaches the plugin (the
//! post-ingest tap), and palette dispatch executes guest-side.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use oc_plugin_host::{BroadcastTap, TapEvent};
use oc_tui::components::command_palette::{CommandDispatcher as _, Dispatch};
use oc_tui::data::{BusEvent, Ingest};
use oc_tui::keymap::{KeyStroke, Keymap, ManualClock, Outcome, OwnedCommandDef};
use oc_tui::reactive::Runtime;
use oc_tui::store::{EventTap, PluginCommandEntry, PluginCommandSink, Store};

/// Adapts the host's tap to the store's seam (the 10-line glue the
/// app layer owns in production).
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
        self.host.execute_command(command, None, Duration::from_secs(5)).is_ok()
    }
}

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-tui-plugin-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/helloworld.ts"),
        dir.join("helloworld.ts"),
    )
    .expect("fixture copy");
    dir
}

fn project(keymap: &mut Keymap, store: &Store, snapshot: &oc_plugin_host::RegistrationSnapshot) {
    // The registration-time projection: commands → owned keymap
    // entries + palette rows + theme documents.
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
    let themes: Vec<(String, String)> =
        snapshot.themes.iter().map(|theme| (theme.name.clone(), theme.document.clone())).collect();
    store.set_plugin_themes(&themes);
}

#[test]
fn plugin_registers_palette_keymap_theme_and_receives_events() {
    let sandbox = sandbox();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = Arc::new(
        oc_plugin_host::PluginHost::detect()
            .start_detached_with(config, &[sandbox.join("helloworld.ts")]),
    );
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready: {:?}", host.start_error());

    let store = Store::new(Runtime::new());
    let mut keymap = Keymap::new();
    let clock = ManualClock::default();
    let snapshot = host.registrations();
    project(&mut keymap, &store, &snapshot);
    drop(snapshot);

    // (a) The command appears in the palette data, group "Plugin".
    let props = oc_tui::components::command_palette::palette_props(&store);
    let row = props
        .options
        .iter()
        .find(|option| option.value == "hw.hello")
        .expect("hw.hello in the palette options");
    assert_eq!(row.category.as_deref(), Some("Plugin"));
    assert_eq!(row.title, "Say hello from the helloworld plugin");

    // (b) The owned keymap entry resolves (after static).
    assert_eq!(
        keymap.lookup(KeyStroke::ctrl("x"), &clock),
        Outcome::Pending { display: "ctrl+x".to_owned() }
    );
    assert_eq!(
        keymap.lookup(KeyStroke::new("h"), &clock),
        Outcome::Command { command: "hw.hello".to_owned(), args: None }
    );

    // (c) The theme document landed in the registry (discovery path).
    assert!(store.registry().get("helloworld-theme").is_some());

    // (d) A synthetic event ingested through the STORE reaches the
    // plugin (post-ingest tap → host dispatch → subscriber).
    store.set_event_tap(Arc::new(HostTap { tap: host.tap() }));
    store.ingest_all([Ingest::Event(BusEvent {
        id: "evt_tui_1".to_owned(),
        type_: "tui.test".to_owned(),
        data: serde_json::json!({ "payload": "from-tui" }),
        created: Some(1),
    })]);
    let marker = sandbox.join("tui-event.txt");
    let landed = {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if marker.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        marker.exists()
    };
    assert!(landed, "plugin received the ingested event");
    assert_eq!(std::fs::read_to_string(&marker).unwrap_or_default(), "tui.test:from-tui");

    // (e) Palette dispatch executes guest-side (timeout-bounded).
    let sink = Rc::new(HostSink { host: Arc::clone(&host), calls: AtomicUsize::new(0) });
    store.set_plugin_command_sink(Rc::clone(&sink) as Rc<dyn PluginCommandSink>);
    let mut dispatcher = oc_tui::components::command_palette::StoreDispatcher;
    assert_eq!(dispatcher.dispatch(&store, "hw.hello", None), Dispatch::Handled);
    assert_eq!(sink.calls.load(Ordering::Relaxed), 1);
    let executed = sandbox.join("tui-executed.txt");
    let landed = {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if executed.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        executed.exists()
    };
    assert!(landed, "guest-side execute ran");
    host.shutdown();
}
