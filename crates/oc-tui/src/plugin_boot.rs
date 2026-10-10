//! The production plugin boot for the TUI (the wave-12 integrator
//! step w9w2/w9w3 named): `app::run` constructs the [`PluginHost`]
//! from the resolved config's `plugins[]` entries and wires every seam
//! the wave-3 tests demonstrated —
//!
//! * the **event tap** (post-ingest `BroadcastTap`: plugins observe
//!   exactly the store's reduced, ordered event stream);
//! * the **palette sink** (`PluginCommandSink` → the host's
//!   timeout-bounded `execute_command_in_session`, carrying the ACTIVE
//!   session id — marker-style commands submit into that session);
//! * the **dialog source** (materialize-at-open through the host's
//!   hard-timeout-bounded call);
//! * the **registration projection** (palette rows, group rules, home
//!   quick actions into the store; keymap chords into the App's owned
//!   layer — re-projected after every keybind re-merge);
//! * the **per-pump UI-request drain** (`drain_ui_requests` →
//!   [`crate::plugin_ui::apply_plugin_ui`]);
//! * the **session port** (`ctx.session.*` from plugin commands → the
//!   SERVER over HTTP through oc-client — the sessions live
//!   server-side in the client/server split; prompt/get/list/messages/
//!   interrupt).
//!
//! Config discovery mirrors the plugin slice of oc-core's loader:
//! `OPENCODE_CONFIG` (explicit file) → the global config root's
//! `opencode.json`/`opencode.jsonc` → the cwd walk-up's files
//! (nearest last, so project entries append after global ones and
//! their removes override). JSONC tolerance via json5 (the same
//! parser choice oc-core's config lane made).
//!
//! A bun-less box degrades gracefully per bunman (managed download
//! first, then one clear log — the TUI runs plugin-less). A config
//! with NO plugin entries skips the host entirely (no bun process for
//! a plugin-less TUI — unlike the server, which keeps the host up for
//! the core Code Mode runtime).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use oc_client::Client;
use oc_plugin_host::registrations::RegistrationSnapshot;
use oc_plugin_host::{
    Bridge, BroadcastTap, PluginHost, PluginHostConfig, PluginOp, Resolution, SessionPort,
    TapEvent, UiRequest,
};

use crate::data::BusEvent;
use crate::keymap::OwnedCommandDef;
use crate::plugin_ui::{apply_plugin_ui, PluginUiEvent};
use crate::store::{
    EventTap, PluginCommandEntry, PluginDialogContent, PluginDialogOption, PluginDialogSource,
    Store,
};

/// How long boot waits for the registration pass (the flagship loads
/// in ~2-5 s).
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// The bound on one agent.list fetch before the host boot (the seed's
/// native agent table).
const SEED_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The bound on one palette command dispatch (host-side hard timeout).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// The bound on one dialog materialization (host-side hard timeout).
const DIALOG_TIMEOUT: Duration = Duration::from_secs(5);

/// The Send+Sync boot outcome (crosses the `spawn_blocking` boundary;
/// the [`TuiPluginRuntime`] wrapper adds the loop-thread-only session
/// cell).
pub struct BootedPluginHost {
    pub host: Arc<PluginHost>,
    pub resolution: Resolution,
    pub ready: bool,
}

/// The booted TUI plugin runtime.
pub struct TuiPluginRuntime {
    pub booted: BootedPluginHost,
    /// The active-session cell the palette sink reads (the run loop
    /// refreshes it each pump).
    pub current_session: Rc<RefCell<Option<String>>>,
}

// ---------------------------------------------------------------------------
// Config discovery (the plugin slice)
// ---------------------------------------------------------------------------

/// The config files whose `plugins` arrays merge (priority order):
/// explicit `OPENCODE_CONFIG` → global root → cwd walk-up (nearest
/// last). Mirrors oc-core's discovery order for the sources the
/// plugin surface reads.
#[must_use]
pub fn discover_config_files(config_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(explicit) = std::env::var_os("OPENCODE_CONFIG") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            files.push(path);
        }
    }
    for name in ["opencode.json", "opencode.jsonc"] {
        let path = config_root.join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    // Walk up from the cwd; ancestors nearest LAST (their entries
    // append — removes override the earlier ones).
    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut current = Some(cwd.to_path_buf());
    while let Some(directory) = current {
        current = directory.parent().map(Path::to_path_buf);
        ancestors.push(directory);
    }
    for directory in ancestors.into_iter().rev() {
        for name in ["opencode.json", "opencode.jsonc"] {
            let path = directory.join(name);
            if path.is_file() && !files.contains(&path) {
                files.push(path);
            }
        }
    }
    files
}

