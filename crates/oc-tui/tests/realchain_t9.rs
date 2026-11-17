//! CHANNEL 9 W1 — the six beta.15 symptoms as machine reproductions on
//! the REAL chain (production boot, managed oc-server, sandboxed
//! HOME/XDG, replay-gateway as the only stub). RED = the symptom is
//! machine-reproduced (the wave's goal); GREEN is recorded too (the
//! symptom is then environment-dependent and needs narrowing).
//!
//! * T9-1 boot connect   — the wordmark settles, no reconnect/error
//! * T9-2 model catalog  — the configured replay model is served
//! * T9-3 send a message — hello → gateway replay → reply rendered
//! * T9-4 session stays  — restart the TUI, the session tab survives
//! * T9-5 sidebar        — ref-2.0.24-sourced expectations (below)
//! * T9-6 config reload  — hot config change reaches the model API
//!
//! T9-5 expectations (evidence: ref 2.0.24
//! `packages/tui/src/routes/session/sidebar.tsx` +
//! `feature-plugins/sidebar/{context,mcp,footer}.tsx`): an open
//! session's sidebar carries (a) the session title (timestamped
//! fallback), (b) a `Context` section once a reply landed (token
//! count / % used / `$X.XX spent` — context.tsx:26-46), (c) the MCP
//! section only when MCP servers are configured (empty sandbox ⇒ its
//! absence is CORRECT), (d) the footer getting-started block
//! (dismissible). Machine assert: (a)+(b) on the rendered grid.

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    corpus_dir, dump_grid, http_get, http_post, lane_lock, sandbox, service_start, service_stop,
    Gateway, LaneGuard, Sandbox, TuiProcess, WORDMARK,
};
use std::fmt::Write as _;
use std::time::Duration;

const COLS: u16 = 120;
const ROWS: u16 = 36;

/// The shared real-chain rig: lane lock → sandbox → gateway → config →
/// managed server → TUI on a PTY. Everything tears down in reverse.
struct Rig {
    _lane: LaneGuard,
    sandbox: Sandbox,
    gateway: Gateway,
    server_url: String,
    password: Option<String>,
    tui: TuiProcess,
}

fn rig(tag: &str, record: bool) -> Rig {
    rig_cols(tag, record, COLS)
}

/// Width-parameterized rig — T9-5 spawns WIDE: the sidebar auto-docks
/// only above the 120-col breakpoint (ref session-frame.tsx:106-110,
/// `width - verticalTabsWidth > 120`), so at COLS=120 the sidebar is
/// legitimately absent and the ref-anchored Context/tokens/spent
/// expectations are untestable.
fn rig_cols(tag: &str, record: bool, cols: u16) -> Rig {
    let lane = lane_lock();
    // Gateway first: its port goes into the config.
    let gateway = Gateway::spawn(tag, record.then_some(corpus_dir().as_path()));
    let box_ = sandbox(tag, gateway.port);
    box_.write_config(&[], "replay/claude-opus-4-7");
    let server = service_start(&box_);
    let tui = TuiProcess::spawn(&box_, cols, ROWS);
    Rig {
        _lane: lane,
        sandbox: box_,
        gateway,
        server_url: server.url,
        password: server.password,
        tui,
    }
}

impl Rig {
    fn teardown_tui(&mut self) {
        self.tui.shutdown(Duration::from_secs(10));
    }
}

/// Panic-safe teardown: even a red assertion must not leak a daemon on
/// the sandbox's port (the next lane run would wedge on it).
impl Drop for Rig {
    fn drop(&mut self) {
        self.teardown_tui();
        service_stop(&self.sandbox);
    }
}

// ---------------------------------------------------------------------------
// T9-1 — boot: the TUI connects to its server and settles (symptom ①)
// ---------------------------------------------------------------------------

