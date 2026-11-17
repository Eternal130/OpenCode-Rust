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
    assert_grids_match, corpus_dir, corpus_ref_dir, dump_grid, http_get, http_post, lane_lock,
    normalize_grid, plugin_lane_skip_reason, ref_lane_skip_reason, ref_root, ref_service_stop,
    ref_stage, ref_tui_spawn, sandbox, service_start, service_stop, stage_plugins, worktree_dir,
    Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
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

/// A footer context-usage percent group (`NN (M%)`, the
/// `formatContextUsage` shape) anywhere in the screen.
fn footer_percent_group(screen: &str) -> bool {
    screen.lines().any(|line| {
        let chars: Vec<char> = line.chars().collect();
        let is_digit = |i: usize| chars.get(i).is_some_and(|c| c.is_ascii_digit());
        let mut index = 0;
        while index < chars.len() {
            if is_digit(index) {
                let mut end = index;
                while is_digit(end) {
                    end += 1;
                }
                // number [index, end), then " (", digits, '%', ')'.
                let mut cursor = end;
                while chars.get(cursor) == Some(&' ') {
                    cursor += 1;
                }
                if chars.get(cursor) == Some(&'(') && is_digit(cursor + 1) {
                    let mut scan = cursor + 1;
                    while is_digit(scan) {
                        scan += 1;
                    }
                    if chars.get(scan) == Some(&'%') && chars.get(scan + 1) == Some(&')') {
                        return true;
                    }
                }
                index = end;
            } else {
                index += 1;
            }
        }
        false
    })
}

/// Settle both ends for `secs` (render/event quiescence).
fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

fn pair_rig(tag: &str, cols: u16) -> PairRig {
    pair_rig_ext(tag, cols, "instant", false, false)
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
    // G-433 tripwire (W4 Important ①): the `(N%)` footer-usage mask in
    // normalize_grid exists because ref resolves a model context limit
    // for custom-provider models and the port does not. If ref stops
    // carrying the group (or the port gains it), this assert goes red —
    // retire replace_percent_group and compare the footer exactly.
    let ref_has_percent = footer_percent_group(&ref_raw);
    let rust_has_percent = footer_percent_group(&rust_raw);
    assert!(
        ref_has_percent && !rust_has_percent,
        "G-433 tripwire: footer usage group ref={ref_has_percent} rust={rust_has_percent} — \
         if the sides now agree, retire the percent mask in normalize_grid"
    );
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
    // Mask tripwires (W4 Important ①): each mask below must be masking
    // a divergence that is STILL PRESENT — when its gap closes, the
    // pinned shape disappears, this assert goes red, and the mask (plus
    // the assert) must be retired. No mask can outlive its gap silently.
    let rust_no_context = !rust_grid.contains("Context");
    assert!(
        rust_no_context && ref_context,
        "G-435 tripwire: the sidebar-region mask assumed the port lacks the \
         Context block (rust={rust_no_context}) while ref has it — if this \
         is red, G-435 has closed: retire the mask_right_columns mask and \
         compare the full sidebar"
    );
    let ref_border_rows = border_only_row_count(&mask_right_columns(&ref_grid, SIDEBAR_MASK_COL));
    let rust_border_rows = border_only_row_count(&mask_right_columns(&rust_grid, SIDEBAR_MASK_COL));
    assert!(
        ref_border_rows > rust_border_rows,
        "G-436 tripwire: the blank/border-row collapse assumed ref's \
         user-message pane is one row taller (ref={ref_border_rows} vs \
         rust={rust_border_rows} border-only rows) — if this is red, G-436 \
         has closed: retire drop_blank_rows and compare rows exactly"
    );
    let ref_left = drop_blank_rows(&mask_right_columns(&ref_grid, SIDEBAR_MASK_COL));
    let rust_left = drop_blank_rows(&mask_right_columns(&rust_grid, SIDEBAR_MASK_COL));
    assert_grids_match("s5-sidebar", &ref_left, &rust_left);
}

/// The S5 sidebar-region mask boundary (W4 Minor ②): at 160 cols the
/// auto-docked right sidebar's left edge is ≈ col 109 (ref
/// session-frame.tsx:106-110's `width - verticalTabsWidth > 120` dock
/// rule; measured: the sidebar title renders from col 119 with its
/// box padding from ≈109). Columns ≥ this value are masked on BOTH
/// ends under G-435.
const SIDEBAR_MASK_COL: usize = 108;

/// Count border-only rows (the G-436 masked shape).
fn border_only_row_count(grid: &str) -> usize {
    grid.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && trimmed.chars().all(|c| c == '┃')
        })
        .count()
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
    // G-437 tripwire (W4 Important ①): the mask below assumes the
    // port's palette stays stale after the hot reload. If this assert
    // goes red, G-437 has closed — retire mask_model and compare the
    // palette rows exactly.
    let rust_palette_stale =
        !rig.rust_tui.wait_for(b"claude-sonnet-4-6", 0, Duration::from_secs(2));
    assert!(
        rust_palette_stale,
        "G-437 tripwire: the port palette REFLECTED the hot-added model — \
         retire mask_model and assert the row exactly"
    );
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
    // G-437 masked-shape pin: the rust grid must still carry the stale
    // palette's "No results found" row (the shape the mask exists for).
    assert!(
        rust_grid.contains("No results found"),
        "G-437 tripwire: the stale-palette row shape is gone — the mask \
         has nothing left to mask; retire it"
    );
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
    // plugins, so `⊙ N MCP` must agree row-exactly.
    // Forensics first: what did each end load? The bodies are dumped
    // above; the pair compare runs on the sidebar-visible 160-col grid
    // with the documented S5 masks (G-435/436) still in force.
    let ref_left = drop_blank_rows(&mask_right_columns(&ref_grid, SIDEBAR_MASK_COL));
    let rust_left = drop_blank_rows(&mask_right_columns(&rust_grid, SIDEBAR_MASK_COL));
    assert_grids_match("t9-7-plugin", &ref_left, &rust_left);
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

    // Strict compare per state with the documented S5 masks.
    for (name, ref_grid, rust_grid) in
        [("p4-empty", &a_ref, &a_rust), ("p4-reply", &b_ref, &b_rust), ("p4-long", &c_ref, &c_rust)]
    {
        let ref_left = drop_blank_rows(&mask_right_columns(ref_grid, SIDEBAR_MASK_COL));
        let rust_left = drop_blank_rows(&mask_right_columns(rust_grid, SIDEBAR_MASK_COL));
        assert_grids_match(name, &ref_left, &rust_left);
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
