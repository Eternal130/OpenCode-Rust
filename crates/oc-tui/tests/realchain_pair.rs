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
    capture_settled, corpus_dir, corpus_ref_dir, default_pace, dump_grid, export_grid_diff,
    export_survey, http_get, http_post, lane_lock, normalize_grid, plugin_lane_skip_reason,
    ref_lane_skip_reason, ref_root, ref_service_stop, ref_stage, ref_tui_spawn, sandbox,
    service_start, service_stop, stage_plugins, worktree_dir, Gateway, LaneGuard, RefStage,
    Sandbox, TuiProcess, WORDMARK,
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
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    drop(rig);
    // W12/2.2#12: no content adjudication in-test — the pair exports.
    export_grid_diff("s1", "boot", &ref_grid, &rust_grid);
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
    export_grid_diff("s2", "reply", &ref_grid, &rust_grid);
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
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
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
    // Ref-side palette sanity ONLY (W12/2.2#12: rig health — a ref
    // that never opened a palette is a rig break, not a diff): the
    // filtered view must still be a palette (footer) and carry the
    // configured model row with its selection dot. Rust-side content
    // judgments ride the exported diff.
    let ref_footer = ref_grid.contains("View all integrations ctrl+a");
    let ref_selected = ref_grid.lines().any(|l| l.contains("● claude-opus-4-7"));
    assert!(
        ref_footer && ref_selected,
        "S3 RED (ref-side rig sanity): palette footer={ref_footer} selected replay \
         model={ref_selected}"
    );
    export_grid_diff("s3", "palette", &ref_grid, &rust_grid);
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
    let ref_grid = rig.normalize(&capture_settled(&ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rust_tui));
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
    export_grid_diff("s4", "restart", &ref_grid, &rust_grid);
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
    // Condition-wait the needle BEFORE capturing (settle class): the
    // block paints with the settle, not with the reply text.
    let _ = rig.rust_tui.wait_for(b"Context", 0, Duration::from_secs(30));
    let _ = rig.ref_tui.wait_for(b"Context", 0, Duration::from_secs(30));
    settle(3);
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("s5-sidebar-ref", &ref_grid);
    dump_grid("s5-sidebar-rust", &rust_grid);
    drop(rig);
    assert!(rust_replied && ref_replied, "S5 RED: reply rust={rust_replied} ref={ref_replied}");
    // Ref-side scenario-content sanity ONLY (W12/2.2#12: the ref MUST
    // render the Context block + tokens + percent row — a ref without
    // them is a broken rig, never a pass). Rust-side content judgments
    // (Context/tokens/percent presence, the old G-433/G-435 closure
    // pins and the G-435-residual vertical-offset pin) all ride the
    // exported diff now — see 2.2#12 for the retirement rationale.
    let ref_context = ref_grid.contains("Context");
    let ref_tokens = ref_grid.contains("38 tokens");
    let ref_used = ref_grid.contains("0% used");
    assert!(
        ref_context && ref_tokens && ref_used,
        "S5 RED (ref-side rig sanity): sidebar shape — context={ref_context} \
         tokens={ref_tokens} used={ref_used}"
    );
    export_grid_diff("s5", "sidebar", &ref_grid, &rust_grid);
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
    // Settle-class wait (quiescence before capture); the rust listing
    // outcome rides the exported diff.
    let _ = rig.rust_tui.wait_for(b"claude-sonnet-4-6", 0, Duration::from_secs(10));
    let ref_open = rig.ref_tui.wait_for(b"claude-sonnet-4-6", 0, Duration::from_secs(30));
    settle(2);
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
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
    // Ref-side sanity only (W12/2.2#12): a ref palette that never
    // listed the hot-added model is a rig break. The rust-side
    // listing (the retired G-437 pin) rides the exported diff.
    assert!(
        ref_open,
        "S6 RED (ref-side rig sanity): the ref palette never listed the hot-added model"
    );
    export_grid_diff("s6", "hotreload", &ref_grid, &rust_grid);
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
    let needle_text = String::from_utf8_lossy(REPLY_NEEDLE).into_owned();
    dump_grid("p1-busy-ref", &ref_busy);
    dump_grid("p1-busy-rust", &rust_busy);
    // M-1: normalize + export the busy state IMMEDIATELY (before the
    // settled grids normalize) so the drained normalization snapshot
    // is this report's own.
    let busy_ref = rig.normalize(&ref_busy);
    let busy_rust = rig.normalize(&rust_busy);
    export_grid_diff("p1", "busy", &busy_ref, &busy_rust);
    let rust_replied = rig.rust_tui.wait_for_screen(&needle_text, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for_screen(&needle_text, Duration::from_secs(60));
    settle(3);
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p1-settled-ref", &ref_grid);
    dump_grid("p1-settled-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_replied && ref_replied,
        "P1 RED: the paced reply never settled rust={rust_replied} ref={ref_replied}"
    );
    export_grid_diff("p1", "settled", &ref_grid, &rust_grid);
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
    let needle_text = "REALCHAIN-REPLAY-OK";
    // M-1: normalize + export the mid state IMMEDIATELY (before the
    // settled grids normalize) so the drained normalization snapshot
    // is this report's own.
    let mid_ref = rig.normalize(&ref_mid);
    let mid_rust = rig.normalize(&rust_mid);
    export_grid_diff("p2", "mid", &mid_ref, &mid_rust);
    let rust_replied = rig.rust_tui.wait_for_screen(needle_text, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for_screen(needle_text, Duration::from_secs(60));
    settle(3);
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p2-settled-ref", &ref_grid);
    dump_grid("p2-settled-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_replied && ref_replied,
        "P2 RED: the streamed reply never completed rust={rust_replied} ref={ref_replied}"
    );
    export_grid_diff("p2", "settled", &ref_grid, &rust_grid);
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
    // W12/2.2#12: absence-of-content is a content judgment — exported
    // as a survey (expected completed-needle count 0 on both ends),
    // adjudicated against the registry, not asserted in-test.
    std::thread::sleep(Duration::from_millis(1500));
    let count_needle = |tui: &TuiProcess| -> u64 {
        tui.screen_text().matches("REALCHAIN-REPLAY-OK").count() as u64
    };
    let rust_cut_count = count_needle(&rig.rust_tui);
    let ref_cut_count = count_needle(&rig.ref_tui);
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
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p3-interrupt-ref", &ref_grid);
    dump_grid("p3-interrupt-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_streaming && ref_streaming,
        "P3 RED: the stream never became visible rust={rust_streaming} ref={ref_streaming}"
    );
    export_grid_diff("p3", "interrupt", &ref_grid, &rust_grid);
    export_survey(
        "p3",
        "interrupt-cut",
        &[
            ("rust full-needle rows after interrupt", 0, rust_cut_count),
            ("ref full-needle rows after interrupt", 0, ref_cut_count),
        ],
        None,
    );
}

impl TuiProcess {
    /// Instant (non-blocking) SCREEN check: the needle is on the
    /// rendered grid. (A paced stream renders chunk-by-chunk with
    /// cursor positioning between chunks, so the RAW stream cannot be
    /// searched for late-stream text on both ends.)
    #[allow(dead_code)] // W12: the p3 cut-check moved to the survey export
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
    eprintln!("T9-7 settled MCP counts: rust={rust_n:?} ref={ref_n:?} (adjudicated via the exported diff)");
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
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
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
    // W12/2.2#12: the settled MCP-count equality and the whole plugin
    // sidebar (the omo-slim sections gap included) ride the exported
    // diff — the old count assert and the S5-region mask it borrowed
    // are retired (the T9-7 masking failure of the three-failure
    // retrospective).
    export_grid_diff("t9-7", "plugin", &ref_grid, &rust_grid);
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
    let a_ref = rig.normalize(&capture_settled(&rig.ref_tui));
    let a_rust = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p4-empty-ref", &a_ref);
    dump_grid("p4-empty-rust", &a_rust);
    // M-1: export immediately after the state's normalize calls so the
    // drained normalization snapshot is this report's own.
    export_grid_diff("p4", "empty", &a_ref, &a_rust);
    // State B: reply settled.
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    let _ = rig.ref_tui.wait_for(TITLE_TEXT.as_bytes(), ref_from, Duration::from_secs(30));
    settle(3);
    let b_ref = rig.normalize(&capture_settled(&rig.ref_tui));
    let b_rust = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p4-reply-ref", &b_ref);
    dump_grid("p4-reply-rust", &b_rust);
    export_grid_diff("p4", "reply", &b_ref, &b_rust);
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
    let c_ref = rig.normalize(&capture_settled(&rig.ref_tui));
    let c_rust = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p4-long-ref", &c_ref);
    dump_grid("p4-long-rust", &c_rust);
    export_grid_diff("p4", "long", &c_ref, &c_rust);
    drop(rig);
    assert!(rust_replied && ref_replied, "P4 RED: first reply did not settle");

    // G-435 narrowing note (forensics): recorded in the dumps; the
    // adjudication side reads the exported reports.
    let rust_context = c_rust.contains("Context");
    let ref_context = c_ref.contains("Context");
    eprintln!("P4 G-435 narrowing: sidebar Context block rust={rust_context} ref={ref_context}");
}

/// P4b: the same state matrix at the 120-col tier (no docked sidebar —
/// the full grid is row-exact, no masks at all).
#[test]
fn p4b_sidebar_120_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig_ext("p4b", COLS, "instant", false, true);
    boot_both(&mut rig);
    let a_ref = rig.normalize(&capture_settled(&rig.ref_tui));
    let a_rust = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p4b-empty-ref", &a_ref);
    dump_grid("p4b-empty-rust", &a_rust);
    export_grid_diff("p4b", "empty", &a_ref, &a_rust);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    settle(3);
    let b_ref = rig.normalize(&capture_settled(&rig.ref_tui));
    let b_rust = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("p4b-reply-ref", &b_ref);
    dump_grid("p4b-reply-rust", &b_rust);
    export_grid_diff("p4b", "reply", &b_ref, &b_rust);
    drop(rig);
    assert!(rust_replied && ref_replied, "P4b RED: reply did not settle");
}