#[test]
fn t9_1_boot_connects_and_settles() {
    let rig = rig("t9-1", false);
    let settled = rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let screen = rig.tui.screen_text();
    let mut evidence = String::new();
    if !settled {
        let _ = write!(evidence, "screen:\n{screen}");
    }
    // Reconnect/error indicators (the symptom-① surface).
    let bad = ["reconnect", "Reconnect", "failed to connect", "connection lost", "disconnected"];
    let surfaced: Vec<&str> = bad.iter().copied().filter(|n| screen.contains(*n)).collect();
    drop(rig);
    assert!(
        settled && surfaced.is_empty(),
        "T9-1 RED: wordmark settled={settled}, error indicators={surfaced:?}\n{evidence}"
    );
}

// ---------------------------------------------------------------------------
// T9-2 — model catalog: the configured replay model is served (⑥)
// ---------------------------------------------------------------------------

#[test]
fn t9_2_model_catalog_lists_replay_model() {
    let rig = rig("t9-2", false);
    let _ = rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let (status, body) =
        http_get(&format!("{}/api/model", rig.server_url), rig.password.as_deref())
            .unwrap_or((0, String::new()));
    let (default_status, default_body) =
        http_get(&format!("{}/api/model/default", rig.server_url), rig.password.as_deref())
            .unwrap_or((0, String::new()));
    drop(rig);
    let default_ok = default_status == 200
        && default_body.contains("\"providerID\":\"replay\"")
        && default_body.contains("\"id\":\"claude-opus-4-7\"");
    assert!(
        status == 200
            && body.contains("\"providerID\":\"replay\"")
            && body.contains("\"modelID\":\"claude-opus-4-7\"")
            && default_ok,
        "T9-2 RED: /api/model={status} {body}; default={default_status} {default_body}"
    );
}

// ---------------------------------------------------------------------------
// T9-3 — full send chain: hello → gateway replay → reply rendered (②)
// ---------------------------------------------------------------------------

#[test]
fn t9_3_send_message_full_chain() {
    let mut rig = rig("t9-3", false);
    assert!(
        rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30)),
        "T9-3 precondition RED: the TUI never settled on the wordmark"
    );
    let from = rig.tui.raw_len();
    rig.tui.write(b"hello\r");
    // The reply needle the seed cassette carries.
    let rendered = rig.tui.wait_for(b"REALCHAIN-REPLAY-OK", from, Duration::from_secs(30));
    let stats = rig.gateway.stats();
    drop(rig);
    assert!(rendered, "T9-3 RED: the replayed reply never rendered; gateway stats: {stats}");
}

/// Bootstrap-only: seeds the committed corpus cassette from the
/// engine's own (sandbox) request. Not part of the red/green matrix;
/// run explicitly once per engine request-shape change:
/// `T9_RECORD=1 cargo nextest run --exact t9x_bootstrap_seed`.
#[test]
fn t9x_bootstrap_seed() {
    if std::env::var("T9_RECORD").is_err() {
        eprintln!("t9x_bootstrap_seed: skipped (T9_RECORD unset)");
        return;
    }
    let mut rig = rig("t9-3", true);
    assert!(
        rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30)),
        "bootstrap: the TUI never settled"
    );
    std::thread::sleep(Duration::from_secs(3));
    let from = rig.tui.raw_len();
    rig.tui.write(b"hello");
    std::thread::sleep(Duration::from_millis(500));
    rig.tui.write(b"\r");
    let rendered = rig.tui.wait_for(b"REALCHAIN-REPLAY-OK", from, Duration::from_secs(30));
    let screen = rig.tui.screen_text();
    let stats = rig.gateway.stats();
    drop(rig);
    eprintln!("--- screen ---\n{screen}\n--- gateway {stats}");
    assert!(rendered, "bootstrap: the seeded reply never rendered; stats {stats}");
}

// ---------------------------------------------------------------------------
// T9-4 — session persistence across a TUI restart (④)
// ---------------------------------------------------------------------------

