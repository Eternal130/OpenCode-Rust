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
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use oc_client::Client;
use oc_plugin_host::registrations::RegistrationSnapshot;
use oc_plugin_host::{
    Bridge, BroadcastTap, PluginHost, PluginHostConfig, PluginOp, PluginSectionSpec, Resolution,
    SessionPort, TapEvent, UiRequest,
};

use crate::data::BusEvent;
use crate::keymap::OwnedCommandDef;
use crate::plugin_ui::{apply_plugin_ui, PluginUiEvent};
use crate::store::{
    EventTap, McpServerRow, PluginCommandEntry, PluginDialogContent, PluginDialogOption,
    PluginDialogSource, Store,
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

/// The bound on one plugin sidebar row click (the guest runs the
/// plugin's `onMouseUp` synchronously — same hard bound family).
const CLICK_TIMEOUT: Duration = Duration::from_secs(2);

/// The Send+Sync boot outcome (crosses the `spawn_blocking` boundary;
/// the [`TuiPluginRuntime`] wrapper adds the loop-thread-only session
/// cell).
pub struct BootedPluginHost {
    pub host: Arc<PluginHost>,
    pub resolution: Resolution,
    pub ready: bool,
}

/// The refreshed MCP rows waiting for the pump (the sync task's
/// post-push `mcp.list` re-fetch + the new pushed-name set).
type McpRowsRx = Rc<RefCell<Option<tokio::sync::mpsc::Receiver<McpSyncOutcome>>>>;
/// One sync outcome: the refreshed rows + the new pushed-name set.
type McpSyncOutcome = (Vec<McpServerRow>, BTreeSet<String>);
/// The plugin route page's materialized content channel.
type RouteContentRx =
    Rc<RefCell<Option<std::sync::mpsc::Receiver<Result<PluginSectionSpec, String>>>>>;

/// The booted TUI plugin runtime.
pub struct TuiPluginRuntime {
    pub booted: BootedPluginHost,
    /// The active-session cell the palette sink reads (the run loop
    /// refreshes it each pump).
    pub current_session: Rc<RefCell<Option<String>>>,
    /// The last route pushed to the guest (the v2 TUI-plugin surface
    /// — change-gated `tui.route` notifications). The outer Option is
    /// "nothing pushed yet"; the inner is "home vs session".
    #[allow(clippy::option_option)]
    pushed_route: Rc<RefCell<Option<Option<String>>>>,
    /// The last theme fingerprint pushed (name + mode).
    pushed_theme: Rc<RefCell<Option<(String, bool)>>>,
    /// Whether the sidebar layout budget was pushed once (SA-F5: the
    /// width is a painter constant — a single `tui.layout` push).
    pushed_layout: Rc<RefCell<bool>>,
    /// The active sidebar claim key per plugin (PLUG-TUI-SLOT: the
    /// click sink routes a section row release through the CLAIM's
    /// `onMouseUp`, not the legacy `sidebar_content` bridge).
    sidebar_claim_keys: Rc<RefCell<BTreeMap<String, String>>>,
    /// The MCP server names already pushed into the server's real
    /// registry (PLUG-D33: `registrations.mcps` → `experimental.
    /// mcp.add` — the ref's `ctx.mcp.transform` → real-registry
    /// editor seam, host.ts:374-403, over the wire in the split).
    pushed_mcps: Rc<RefCell<BTreeSet<String>>>,
    /// The refreshed MCP rows waiting for the pump (the sync task's
    /// post-push `mcp.list` re-fetch + the new pushed-name set).
    mcp_rows_rx: McpRowsRx,
    /// The owned keymap refresh queued for the App (dynamic layer
    /// registrations — a claim render installed a layer mid-flight).
    owned_updates: Rc<RefCell<Option<Vec<OwnedCommandDef>>>>,
    /// The last owned+palette fingerprint pushed (change detection
    /// for the dynamic keymap/palette refresh).
    dynamic_fingerprint: Rc<RefCell<String>>,
    /// The plugin route page's materialized content (the bounded
    /// `route.render` replies, applied by the pump).
    route_content_rx: RouteContentRx,
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
/// tolerant; unreadable files log + skip, never brick the TUI) —
/// PLUG-D01: each config file's parent directory (the v2 config layer
/// root) contributes its `plugin`/`plugins` drop-in children FIRST, so
/// an explicit `-name` remove in a document drops a discovered plugin.
#[must_use]
pub fn collect_plugin_ops(files: &[PathBuf]) -> Vec<PluginOp> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for file in files {
        if let Some(parent) = file.parent() {
            if !roots.contains(&parent.to_path_buf()) {
                roots.push(parent.to_path_buf());
            }
        }
    }
    let mut ops = oc_plugin_host::discover_directory_ops(&roots);
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
        Ok(document_ops) => ops.extend(document_ops),
        Err(error) => {
            crate::keymap::log_skip(&format!("config plugins[] rejected: {error}"));
        }
    }
    ops
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

impl TuiPluginRuntime {
    /// Wrap a boot outcome (the loop thread's wrapper).
    #[must_use]
    pub fn new(booted: BootedPluginHost) -> Self {
        Self {
            booted,
            current_session: Rc::new(RefCell::new(None)),
            pushed_route: Rc::new(RefCell::new(None)),
            pushed_theme: Rc::new(RefCell::new(None)),
            pushed_layout: Rc::new(RefCell::new(false)),
            sidebar_claim_keys: Rc::new(RefCell::new(BTreeMap::new())),
            pushed_mcps: Rc::new(RefCell::new(BTreeSet::new())),
            mcp_rows_rx: Rc::new(RefCell::new(None)),
            owned_updates: Rc::new(RefCell::new(None)),
            dynamic_fingerprint: Rc::new(RefCell::new(String::new())),
            route_content_rx: Rc::new(RefCell::new(None)),
        }
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
        // The plugin sections' click sink (beta.13 bug 2's
        // plugin-world path): a row release runs the plugin's own
        // `onMouseUp` through the host's `slot.click`; the plugin's
        // navigation flows back as a NavigateToSession UI request the
        // next drain applies.
        {
            let host = Arc::clone(host);
            let claim_keys = Rc::clone(&self.sidebar_claim_keys);
            store.set_plugin_section_click_sink(Some(Rc::new(
                move |plugin_id: &str, row: usize| {
                    // PLUG-TUI-SLOT: a claim-backed section routes
                    // through the claim's own handler table; the
                    // legacy `sidebar_content` bridge keeps its path.
                    let claim = claim_keys.borrow().get(plugin_id).cloned();
                    match claim {
                        Some(key) => {
                            let _ = host.click_slot_claim(plugin_id, &key, row, CLICK_TIMEOUT);
                        }
                        None => {
                            let _ = host.click_sidebar_row(plugin_id, row, CLICK_TIMEOUT);
                        }
                    }
                },
            )));
        }
        self.materialize_slot_claims(store);
        let snapshot = host.registrations();
        let projection = project_registrations(store, &snapshot);
        let owned = Self::owned_keymap_entries(&snapshot);
        *self.dynamic_fingerprint.borrow_mut() = dynamic_fingerprint(&owned, &snapshot);
        project_palette(store, &snapshot);
        PluginProjection { owned, themes: projection.themes }
    }

    /// Resolve + materialize the snapshot's UI slot claims
    /// (PLUG-TUI-SLOT): the pure router buckets the claims
    /// (`plugin_slots::resolve_slots` — the ref's structure.ts
    /// policy), then each live claim renders ONCE through the host's
    /// bounded call (sidebar claims land as sections; `app` claims
    /// run for their side effects — the ref's stats plugin installs
    /// its keymap layer from the app claim's render,
    /// stats.tsx:185-203).
    fn materialize_slot_claims(&self, store: &Store) {
        let host = &self.booted.host;
        let snapshot = host.registrations();
        let claims = crate::components::plugin_slots::project_claims(&snapshot.slot_claims);
        if claims.is_empty() {
            return;
        }
        let resolution = crate::components::plugin_slots::resolve_slots(
            &crate::components::plugin_slots::mounted_paths(),
            &claims,
        );
        for (path, bucket) in &resolution.slotted {
            for claim in bucket.prepend.iter().chain(&bucket.append) {
                let plugin = claim.plugin.clone();
                let key = claim.key.clone();
                let host = Arc::clone(host);
                let content = offload_bounded(Duration::from_secs(5), move || {
                    host.render_slot_claim(&plugin, &key, Duration::from_secs(4))
                });
                match content {
                    Ok(Ok(spec)) => {
                        if path == "sidebar.content" {
                            self.sidebar_claim_keys
                                .borrow_mut()
                                .insert(claim.plugin.clone(), claim.key.clone());
                            store.set_plugin_sidebar_section(
                                &crate::components::sidebar::SidebarSection {
                                    plugin_id: claim.plugin.clone(),
                                    title: spec.title,
                                    rows: spec.rows,
                                    clickable: spec.clickable,
                                },
                            );
                        }
                        // `app` claims run for their side effects (the
                        // renders install keymap layers guest-side;
                        // the next registration read picks them up).
                    }
                    Ok(Err(error)) | Err(error) => crate::keymap::log_skip(&format!(
                        "plugin slot claim {} {}: {error}",
                        claim.plugin, claim.key
                    )),
                }
            }
        }
    }

    /// The full owned keymap entry set (PLUG-TUI-SLOT): the
    /// `command.add` keybind chords PLUS every keymap-layer command
    /// (`ctx.keymap.layer` — api.tsx:179-199; dispatched as
    /// `<plugin>::<id>` through the palette sink).
    fn owned_keymap_entries(snapshot: &RegistrationSnapshot) -> Vec<OwnedCommandDef> {
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
        for command in &snapshot.keymap_commands {
            owned.push(OwnedCommandDef {
                command: format!("{}::{}", command.plugin_id, command.id),
                description: command.title.clone(),
                defaults: vec![command.bind.clone()],
                layer: "base".to_owned(),
                palette: command.palette,
                plugin: command.plugin_id.clone(),
            });
        }
        owned
    }

    /// The dynamic keymap refresh (the pump's post-drain step): a
    /// claim render that installed a layer mid-flight re-projects
    /// the owned set when its fingerprint changed, and the palette
    /// rows pick up palette-visible layer commands.
    fn refresh_dynamic_registrations(&self, store: &Store) {
        let snapshot = self.booted.host.registrations();
        let owned = Self::owned_keymap_entries(&snapshot);
        let fingerprint = dynamic_fingerprint(&owned, &snapshot);
        if fingerprint == *self.dynamic_fingerprint.borrow() {
            return;
        }
        *self.dynamic_fingerprint.borrow_mut() = fingerprint;
        *self.owned_updates.borrow_mut() = Some(owned);
        project_palette(store, &snapshot);
    }

    /// The App-side owned update, when the dynamic refresh queued one.
    #[must_use]
    pub fn take_owned_updates(&self) -> Option<Vec<OwnedCommandDef>> {
        self.owned_updates.borrow_mut().take()
    }

    /// PLUG-D33 — the registry integration seam: push the plugin
    /// snapshot's MCP servers into the REAL runtime registry the app
    /// serves/queries. The ref's plugin host mutates the MCP service
    /// editor directly (`ctx.mcp.transform` → `mcp.transform`,
    /// host.ts:374-403); in the client/server split the TUI plugin
    /// host carries the servers to the server's registry over the
    /// wire — `experimental.mcp.add` (`McpRegistry::add`: the config
    /// row lands, the connection starts, `mcp.status.changed`
    /// publishes) — so `/api/mcp` (the sidebar's MCP section +
    /// `DialogMcp`) and the tool bridge see them like every
    /// config-sourced server. After the diff settles, the refreshed
    /// `mcp.list` rows flow back for the pump to apply.
    pub fn start_mcp_sync(&self, client: &Client) {
        if !self.booted.host.enabled() {
            return;
        }
        // The diff inputs cross the spawn boundary as plain data (the
        // runtime's Rc cells stay loop-thread-owned).
        let snapshot = self.booted.host.registrations();
        let servers: BTreeMap<String, Value> =
            snapshot.mcps.iter().map(|mcp| (mcp.name.clone(), mcp.config.clone())).collect();
        let previously_pushed: BTreeSet<String> = self.pushed_mcps.borrow().clone();
        let client = client.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<(Vec<McpServerRow>, BTreeSet<String>)>(1);
        *self.mcp_rows_rx.borrow_mut() = Some(rx);
        tokio::spawn(async move {
            let (rows, pushed) = sync_and_fetch(&client, servers, previously_pushed).await;
            let _ = tx.send((rows, pushed)).await;
        });
    }

    /// The post-sync MCP rows (the pump applies them to the store
    /// and adopts the new pushed-name set).
    #[must_use]
    pub fn take_mcp_rows(&self) -> Option<McpSyncOutcome> {
        let mut slot = self.mcp_rows_rx.borrow_mut();
        let rx = slot.as_mut()?;
        rx.try_recv().ok()
    }

    /// Adopt the sync task's pushed-name set (the pump's step).
    pub fn adopt_pushed_mcps(&self, pushed: BTreeSet<String>) {
        *self.pushed_mcps.borrow_mut() = pushed;
    }

    /// Materialize the open plugin route page's content (the pump's
    /// step when a `NavigateToPluginRoute` opened a page without
    /// content yet) — bounded, thread-offloaded like the dialog
    /// source (the facade's `block_on` may not nest in the loop's
    /// executor). One materialization in flight at a time
    /// (idempotent across pumps); a closed page cancels the wait.
    fn materialize_open_route(&self, store: &Store) {
        if self.route_content_rx.borrow().is_some() {
            return; // already in flight
        }
        let Some(page) = store.plugin_route_page() else { return };
        if !page.rows.is_empty() || page.title != page.name {
            return;
        }
        let host = Arc::clone(&self.booted.host);
        let (tx, rx) = std::sync::mpsc::channel();
        *self.route_content_rx.borrow_mut() = Some(rx);
        let plugin = page.plugin_id.clone();
        let name = page.name.clone();
        let data = page.data.clone();
        let spawned = std::thread::Builder::new()
            .name("oc-tui-plugin-route".into())
            .spawn(move || {
                let content = host.render_route(&plugin, &name, data.as_ref(), DIALOG_TIMEOUT);
                let _ = tx.send(content);
            })
            .is_ok();
        if !spawned {
            *self.route_content_rx.borrow_mut() = None;
        }
    }

    /// Apply a finished route materialization (the pump's step); a
    /// closed page drops the in-flight wait.
    fn apply_route_content(&self, store: &Store) {
        let content = {
            let mut slot = self.route_content_rx.borrow_mut();
            let Some(rx) = slot.as_mut() else { return };
            match rx.try_recv() {
                Ok(content) => {
                    *slot = None;
                    content
                }
                Err(_) => return,
            }
        };
        if store.plugin_route_page().is_none() {
            return;
        }
        match content {
            Ok(spec) => store.set_plugin_route_content(&spec.title, spec.rows),
            // TUI-TD28: the bounded render replied miss — no
            // registered route `{plugin}/{name}` (v2's
            // `plugins.route(id, name)` undefined → the
            // `PluginRouteMissing` fallback).
            Err(_) => store.mark_plugin_route_missing(),
        }
    }
    /// The per-pump UI-request drain: every queued plugin UI request
    /// lands in the store through the neutral apply seam. The same
    /// pump pushes the live route + theme view to the guest (the v2
    /// TUI-plugin surface — the flagship sidebar reads both at every
    /// materialize; change-gated, fire-and-forget notifications).
    pub fn drain_ui_requests(&self, store: &Store) {
        if self.booted.host.enabled() {
            let route_session = match store.route() {
                crate::store::Route::Session { session_id } => Some(session_id),
                crate::store::Route::Home => None,
            };
            let changed = { *self.pushed_route.borrow() != Some(route_session.clone()) };
            if changed {
                *self.pushed_route.borrow_mut() = Some(route_session.clone());
                self.booted.host.notify_tui_route(route_session.as_deref());
            }
            let theme = store.theme();
            let fingerprint = (theme.name.clone(), theme.mode == oc_theme::types::Mode::Light);
            if *self.pushed_theme.borrow() != Some(fingerprint.clone()) {
                *self.pushed_theme.borrow_mut() = Some(fingerprint);
                let styles = theme.styles();
                self.booted.host.notify_tui_theme(serde_json::json!({
                    "accent": hex_color(styles.accent),
                    "background": hex_color(styles.background),
                    "borderActive": hex_color(styles.border),
                    "text": styles.text.fg.map_or_else(|| "#eeeeee".into(), hex_color),
                    "textMuted": styles
                        .text_muted
                        .fg
                        .map_or_else(|| "#888888".into(), hex_color),
                }));
            }
            // SA-F5: the sidebar's row budget — the guest flattener
            // lays the plugin's space-between/flex rows out against
            // the painter's actual cell span (pushed once; the width
            // is a layout constant).
            if !*self.pushed_layout.borrow() {
                *self.pushed_layout.borrow_mut() = true;
                let budget =
                    crate::components::session_tabs::SESSION_SIDEBAR_WIDTH.saturating_sub(4);
                self.booted.host.notify_tui_layout(budget);
            }
        }
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
        // TUI-24: the plugin fault table (the plugins dialog's
        // error-rows) — the host's fault strings, keyed by plugin id.
        if self.booted.host.enabled() {
            let faults = self
                .booted
                .host
                .plugin_faults()
                .into_iter()
                .map(|fault| {
                    // The fault strings are "plugin_id: message"-ish;
                    // key by the leading id when present.
                    match fault.split_once(':') {
                        Some((id, message)) => (id.trim().to_owned(), message.trim().to_owned()),
                        None => (String::new(), fault),
                    }
                })
                .collect();
            store.set_plugin_faults(faults);
        }
        // TUI-25: the ctx.ui.alert/confirm resolutions drain back
        // into the host (the store's settle-on-close hooks queued
        // them; the guest promises now settle).
        for (request_id, result) in store.take_ui_dialog_resolutions() {
            self.booted.host.resolve_ui_request(request_id, result);
        }
        // PLUG-TUI-SLOT's pump steps: a freshly navigated plugin route
        // materializes its content (bounded), finished materializations
        // apply, and dynamic keymap layers (installed by a claim's
        // render mid-flight) re-project when changed.
        self.materialize_open_route(store);
        self.apply_route_content(store);
        self.refresh_dynamic_registrations(store);
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
///
/// PLUG-TUI-SLOT: `<plugin_id>::<command_id>` names are keymap-layer
/// commands (`ctx.keymap.layer`) — they dispatch to the guest's layer
/// `run` callback instead of a `command.add` registration
/// (api.tsx:179-199).
struct HostSink {
    host: Arc<PluginHost>,
    current: Rc<RefCell<Option<String>>>,
}

impl crate::store::PluginCommandSink for HostSink {
    fn execute(&self, command: &str, args: Option<&str>) -> bool {
        let host = Arc::clone(&self.host);
        // A keymap-layer command (the `::` routing convention).
        if let Some((plugin, id)) = command.split_once("::") {
            if !plugin.is_empty() && !id.is_empty() {
                let plugin = plugin.to_owned();
                let id = id.to_owned();
                return std::thread::Builder::new()
                    .name("oc-tui-plugin-keymap".into())
                    .spawn(move || {
                        if let Err(error) = host.run_keymap_command(&plugin, &id, COMMAND_TIMEOUT) {
                            crate::keymap::log_skip(&format!(
                                "plugin keymap command {plugin}::{id}: {error}"
                            ));
                        }
                    })
                    .is_ok();
            }
        }
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
        UiRequest::SidebarSection { title, rows, clickable, .. } => PluginUiEvent::SidebarSection {
            title: title.clone(),
            rows: rows.clone(),
            clickable: clickable.clone(),
        },
        UiRequest::NavigateToSession { session_id } => {
            PluginUiEvent::NavigateToSession { session_id: session_id.clone() }
        }
        UiRequest::FooterHint { text, .. } => PluginUiEvent::FooterHint { text: text.clone() },
        // TUI-25: the deferred alert/confirm requests (the guest's
        // request id settles its promise through the pump).
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

// ---------------------------------------------------------------------------
// PLUG-D33: the MCP registry sync (registrations.mcps → the real
// server-side registry over the wire)
// ---------------------------------------------------------------------------

/// Push the plugin snapshot's MCP servers into the server's REAL
/// registry (`experimental.mcp.add` → `McpRegistry::add` — the ref's
/// `ctx.mcp.transform` seam, host.ts:374-403, carried over the wire
/// in the client/server split). `previously_pushed` tracks the diff:
/// adds PUT their config, servers a (hot-reload) removal dropped
/// DELETE; the TUI's seed table starts EMPTY (the server's registry
/// owns the config-sourced rows) so every snapshot entry is a plugin
/// contribution. Returns the refreshed rows + the new pushed set.
/// Deterministic callers (tests) drive a scripted local listener; no
/// real network.
pub async fn sync_and_fetch(
    client: &Client,
    servers: BTreeMap<String, Value>,
    previously_pushed: BTreeSet<String>,
) -> (Vec<McpServerRow>, BTreeSet<String>) {
    let mut pushed = previously_pushed;
    // Adds/updates: PUT the config (idempotent registry insert-or-
    // replace — the ref's editor `set`). The config JSON is the v2
    // `Mcp.ServerConfig` the guest's editor produced; the client's
    // typed body re-validates it exactly like the ref's
    // `Schema.decodeUnknownSync(ServerConfig)` (host.ts:387-397) —
    // an invalid config declines THIS server (contained), never the
    // whole pass.
    for (name, config) in &servers {
        let parsed = match serde_json::from_value::<oc_client::types::ExperimentalMcpAddBodyConfig>(
            config.clone(),
        ) {
            Ok(parsed) => parsed,
            Err(error) => {
                crate::keymap::log_skip(&format!("plugin mcp config {name}: {error}"));
                continue;
            }
        };
        let body = oc_client::types::ExperimentalMcpAddBody { config: parsed };
        if let Err(error) = client
            .mcp()
            .add(name, &oc_client::types::ExperimentalMcpAddQuery { location: None }, &body)
            .await
        {
            crate::keymap::log_skip(&format!("experimental.mcp.add {name}: {error}"));
            continue;
        }
        pushed.insert(name.clone());
    }
    // Removals: a plugin (or its hot reload) dropped a previously
    // pushed server — DELETE it from the registry.
    let live: BTreeSet<String> = servers.keys().cloned().collect();
    let stale: Vec<String> = pushed.difference(&live).cloned().collect();
    for name in stale {
        if let Err(error) = client
            .mcp()
            .remove(name.as_str(), &oc_client::types::ExperimentalMcpRemoveQuery { location: None })
            .await
        {
            crate::keymap::log_skip(&format!("experimental.mcp.remove {name}: {error}"));
            continue;
        }
        pushed.remove(&name);
    }
    let rows = fetch_mcp_rows(client).await.unwrap_or_default();
    (rows, pushed)
}

/// The post-sync `mcp.list` re-fetch: the refreshed rows (server
/// truth — the registry now contains the plugin servers) mapped to
/// the store's row shape (the same mapping the boot fetch chain
/// uses).
pub async fn fetch_mcp_rows(client: &Client) -> Option<Vec<McpServerRow>> {
    let response =
        client.mcp().list(&oc_client::types::McpListQuery { location: None }).await.ok()?;
    Some(
        response
            .data
            .iter()
            .map(|server| {
                let status = match &server.status {
                    oc_client::types::McpServerStatus::Connected(_) => "connected",
                    oc_client::types::McpServerStatus::Pending(_) => "pending",
                    oc_client::types::McpServerStatus::Failed(_) => "failed",
                    oc_client::types::McpServerStatus::Disabled(_) => "disabled",
                    oc_client::types::McpServerStatus::NeedsAuth(_) => "needs_auth",
                };
                let error = match &server.status {
                    oc_client::types::McpServerStatus::Failed(failed) => Some(failed.error.clone()),
                    oc_client::types::McpServerStatus::NeedsAuth(auth) => Some(auth.error.clone()),
                    _ => None,
                };
                McpServerRow { name: server.name.clone(), status, error }
            })
            .collect(),
    )
}

/// Project the palette rows: `command.add` palette commands plus the
/// palette-visible keymap-layer commands (`ctx.keymap.layer` — the
/// ref's layer IS the feature-plugin command surface, api.tsx:
/// 179-199; dispatched as `<plugin>::<id>`).
fn project_palette(store: &Store, snapshot: &RegistrationSnapshot) {
    let mut palette: Vec<PluginCommandEntry> = snapshot
        .commands
        .iter()
        .filter(|command| command.palette)
        .map(|command| PluginCommandEntry {
            command: command.name.clone(),
            title: command.description.clone(),
            plugin_id: command.plugin_id.clone(),
        })
        .collect();
    for command in &snapshot.keymap_commands {
        if command.palette {
            palette.push(PluginCommandEntry {
                command: format!("{}::{}", command.plugin_id, command.id),
                title: command.title.clone(),
                plugin_id: command.plugin_id.clone(),
            });
        }
    }
    store.set_plugin_commands(palette);
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Run one bounded, thread-offloaded host call (the facade's
/// `block_on` may not nest in the loop's executor — the dialog
/// source's pattern).
fn offload_bounded<T: Send + 'static>(
    bound: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("oc-tui-plugin-bounded".into())
        .spawn(move || {
            let _ = tx.send(call());
        })
        .map_err(|error| error.to_string())?;
    rx.recv_timeout(bound + Duration::from_secs(1)).map_err(|error| error.to_string())
}

/// The owned keymap entries' change fingerprint (command + bind
/// pairs, order-stable).
fn owned_fingerprint(owned: &[OwnedCommandDef]) -> String {
    owned.iter().fold(String::new(), |acc, entry| {
        format!("{acc}{}|{}|{};", entry.command, entry.plugin, entry.defaults.join(","))
    })
}

/// The dynamic-registration fingerprint (owned entries + the
/// palette-visible command table — one string, one comparison).
fn dynamic_fingerprint(owned: &[OwnedCommandDef], snapshot: &RegistrationSnapshot) -> String {
    let palette = snapshot
        .commands
        .iter()
        .filter(|command| command.palette)
        .map(|command| command.name.as_str())
        .chain(
            snapshot
                .keymap_commands
                .iter()
                .filter(|command| command.palette)
                .map(|command| command.id.as_str()),
        )
        .collect::<Vec<_>>()
        .join(",");
    format!("{}#{}", owned_fingerprint(owned), palette)
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
#[allow(clippy::too_many_lines)] // the v2 SessionDomain dispatch mirrors the engine port
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
            // -- PLUG-D10: the remaining v2 SessionDomain methods over
            //    the generated client (the TUI embedder's port mirrors
            //    the engine port's surface).
            "create" => {
                let mut body = oc_client::types::SessionCreateBody::default();
                if let Some(title) = args.get("title").and_then(Value::as_str) {
                    body.title = Some(title.to_owned());
                }
                let session =
                    self.client.session().create(&body).await.map_err(|error| error.to_string())?;
                serde_json::to_value(&session).map_err(|error| error.to_string())
            }
            "switchAgent" => {
                let body = oc_client::types::SessionSwitchAgentBody {
                    agent: args.get("agent").and_then(Value::as_str).unwrap_or_default().to_owned(),
                };
                self.client
                    .session()
                    .switch_agent(session_id, &body)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "command" => {
                let mut body = oc_client::types::SessionCommandBody {
                    name: args.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    text: args.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    ..oc_client::types::SessionCommandBody::default()
                };
                if let Some(delivery) = args.get("delivery").and_then(Value::as_str) {
                    body.delivery = Some(match delivery {
                        "queue" => oc_client::types::SessionInboxDelivery::Queue,
                        _ => oc_client::types::SessionInboxDelivery::Steer,
                    });
                }
                self.client
                    .session()
                    .command(session_id, &body)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "compact" => {
                let compaction = self
                    .client
                    .session()
                    .compact(session_id, &oc_client::types::SessionCompactBody::default())
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&compaction).map_err(|error| error.to_string())
            }
            "context" => {
                let context = self
                    .client
                    .session()
                    .context(session_id)
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&context).map_err(|error| error.to_string())
            }
            "generate" => {
                let generated = self
                    .client
                    .session()
                    .generate(
                        session_id,
                        &oc_client::types::SessionGenerateBody {
                            prompt: args
                                .get("prompt")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&generated).map_err(|error| error.to_string())
            }
            "move" => {
                let directory =
                    args.pointer("/location/directory").and_then(Value::as_str).unwrap_or_default();
                self.client
                    .session()
                    .move_(
                        session_id,
                        &oc_client::types::SessionMoveBody {
                            directory: directory.to_owned(),
                            ..oc_client::types::SessionMoveBody::default()
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "synthetic" => {
                let mut body = oc_client::types::SessionSyntheticBody {
                    text: args.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    ..oc_client::types::SessionSyntheticBody::default()
                };
                if let Some(description) = args.get("description").and_then(Value::as_str) {
                    body.description = Some(description.to_owned());
                }
                let synthetic = self
                    .client
                    .session()
                    .synthetic(session_id, &body)
                    .await
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(&synthetic).map_err(|error| error.to_string())
            }
            "wait" => {
                self.client.session().wait(session_id).await.map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            other => Err(format!(
                "session.{other}: not a v2 SessionDomain method (allowlist: {:?})",
                oc_plugin_host::SESSION_METHODS
            )),
        }
    }
}
/// One theme token as a `#rrggbb` string (the v2 TUI-plugin theme
/// view the guest's sidebar render consumes).
fn hex_color(color: ratatui::style::Color) -> String {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#888888".to_owned(),
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

    /// PLUG-D01: a config file's parent dir (the v2 layer root)
    /// contributes its `plugin`/`plugins` drop-in children FIRST, and a
    /// document remove drops a discovered entry.
    #[test]
    fn local_plugin_dir_discovery() {
        let root = temp_root("plugindir");
        let layer = root.join("layer");
        std::fs::create_dir_all(layer.join("plugins").join("alpha")).unwrap();
        std::fs::create_dir_all(layer.join("plugin")).unwrap();
        std::fs::write(layer.join("plugins").join("alpha").join("index.ts"), "// alpha").unwrap();
        std::fs::write(layer.join("plugin").join("beta.js"), "// beta").unwrap();
        std::fs::write(layer.join("opencode.json"), r#"{ "plugins": ["-alpha"] }"#).unwrap();

        let ops = collect_plugin_ops(&[layer.join("opencode.json")]);
        let targets: Vec<&str> = ops
            .iter()
            .map(|op| match op {
                PluginOp::Add { target, .. } | PluginOp::Remove { target } => target.as_str(),
            })
            .collect();
        assert_eq!(
            targets,
            vec![
                layer.join("plugin").join("beta.js").display().to_string().as_str(),
                layer.join("plugins").join("alpha").display().to_string().as_str(),
                "alpha",
            ],
            "discovered adds before document ops: {targets:?}"
        );
        let resolution = oc_plugin_host::resolve(&ops, None);
        assert_eq!(resolution.plugins.len(), 1, "remove drops the discovered drop-in");
        assert!(resolution.plugins[0].target.ends_with("beta.js"));
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
