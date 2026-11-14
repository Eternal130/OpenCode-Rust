//! Channel 8 (TUI full-screen diff) — Task 3: rust in-process scenario
//! replay driver.
//!
//! Env-gated (no-op when unset; nextest sees a passing no-op — the
//! `provider_recordings_diff_driver.rs:170-175` pattern):
//!   `TUI_DIFF_MANIFEST=<file> TUI_DIFF_OUT=<dir>` \
//!     `cargo nextest run -p oc-tui --test tui_fullscreen_diff_driver`
//!
//! For every `tui-diff-manifest/1` case (sorted by id) the
//! `tui-diff-scenario/1` fixture (tools/diff/corpus/tui/SCHEMA.md) is
//! replayed through the PRODUCTION ingest + render chain and the
//! resting frame exported as a `tui-diff-grid/1` JSON — field-for-field
//! the same output shape as `tools/diff/drivers/tui_fullscreen_ref.ts`.
//!
//! # Ingestion seam (task brief Step 1 spike — option (a), first choice)
//!
//! The live loop's SSE-event application path IS directly callable:
//! `run_with_size` spawns [`oc_tui::app::spawn_connection`]
//! (crates/oc-tui/src/app.rs:7883 — `Connection::run` +
//! `HttpResync`, the SSE reconnect state machine) and applies its
//! output batch-for-batch through `app.store.ingest_all`
//! (crates/oc-tui/src/app.rs:9143-9147). The boot fetch chain rides
//! [`oc_tui::app::spawn_server_lists`] (app.rs:4260), the loop fetcher
//! [`oc_tui::app::spawn_loop_fetcher`] (app.rs:5866) and the boot
//! route `app.store.select_tab` (`apply_boot_route`'s `--session` arm,
//! app.rs:7955). This driver pumps exactly those seams against a
//! scripted loopback HTTP+SSE server serving the scenario's `fetch`
//! map — no PTY, no render emission, but the FULL real chain:
//! `oc_client` wire decode → `Ingest` → `Store::ingest_all` →
//! `App::frame()` (app.rs:3432 — the render chain is never bypassed).
//!
//! # Determinism discipline
//!
//! * The clock stays frozen: `advance_clock` is never called (store
//!   test-state time 0), matching the ref fixture's frozen clock.
//! * Fixed-point drain: [`oc_tui::render::serialize_canonical`] of the
//!   frame must be identical 3 pumps in a row, cap 64 pumps — a
//!   non-settling case is recorded as an `error` on its grid and fails
//!   the run.
//! * Theme: the pinned corpus theme (opencode v2 dark, SCHEMA 主题
//!   pinning) is the rust theme model's own default
//!   (`ThemeState::v2_dark`); `CellColor::Default` resolves to its
//!   `text.base` / `background.base` RGB — no hardcoded magic numbers.
//! * Indexed colors map through the SAME xterm table as the ref's
//!   `v1-migrate.ts:470-495` (controller ruling; both sides unified).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_tui::app::{
    spawn_connection, spawn_loop_fetcher, spawn_server_lists, App, LoopFetch, LoopRequest,
    ServerListEvent,
};
use oc_tui::data::{ConnectionConfig, Ingest};
use oc_tui::render::{serialize_canonical, CellColor, Frame, CONTINUATION};
use oc_tui::store::ListState;
use oc_tui::theme::ThemeState;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------------
// scripted server: the scenario's `fetch` map over loopback HTTP (+SSE)
// ---------------------------------------------------------------------------

struct ServerState {
    fetch: BTreeMap<String, (u16, Value)>,
    census: Mutex<BTreeMap<String, u64>>,
    gaps: Mutex<BTreeSet<String>>,
    sse: Mutex<Vec<Arc<tokio::sync::Mutex<TcpStream>>>>,
}

impl ServerState {
    fn record(&self, path: &str) {
        *self.census.lock().expect("census").entry(path.to_owned()).or_insert(0) += 1;
    }

    /// Push one SSE frame (`data: <json>\n\n`) to every live subscriber.
    async fn push_event(&self, event: &Value) {
        let frame =
            format!("data: {}\n\n", serde_json::to_string(event).expect("event serializes"));
        let streams = self.sse.lock().expect("sse").clone();
        for stream in streams {
            let _ = stream.lock().await.write_all(frame.as_bytes()).await;
        }
    }
}

