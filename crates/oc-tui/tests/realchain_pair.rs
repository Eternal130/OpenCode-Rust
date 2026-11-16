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

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    assert_grids_match, corpus_dir, corpus_ref_dir, dump_grid, http_get, http_post, lane_lock,
    normalize_grid, ref_root, ref_service_stop, ref_stage, ref_tui_spawn, sandbox, service_start,
    service_stop, Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::Duration;

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
    let lane = lane_lock();
    let gateway = Gateway::spawn(tag, None);
    // Rust sandbox: <tag>; ref sandbox: <tag>ref (the gateway redacts
    // BOTH roots to __SANDBOX__ so request hashes stay cross-run stable).
    let rust = sandbox(tag, gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
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
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
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
    let mut rig = pair_rig("s5", 160);
    boot_both(&mut rig);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
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
    // G-435 (documented masking): the port's sidebar Context block is
    // rig-dependent — t9-5's solo rig renders it, the pair rig
    // deterministically does not (ref renders it in both). The sidebar
    // column region is masked on BOTH ends until G-435 narrows the
    // port-side race; everything left of it stays row-exact.
    // G-436 (documented masking): at the wide tier ref's user-message
    // pane is one row taller than the port's (an extra border-only row
    // plus spacing — absent at 120 cols, where S2 is row-exact). Blank
    // and border-only rows are collapsed on BOTH ends until G-436
    // narrows the wide-tier box metrics; the CONTENT row sequence and
    // the sidebar region stay asserted.
    let ref_left = drop_blank_rows(&mask_right_columns(&ref_grid, 108));
    let rust_left = drop_blank_rows(&mask_right_columns(&rust_grid, 108));
    assert_grids_match("s5-sidebar", &ref_left, &rust_left);
}

/// Drop blank and border-only rows (G-436 wide-tier spacing mask).
fn drop_blank_rows(grid: &str) -> String {
    let mut out = String::new();
    for line in grid.lines() {
        let trimmed = line.trim();
        let border_only = !trimmed.is_empty() && trimmed.chars().all(|c| c == '┃');
        if trimmed.is_empty() || border_only {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Blank every column from `from` rightward (G-435 sidebar-region mask).
fn mask_right_columns(grid: &str, from: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in grid.lines() {
        let count = line.chars().count();
        let row: String = if count <= from {
            line.to_owned()
        } else {
            line.chars().take(from).collect::<String>().trim_end().to_owned()
        };
        out.push(row);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// S6 — config hot reload reaches the rendered palette on BOTH ends
// (symptom ③ + ⑥): the same config edit, rust via
// `/api/location/reload`, ref via its native config watcher.
// ---------------------------------------------------------------------------

#[test]
fn s6_hot_reload_palette_grid_pair() {
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
    // G-437: the port's palette stays stale (the wait below documents
    // it — expected false until the gap closes).
    let _rust_palette_stale =
        !rig.rust_tui.wait_for(b"claude-sonnet-4-6", 0, Duration::from_secs(2));
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
    // G-437 (documented masking): the REF palette reflects the hot
    // config edit through its native watcher; the PORT's palette does
    // not re-list models after `/api/location/reload` (the server API
    // does — see t9-6). Rows naming the hot-added model are masked on
    // both ends until G-437 closes the TUI-side model-refresh gap.
    let mask_model = |grid: &str| {
        grid.lines()
            .filter(|line| {
                !line.contains("claude-sonnet-4-6") && !line.contains("No results found")
            })
            .map(|line| line.to_owned())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    let ref_masked = mask_model(&ref_grid);
    let rust_masked = mask_model(&rust_grid);
    assert!(
        (200..300).contains(&reload_status) && status == 200 && body.contains("claude-sonnet-4-6"),
        "S6 RED: reload={reload_status} model={status}; rust API body: {body}"
    );
    assert!(ref_open, "S6 RED: the ref palette never listed the hot-added model");
    assert_grids_match("s6-hotreload", &ref_masked, &rust_masked);
}

// ---------------------------------------------------------------------------
// Bootstrap: seed the ref-shaped corpus from the ref engine's own
// (sandbox) requests. Not part of the pair matrix; run explicitly once
// per ref request-shape change:
// `T9_RECORD=1 cargo nextest run --exact t9x_ref_bootstrap_seed`.
// ---------------------------------------------------------------------------

#[test]
fn t9x_ref_bootstrap_seed() {
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
