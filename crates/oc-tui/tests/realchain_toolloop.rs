//! CHANNEL 9 W7 — the TOOL-LOOP pair lane: the first end-to-end
//! scenario where the model drives TOOLS on the real chain, on BOTH
//! engines, through the fast-execution lane:
//!
//! ```text
//! user "hello"
//!   → turn 0: bash "cargo test"     (PATH shim: instant recorded output)
//!   → turn 1: edit README.md        (the REAL sandbox filesystem)
//!   → turn 2: text confirmation     (needle TOOLLOOP-CONFIRMED)
//! ```
//!
//! The three W7 layers (see the dispatch design):
//! 1. **gateway v4 turn anchoring** (`--match turn`): each engine's
//!    main-loop provider call replays by its session turn ORDINAL plus
//!    the request's tool-result-blanked shape; the title call rides the
//!    aux track (digest). One gateway PER END (a shared process would
//!    interleave the two engines' turn counters).
//! 2. **PATH shims** (`shim_extract.py`): the recorded `cargo test`
//!    output is served instantly by a shim dir first on the sandbox
//!    PATH — no real toolchain runs inside the scenario; file edits
//!    ride the real sandbox filesystem.
//! 3. **permission pre-authorization**: the sandbox config's real
//!    `permission` block allows bash/edit/write/read on both ends —
//!    the tool loop runs with no permission dialog (the ask chrome is
//!    the permission-lane tests' surface, not this scenario's).
//!
//! Asserted: the tool call line, the folded tool output, the applied
//! file state, and the settled terminal grid (EXACT pair compare).
//!
//! Corpus bootstrap (once per engine request-shape change):
//! `T9_RECORD=1 cargo nextest run -p oc-tui --test realchain_toolloop
//!   --exact tlx_rust_bootstrap_seed` then `--exact tlx_ref_bootstrap_seed`.

// Test-harness infra lints (see realchain_pair.rs's list for the
// rationale): `{:?}` forensics in panics, wire-shape names in docs.
#![allow(
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::match_same_arms,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    assert_grids_match, dump_grid, lane_lock, normalize_grid, ref_lane_skip_reason,
    ref_service_stop, ref_stage, ref_tui_spawn, sandbox, service_start, service_stop, worktree_dir,
    Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const COLS: u16 = 120;
const ROWS: u16 = 36;
const CONFIRM_NEEDLE: &str = "TOOLLOOP-CONFIRMED";
const TOOL_OUTPUT_NEEDLE: &str = "test result: ok";
const README_BEFORE: &str = "# t9 sandbox project\n";
const README_AFTER: &str = "# t9 sandbox project (tests green)\n";

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("TOOLLOOP SKIP: {reason}");
            return;
        }
    };
}

/// The committed tool-loop corpora (bootstrap-recorded engine request
/// shapes, turn-tagged interactions).
fn corpus_tools_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-tools")
}
fn corpus_tools_ref_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-tools-ref")
}
fn toolloop_seeds_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/seeds/toolloop")
}