async fn serve_connection(mut socket: TcpStream, state: Arc<ServerState>) {
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    // Read the request head (the driver never needs bodies; every
    // response closes the connection).
    loop {
        let Ok(read) = socket.read(&mut chunk).await else { return };
        if read == 0 {
            return;
        }
        head.extend_from_slice(&chunk[..read]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if head.len() > 64 * 1024 {
            return;
        }
    }
    let head = String::from_utf8_lossy(&head);
    let Some(line) = head.lines().next() else { return };
    let path =
        line.split(' ').nth(1).unwrap_or_default().split('?').next().unwrap_or_default().to_owned();
    state.record(&path);

    if path == "/api/event" {
        // The SSE stream: open with the scenario's `initial`
        // (`server.connected`), then hold for `push_event`.
        let initial = state
            .fetch
            .get("/api/event")
            .and_then(|(_, body)| body.get("initial"))
            .cloned()
            .unwrap_or(Value::Null);
        let frame =
            format!("data: {}\n\n", serde_json::to_string(&initial).expect("initial serializes"));
        let headers =
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
        if socket.write_all(headers.as_bytes()).await.is_err()
            || socket.write_all(frame.as_bytes()).await.is_err()
        {
            return;
        }
        let shared = Arc::new(tokio::sync::Mutex::new(socket));
        state.sse.lock().expect("sse").push(Arc::clone(&shared));
        // Hold the stream open (the connection task drops it at case
        // end; the write half errors surface in `push_event`).
        drop(shared);
        return;
    }
    let response = if let Some((status, body)) = state.fetch.get(&path) {
        let body = serde_json::to_string(body).expect("body serializes");
        format!(
            "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        // stub_gap (SCHEMA): an infra failure, never a silent
        // fallback — same 503 shape as the ref driver.
        state.gaps.lock().expect("gaps").insert(path.clone());
        let body = serde_json::json!({ "error": "stub_gap", "path": path });
        let body = serde_json::to_string(&body).expect("gap serializes");
        format!(
                "HTTP/1.1 503 Service Unavailable\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
        )
    };
    let _ = socket.write_all(response.as_bytes()).await;
}

async fn spawn_server(
    fetch: BTreeMap<String, (u16, Value)>,
) -> (std::net::SocketAddr, Arc<ServerState>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let state = Arc::new(ServerState {
        fetch,
        census: Mutex::new(BTreeMap::new()),
        gaps: Mutex::new(BTreeSet::new()),
        sse: Mutex::new(Vec::new()),
    });
    let task_state = Arc::clone(&state);
    let task = tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { return };
            let state = Arc::clone(&task_state);
            tokio::spawn(serve_connection(socket, state));
        }
    });
    (addr, state, task)
}

// ---------------------------------------------------------------------------
// the replay pump: the live loop's drain sequence, minus the PTY/clock
// ---------------------------------------------------------------------------

/// F1 — the footer location label (the live loop's arm verbatim,
/// app.rs:9908-9921): `abbreviateHome(directory)` plus `:{branch}` when
/// vcs resolves one. The driver fires the boot request the live loop's
/// boot block sends (app.rs:8703-8707) with the scenario's location
/// directory (the ref harness resolves it from /api/location — its
/// census shows the boot walk).
fn footer_location_label(directory: &str, branch: Option<String>) -> String {
    let home = std::env::var("HOME").ok().filter(|home| !home.is_empty());
    let mut label = match home {
        Some(home) => oc_tui::components::dialog_worktrees::abbreviate_home(directory, &home),
        None => directory.to_owned(),
    };
    if let Some(branch) = branch {
        label.push(':');
        label.push_str(&branch);
    }
    label
}

/// The project-list sink (the live loop's `HttpProjectSink`,
/// app.rs:7053-7063): `open.menu`'s refetch round-trips the loop
/// fetcher, so the refetch resolves against the scenario's
/// `/api/project` fixture like production.
struct DriverProjectSink {
    tx: tokio::sync::mpsc::UnboundedSender<LoopRequest>,
}

impl oc_tui::store::ProjectSink for DriverProjectSink {
    fn list(&self) {
        let _ = self.tx.send(LoopRequest::ProjectList);
    }
}