// ===========================================================================
// va4 — GEOMETRY pair scenarios (the graduated va3 one-shot probe, now a
// realchain battery member). Three terminal sizes — 10x6 (extreme
// narrow), 40x12 (narrow), 160x40 (wide, docked sidebar) — × both ends
// × three states: A = home after a session-seeded boot, B = after
// `<leader>l` (\x18l, Sessions dialog), C = after Enter (dialog action).
// Two history sessions ("Alpha" and the long truncating title) are
// created over EACH side's own HTTP API before the probed boot (boot 1
// starts the daemons — the ref server self-spawns on TUI boot, its
// password read via ref_registration() — then sessions are POSTed, then
// the TUIs restart), so the Sessions dialog has rows on both ends.
//
// W12/2.2#11 model, unchanged: EXPORT ONLY. In-test assertions are
// rig-health class exclusively (both ends' wordmark settle, the REF
// dialog listing Alpha, both TUI processes alive at every capture).
// ALL grid content — including the known divergences below — rides the
// exported diff-reports and the controller's adjudication registry.
//
// Un-normalized diff triage (va4 rounds 1-2, exported grids under
// .test-logs/diff-reports/geom*):
//  - G-465: Sessions dialog header muted ` for <project>` suffix
//    missing on rust (geom40/geom160 dialog) — accepted-gap, fix in
//    flight.
//  - G-466 sub-items: 10x6 post-enter tab-title loss (geom10 enter);
//    40x12 dialog footer-hint overprint + zero-row-viewport ref-side
//    overdraw (geom40 dialog); 160x40 sidebar long-title wrap-vs-
//    truncate (geom160 enter). All accepted-gap, fix in flight.
//  - G-466.2 (40x12 home wordmark, ref two-line large vs rust one-line
//    small): the post-seed boot-2 home settled wordmark-less on BOTH
//    ends in va4 round 1, so the state-`home0` FIRST-BOOT PRE-SEED
//    capture (controller ruling 2026-11-23) keeps the shape visible —
//    va3 boot1 grids carry the wordmark in exactly that state.
//  - FIVE needs-controller shapes beyond the controller's va4 baseline
//    (ruling 2026-11-23: NOT folded into G-465/G-466, registered as
//    ordinary adjudicated entries annotated needs-controller, feed the
//    fix lane's scope): 10x6 home composer box one-row offset
//    (ref r02-r05 borderless-bottom vs rust r01-r04 + r05 bottom
//    border); 10x6 post-enter composer border row offset; 10x6 dialog
//    truncated-title glyph divergence (ref `S` vs rust `e`); 40x12
//    post-enter composer left-border indent off by one space; 160x40
//    footer bottom path row truncating one char earlier on rust.
//
// Test-fn layout: one fn per size. The three legs serialize on
// lane_lock either way (identical total cost, va3 measured ~17s/leg),
// and separate fns buy per-leg nextest retry granularity and failure
// isolation.
// ===========================================================================

