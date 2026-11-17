//! CHANNEL 9 W3 — the ref-side real chain PAIR lane (the user-ruled
//! design, verbatim): the SAME replay gateway, sandboxed BOTH ends
//! (ref 2.0.24 via bun AND the rust port), identical inputs, identical
//! PTY keystroke sequences — then the settled text grids are normalized
//! and compared. The differential philosophy of channel 8, extended to
//! the REAL chain: the only stub (the gateway) sits behind BOTH
//! production topologies, so any observed divergence is a port gap, an
//! environment artifact, or a normalization gap — never a stub-shape
//! difference.
//!
//! Ref boot topology (probed, see `realchain_harness.rs` §W3):
//! staged `bun` + `bunfig.toml` solid preload + `$XDG_CONFIG_HOME/
//! opencode/opencode.json` replay provider → the ref CLI's default
//! handler self-spawns its managed background server (`serve
//! --service`, `$XDG_STATE_HOME/opencode/service-local.json`).
//!
//! Cassettes: the rust engine's request shapes live in
//! `tools/diff/realchain/corpus`, the ref engine's in `corpus-ref`
//! (seeded via `T9_RECORD=1 … --exact t9x_ref_bootstrap_seed`); the
//! session-title call normalizes to the SAME digest on both ends and
//! rides the shared corpus. The gateway serves both corpora in every
//! run, so both ends replay real bodies through the same wire.

// Test-harness infra lints: this file is the PTY/grid differential
// harness — `{:?}` in panic/assert args is DELIBERATE forensics, docs
// name wire shapes, and the bridge code prefers explicit steps. The
// workspace pedantic set stays active for everything else.
#![allow(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::map_or_identity,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::semicolon_if_nothing_returned,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    assert_grids_match, corpus_dir, corpus_ref_dir, default_pace, dump_grid, http_get, http_post,
    lane_lock, normalize_grid, plugin_lane_skip_reason, ref_lane_skip_reason, ref_root,
    ref_service_stop, ref_stage, ref_tui_spawn, sandbox, service_start, service_stop,
    stage_plugins, worktree_dir, Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::{Duration, Instant};

/// The pair lane's CI semantics (W4 Important ②): the ref checkout and
/// the pinned bun runtime are ENVIRONMENT dependencies this lane does
/// not assume — absent means SKIP with the reason (never a red); on a
/// lane-carrying machine the tests run directly.
macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("PAIR SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;
const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";
/// The distinct session-title seed text (W3 hardening ②: the title and
/// the reply needle must be distinguishable on the grid).
const TITLE_TEXT: &str = "Sandbox Replay Session";

struct PairRig {
    _lane: LaneGuard,
    gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_server_url: String,
    rust_password: Option<String>,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}
impl PairRig {
    fn roots(&self) -> Vec<String> {
        vec![
            self.refbox.root.to_string_lossy().into_owned(),
            self.rust.root.to_string_lossy().into_owned(),
        ]
    }

    fn normalize(&self, grid: &str) -> String {
        let roots = self.roots();
        let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
        normalize_grid(grid, &refs)
    }

    /// Send the same keystrokes to both ends.
    fn write_both(&mut self, bytes: &[u8]) {
        self.rust_tui.write(bytes);
        self.ref_tui.write(bytes);
    }
}


/// Settle both ends for `secs` (render/event quiescence).
fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

fn pair_rig(tag: &str, cols: u16) -> PairRig {
    pair_rig_ext(tag, cols, &default_pace(), false, false)
}

/// W5: the rig with a pace mode (`instant` = the historical shape) and
/// optional plugin staging (`plugins` = the symptom-⑦ rig) / gateway
/// miss-seeding (`seed_misses` = long-transcript scenarios whose later
/// turns are strict misses the standard seed answers deterministically).
fn pair_rig_ext(tag: &str, cols: u16, pace: &str, plugins: bool, seed_misses: bool) -> PairRig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(
        tag,
        seed_misses.then(|| worktree_dir("tools/diff/realchain/corpus-w5")).as_deref(),
        pace,
    );
    // Rust sandbox: <tag>; ref sandbox: <tag>ref (the gateway redacts
    // BOTH roots to __SANDBOX__ so request hashes stay cross-run stable).
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    if plugins {
        let rust_specs = stage_plugins(&rust, false);
        let ref_specs = stage_plugins(&refbox, true);
        assert_eq!(
            rust_specs, ref_specs,
            "both ends must carry the SAME plugin specs (differential fairness)"
        );
        let specs: Vec<&str> = rust_specs.iter().map(String::as_str).collect();
        rust.write_config_ext(&[], "replay/claude-opus-4-7", &specs);
        refbox.write_ref_config_ext(&[], "replay/claude-opus-4-7", &specs);
    } else {
        rust.write_config(&[], "replay/claude-opus-4-7");
        refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    }
    let stage = ref_stage(&refbox);
    // Defensive pre-clean: a previous run's ref daemon on this sandbox's
    // fixed port would wedge its own `ensure`.
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, cols, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, cols, ROWS);
    PairRig {
        _lane: lane,
        gateway,
        rust,
        refbox,
        stage,
        rust_server_url: server.url,
        rust_password: server.password,
        rust_tui,
        ref_tui,
    }
}