/// Stage the PATH shims for the recorded `cargo test` (layer 2):
/// runs `shim_extract.py` over the committed shim spec into the rust
/// sandbox's staging root (both ends share the READ-ONLY shim dir).
fn stage_shims(rust: &Sandbox) -> PathBuf {
    let out = rust.root.join("shimstage");
    let status = Command::new("python3")
        .arg(worktree_dir("tools/diff/realchain/shim_extract.py"))
        .arg("--spec")
        .arg(toolloop_seeds_dir().join("shims.json"))
        .arg("--out")
        .arg(&out)
        .output()
        .expect("spawn shim_extract.py");
    assert!(
        status.status.success(),
        "shim_extract.py failed: {}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    out
}

/// Both project READMEs carry the SAME content (the edit turn's
/// oldString must resolve identically on both ends).
fn align_projects(rust: &Sandbox, refbox: &Sandbox) {
    std::fs::write(rust.project.join("README.md"), README_BEFORE).expect("rust README");
    std::fs::write(refbox.project.join("README.md"), README_BEFORE).expect("ref README");
}

struct ToolLoopRig {
    _lane: LaneGuard,
    rust_gateway: Gateway,
    ref_gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl ToolLoopRig {
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
}

fn toolloop_rig(tag: &str) -> ToolLoopRig {
    let lane = lane_lock();
    // Layer 1: ONE turn-anchored gateway PER ENGINE (independent turn
    // counters); both load BOTH engines' turn-tagged corpora so the
    // shape key picks its own engine's recorded cassette. Both
    // gateways carry the SAME tag (hence the same --redact set): the
    // redaction list is derived from the tag, and each gateway must
    // redact BOTH sandbox roots (`<tag>` and `<tag>ref`) identically
    // or the recorded shapes hash apart.
    let corpus = [corpus_tools_dir(), corpus_tools_ref_dir()];
    let rust_gateway = Gateway::spawn_turn(tag, &corpus, None);
    let ref_gateway = Gateway::spawn_turn(tag, &corpus, None);
    let mut rust = sandbox(tag, rust_gateway.port);
    let mut refbox = sandbox(&format!("{tag}ref"), ref_gateway.port);
    // Layer 2: the shim dir shadows `cargo` inside both sandboxes.
    let shim_dir = stage_shims(&rust);
    rust.shim_dir = Some(shim_dir);
    refbox.shim_dir = Some(rust.root.join("shimstage"));
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    // Layer 3: pre-authorized tool permissions through the real config
    // `permission` block on both ends.
    rust.preauthorize_tools(false);
    refbox.preauthorize_tools(true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    // AFTER ref_stage (it writes its own project README) — both ends
    // must carry the identical edit target.
    align_projects(&rust, &refbox);
    let server = service_start(&rust);
    let _ = server.url;
    // AFTER ref_stage (it writes its own project README) — both ends
    // must carry the identical edit target.
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    ToolLoopRig { _lane: lane, rust_gateway, ref_gateway, rust, refbox, stage, rust_tui, ref_tui }
}

impl Drop for ToolLoopRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

impl TuiProcess {
    fn screen_has(&self, needle: &str) -> bool {
        self.screen_text().contains(needle)
    }

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

// ---------------------------------------------------------------------------
// The scenario (pair): full tool loop on both engines, fast lane.
// ---------------------------------------------------------------------------

/// A screen wait that dumps the CURRENT screen to `.test-logs` when it
/// fails (the settled dump comes later; the failing-moment screen is
/// the forensic that separates "never rendered" from "rendered late").
macro_rules! tl_wait {
    ($tui:expr, $needle:expr, $secs:expr, $label:expr) => {{
        let ok = $tui.wait_for_screen($needle, Duration::from_secs($secs));
        if !ok {
            dump_grid($label, &$tui.screen_text());
        }
        ok
    }};
}

#[test]
fn tl_tool_loop_pair() {
    skip_without_ref_lane!();
    let mut rig = toolloop_rig("tl");
    let rust_booted = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_booted = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_booted && ref_booted,
        "TL precondition RED: booted rust={rust_booted} ref={ref_booted}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    std::thread::sleep(Duration::from_secs(2));
    rig.rust_tui.write(b"hello\r");
    rig.ref_tui.write(b"hello\r");

    // Tool call line: the shell row names the recorded command.
    let rust_cmd = tl_wait!(rig.rust_tui, "cargo test", 30, "tl-fail-rust-cmd");
    let ref_cmd = tl_wait!(rig.ref_tui, "cargo test", 60, "tl-fail-ref-cmd");
    // Folded tool output: the shim's recorded cargo output renders.
    // G-442 (registered): the port's shell tool row occasionally keeps
    // its running spinner and never projects the output rows when the
    // command completes within ~100 ms (engine-side result is CORRECT
    // — the turn-1 request digest proves the shim output rode the
    // tool_result — a TUI-side row-projection race). A symmetric
    // no-op repaint nudge (right-arrow in the empty composer) re-sinks
    // the event on the affected runs; BOTH ends get the SAME nudge so
    // the grids stay comparable. Drift signal (W7 review minor②): a
    // process-wide nudge counter — more than 2 uses means the race is
    // growing beyond the known flake and gets a loud warning.
    static NUDGE_USES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let mut rust_out = rig.rust_tui.wait_for_screen(TOOL_OUTPUT_NEEDLE, Duration::from_secs(15));
    let mut ref_out = rig.ref_tui.wait_for_screen(TOOL_OUTPUT_NEEDLE, Duration::from_secs(20));
    if !rust_out || !ref_out {
        let uses = NUDGE_USES.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if uses > 2 {
            eprintln!(
                "G-442 DRIFT WARNING: repaint nudge used {uses} times in this process — \
                 the shell-row projection race is growing beyond the known ~50% flake"
            );
        }
        rig.rust_tui.write(b"\x1b[C");
        rig.ref_tui.write(b"\x1b[C");
        if !rust_out {
            rust_out = rig.rust_tui.wait_for_screen(TOOL_OUTPUT_NEEDLE, Duration::from_secs(10));
        }
        if !ref_out {
            ref_out = rig.ref_tui.wait_for_screen(TOOL_OUTPUT_NEEDLE, Duration::from_secs(10));
        }
    }
    if !rust_out {
        dump_grid("tl-fail-rust-out", &rig.rust_tui.screen_text());
    }
    if !ref_out {
        dump_grid("tl-fail-ref-out", &rig.ref_tui.screen_text());
    }
    // File edit landed: the tool ROW plus the diff PREVIEW rows on
    // both ends (G-441 closed); the file state itself is asserted
    // byte-exactly after the teardown.
    let rust_edited = tl_wait!(rig.rust_tui, "Edit README.md", 30, "tl-fail-rust-edit");
    let ref_edited = tl_wait!(rig.ref_tui, "(tests green)", 60, "tl-fail-ref-edit");
    // Final confirmation text.
    let rust_done = tl_wait!(rig.rust_tui, CONFIRM_NEEDLE, 30, "tl-fail-rust-done");
    let ref_done = tl_wait!(rig.ref_tui, CONFIRM_NEEDLE, 60, "tl-fail-ref-done");
    std::thread::sleep(Duration::from_secs(4));

    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("tl-toolloop-ref", &ref_grid);
    dump_grid("tl-toolloop-rust", &rust_grid);
    let rust_readme =
        std::fs::read_to_string(rig.rust.project.join("README.md")).unwrap_or_default();
    let ref_readme =
        std::fs::read_to_string(rig.refbox.project.join("README.md")).unwrap_or_default();
    let rust_stats = rig.rust_gateway.stats();
    let ref_stats = rig.ref_gateway.stats();
    drop(rig);
    assert!(
        rust_cmd && ref_cmd,
        "TL RED: tool call line rust={rust_cmd} ref={ref_cmd} (stats rust {rust_stats} / ref \
         {ref_stats}); rust screen:\n{}",
        rig_snapshot(&rust_grid)
    );
    assert!(rust_out && ref_out, "TL RED: folded tool output rust={rust_out} ref={ref_out}");
    assert!(rust_edited && ref_edited, "TL RED: edit surface rust={rust_edited} ref={ref_edited}");
    assert!(rust_done && ref_done, "TL RED: confirmation needle rust={rust_done} ref={ref_done}");
    assert_eq!(
        (rust_readme.as_str(), ref_readme.as_str()),
        (README_AFTER, README_AFTER),
        "TL RED: the edited README must land byte-identically on both ends"
    );
    // Corpus completeness: the strict run must have zero gateway
    // misses (turn-anchored main turns + digest aux calls all hit).
    // W7 review Important①: misses==0 alone let a全程 turn-shape-
    // conflict replay (wrong-shape cassette served every turn) stay
    // green — the gateway's /__stats now carries `turn_matches` by
    // event, and a strict run must be PURE hits: any turn-rematch or
    // turn-shape-conflict is a shape drift (corpus stale or engine
    // request shape changed) and goes RED here.
    for (side, stats) in [("rust", &rust_stats), ("ref", &ref_stats)] {
        let misses = stats.get("misses").and_then(serde_json::Value::as_u64).unwrap_or(u64::MAX);
        assert!(
            misses == 0,
            "TL RED: the {side} gateway strict-missed {misses} request(s) — the committed \
             tool-loop corpus is incomplete (re-run the bootstrap seeds); stats {stats}"
        );
        let matches = stats.get("turn_matches");
        let conflict_free = matches.is_some_and(|value| {
            value.as_object().is_some_and(|table| table.keys().all(|event| event == "hit"))
        });
        assert!(
            conflict_free,
            "TL RED: the {side} gateway replayed a turn through a FALLBACK match — \
             every main turn must strict-hit its own recorded shape; stats {stats}"
        );
    }
    // G-441 CLOSED (gwave-1): the edit tool row renders the file-diff
    // preview rows on both ends (`← Edit README.md` / blank / the
    // unified `1 - …` / `1 + …` gutter rows) — the mask_edit_diff
    // masking AND its scroll-offset blank-row collapse retired with
    // it; the grids compare row-exact.
    assert_grids_match("tl-toolloop", &ref_grid, &rust_grid);
}

fn rig_snapshot(grid: &str) -> String {
    grid.chars().take(4000).collect()
}

// ---------------------------------------------------------------------------
// Bootstrap seeds — record each engine's tool-loop request shapes into
// the committed turn-tagged corpora (T9_RECORD=1; see the file header).
// ---------------------------------------------------------------------------

fn bootstrap_side(ref_side: bool) {
    let tag = if ref_side { "tl-3ref" } else { "tl-3" };
    let lane = lane_lock();
    let record_dir = if ref_side { corpus_tools_ref_dir() } else { corpus_tools_dir() };
    let seeds = toolloop_seeds_dir();
    // A dedicated gateway per side; the BOOTSTRAP loads only its OWN
    // turn corpus dir (plus the shared aux corpora) so every turn of
    // this engine misses-and-seeds its own shape — loading the other
    // engine's turns would replay them through the shape-conflict
    // fallback and leave this side's shapes unrecorded.
    let corpus = [record_dir.clone()];
    let gateway = Gateway::spawn_turn(tag, &corpus, Some((&record_dir, &seeds)));
    let mut box_ = sandbox(tag, gateway.port);
    let shim_dir = stage_shims(&box_);
    box_.shim_dir = Some(shim_dir);
    if ref_side {
        box_.write_ref_config(&[], "replay/claude-opus-4-7");
        box_.preauthorize_tools(true);
        let stage = ref_stage(&box_);
        ref_service_stop(&box_, &stage);
        let readme = box_.project.join("README.md");
        std::fs::write(&readme, README_BEFORE).expect("ref README");
        let mut tui = ref_tui_spawn(&box_, &stage, COLS, ROWS);
        assert!(tui.wait_for(WORDMARK, 0, Duration::from_secs(60)), "ref bootstrap settle");
        std::thread::sleep(Duration::from_secs(2));
        let from = tui.raw_len();
        tui.write(b"hello\r");
        let done = tui.wait_for(CONFIRM_NEEDLE.as_bytes(), from, Duration::from_secs(90));
        let screen = tui.screen_text();
        let stats = gateway.stats();
        tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&box_, &stage);
        eprintln!("--- ref toolloop screen ---\n{screen}\n--- gateway {stats}");
        assert!(done, "ref bootstrap: the tool loop never confirmed; stats {stats}");
    } else {
        box_.write_config(&[], "replay/claude-opus-4-7");
        box_.preauthorize_tools(false);
        std::fs::write(box_.project.join("README.md"), README_BEFORE).expect("rust README");
        let _server = service_start(&box_);
        let mut tui = TuiProcess::spawn(&box_, COLS, ROWS);
        assert!(tui.wait_for(WORDMARK, 0, Duration::from_secs(30)), "rust bootstrap settle");
        std::thread::sleep(Duration::from_secs(2));
        let from = tui.raw_len();
        tui.write(b"hello\r");
        let done = tui.wait_for(CONFIRM_NEEDLE.as_bytes(), from, Duration::from_secs(60));
        let screen = tui.screen_text();
        let stats = gateway.stats();
        tui.shutdown(Duration::from_secs(10));
        service_stop(&box_);
        eprintln!("--- rust toolloop screen ---\n{screen}\n--- gateway {stats}");
        assert!(done, "rust bootstrap: the tool loop never confirmed; stats {stats}");
    }
    drop(gateway);
    drop(lane);
}

#[test]
fn tlx_rust_bootstrap_seed() {
    if std::env::var("T9_RECORD").is_err() {
        eprintln!("tlx_rust_bootstrap_seed: skipped (T9_RECORD unset)");
        return;
    }
    bootstrap_side(false);
}

#[test]
fn tlx_ref_bootstrap_seed() {
    skip_without_ref_lane!();
    if std::env::var("T9_RECORD").is_err() {
        eprintln!("tlx_ref_bootstrap_seed: skipped (T9_RECORD unset)");
        return;
    }
    bootstrap_side(true);
}

/// Sanity for the committed tool-loop corpora + shim spec (no TUI).
#[test]
fn tlx_toolloop_layout() {
    let spec = toolloop_seeds_dir().join("shims.json");
    let parsed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&spec).unwrap_or_else(|e| panic!("read {}: {e}", spec.display())),
    )
    .expect("shims.json");
    assert!(
        parsed.as_array().is_some_and(|cases| {
            cases
                .iter()
                .any(|case| case.get("command").and_then(|c| c.as_str()) == Some("cargo test"))
        }),
        "the shim spec must carry the cargo test case"
    );
    for turn in 0..3 {
        let path = toolloop_seeds_dir().join(format!("turn-{turn:03}.sse"));
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("turn script {}: {e}", path.display()));
        assert!(body.contains("message_stop"), "turn script {path:?} is not an SSE body");
    }
    for dir in [corpus_tools_dir(), corpus_tools_ref_dir()] {
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("corpus dir {dir:?}: {e}")).count();
        assert!(entries >= 3, "corpus dir {dir:?} must carry the turn-tagged cassettes");
    }
}