/// The merged `plugins[]` ops across the discovered files (JSONC
/// tolerant; unreadable files log + skip, never brick the TUI).
#[must_use]
pub fn collect_plugin_ops(files: &[PathBuf]) -> Vec<PluginOp> {
    let mut raw: Vec<Value> = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let Ok(document) = json5::from_str::<Value>(&text) else {
            crate::keymap::log_skip(&format!(
                "plugin config {}: unparseable (skipped)",
                path.display()
            ));
            continue;
        };
        if let Some(entries) = document.get("plugins").and_then(Value::as_array) {
            raw.extend(entries.iter().cloned());
        }
    }
    match oc_plugin_host::parse_ops(&raw) {
        Ok(ops) => ops,
        Err(error) => {
            crate::keymap::log_skip(&format!("config plugins[] rejected: {error}"));
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

impl TuiPluginRuntime {
    /// Wrap a boot outcome (the loop thread's wrapper).
    #[must_use]
    pub fn new(booted: BootedPluginHost) -> Self {
        Self { booted, current_session: Rc::new(RefCell::new(None)) }
    }

    /// Boot from the discovered config. `None` when no plugin entries
    /// are configured (the TUI skips the bun process entirely) — the
    /// caller treats that as "no plugin seams".
    ///
    /// Blocking (the registration wait): the app's run loop calls this
    /// through `spawn_blocking`, after [`fetch_agent_seed`].
    #[must_use]
    pub fn boot(
        client: &Client,
        agents: Vec<Value>,
        config_root: &Path,
        cwd: &Path,
        app_version: &str,
    ) -> Option<BootedPluginHost> {
        let files = discover_config_files(config_root, cwd);
        let ops = collect_plugin_ops(&files);
        if !ops.iter().any(|op| matches!(op, PluginOp::Add { .. })) {
            return None;
        }
        let resolution = oc_plugin_host::resolve(&ops, None);
        for (target, error) in &resolution.failures {
            crate::keymap::log_skip(&format!("plugin {target}: {error}"));
        }
        let entries = resolution.entries();
        if entries.is_empty() {
            return None;
        }

        let host_state = oc_plugin_host::HostState {
            app_version: app_version.to_owned(),
            project_id: String::new(),
            directory: cwd.display().to_string(),
            agents,
            mcps: std::collections::BTreeMap::default(),
        };
        let config = PluginHostConfig {
            host_state,
            session_port: Some(Arc::new(HttpSessionPort { client: client.clone() })),
            ..PluginHostConfig::default()
        };
        let host = Arc::new(PluginHost::detect().start_detached_with(config, &entries));
        if !host.enabled() {
            // bunman already logged the graceful-disable reason.
            return Some(BootedPluginHost { host, resolution, ready: false });
        }
        let ready = host.wait_ready(READY_TIMEOUT);
        if !ready {
            crate::keymap::log_skip(&format!(
                "plugin host did not come up: {:?}",
                host.start_error()
            ));
        }
        Some(BootedPluginHost { host, resolution, ready })
    }

    /// Attach every TUI seam (the `plugin_slots.rs` recipe): event tap,
    /// palette sink, dialog source, and the store-side registration
    /// projection. The owned keymap chords + plugin themes return for
    /// the App (chords re-applied after keybind re-merges; themes
    /// re-applied on every config reload's registry rebuild).
    pub fn attach(&self, store: &Store) -> PluginProjection {
        let host = &self.booted.host;
        store.set_event_tap(Arc::new(HostTap { tap: host.tap() }));
        store.set_plugin_command_sink(Rc::new(HostSink {
            host: Arc::clone(host),
            current: Rc::clone(&self.current_session),
        }));
        store.set_plugin_dialog_source(Rc::new(HostDialogSource { host: Arc::clone(host) }));
        let snapshot = host.registrations();
        project_registrations(store, &snapshot)
    }

    /// The per-pump UI-request drain: every queued plugin UI request
    /// lands in the store through the neutral apply seam.
    pub fn drain_ui_requests(&self, store: &Store) {
        for _ in 0..64 {
            let requests = self.booted.host.drain_ui_requests();
            if requests.is_empty() {
                break;
            }
            for request in requests {
                let plugin_id = request.plugin_id().to_owned();
                apply_plugin_ui(store, &plugin_id, &to_ui_event(&request));
            }
        }
    }

    /// Refresh the palette sink's active session (the run loop's
    /// per-pump step).
    pub fn set_current_session(&self, session: Option<String>) {
        *self.current_session.borrow_mut() = session;
    }

    /// Shut the guest down (the run loop's teardown).
    pub fn shutdown(&self) {
        self.booted.host.shutdown();
    }
}

/// The native agent table from the server's `agent.list` (v2
/// `Agent.Info` JSON; the wire rows carry the fields the guest's
/// editors snapshot — the flagship's setup REQUIRES `ctx.agent.transform`,
/// which snapshots the native table first). A failed fetch degrades to
/// the minimal builtin shape so the load still proceeds.
pub async fn fetch_agent_seed(client: &Client) -> Vec<Value> {
    let query = oc_client::types::AgentListQuery { location: None };
    let agent_api = client.agent();
    let list = agent_api.list(&query);
    match tokio::time::timeout(SEED_FETCH_TIMEOUT, list).await {
        Ok(Ok(response)) => response
            .data
            .iter()
            .map(|agent| {
                serde_json::json!({
                    "id": agent.id,
                    "name": agent.name,
                    "description": agent.description,
                    "mode": agent.mode.as_str(),
                    "hidden": agent.hidden,
                })
            })
            .collect(),
        _ => builtin_seed_agents(),
    }
}

/// The minimal native table (build/general/compaction/title — the
/// engine's builtin seed shape) when agent.list is unreachable.
fn builtin_seed_agents() -> Vec<Value> {
    [
        ("build", "Build", "primary", false),
        ("general", "General", "subagent", false),
        ("compaction", "Compaction", "primary", true),
        ("title", "Title", "primary", true),
    ]
    .into_iter()
    .map(|(id, name, mode, hidden)| {
        serde_json::json!({
            "id": id,
            "name": name,
            "mode": mode,
            "hidden": hidden,
        })
    })
    .collect()
}

// ---------------------------------------------------------------------------
// The registration projection (the plugin_slots.rs recipe)
// ---------------------------------------------------------------------------

/// The registration projection's App-side pieces (the store-side
/// pieces — palette rows, group rules, home actions — land inside
/// [`project_registrations`]).
pub struct PluginProjection {
    /// The owned keymap chords (`register_owned` onto the App's
    /// keymap; re-applied after keybind re-merges).
    pub owned: Vec<OwnedCommandDef>,
    /// Plugin theme documents (`(name, source)` — inserted into the
    /// theme registry, re-applied on config reloads).
    pub themes: Vec<(String, String)>,
}

/// Project the registration snapshot into the store (palette rows,
/// group rules, home quick actions) + the App-side pieces (owned
/// keymap chords, theme documents).
#[must_use]
pub fn project_registrations(store: &Store, snapshot: &RegistrationSnapshot) -> PluginProjection {
    let mut owned = Vec::new();
    for command in &snapshot.commands {
        if !command.keybinds.is_empty() {
            owned.push(OwnedCommandDef {
                command: command.name.clone(),
                description: command.description.clone(),
                defaults: command.keybinds.clone(),
                layer: "base".to_owned(),
                palette: command.palette,
                plugin: command.plugin_id.clone(),
            });
        }
    }
    let themes: Vec<(String, String)> =
        snapshot.themes.iter().map(|theme| (theme.name.clone(), theme.document.clone())).collect();
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
    store.set_plugin_group_rules(crate::components::message::model::GroupRules {
        rules: snapshot
            .group_rules
            .iter()
            .map(|rule| crate::components::message::model::GroupRule {
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
            .map(|action| crate::components::home::PluginQuickAction {
                plugin_id: action.plugin_id.clone(),
                label: action.label.clone(),
                command: action.command.clone(),
            })
            .collect(),
    );
    PluginProjection { owned, themes }
}

// ---------------------------------------------------------------------------
// The adapters (the app glue the wave-3 test demonstrated)
// ---------------------------------------------------------------------------

/// Adapts the store's post-ingest seam onto the host's broadcast tap.
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

/// Bridges the palette dispatch onto the host's timeout-bounded
/// `execute_command_in_session` (the ACTIVE session id — marker-style
/// commands submit their prompt into that session, v2's
/// `CommandInvocation.sessionID`).
///
/// The dispatch is fire-and-forget (v2's `command.execute` promise is
/// unawaited): the sink confirms the command is KNOWN, a dedicated
/// thread parks the host's hard timeout, and the command's UI effects
/// (toasts/dialogs) arrive through the per-pump drain. This also keeps
/// the sink legal inside the async run loop (the facade's `block_on`
/// may not nest in the executor).
struct HostSink {
    host: Arc<PluginHost>,
    current: Rc<RefCell<Option<String>>>,
}

impl crate::store::PluginCommandSink for HostSink {
    fn execute(&self, command: &str, args: Option<&str>) -> bool {
        let host = Arc::clone(&self.host);
        let command = command.to_owned();
        let args = args.map(str::to_owned);
        let session = self.current.borrow().clone().unwrap_or_default();
        std::thread::Builder::new()
            .name("oc-tui-plugin-command".into())
            .spawn(move || {
                let outcome = host.execute_command_in_session(
                    &command,
                    args.as_deref(),
                    &session,
                    COMMAND_TIMEOUT,
                );
                if let Err(error) = outcome {
                    crate::keymap::log_skip(&format!("plugin command {command}: {error}"));
                }
            })
            .is_ok()
    }
}

/// The delegation seam's app-side wiring: materialize through the
/// host's HARD-timeout-bounded call. The sync contract stays (the
/// pump blocks at most `DIALOG_TIMEOUT`); the call itself parks a
/// dedicated thread so the facade's `block_on` never nests inside the
/// async run loop.
struct HostDialogSource {
    host: Arc<PluginHost>,
}

impl PluginDialogSource for HostDialogSource {
    fn content(&self, id: &str) -> Option<PluginDialogContent> {
        let host = Arc::clone(&self.host);
        let id = id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("oc-tui-plugin-dialog".into())
            .spawn(move || {
                let outcome = host.materialize_dialog(&id, DIALOG_TIMEOUT);
                let _ = tx.send(outcome);
            })
            .ok()?;
        let spec: oc_plugin_host::PluginDialogSpec =
            rx.recv_timeout(DIALOG_TIMEOUT + Duration::from_secs(1)).ok()?.ok()?;
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
        UiRequest::SidebarSection { title, rows, .. } => {
            PluginUiEvent::SidebarSection { title: title.clone(), rows: rows.clone() }
        }
        UiRequest::FooterHint { text, .. } => PluginUiEvent::FooterHint { text: text.clone() },
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

// ---------------------------------------------------------------------------
// The HTTP session port (ctx.session.* → the server)
// ---------------------------------------------------------------------------
/// `ctx.session.*` from TUI-loaded plugins routes to the SERVER over
/// HTTP (the sessions live server-side in the client/server split —
/// the same calls the TUI's own sinks make). Prompt submits exactly
/// like the composer's `session.prompt` POST.
pub struct HttpSessionPort {
    client: Client,
}

#[async_trait]
impl SessionPort for HttpSessionPort {
    async fn call(&self, _bridge: &Bridge, method: &str, args: &Value) -> Result<Value, String> {
        let session_id = args.get("sessionID").and_then(Value::as_str).unwrap_or_default();
        match method {
            "prompt" => {
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
                self.client
                    .session()
                    .prompt(
                        session_id,
                        &oc_client::types::SessionPromptBody {
                            text: text.to_owned(),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "get" => {
                let session = self
                    .client
                    .session()
                    .get(session_id)
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&session).map_err(|error| error.to_string())
            }
            "list" => {
                let sessions = self
                    .client
                    .session()
                    .list(&oc_client::types::SessionListQuery { ..Default::default() })
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&sessions).map_err(|error| error.to_string())
            }
            "messages" => {
                let messages = self
                    .client
                    .message()
                    .list(session_id, &oc_client::types::SessionMessageListQuery::default())
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&messages).map_err(|error| error.to_string())
            }
            "interrupt" => {
                self.client
                    .session()
                    .interrupt(
                        session_id,
                        &oc_client::types::SessionInterruptQuery { ..Default::default() },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "update" => {
                let mut body = oc_client::types::SessionUpdateBody::default();
                if let Some(title) = args.get("title").and_then(Value::as_str) {
                    body.title = Some(title.to_owned());
                }
                self.client
                    .session()
                    .update(session_id, &body)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "remove" => {
                self.client
                    .session()
                    .remove(session_id)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "switchModel" => {
                let model = args.get("model").cloned().unwrap_or(Value::Null);
                let body = oc_client::types::SessionSwitchModelBody {
                    model: oc_client::types::ModelRef {
                        id: model.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        provider_id: model
                            .get("providerID")
                            .or_else(|| model.get("provider_id"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        variant: model.get("variant").and_then(Value::as_str).map(str::to_owned),
                    },
                };
                self.client
                    .session()
                    .switch_model(session_id, &body)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            other => Err(format!(
                "session.{other}: not wired in the TUI HTTP port (seam: prompt/get/list/messages/interrupt)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oc-tui-plugin-boot-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Discovery finds `OPENCODE_CONFIG`, the global root's files, and the
    /// cwd walk-up (nearest LAST); JSONC parses; unreadable files skip.
    #[test]
    fn discovers_and_merges_plugin_config_sources() {
        let root = temp_root("discovery");
        let global = root.join("global");
        let project = root.join("work").join("project");
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        // Global jsonc with a comment + one plugin; project json with
        // another + a remove of the global one.
        std::fs::write(
            global.join("opencode.jsonc"),
            "// plugin config\n{ \"plugins\": [\"global-pkg\"] }",
        )
        .unwrap();
        std::fs::write(
            project.join("opencode.json"),
            r#"{ "plugins": ["local-pkg", "-global-pkg"] }"#,
        )
        .unwrap();

        let saved = std::env::var_os("OPENCODE_CONFIG");
        std::env::remove_var("OPENCODE_CONFIG");
        let files = discover_config_files(&global, &project);
        assert_eq!(
            files,
            vec![global.join("opencode.jsonc"), project.join("opencode.json")],
            "global first, project (nearest) last"
        );
        let ops = collect_plugin_ops(&files);
        assert_eq!(
            ops,
            vec![
                PluginOp::Add { target: "global-pkg".into(), options: Value::Null },
                PluginOp::Add { target: "local-pkg".into(), options: Value::Null },
                PluginOp::Remove { target: "global-pkg".into() },
            ]
        );
        // An explicit OPENCODE_CONFIG file leads.
        let explicit = root.join("explicit.json");
        std::fs::write(
            &explicit,
            json!({ "plugins": [{ "package": "p", "options": { "k": 1 } }] }).to_string(),
        )
        .unwrap();
        std::env::set_var("OPENCODE_CONFIG", &explicit);
        let files = discover_config_files(&global, &project);
        assert_eq!(files.first(), Some(&explicit));
        let ops = collect_plugin_ops(&files);
        assert_eq!(
            ops.first(),
            Some(&PluginOp::Add { target: "p".into(), options: json!({ "k": 1 }) }),
            "the explicit file's entries lead"
        );
        match saved {
            Some(value) => std::env::set_var("OPENCODE_CONFIG", value),
            None => std::env::remove_var("OPENCODE_CONFIG"),
        }
        // An unparseable file logs + skips (never bricks the TUI).
        std::fs::write(global.join("opencode.jsonc"), "{ broken").unwrap();
        std::env::remove_var("OPENCODE_CONFIG");
        let files = discover_config_files(&global, &project);
        assert_eq!(collect_plugin_ops(&files).len(), 2, "the broken global skips");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The builtin seed shape (the fallback when agent.list is
    /// unreachable).
    #[test]
    fn builtin_seed_agents_cover_the_native_table() {
        let agents = builtin_seed_agents();
        let ids: Vec<&str> =
            agents.iter().filter_map(|agent| agent.get("id").and_then(Value::as_str)).collect();
        assert_eq!(ids, ["build", "general", "compaction", "title"]);
    }
}