/// The config sink mirroring the ref harness fixture's config service
/// (ref test/fixture/app.ts:37: `update: async () => ({})`): config
/// writes are DROPPED (reconciled to the fixture's resolved defaults),
/// never landing in a local cli.json — so the store's
/// `update_session_sidebar` contract sees "write did not land" and the
/// resolved sidebar mode never changes post-boot, exactly the ref
/// fixture's observable (T8-320: the sidebar-toggle scenario's
/// `<leader>b` at 320 cols).
struct DriverConfigSink;

impl oc_tui::store::ConfigSink for DriverConfigSink {
    fn update_theme_name(&self, _name: &str) {}
    fn update_theme_mode(&self, _mode: &str) {}
    fn update_session_sidebar(&self, _hide: bool) -> bool {
        false
    }
    fn update_session_grouping(&self, _grouped: bool) {}
}

struct Pump {
    app: App,
    ingest_rx: tokio::sync::mpsc::Receiver<Ingest>,
    list_rx: tokio::sync::mpsc::Receiver<ServerListEvent>,
    fetch_rx: tokio::sync::mpsc::UnboundedReceiver<LoopFetch>,
    fetch_tx: tokio::sync::mpsc::UnboundedSender<LoopRequest>,
    open_sessions: oc_tui::data::SharedOpenSessions,
    fetched_transcripts: BTreeSet<String>,
    saw_connected: bool,
    saw_models: bool,
}

impl Pump {
    /// One live-loop drain iteration (app.rs:9128-9353's order: ingest
    /// → bus commands → server lists → loop fetcher).
    #[allow(clippy::too_many_lines)] // W4: the ProjectList arm mirrors app.rs's fetch arms
    fn drain(&mut self) {
        while let Ok(message) = self.ingest_rx.try_recv() {
            if matches!(message, Ingest::Connected) {
                self.saw_connected = true;
            }
            self.app.store.ingest_all([message]);
        }
        self.app.drain_bus_commands();
        while let Ok(event) = self.list_rx.try_recv() {
            match event {
                ServerListEvent::Config(model) => self.app.store.set_config_model(model),
                ServerListEvent::Models(Ok((models, providers))) => {
                    self.saw_models = true;
                    self.app.store.set_models(ListState::Loaded(models), providers);
                }
                ServerListEvent::Models(Err(error)) => {
                    self.saw_models = true;
                    self.app.store.set_models(ListState::Failed(error), Vec::new());
                }
                ServerListEvent::Agents(Ok(agents)) => {
                    self.app.store.set_agents(ListState::Loaded(agents));
                }
                ServerListEvent::Agents(Err(error)) => {
                    self.app.store.set_agents(ListState::Failed(error));
                }
                ServerListEvent::Connected(connected) => {
                    self.app.store.set_integration_connected(connected);
                }
                ServerListEvent::Commands(commands) => {
                    self.app.store.set_server_slash_commands(commands);
                }
                ServerListEvent::Mcp(rows) => {
                    self.app.store.set_mcp_servers(rows);
                }
                ServerListEvent::Skills(Ok(skills)) => {
                    self.app.store.set_skills(ListState::Loaded(skills));
                }
                ServerListEvent::Skills(Err(error)) => {
                    self.app.store.set_skills(ListState::Failed(error));
                }
                ServerListEvent::References(references) => {
                    self.app.store.set_references(references);
                }
            }
        }
        // The open-session registry sync + transcript hydration
        // (app.rs:9698-9753, verbatim).
        let open_now = self.app.store.open_session_ids();
        if let Ok(mut ids) = self.open_sessions.lock() {
            ids.clone_from(&open_now);
        }
        for id in open_now {
            if self.fetched_transcripts.insert(id.clone()) {
                self.app.store.set_history_loading(&id, true);
                let _ = self.fetch_tx.send(LoopRequest::Transcript(id));
            }
        }
        while let Ok(fetch) = self.fetch_rx.try_recv() {
            match fetch {
                LoopFetch::Transcript { session_id, result } => {
                    self.app.store.set_history_loading(&session_id, false);
                    match result {
                        Ok((messages, cursor)) => {
                            self.app.store.set_message_cursor(&session_id, cursor);
                            self.app
                                .store
                                .ingest_all([Ingest::MessagesSnapshot { session_id, messages }]);
                        }
                        Err(error) => {
                            // F7 — one-shot on failure (app.rs's live-loop
                            // arm; ref's route-mount `message.sync` never
                            // re-arms a rejected fetch — removing the id
                            // here re-armed the loading label every pump).
                            let _ = error;
                        }
                    }
                }
                LoopFetch::TranscriptOlder { session_id, result } => {
                    self.app.store.set_history_loading(&session_id, false);
                    if let Ok((messages, cursor)) = result {
                        self.app.store.set_message_cursor(&session_id, cursor.clone());
                        self.app.store.ingest_all([Ingest::MessagesOlder {
                            session_id,
                            messages,
                            cursor,
                        }]);
                    }
                }
                // F1 — the footer location label: the live loop's arm
                // (app.rs:9908-9921) — see `footer_location_label`.
                LoopFetch::LocationVcs { directory, branch: Ok(branch) } => {
                    self.app.store.set_footer_location(footer_location_label(&directory, branch));
                }
                // app.rs:9926-9928: a failed/absent vcs lookup is
                // silent (the label stays unset) — rides the wildcard
                // drop arm below like the interactive fetches.
                LoopFetch::SessionList(Ok(sessions)) => {
                    let unread = oc_tui::data::resync_unread(&sessions);
                    self.app.store.ingest_all([Ingest::Resync {
                        sessions,
                        unread,
                        messages: Vec::new(),
                        cursors: Vec::new(),
                        permissions: Vec::new(),
                        forms: Vec::new(),
                    }]);
                }
                // TUI-6 (app.rs:9834-9837's live-loop arm): the boot
                // project-registry sync lands in the Open menu's
                // project list.
                LoopFetch::ProjectList(result) => match result {
                    Ok(rows) => self.app.store.project_list_loaded(rows),
                    Err(error) => self.app.store.project_list_failed(error),
                },
                // T8-320 — the terminal capability probe
                // (session-terminals.tsx:59-76: the event stream opens
                // → refresh `info` → `capabilities?.persistentPty !==
                // false`): the boot send mirrors the live loop's
                // probe, gating the palette's terminal rows.
                LoopFetch::TerminalCapability(available) => {
                    self.app.store.set_terminal_available(available);
                }
                // The interactive fetch results (dialog search,
                // autocomplete, terminals — app.rs:9745+) cannot fire
                // in the replay corpus (their requests are key-driven);
                // dropped by design.
                _ => {}
            }
        }
    }