const GEOM_LONG_TITLE: &str = "Long Session Title That Must Truncate In The Tab Strip";

struct GeomRig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_url: String,
    rust_password: Option<String>,
    ref_url: String,
    ref_password: Option<String>,
    rust_tui: Option<TuiProcess>,
    ref_tui: Option<TuiProcess>,
    cols: u16,
    rows: u16,
}

impl GeomRig {
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

    fn write_both(&mut self, bytes: &[u8]) {
        self.rust_tui.as_mut().expect("rust tui").write(bytes);
        self.ref_tui.as_mut().expect("ref tui").write(bytes);
    }

    fn spawn_tuis(&mut self) {
        self.rust_tui = Some(TuiProcess::spawn(&self.rust, self.cols, self.rows));
        self.ref_tui = Some(ref_tui_spawn(&self.refbox, &self.stage, self.cols, self.rows));
    }

    fn stop_tuis(&mut self) {
        if let Some(mut tui) = self.rust_tui.take() {
            tui.shutdown(Duration::from_secs(8));
        }
        if let Some(mut tui) = self.ref_tui.take() {
            tui.shutdown(Duration::from_secs(8));
        }
    }

    /// Both TUI processes still alive (rig health before every capture).
    /// `try_wait` needs `&mut`, so the handles are taken and restored.
    fn alive(&mut self) -> bool {
        let mut rust_tui = self.rust_tui.take();
        let mut ref_tui = self.ref_tui.take();
        let ok = |t: &mut Option<TuiProcess>| {
            t.as_mut().is_some_and(|p| p.child.try_wait().map(|w| w.is_none()).unwrap_or(false))
        };
        let rust_ok = ok(&mut rust_tui);
        let ref_ok = ok(&mut ref_tui);
        self.rust_tui = rust_tui;
        self.ref_tui = ref_tui;
        rust_ok && ref_ok
    }
}