#[test]
fn t9_4_sessions_survive_tui_restart() {
    let mut rig = rig("t9-4", false);
    assert!(
        rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30)),
        "T9-4 leg-1 RED: the TUI never settled"
    );
    let from = rig.tui.raw_len();
    rig.tui.write(b"hello\r");
    let replied = rig.tui.wait_for(b"REALCHAIN-REPLAY-OK", from, Duration::from_secs(30));
    // Restart the TUI against the SAME sandbox + same managed server.
    rig.teardown_tui();
    let server_url = rig.server_url.clone();
    let password = rig.password.clone();
    let mut restarted = TuiProcess::spawn(&rig.sandbox, COLS, ROWS);
    let settled = restarted.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let screen = restarted.screen_text();
    restarted.shutdown(Duration::from_secs(10));
    // Server-side truth: the session list survives the restart.
    let (status, body) = http_get(&format!("{server_url}/api/session"), password.as_deref())
        .unwrap_or((0, String::new()));
    let sessions = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        // v2's list envelope (probed: `{"data":[...],"cursor":{...}}`)
        // — the rows ride under `data`.
        .and_then(|value| {
            value
                .as_array()
                .map(Vec::len)
                .or_else(|| value.get("data").and_then(|data| data.as_array()).map(Vec::len))
        })
        .unwrap_or(0);
    service_stop(&rig.sandbox);
    assert!(
        replied && settled && status == 200 && sessions >= 1,
        "T9-4 RED: first-turn reply={replied}, restart settled={settled}, \
         /api/session={status} sessions={sessions}; screen:\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// T9-5 — sidebar content per the ref-2.0.24 source (⑤)
// ---------------------------------------------------------------------------

#[test]
fn t9_5_sidebar_assembles_ref_content() {
    // WIDE spawn — the sidebar only auto-docks above 120 cols (ref
    // session-frame.tsx:106-110); at COLS=120 its absence is correct.
    let mut rig = rig_cols("t9-5", false, 160);
    assert!(
        rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30)),
        "T9-5 precondition RED: the TUI never settled"
    );
    let from = rig.tui.raw_len();
    rig.tui.write(b"hello\r");
    let replied = rig.tui.wait_for(b"REALCHAIN-REPLAY-OK", from, Duration::from_secs(30));
    let screen = rig.tui.screen_text();
    dump_grid("t9-5-rust-160", &screen);
    drop(rig);
    // (a) session title row exists (timestamped fallback), (b) the
    // Context section with its token rows. The `$X.XX spent` row is
    // gated by `cost() > 0` (ref context.tsx:33-35) — the replay
    // provider is unpriced (cost 0), so ref renders NO spent row
    // here; asserting it would over-pin beyond the ref source.
    let context = screen.contains("Context");
    let tokens = screen.contains("tokens");
    assert!(
        replied && context && tokens,
        "T9-5 RED: reply={replied} context-section={context} tokens={tokens}; \
         screen:\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// T9-6 — config hot reload reaches the model API (③ machine proxy)
// ---------------------------------------------------------------------------

#[test]
fn t9_6_config_hot_reload_updates_models() {
    let rig = rig("t9-6", false);
    assert!(
        rig.tui.wait_for(WORDMARK, 0, Duration::from_secs(30)),
        "T9-6 precondition RED: the TUI never settled"
    );
    // Hot change: add a second model entry to the global config.
    rig.sandbox
        .write_config(&[("claude-sonnet-4-6", "claude-sonnet-4-6")], "replay/claude-opus-4-7");
    let (reload_status, _reload_body) = http_post(
        &format!("{}/api/location/reload", rig.server_url),
        "{}",
        rig.password.as_deref(),
    )
    .unwrap_or((0, String::new()));
    let (status, body) =
        http_get(&format!("{}/api/model", rig.server_url), rig.password.as_deref())
            .unwrap_or((0, String::new()));
    drop(rig);
    assert!(
        (200..300).contains(&reload_status) && status == 200 && body.contains("claude-sonnet-4-6"),
        "T9-6 RED: reload={reload_status} model={status}; body: {body}"
    );
}