impl Drop for PairRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut PairRig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "PAIR precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

// ---------------------------------------------------------------------------
// S1 — boot: both wordmarks settle, the resting grids match (symptom ①
// on the real chain, BOTH ends)
// ---------------------------------------------------------------------------

#[test]
fn s1_boot_grid_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("s1", COLS);
    boot_both(&mut rig);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    drop(rig);
    assert_grids_match("s1-boot", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// S2 — full send chain: the same keystrokes, the same gateway, the
// reply renders on BOTH grids identically (symptom ②)
// ---------------------------------------------------------------------------

#[test]
fn s2_reply_grid_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("s2", COLS);
    boot_both(&mut rig);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    // Both titles should ALSO settle to the distinct seed text.
    let rust_title =
        rig.rust_tui.wait_for(TITLE_TEXT.as_bytes(), rust_from, Duration::from_secs(20));
    let ref_title = rig.ref_tui.wait_for(TITLE_TEXT.as_bytes(), ref_from, Duration::from_secs(30));
    settle(3);
    let ref_raw = rig.ref_tui.screen_text();
    let rust_raw = rig.rust_tui.screen_text();
    // G-433 CLOSED (gwave-1): the port's model listing now seeds
    // base-less custom-provider models from the ref's
    // `Model.Info.default` (limit context 200_000 / output 32_000) —
    // both ends resolve the limit, the footer's `(N%)` group compares
    // exactly (replace_percent_group retired from normalize_grid).
    let ref_grid = rig.normalize(&ref_raw);
    let rust_grid = rig.normalize(&rust_raw);
    dump_grid("s2-reply-ref", &ref_grid);
    dump_grid("s2-reply-rust", &rust_grid);
    let stats = rig.gateway.stats();
    drop(rig);
    assert!(
        rust_replied && ref_replied,
        "S2 RED: reply rendered rust={rust_replied} ref={ref_replied}; gateway {stats}"
    );
    assert!(
        rust_title && ref_title,
        "S2 RED: session title distinct-seed text rendered rust={rust_title} ref={ref_title} \
         (needle/title separation, W3 hardening ②)"
    );
    assert_grids_match("s2-reply", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// S3 — model palette CONTENT as a true grid assertion (W1 review
// Important: symptom ⑥ upgraded from the server-side proxy to the
// rendered palette). `<leader>m` = ctrl+x, m on both ends.
//
// G-434 (documented narrowing): below the configured models the
// palette lists the no-auth "free" tier from each side's models.dev
// catalog vintage — ref's tree snapshot and the port's embedded
// snapshot are different SNAPSHOTS (production fetches one live
// catalog), so those rows are asserted for PRESENCE, not row-exact
// alignment. The palette chrome (search field, headers, the
// configured replay model with its selection dot, the footer) IS
// compared row-exact.
// ---------------------------------------------------------------------------

#[test]
fn s3_palette_grid_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("s3", COLS);
    boot_both(&mut rig);
    // `<leader>m` opens the palette; typing the configured model id
    // FILTERS the list down to it on both ends — a row-exact content
    // assertion that sidesteps the G-434 catalog-vintage free tier
    // (asserted separately below for presence).
    rig.write_both(b"\x18m");
    let rust_open = rig.rust_tui.wait_for(b"claude-opus-4-7", 0, Duration::from_secs(20));
    let ref_open = rig.ref_tui.wait_for(b"claude-opus-4-7", 0, Duration::from_secs(30));
    rig.write_both(b"claude-opus-4-7");
    // CI-slower runners paint the selection dot asynchronously — the
    // first CI triggers asserted a pre-settle capture (forensics dumps
    // taken seconds later HAD the dot on both ends). Wait for the
    // needle BEFORE capturing, not just before asserting.
    // W9 hardening: the wait polls the PARSED GRID, not the raw byte
    // stream — the dot cell and the label cells are painted as
    // separate writes (cursor jumps + SGR runs between them), so the
    // contiguous `● <model>` byte run never appears in the raw stream
    // even though the settled grid carries it (the W8 raw waits flaked
    // 4/4 locally while the settled dumps always had the dot).
    let dot_grid = |tui: &TuiProcess| tui.screen_text().contains("● claude-opus-4-7");
    let wait_grid = |tui: &TuiProcess, secs: u64| -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if dot_grid(tui) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    let rust_dot = wait_grid(&rig.rust_tui, 15);
    let ref_dot = wait_grid(&rig.ref_tui, 25);
    settle(2);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("s3-palette-ref", &ref_grid);
    dump_grid("s3-palette-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_open && ref_open,
        "S3 RED: palette lists the replay model rust={rust_open} ref={ref_open}"
    );
    assert!(
        rust_dot && ref_dot,
        "S3 RED: selection dot never settled rust={rust_dot} ref={ref_dot}"
    );
    // Presence asserts on the vintage-dependent tail (G-434): the
    // filtered view must still be a palette (footer) and both grids
    // must carry the configured model row with its selection dot.
    let ref_footer = ref_grid.contains("View all integrations ctrl+a");
    let rust_footer = rust_grid.contains("View all integrations ctrl+a");
    let ref_selected = ref_grid.lines().any(|l| l.contains("● claude-opus-4-7"));
    let rust_selected = rust_grid.lines().any(|l| l.contains("● claude-opus-4-7"));
    assert!(
        ref_footer && rust_footer && ref_selected && rust_selected,
        "S3 RED: palette footer/selection — footer ref={ref_footer} rust={rust_footer}, \
         selected replay model ref={ref_selected} rust={rust_selected}"
    );
    assert_grids_match("s3-palette", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// S4 — session persistence across a TUI restart, BOTH ends (symptom ④)
// ---------------------------------------------------------------------------

#[test]
fn s4_restart_session_grid_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("s4", COLS);
    boot_both(&mut rig);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    settle(2);
    // Restart BOTH TUIs against the SAME sandboxes/servers.
    rig.rust_tui.shutdown(Duration::from_secs(10));
    rig.ref_tui.shutdown(Duration::from_secs(10));
    let mut rust_tui = TuiProcess::spawn(&rig.rust, COLS, ROWS);
    let mut ref_tui = ref_tui_spawn(&rig.refbox, &rig.stage, COLS, ROWS);
    let rust_settled = rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    // The session tab carries the (seeded) title on both ends.
    let rust_tab = rust_tui.wait_for(TITLE_TEXT.as_bytes(), 0, Duration::from_secs(20));
    let ref_tab = ref_tui.wait_for(TITLE_TEXT.as_bytes(), 0, Duration::from_secs(30));
    std::thread::sleep(Duration::from_secs(3));
    let ref_grid = rig.normalize(&ref_tui.screen_text());
    let rust_grid = rig.normalize(&rust_tui.screen_text());
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    drop(rig);
    assert!(
        rust_replied && ref_replied && rust_settled && ref_settled,
        "S4 RED: reply rust={rust_replied} ref={ref_replied}; restart settled \
         rust={rust_settled} ref={ref_settled}"
    );
    assert!(
        rust_tab && ref_tab,
        "S4 RED: session tab survived restart rust={rust_tab} ref={ref_tab}"
    );
    assert_grids_match("s4-restart", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// S5 — the sidebar at 160 cols (T9-5 upgraded to a BOTH-ends grid
// comparison; the sidebar auto-docks only above the 120-col breakpoint)
// ---------------------------------------------------------------------------

#[test]
fn s5_sidebar_grid_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("s5", 160);
    boot_both(&mut rig);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    // G-435 closure: the Context block now renders on BOTH ends
    // (matcher aligned to ref's util/session.ts:45-56 — no
    // cache_read/retry conditions). Condition-wait the needle BEFORE
    // capturing: the block paints with the settle, not with the
    // reply text (the CI-slower-runner lesson from S3).
    let rust_ctx = rig.rust_tui.wait_for(b"Context", 0, Duration::from_secs(30));
    let ref_ctx = rig.ref_tui.wait_for(b"Context", 0, Duration::from_secs(30));
    settle(3);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("s5-sidebar-ref", &ref_grid);
    dump_grid("s5-sidebar-rust", &rust_grid);
    drop(rig);
    assert!(rust_replied && ref_replied, "S5 RED: reply rust={rust_replied} ref={ref_replied}");
    // Ref's actual-run sidebar shape (the T9-5 source-derived
    // expectations, now pinned from the ref REAL run): the Context
    // section + its tokens row + its percent row (ref has a model
    // context limit — G-433 — so `0% used` renders).
    let ref_context = ref_grid.contains("Context");
    let ref_tokens = ref_grid.contains("38 tokens");
    let ref_used = ref_grid.contains("0% used");
    assert!(
        ref_context && ref_tokens && ref_used,
        "S5 RED: ref sidebar shape — context={ref_context} tokens={ref_tokens} used={ref_used}"
    );
    // G-435 CLOSED (root cause: the sidebar usage matcher required
    // `cache_read: None`/`retry: None` — ref has neither condition —
    // while fetched transcript pages materialize `cache_read: Some(0)`;
    // a post-settle snapshot (tie -> fetched wins) then dropped the
    // Context block on turns whose message.list landed late. The
    // matcher now mirrors ref; the column mask is RETIRED and the
    // full sidebar region is compared (G-433's percent row stays
    // masked below until the limit parsing closes).
    let rust_context = rust_grid.contains("Context");
    let rust_tokens = rust_grid.contains("38 tokens");
    assert!(
        rust_ctx && ref_ctx && rust_context && rust_tokens,
        "S5 RED: G-435 regression — Context block rust={rust_context} (waited rust={rust_ctx} \
         ref={ref_ctx}) tokens={rust_tokens}; the sidebar usage matcher must mirror \
         util/session.ts:45-56 (no cache_read/retry conditions)"
    );
    // G-433 CLOSED (gwave-1): the port resolves the model-catalog
    // context limit (`Model.Info.default` seeding), so `0% used`
    // renders on BOTH ends.
    let rust_used = rust_grid.contains("% used");
    assert!(
        rust_used && ref_used,
        "S5 RED: G-433 regression — the sidebar percent row must render on both ends \
         (rust={rust_used} ref={ref_used}); the base-less custom-model seeding must keep \
         the ref's default limit (schema/model.ts:146-158)"
    );
    // G-435-residual CLOSED (gwave-1): the sidebar's title box carries
    // `paddingBottom={1}` (sidebar.tsx:34) — the content slot starts
    // one blank row below the title on BOTH ends (the port painted the
    // first section directly under the title). The section rows now
    // align, so the sidebar-region column mask, the G-436
    // border-count pin and the drop_blank_rows normalization all
    // RETIRE — the full grid compares row-exact.
    let rust_context_on_hello =
        rust_grid.lines().any(|l| l.contains("Context") && l.contains("hello"));
    let ref_context_on_hello =
        ref_grid.lines().any(|l| l.contains("Context") && l.contains("hello"));
    assert!(
        !rust_context_on_hello && !ref_context_on_hello,
        "G-435-residual regression: the Context title must sit one row below \
         the title's padding row, never on the hello row (rust={rust_context_on_hello} \
         ref={ref_context_on_hello}) — the sidebar title box's paddingBottom must stay"
    );
    assert_grids_match("s5-sidebar", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// S6 — config hot reload reaches the rendered palette on BOTH ends
// (symptom ③ + ⑥): the same config edit, rust via
// `/api/location/reload`, ref via its native config watcher.
// ---------------------------------------------------------------------------

#[test]
fn s6_hot_reload_palette_grid_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("s6", COLS);
    boot_both(&mut rig);
    // The same hot edit on both ends.
    rig.rust.write_config(&[("claude-sonnet-4-6", "claude-sonnet-4-6")], "replay/claude-opus-4-7");
    rig.refbox
        .write_ref_config(&[("claude-sonnet-4-6", "claude-sonnet-4-6")], "replay/claude-opus-4-7");
    let (reload_status, _body) = http_post(
        &format!("{}/api/location/reload", rig.rust_server_url),
        "{}",
        rig.rust_password.as_deref(),
    )
    .unwrap_or((0, String::new()));
    // Ref: its native watcher picks the change up (no reload endpoint
    // on the ref server); give it a beat.
    std::thread::sleep(Duration::from_secs(3));
    // Same as S3: open the palette and FILTER to the new model — the
    // unfiltered viewport's top is the catalog-vintage free tier
    // (G-434), so the configured model rows are reached via search.
    rig.write_both(b"\x18m");
    std::thread::sleep(Duration::from_millis(500));
    rig.write_both(b"claude-sonnet-4-6");
    // G-437 CLOSED (gwave-1): the TUI re-syncs the model/provider
    // lists on the `model.updated`/`provider.updated`/
    // `integration.updated`/`config.updated` inventory events (the
    // ref's data.ts:1204-1210/1253-1257 invalidate+sync chain) — the
    // palette reflects the hot-added model on BOTH ends; the rows
    // compare exactly (mask_model retired).
    let rust_open = rig.rust_tui.wait_for(b"claude-sonnet-4-6", 0, Duration::from_secs(10));
    let ref_open = rig.ref_tui.wait_for(b"claude-sonnet-4-6", 0, Duration::from_secs(30));
    settle(2);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("s6-hotreload-ref", &ref_grid);
    dump_grid("s6-hotreload-rust", &rust_grid);
    // Rust-side outcome parity at the server level (the API reflects
    // the hot edit — t9-6's machine proxy).
    let (status, body) =
        http_get(&format!("{}/api/model", rig.rust_server_url), rig.rust_password.as_deref())
            .unwrap_or((0, String::new()));
    drop(rig);
    assert!(
        (200..300).contains(&reload_status) && status == 200 && body.contains("claude-sonnet-4-6"),
        "S6 RED: reload={reload_status} model={status}; rust API body: {body}"
    );
    assert!(ref_open, "S6 RED: the ref palette never listed the hot-added model");
    assert!(rust_open, "S6 RED: the port palette never listed the hot-added model");
    assert_grids_match("s6-hotreload", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// Bootstrap: seed the ref-shaped corpus from the ref engine's own
// (sandbox) requests. Not part of the pair matrix; run explicitly once
// per ref request-shape change:
// `T9_RECORD=1 cargo nextest run --exact t9x_ref_bootstrap_seed`.
// ---------------------------------------------------------------------------

#[test]
fn t9x_ref_bootstrap_seed() {
    skip_without_ref_lane!();
    if std::env::var("T9_RECORD").is_err() {
        eprintln!("t9x_ref_bootstrap_seed: skipped (T9_RECORD unset)");
        return;
    }
    // The ref tree must exist for the ref lane at all.
    assert!(ref_root().exists(), "ref tree missing: {}", ref_root().display());
    let lane = lane_lock();
    let gateway = Gateway::spawn("t9-3ref", Some(&corpus_ref_dir()));
    let refbox = sandbox("t9-3ref", gateway.port);
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let mut ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    assert!(
        ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60)),
        "ref bootstrap: the ref TUI never settled"
    );
    std::thread::sleep(Duration::from_secs(3));
    let from = ref_tui.raw_len();
    ref_tui.write(b"hello");
    std::thread::sleep(Duration::from_millis(500));
    ref_tui.write(b"\r");
    let rendered = ref_tui.wait_for(REPLY_NEEDLE, from, Duration::from_secs(60));
    let _title = ref_tui.wait_for(TITLE_TEXT.as_bytes(), from, Duration::from_secs(30));
    let screen = ref_tui.screen_text();
    let stats = gateway.stats();
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    drop(gateway);
    drop(lane);
    eprintln!("--- ref screen ---\n{screen}\n--- gateway {stats}");
    assert!(rendered, "ref bootstrap: the seeded reply never rendered; stats {stats}");
}

/// Sanity for the committed corpora (no TUI needed): both dirs exist
/// and carry at least the two seed cassettes the pair lane replays.
#[test]
fn t9x_corpus_layout() {
    for (dir, needle_file) in
        [(corpus_dir(), "main turn (rust engine)"), (corpus_ref_dir(), "main turn (ref engine)")]
    {
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("corpus dir {dir:?}: {e}")).count();
        assert!(entries >= 1, "corpus dir {dir:?} empty ({needle_file})");
    }
}

// ===========================================================================
// W5 — TIMING-FIDELITY scenarios (P1/P2/P3): the gateway paces the SAME
// schedule to BOTH ends (deterministic synthetic tiers — differential
// fairness holds: identical I/O INCLUDING timing), so the STREAMING UI
// state surfaces (spinner window, partial text, interrupt affordance)
// are exercised on the real chain, not just the settled grid.
// ===========================================================================

/// P1 — slow TTFT (4s): the busy window is wide enough to assert the
/// pre-first-token UI state on BOTH ends (spinner/interrupt affordance
/// visible while no reply text exists), then the settled grids match.
#[test]
fn p1_slow_ttft_state_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig_ext("p1", COLS, "synthetic:seed=1:ttft=4000:tps=120", false, false);
    boot_both(&mut rig);
    rig.write_both(b"hello\r");
    // Inside the 4s TTFT window: both ends are busy with NO reply text.
    std::thread::sleep(Duration::from_millis(1800));
    let rust_busy = rig.rust_tui.screen_text();
    let ref_busy = rig.ref_tui.screen_text();
    let rust_interrupt = rust_busy.contains("interrupt");
    let ref_interrupt = ref_busy.contains("interrupt");
    let needle_text = String::from_utf8_lossy(REPLY_NEEDLE).into_owned();
    let rust_no_text = !rust_busy.contains(&needle_text);
    let ref_no_text = !ref_busy.contains(&needle_text);
    dump_grid("p1-busy-ref", &ref_busy);
    dump_grid("p1-busy-rust", &rust_busy);
    let rust_replied = rig.rust_tui.wait_for_screen(&needle_text, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for_screen(&needle_text, Duration::from_secs(60));
    settle(3);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p1-settled-ref", &ref_grid);
    dump_grid("p1-settled-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_interrupt && ref_interrupt,
        "P1 RED: the busy interrupt affordance must render in the TTFT window — \
         rust={rust_interrupt} ref={ref_interrupt}"
    );
    assert!(
        rust_no_text && ref_no_text,
        "P1 RED: no reply text may exist inside the TTFT window — \
         rust={rust_no_text} ref={ref_no_text}"
    );
    assert!(
        rust_replied && ref_replied,
        "P1 RED: the paced reply never settled rust={rust_replied} ref={ref_replied}"
    );
    assert_grids_match("p1-ttft", &ref_grid, &rust_grid);
}

/// P2 — slow TPS streaming: the token stream renders progressively on
/// BOTH ends (a PARTIAL prefix visible mid-stream, before the needle
/// completes), then the settled grids match.
#[test]
fn p2_slow_tps_stream_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig_ext("p2", COLS, "synthetic:seed=2:ttft=300:tps=5", false, false);
    boot_both(&mut rig);
    rig.write_both(b"hello\r");
    // The stream is ~4s of per-token deltas: mid-stream, a PARTIAL
    // prefix of the needle must already be on BOTH grids.
    std::thread::sleep(Duration::from_millis(2400));
    let rust_mid = rig.rust_tui.screen_text();
    let ref_mid = rig.ref_tui.screen_text();
    dump_grid("p2-mid-ref", &ref_mid);
    dump_grid("p2-mid-rust", &rust_mid);
    let rust_partial = rust_mid.contains("REALCH") && !rust_mid.contains("REALCHAIN-REPLAY-OK");
    let ref_partial = ref_mid.contains("REALCH") && !ref_mid.contains("REALCHAIN-REPLAY-OK");
    let needle_text = "REALCHAIN-REPLAY-OK";
    let rust_replied = rig.rust_tui.wait_for_screen(needle_text, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for_screen(needle_text, Duration::from_secs(60));
    settle(3);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p2-settled-ref", &ref_grid);
    dump_grid("p2-settled-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_partial && ref_partial,
        "P2 RED: a partial token prefix must render mid-stream — \
         rust={rust_partial} ref={ref_partial}"
    );
    assert!(
        rust_replied && ref_replied,
        "P2 RED: the streamed reply never completed rust={rust_replied} ref={ref_replied}"
    );
    assert_grids_match("p2-tps", &ref_grid, &rust_grid);
}

/// P3 — ESC interrupt MID-STREAM (real timing, not the channel-8
/// virtual clock): once partial text is visible, the interrupt chord
/// aborts the turn on BOTH ends; the post-interrupt state machines are
/// compared on the settled grid.
#[test]
fn p3_stream_interrupt_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig_ext("p3", COLS, "synthetic:seed=3:ttft=300:tps=3", false, false);
    boot_both(&mut rig);
    rig.write_both(b"hello\r");
    // Wait until the stream is visibly in progress on both ends, then
    // interrupt. Ref's prompt arms on the first ESC and interrupts on
    // the second ("again to interrupt"); the port mirrors that — so
    // send the double chord to BOTH.
    let rust_streaming = rig.rust_tui.wait_for_screen("REALCH", Duration::from_secs(30));
    let ref_streaming = rig.ref_tui.wait_for_screen("REALCH", Duration::from_secs(60));
    rig.write_both(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    rig.write_both(b"\x1b");
    // The full needle must NOT complete on either end (the stream was
    // cut mid-flight). Give any straggler delta time to land first.
    std::thread::sleep(Duration::from_millis(1500));
    let rust_cut = !rig.rust_tui.screen_has("REALCHAIN-REPLAY-OK");
    let ref_cut = !rig.ref_tui.screen_has("REALCHAIN-REPLAY-OK");
    settle(4);
    // Forensics (symptom triage): the settled assistant messages from
    // the rust server — does the interrupted message carry the ref's
    // `error.message === "Step interrupted"` shape?
    let sessions =
        http_get(&format!("{}/api/session", rig.rust_server_url), rig.rust_password.as_deref())
            .map(|(_, body)| body)
            .unwrap_or_default();
    if let Some(id_start) = sessions.find("\"ses_") {
        let id_end =
            sessions[id_start + 1..].find('"').map(|e| id_start + 1 + e).unwrap_or(sessions.len());
        let sid = &sessions[id_start + 1..id_end];
        let messages = http_get(
            &format!("{}/api/session/{sid}/message", rig.rust_server_url),
            rig.rust_password.as_deref(),
        )
        .map(|(_, body)| body)
        .unwrap_or_default();
        eprintln!("P3 rust messages ({sid}): {}", &messages[..messages.len().min(3000)]);
    }
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p3-interrupt-ref", &ref_grid);
    dump_grid("p3-interrupt-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_streaming && ref_streaming,
        "P3 RED: the stream never became visible rust={rust_streaming} ref={ref_streaming}"
    );
    assert!(
        rust_cut && ref_cut,
        "P3 RED: the interrupt must cut the stream before completion — \
         rust_cut={rust_cut} ref_cut={ref_cut}"
    );
    assert_grids_match("p3-interrupt", &ref_grid, &rust_grid);
}

impl TuiProcess {
    /// Instant (non-blocking) SCREEN check: the needle is on the
    /// rendered grid. (A paced stream renders chunk-by-chunk with
    /// cursor positioning between chunks, so the RAW stream cannot be
    /// searched for late-stream text on both ends.)
    fn screen_has(&self, needle: &str) -> bool {
        self.screen_text().contains(needle)
    }

    /// Polling screen wait.
    fn wait_for_screen(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.screen_has(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}

// ===========================================================================
// W5 — T9-7: the REAL-plugin rig (symptom ⑦: plugin-added sidebar
// content). Both sandboxes carry read-only copies of the REAL plugin
// payload trees under their install caches and name them in the config
// `plugins[]` array — the real config discovery path, never driver
// injection. Red here = the user's symptom ⑦ is machine-reproduced.
// ===========================================================================

#[test]
fn t9_7_plugin_sidebar_pair() {
    skip_without_ref_lane!();
    if let Some(reason) = plugin_lane_skip_reason() {
        eprintln!("T9-7 SKIP: {reason}");
        return;
    }
    let mut rig = pair_rig_ext("t9-7", 160, "instant", true, false);
    boot_both(&mut rig);
    // The ref plugin UI settles AFTER the wordmark (plugin load is
    // async): the footer's MCP row is ref's plugin-contributed face —
    // wait for it (a ref that never loads its plugins is a rig RED,
    // not a mask).
    // The ref plugin UI settles AFTER the wordmark (plugin load is
    // async): the footer's MCP row is ref's plugin-contributed face.
    // The row appears once an MCP CONNECTS — a plugin MCP connecting
    // late flips `1 MCP` → `2 MCP` — so poll BOTH screens until the
    // counts agree (same plugins on both ends ⇒ same settled count).
    let mcp_count = |tui: &TuiProcess| -> Option<u32> {
        let screen = tui.screen_text();
        screen.lines().find_map(|line| {
            let at = line.find('⊙')?; // e.g. `  ⊙ 2 MCP /mcps`
            let rest = line[at + '⊙'.len_utf8()..].trim_start();
            rest.split_whitespace().next()?.parse().ok()
        })
    };
    let mut counts = (None, None);
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        counts = (mcp_count(&rig.rust_tui), mcp_count(&rig.ref_tui));
        if let (Some(rust_n), Some(ref_n)) = counts {
            if rust_n == ref_n && rust_n > 0 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let (rust_n, ref_n) = counts;
    let (agents_status, agents_body) =
        http_get(&format!("{}/api/agent", rig.rust_server_url), rig.rust_password.as_deref())
            .unwrap_or((0, String::new()));
    let (mcp_status, mcp_body) =
        http_get(&format!("{}/api/mcp", rig.rust_server_url), rig.rust_password.as_deref())
            .unwrap_or((0, String::new()));
    // The rust server's plugin API tells us whether the real chain
    // even LOADED the plugins (the boot chain vs slot registration vs
    // sidebar assembly triage).
    let (status, body) =
        http_get(&format!("{}/api/plugin", rig.rust_server_url), rig.rust_password.as_deref())
            .unwrap_or((0, String::new()));
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("t9-7-plugin-ref", &ref_grid);
    dump_grid("t9-7-plugin-rust", &rust_grid);
    let stats = rig.gateway.stats();
    drop(rig);
    eprintln!("T9-7 rust /api/plugin ({status}): {body}");
    eprintln!(
        "T9-7 rust /api/agent ({agents_status}): {}",
        &agents_body[..agents_body.len().min(2000)]
    );
    eprintln!("T9-7 rust /api/mcp ({mcp_status}): {}", &mcp_body[..mcp_body.len().min(2000)]);
    eprintln!("T9-7 gateway stats: {stats}");
    assert_eq!(status, 200, "T9-7 RED: rust /api/plugin status {status}; body {body}");
    assert!(
        matches!((rust_n, ref_n), (Some(n), Some(m)) if n == m && n > 0),
        "T9-7 RED: the plugin MCP row never settled equal — rust={rust_n:?} \
         ref={ref_n:?} (see /api/plugin and /api/mcp above)"
    );
    // The settled count is a grid assertion: both ends run the SAME
    // plugins, so `⊙ N MCP` must agree row-exactly. G-435-residual
    // closed (gwave-1): the sidebar region compares unmasked.
    assert_grids_match("t9-7-plugin", &ref_grid, &rust_grid);
}

// ===========================================================================
// W5 — P4: the no-mask sidebar deep compare (G-435/G-436 narrowing).
// State matrix × width tier: {empty session, reply settled, long
// transcript} × {160, 120}. Every state's FULL grid (sidebar region
// unmasked) is dumped for the gap ledger; the strict compare runs with
// the existing documented masks, and any NEW divergence is registered
// from the dumps.
// ===========================================================================

#[test]
fn p4_sidebar_deep_compare_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig_ext("p4", 160, "instant", false, true);
    boot_both(&mut rig);
    // State A: empty session (sidebar region docked at 160 cols).
    let a_ref = rig.normalize(&rig.ref_tui.screen_text());
    let a_rust = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p4-empty-ref", &a_ref);
    dump_grid("p4-empty-rust", &a_rust);
    // State B: reply settled.
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    let _ = rig.ref_tui.wait_for(TITLE_TEXT.as_bytes(), ref_from, Duration::from_secs(30));
    settle(3);
    let b_ref = rig.normalize(&rig.ref_tui.screen_text());
    let b_rust = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p4-reply-ref", &b_ref);
    dump_grid("p4-reply-rust", &b_rust);
    // State C: long transcript (later turns are gateway-seeded misses
    // — the SAME deterministic seed on both ends).
    for turn in 2..=4 {
        let msg = format!("turn {turn}\r");
        let rust_from = rig.rust_tui.raw_len();
        let ref_from = rig.ref_tui.raw_len();
        rig.write_both(msg.as_bytes());
        let rust_ok = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
        let ref_ok = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
        assert!(
            rust_ok && ref_ok,
            "P4 RED: long-transcript turn {turn} did not settle rust={rust_ok} ref={ref_ok}"
        );
    }
    settle(3);
    let c_ref = rig.normalize(&rig.ref_tui.screen_text());
    let c_rust = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p4-long-ref", &c_ref);
    dump_grid("p4-long-rust", &c_rust);
    drop(rig);
    assert!(rust_replied && ref_replied, "P4 RED: first reply did not settle");

    // G-435 narrowing (live): does the port sidebar carry the Context
    // block in the pair rig TODAY? Either outcome is recorded in the
    // dumps; the mask stays until the SIDEBAR REGION compares clean.
    let rust_context = c_rust.contains("Context");
    let ref_context = c_ref.contains("Context");
    eprintln!(
        "P4 G-435 narrowing: sidebar Context block rust={rust_context} ref={ref_context} \
         (S5's mask assumed rust=false ref=true)"
    );

    // Strict compare per state — G-435-residual closed (gwave-1): the
    // full grids (sidebar region included) compare row-exact.
    for (name, ref_grid, rust_grid) in
        [("p4-empty", &a_ref, &a_rust), ("p4-reply", &b_ref, &b_rust), ("p4-long", &c_ref, &c_rust)]
    {
        assert_grids_match(name, ref_grid, rust_grid);
    }
}

/// P4b: the same state matrix at the 120-col tier (no docked sidebar —
/// the full grid is row-exact, no masks at all).
#[test]
fn p4b_sidebar_120_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig_ext("p4b", COLS, "instant", false, true);
    boot_both(&mut rig);
    let a_ref = rig.normalize(&rig.ref_tui.screen_text());
    let a_rust = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p4b-empty-ref", &a_ref);
    dump_grid("p4b-empty-rust", &a_rust);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    settle(3);
    let b_ref = rig.normalize(&rig.ref_tui.screen_text());
    let b_rust = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("p4b-reply-ref", &b_ref);
    dump_grid("p4b-reply-rust", &b_rust);
    drop(rig);
    assert!(rust_replied && ref_replied, "P4b RED: reply did not settle");
    assert_grids_match("p4b-empty", &a_ref, &a_rust);
    assert_grids_match("p4b-reply", &b_ref, &b_rust);
}