impl Drop for GeomRig {
    fn drop(&mut self) {
        self.stop_tuis();
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn geom_rig(tag: &str, cols: u16, rows: u16) -> GeomRig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let ref_url = format!("http://127.0.0.1:{}", refbox.ref_service_port());
    GeomRig {
        _lane: lane,
        _gateway: gateway,
        rust,
        refbox,
        stage,
        rust_url: server.url,
        rust_password: server.password,
        ref_url,
        ref_password: None,
        rust_tui: None,
        ref_tui: None,
        cols,
        rows,
    }
}

fn geom_wait_api(url: &str, password: Option<&str>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let hit =
            http_get(&format!("{url}/api/session"), password).filter(|(code, _)| *code == 200);
        if hit.is_some() || Instant::now() > deadline {
            return hit.is_some();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn geom_seed_sessions(url: &str, password: Option<&str>, project: &std::path::Path) {
    for title in ["Alpha", GEOM_LONG_TITLE] {
        let body = format!(
            "{{\"title\":{},\"location\":{{\"directory\":{}}}}}",
            serde_json::to_string(title).expect("title json"),
            serde_json::to_string(&project.to_string_lossy()).expect("dir json")
        );
        let (code, payload) =
            http_post(&format!("{url}/api/session"), &body, password).unwrap_or((0, String::new()));
        assert!(
            (200..300).contains(&code),
            "GEOM rig RED: session create {title:?} on {url} -> {code}: {}",
            payload.chars().take(200).collect::<String>()
        );
    }
}

/// One geometry leg: seed, probed boot, three exported states.
fn geom_probe(tag: &str, scenario: &str, cols: u16, rows: u16) {
    let mut rig = geom_rig(tag, cols, rows);
    // Boot 1: start the daemons (the ref server self-spawns on TUI
    // boot; wait for its registration to carry the password — va3's
    // first run 401'd reading it any other way).
    rig.spawn_tuis();
    // State home0 — first-boot PRE-SEED home (controller ruling
    // 2026-11-23): the empty-project boot home carries the wordmark on
    // both ends at 40x12 (va3 boot1 grids), so G-466.2 (ref two-line
    // large vs rust one-line small wordmark) stays visible to this
    // regularized tripwire until fixed. The post-seed boot-2 home can
    // settle into a wordmark-less composer layout (observed va4 first
    // round), which is why the pre-seed capture exists — do NOT paper
    // over the shape with a normalization. CAPTURE EARLY: fixed 5s
    // after spawn, BEFORE the API/registration waits — the boot hero
    // wordmark is cleared ~8-10s after spawn on BOTH ends, and cold
    // ref registration alone can burn past that window (va4 rounds
    // 1-3 lost the state exactly this way; the va3 repro kept it only
    // because its capture preceded the slow waits).
    settle(5);
    assert!(rig.alive(), "GEOM rig RED: a TUI died before state home0");
    // CAPTURE-WITH-RETRY: the boot hero paints a few seconds AFTER
    // spawn AND flickers on the wire (va4 diag rounds: the screen
    // carries the wordmark at one screen_text() call, hero-less at the
    // next microseconds later, back again after; fixed marks and
    // condition-then-capture both lose it) — so the capture retries
    // until the CAPTURED GRID itself carries the hero on each end
    // (bounded 30s; the whatever-state-at-timeout exports as-is, no
    // content assertion — the G-466.2 diff rides the export either
    // way). This is a settle-class condition wait, the same license
    // as s3's selection-dot poll.
    let capture_with_hero = |tui: Option<&TuiProcess>| -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let grid = tui.expect("tui").screen_text();
            if grid.contains("█▀▀█") || Instant::now() > deadline {
                return grid;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let h0_ref = rig.normalize(&capture_with_hero(rig.ref_tui.as_ref()));
    let h0_rust = rig.normalize(&capture_with_hero(rig.rust_tui.as_ref()));
    dump_grid(&format!("{scenario}-home0-ref"), &h0_ref);
    dump_grid(&format!("{scenario}-home0-rust"), &h0_rust);
    export_grid_diff(scenario, "home0", &h0_ref, &h0_rust);
    assert!(
        geom_wait_api(&rig.rust_url, rig.rust_password.as_deref()),
        "GEOM rig RED: rust API never came up"
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    while rig.refbox.ref_registration().get("password").is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
    }
    rig.ref_password =
        rig.refbox.ref_registration().get("password").and_then(|v| v.as_str()).map(str::to_owned);
    assert!(
        geom_wait_api(&rig.ref_url, rig.ref_password.as_deref()),
        "GEOM rig RED: ref API never came up (password read: {})",
        rig.ref_password.is_some()
    );
    geom_seed_sessions(&rig.rust_url, rig.rust_password.as_deref(), &rig.rust.project);
    geom_seed_sessions(&rig.ref_url, rig.ref_password.as_deref(), &rig.refbox.project);
    rig.stop_tuis();
    settle(2);
    // Boot 2: the probed boot, with the seeded history.
    rig.spawn_tuis();
    // State A — home. Rig health (settle class): condition-wait each
    // end's boot face — the hero wordmark at >=40 cols paints a few
    // seconds after spawn and persists (G-466.2's stage; fixed marks
    // lose it, va4 bisect), the composer "Ask" prompt at 10x6 (the
    // wordmark does not fit on EITHER end there — va3 grid evidence).
    // The needle waits scan the live screen; the needle merely
    // observed (rig health), all content judgments ride the export.
    let needle_text: &str = if cols <= 16 { "Ask" } else { "█▀▀█" };
    let face =
        |t: Option<&TuiProcess>| t.as_ref().is_some_and(|t| t.screen_text().contains(needle_text));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !(face(rig.rust_tui.as_ref()) && face(rig.ref_tui.as_ref())) && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    let rust_a = face(rig.rust_tui.as_ref());
    let ref_a = face(rig.ref_tui.as_ref());
    assert!(rig.alive(), "GEOM rig RED: a TUI died before state A");
    // Capture the instant the condition holds — same hero-lifecycle
    // discipline as home0 (no settle slack between wait and capture).
    let a_ref = rig.normalize(&rig.ref_tui.as_ref().expect("ref tui").screen_text());
    let a_rust = rig.normalize(&rig.rust_tui.as_ref().expect("rust tui").screen_text());
    dump_grid(&format!("{scenario}-home-ref"), &a_ref);
    dump_grid(&format!("{scenario}-home-rust"), &a_rust);
    export_grid_diff(scenario, "home", &a_ref, &a_rust);
    // State B — `<leader>l` opens the Sessions dialog. Rig health: the
    // REF dialog must show its session rows (a ref that never opened it
    // is a rig break, never a pass); at 10x6 the titles truncate away
    // entirely (va3 grid: the ref dialog rows degrade to `S` glyphs),
    // so the needle is width-dependent. The rust-side dialog content
    // rides the export.
    let ref_needle: &[u8] = if cols <= 16 { b"S" } else { b"Alpha" };
    let ref_b_from = rig.ref_tui.as_ref().map(TuiProcess::raw_len).unwrap_or(0);
    rig.write_both(b"\x18l");
    let ref_b_alpha = rig.ref_tui.as_ref().expect("ref tui").wait_for(
        ref_needle,
        ref_b_from,
        Duration::from_secs(10),
    );
    settle(2);
    assert!(rig.alive(), "GEOM rig RED: a TUI died before state B");
    let b_ref = rig.normalize(&capture_settled(rig.ref_tui.as_ref().expect("ref tui")));
    let b_rust = rig.normalize(&capture_settled(rig.rust_tui.as_ref().expect("rust tui")));
    dump_grid(&format!("{scenario}-dialog-ref"), &b_ref);
    dump_grid(&format!("{scenario}-dialog-rust"), &b_rust);
    export_grid_diff(scenario, "dialog", &b_ref, &b_rust);
    // State C — Enter (dialog action on the selected row).
    rig.write_both(b"\r");
    settle(3);
    assert!(rig.alive(), "GEOM rig RED: a TUI died before state C");
    let c_ref = rig.normalize(&capture_settled(rig.ref_tui.as_ref().expect("ref tui")));
    let c_rust = rig.normalize(&capture_settled(rig.rust_tui.as_ref().expect("rust tui")));
    dump_grid(&format!("{scenario}-enter-ref"), &c_ref);
    dump_grid(&format!("{scenario}-enter-rust"), &c_rust);
    export_grid_diff(scenario, "enter", &c_ref, &c_rust);
    drop(rig);
    assert!(
        rust_a && ref_a,
        "GEOM RED (rig): probed boot settle needle (wordmark / Ask) rust={rust_a} ref={ref_a}"
    );
    assert!(
        ref_b_alpha,
        "GEOM RED (ref-side rig sanity): the ref Sessions dialog never showed its rows"
    );
}

#[test]
fn geom10_narrow_extreme_pair() {
    skip_without_ref_lane!();
    geom_probe("geom10", "geom10", 10, 6);
}

#[test]
fn geom40_narrow_pair() {
    skip_without_ref_lane!();
    geom_probe("geom40", "geom40", 40, 12);
}

#[test]
fn geom160_wide_pair() {
    skip_without_ref_lane!();
    geom_probe("geom160", "geom160", 160, 40);
}

// ===========================================================================
// va15 T5 — the selection→copy-toast pair (G-469 leftover, the
// "fix-lane anchors its own face" rule's first execution item): a
// drag-select over the transcript reply → release → copy-on-select
// (`terminal.copy = "select"`, the non-win32 default) paints the
// UNTITLED toast face (`Copied to clipboard` — ui/toast.tsx:64-81's
// no-title shape). EXPORT ONLY (W12/2.2#11): the in-test assertions
// are rig-health class exclusively — both ends' reply settles, the REF
// side shows its copy toast (scenario-content sanity), both TUIs alive
// at capture. The rust side's toast face rides the exported diff
// (.test-logs/diff-reports/p5sel-selectcopy.*) and the controller's
// adjudication registry; a known export conflict would be flagged
// needs-controller.
// ===========================================================================

/// The needle's (col, row) on one end's CURRENT grid (the drag anchor —
/// each end's own wrap keeps the stroke inside its own selectable
/// transcript region; SGR wire coords are 1-based).
fn p5_find(tui: &TuiProcess, needle: &str) -> Option<(usize, usize)> {
    for (row, line) in tui.screen_text().lines().enumerate() {
        if let Some(at) = line.find(needle) {
            let col = line[..at].chars().count();
            return Some((col, row));
        }
    }
    None
}

fn p5_drag_select(tui: &mut TuiProcess, x: usize, y: usize, span: usize) {
    let press = format!("\x1b[<0;{};{}M", x + 1, y + 1);
    let drag = format!("\x1b[<32;{};{}M", x + 1 + span, y + 1);
    let release = format!("\x1b[<0;{};{}m", x + 1 + span, y + 1);
    tui.write(press.as_bytes());
    std::thread::sleep(Duration::from_millis(150));
    tui.write(drag.as_bytes());
    std::thread::sleep(Duration::from_millis(250));
    tui.write(release.as_bytes());
}

#[test]
fn p5_selection_copy_toast_pair() {
    skip_without_ref_lane!();
    let mut rig = pair_rig("p5sel", COLS);
    boot_both(&mut rig);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    settle(2);
    // Each end drags over ITS OWN reply row (the anchor cell sits a
    // couple of cells inside the needle so the press lands on text, not
    // a rail; the span crosses cells — a real crossing drag).
    let rust_anchor = p5_find(&rig.rust_tui, "REALCHAIN-REPLAY-OK");
    let ref_anchor = p5_find(&rig.ref_tui, "REALCHAIN-REPLAY-OK");
    // Each end waits for ITS OWN copy toast, then captures immediately
    // (the toast TTL is 5 s — a shared settle beat raced the rust
    // face's retirement and the first cut's export lost it; the waits
    // are TIMING ONLY: the rust side's face is never judged in-test,
    // its presence rides the exported diff).
    // Each end waits for ITS OWN copy toast (one bounded RETRY of the
    // stroke — the ref's first stroke lands unreliably under PTY
    // timing; a repeated press at the same anchor at worst re-selects
    // word/line, still a copy), then captures immediately (the toast
    // TTL is 5 s — a quiescence-settled capture outlives the face, so
    // the grid is read DIRECTLY at the face-present moment; no
    // capture_settled, hence no quiescence marks for this state). The
    // waits are TIMING ONLY: the rust side's face is never judged
    // in-test, its presence rides the exported diff.
    let wait_toast = |tui: &TuiProcess| -> bool {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if tui.screen_text().contains("Copied to clipboard") {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let select_until_toast = |tui: &mut TuiProcess, x: usize, y: usize| -> bool {
        p5_drag_select(tui, x, y, 8);
        if wait_toast(tui) {
            return true;
        }
        p5_drag_select(tui, x, y, 8);
        wait_toast(tui)
    };
    let rust_toast = rust_anchor.is_some_and(|(x, y)| select_until_toast(&mut rig.rust_tui, x + 2, y));
    std::thread::sleep(Duration::from_millis(300));
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    // REF-side scenario-content sanity ONLY (W12: the ref toast is the
    // rig's proof the stroke selected + released over selectable text).
    let ref_toasted = ref_anchor.is_some_and(|(x, y)| select_until_toast(&mut rig.ref_tui, x + 2, y));
    std::thread::sleep(Duration::from_millis(300));
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    // Diagnostic ONLY (no assertion — W12): whether the rust side ever
    // painted the toast (distinguishes "never copied" from capture
    // timing).
    eprintln!("P5SEL diagnostic: the rust copy toast painted = {rust_toast}");
    dump_grid("p5sel-selectcopy-ref", &ref_grid);
    dump_grid("p5sel-selectcopy-rust", &rust_grid);
    export_grid_diff("p5sel", "selectcopy", &ref_grid, &rust_grid);
    drop(rig);
    assert!(rust_replied && ref_replied, "P5SEL RED (rig): reply did not settle");
    assert!(rust_anchor.is_some(), "P5SEL RED (rig): the rust reply needle left the grid");
    assert!(ref_toasted, "P5SEL RED (ref-side sanity): the ref copy toast never painted");
}