    /// Fixed-point drain (task brief Step 3): pump until the canonical
    /// serialization is identical 3 iterations in a row, 64-cap.
    async fn settle(&mut self, case_id: &str) -> Result<(), String> {
        let mut stable = 0;
        let mut last = String::new();
        for _ in 0..64 {
            self.drain();
            let frame = self.app.frame();
            let serialized = serialize_canonical(&frame);
            if serialized == last {
                stable += 1;
                if stable >= 3 && self.saw_connected && self.saw_models {
                    return Ok(());
                }
            } else {
                stable = 0;
                last = serialized;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Err(format!("{case_id}: no resting frame within 64 pumps"))
    }
}

// ---------------------------------------------------------------------------
// key vocabulary (SCHEMA 键名表) → `App::press` specs
// ---------------------------------------------------------------------------

fn press_token(app: &mut App, token: &str) {
    if let Some(literal) = token.strip_prefix("raw:") {
        for ch in literal.chars() {
            app.press(&ch.to_string());
        }
        return;
    }
    if let Some(letter) = token.strip_prefix("ctrl+").filter(|rest| rest.len() == 1) {
        app.press(&format!("ctrl+{letter}"));
        return;
    }
    let spec = match token {
        "enter" | "return" => "return",
        "esc" | "escape" => "escape",
        "space" => " ",
        "leader" => "ctrl+x",
        named @ ("tab" | "backspace" | "delete" | "up" | "down" | "right" | "left" | "home"
        | "end" | "pageup" | "pagedown" | "alt+up" | "alt+down" | "alt+right"
        | "alt+left") => named,
        unknown => panic!("unknown key token: {unknown}"),
    };
    app.press(spec);
}

// ---------------------------------------------------------------------------
// grid export (`tui-diff-grid/1`, field-aligned with the ref driver)
// ---------------------------------------------------------------------------

const ATTR_BOLD: u16 = 1 << 0;
const ATTR_DIM: u16 = 1 << 1;
const ATTR_ITALIC: u16 = 1 << 2;
const ATTR_UNDERLINE: u16 = 1 << 3;
const ATTR_BLINK: u16 = 1 << 4;
const ATTR_INVERSE: u16 = 1 << 5;
const ATTR_HIDDEN: u16 = 1 << 6;
const ATTR_STRIKE: u16 = 1 << 7;

/// The canonical attr set (the ref driver's `CANONICAL_ATTRS` order),
/// then out-of-set bits as `unknown:<bit>` ascending.
fn attr_list(attrs: u16) -> Vec<String> {
    let mut out = Vec::new();
    for (bit, name) in [
        (ATTR_BOLD, "bold"),
        (ATTR_DIM, "dim"),
        (ATTR_ITALIC, "italic"),
        (ATTR_UNDERLINE, "underline"),
        (ATTR_STRIKE, "strikethrough"),
        (ATTR_INVERSE, "reverse"),
    ] {
        if attrs & bit != 0 {
            out.push(name.to_owned());
        }
    }
    for bit in [ATTR_BLINK, ATTR_HIDDEN] {
        if attrs & bit != 0 {
            out.push(format!("unknown:{bit}"));
        }
    }
    out
}

/// The xterm 256-color palette of the ref `v1-migrate.ts:470-495`
/// (`ansi()` — controller ruling: both sides use THIS table).
fn indexed_to_rgb(index: u8) -> [u8; 3] {
    let i = u32::from(index);
    if i < 16 {
        const TABLE: [[u8; 3]; 16] = [
            [0x00, 0x00, 0x00],
            [0x80, 0x00, 0x00],
            [0x00, 0x80, 0x00],
            [0x80, 0x80, 0x00],
            [0x00, 0x00, 0x80],
            [0x80, 0x00, 0x80],
            [0x00, 0x80, 0x80],
            [0xc0, 0xc0, 0xc0],
            [0x80, 0x80, 0x80],
            [0xff, 0x00, 0x00],
            [0x00, 0xff, 0x00],
            [0xff, 0xff, 0x00],
            [0x00, 0x00, 0xff],
            [0xff, 0x00, 0xff],
            [0x00, 0xff, 0xff],
            [0xff, 0xff, 0xff],
        ];
        return TABLE[usize::try_from(i).expect("i < 16")];
    }
    if i < 232 {
        let index = i - 16;
        let value =
            |part: u32| if part == 0 { 0 } else { u8::try_from(part * 40 + 55).expect("byte") };
        return [value(index / 36), value((index / 6) % 6), value(index % 6)];
    }
    let gray = u8::try_from((i - 232) * 10 + 8).expect("byte");
    [gray, gray, gray]
}

fn cell_color(color: CellColor, theme_default: [u8; 3]) -> [u8; 3] {
    match color {
        CellColor::Default => theme_default,
        CellColor::Indexed(index) => indexed_to_rgb(index),
        CellColor::Rgb(r, g, b) => [r, g, b],
    }
}

/// The theme-default RGB pair `CellColor::Default` resolves through
/// (raster blanks + default-styled cells).
struct ThemeDefaults {
    fore: [u8; 3],
    back: [u8; 3],
}

#[derive(serde::Serialize)]
struct GridCell {
    ch: String,
    attrs: Vec<String>,
    fg: [u8; 3],
    bg: [u8; 3],
}

#[derive(serde::Serialize)]
struct GridFile {
    schema: &'static str,
    id: String,
    width: u64,
    height: u64,
    cells: Vec<Vec<GridCell>>,
    stub_gaps: Vec<String>,
    request_census: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn rasterize(frame: &Frame, theme: &ThemeDefaults) -> Vec<Vec<GridCell>> {
    let mut grid = Vec::with_capacity(frame.height);
    for y in 0..frame.height {
        let mut row = Vec::with_capacity(frame.width);
        for x in 0..frame.width {
            let cell = frame.cell(y, x);
            // The wide-cluster continuation cell emits a blank of the
            // same style (the ref raster's trailing blank after a wide
            // glyph); a cluster sidecar symbol carries the full grapheme.
            let ch = if cell.ch == CONTINUATION {
                " ".to_owned()
            } else {
                frame.cluster_at(y, x).map_or_else(|| cell.ch.to_string(), str::to_owned)
            };
            row.push(GridCell {
                ch,
                attrs: attr_list(cell.attrs),
                fg: cell_color(cell.fg, theme.fore),
                bg: cell_color(cell.bg, theme.back),
            });
        }
        grid.push(row);
    }
    grid
}

// ---------------------------------------------------------------------------
// driver
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ManifestCase {
    id: String,
    scenario: String,
    width: u16,
    height: u16,
}

#[derive(serde::Deserialize)]
struct Manifest {
    schema: String,
    cases: Vec<ManifestCase>,
}

#[derive(serde::Deserialize)]
struct Scenario {
    schema: String,
    #[allow(dead_code)] // identity is asserted by the manifest path
    id: String,
    #[allow(dead_code)] // the manifest's tier overrides the fixture's
    width: u16,
    #[allow(dead_code)]
    height: u16,
    fetch: BTreeMap<String, FetchStub>,
    events: Vec<Value>,
    keys: Vec<String>,
    settle_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
struct FetchStub {
    status: Option<u16>,
    body: Value,
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one pass per manifest case, same shape as the ref driver's main
async fn tui_fullscreen_diff_driver() {
    let Some(manifest_path) = std::env::var_os("TUI_DIFF_MANIFEST") else {
        return; // no manifest staged: driver inactive
    };
    let out_dir = PathBuf::from(std::env::var_os("TUI_DIFF_OUT").expect("TUI_DIFF_OUT"));
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    assert_eq!(manifest.schema, "tui-diff-manifest/1", "unknown manifest schema");
    let mut cases = manifest.cases;
    cases.sort_by(|a, b| a.id.cmp(&b.id));

    // The pinned corpus theme (SCHEMA 主题 pinning) via the rust theme
    // model's own default — `CellColor::Default` resolves through it.
    let resolved = ThemeState::v2_dark();
    let theme = ThemeDefaults {
        fore: [
            resolved.resolved.tokens.text.base.r,
            resolved.resolved.tokens.text.base.g,
            resolved.resolved.tokens.text.base.b,
        ],
        back: [
            resolved.resolved.tokens.background.base.r,
            resolved.resolved.tokens.background.base.g,
            resolved.resolved.tokens.background.base.b,
        ],
    };

    std::fs::create_dir_all(&out_dir).expect("create out dir");
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        // The scenario path is repo-relative (the ref driver resolves
        // against ITS cwd); nextest runs crate-local, so fall back to
        // the workspace root via `CARGO_MANIFEST_DIR`.
        let scenario_path = {
            let direct = PathBuf::from(&case.scenario);
            if direct.exists() {
                direct
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(&case.scenario)
            }
        };
        let scenario: Scenario = serde_json::from_str(
            &std::fs::read_to_string(&scenario_path).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
        )
        .unwrap_or_else(|e| panic!("{}: parse scenario: {e}", case.id));
        assert_eq!(scenario.schema, "tui-diff-scenario/1", "{}: bad scenario schema", case.id);

        // Session-route convention (SCHEMA): boot into the session id
        // from the `/api/session` list + `/api/session/active` stubs.
        let session_id = scenario
            .fetch
            .get("/api/session/active")
            .and_then(|stub| stub.body.get("data").and_then(|data| data.get("id")))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                scenario
                    .fetch
                    .get("/api/session")
                    .and_then(|stub| stub.body.get("data"))
                    .and_then(Value::as_array)
                    .and_then(|list| list.first())
                    .and_then(|row| row.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });

        // F1 — the scenario's location directory (the /api/location
        // stub's `directory`; any stub's `location.directory` as
        // fallback): the ref harness's boot location worker resolves
        // it from the server (its census shows /api/location +/vcs),
        // and the live loop sends one boot LocationVcs for the launch
        // cwd (app.rs:8703-8707) — the driver replays that request
        // with the scenario's directory.
        let location_directory = scenario
            .fetch
            .get("/api/location")
            .and_then(|stub| stub.body.get("directory"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                scenario.fetch.values().find_map(|stub| {
                    stub.body
                        .get("location")
                        .and_then(|location| location.get("directory"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
            });

        let fetch: BTreeMap<String, (u16, Value)> = scenario
            .fetch
            .into_iter()
            .map(|(path, stub)| {
                let status = stub.status.unwrap_or(200);
                (path, (status, stub.body))
            })
            .collect();
        let (addr, server, server_task) = spawn_server(fetch).await;
        let client = oc_client::Client::new(&format!("http://{addr}")).expect("client");

        let size = (case.width, case.height);
        let app = App::new(size);
        // The boot route (`apply_boot_route`'s `--session` arm) + the
        // open-session registry seed (app.rs:8594-8602).
        let (mut pump, connection_task) = {
            let (ingest_rx, connection_task, open_sessions) = spawn_connection(
                client.clone(),
                ConnectionConfig {
                    backoff_base: Duration::from_millis(10),
                    backoff_max: Duration::from_millis(10),
                    max_attempts: None,
                    connect_timeout: Duration::from_secs(5),
                    idle_timeout: Duration::from_secs(600),
                },
            );
            let (list_rx, _list_task) = spawn_server_lists(client.clone());
            let (fetch_tx, fetch_rx) = spawn_loop_fetcher(client.clone());
            if let Some(session_id) = &session_id {
                app.store.select_tab(session_id);
            }
            if let Ok(mut ids) = open_sessions.lock() {
                *ids = app.store.open_session_ids();
            }
            (
                Pump {
                    app,
                    ingest_rx,
                    list_rx,
                    fetch_rx,
                    fetch_tx,
                    open_sessions,
                    fetched_transcripts: BTreeSet::new(),
                    saw_connected: false,
                    saw_models: false,
                },
                connection_task,
            )
        };
        pump.app
            .store
            .set_project_sink(std::rc::Rc::new(DriverProjectSink { tx: pump.fetch_tx.clone() }));
        pump.app.store.set_config_sink(std::rc::Rc::new(DriverConfigSink));

        // F1 — the ref harness's pinned `TuiApp { version: "test" }`
        // injection (test/fixture/app.ts:35): the home footer's
        // right-aligned label reads it (`context.app.version`,
        // home/footer.tsx:99).
        pump.app.store.set_app_version("test".to_owned());
        if let Some(directory) = &location_directory {
            if !directory.is_empty() {
                let _ = pump.fetch_tx.send(LoopRequest::LocationVcs(directory.clone()));
            }
        }
        // TUI-6 (lane-tui2): the boot project-registry sync
        // (`data.project.sync()`, app.rs:8678's live-loop boot block —
        // the Open menu's Projects rows + the session footers' labels
        // read the warm registry).
        let _ = pump.fetch_tx.send(LoopRequest::ProjectList);
        // T8-320 — the terminal capability probe (see the drain arm).
        let _ = pump.fetch_tx.send(LoopRequest::TerminalCapability);

        // Boot settle → scenario events → keys → settle.
        let mut error = pump.settle(&case.id).await.err();
        if error.is_none() {
            for event in &scenario.events {
                server.push_event(event).await;
            }
            for token in &scenario.keys {
                press_token(&mut pump.app, token);
            }
            error = pump.settle(&case.id).await.err();
        }
        if let Some(settle_ms) = scenario.settle_ms {
            tokio::time::sleep(Duration::from_millis(settle_ms)).await;
        }
        pump.drain();
        let frame = pump.app.frame();

        let stub_gaps: Vec<String> = server.gaps.lock().expect("gaps").iter().cloned().collect();
        let request_census = server.census.lock().expect("census").clone();
        let grid = GridFile {
            schema: "tui-diff-grid/1",
            id: case.id.clone(),
            width: u64::from(case.width),
            height: u64::from(case.height),
            cells: rasterize(&frame, &theme),
            stub_gaps: stub_gaps.clone(),
            request_census,
            error,
        };
        let file = out_dir.join(format!("{}.json", case.id));
        let mut json = serde_json::to_string(&grid).expect("grid serializes");
        json.push('\n');
        std::fs::write(&file, json).expect("write grid");
        if let Some(error) = &grid.error {
            failures.push(format!("{}: {error}", case.id));
        }
        if !stub_gaps.is_empty() {
            failures.push(format!("{}: stub_gaps {stub_gaps:?}", case.id));
        }
        println!(
            "[tui-fullscreen-diff] {} cells={}x{} gaps={} paths={}",
            case.id,
            case.width,
            case.height,
            stub_gaps.len(),
            grid.request_census.len()
        );

        connection_task.abort();
        server_task.abort();
    }

    assert!(failures.is_empty(), "driver failures: {failures:?}");
}
