//! CHANNEL 9 va18 — the MINI statusline's narrow-geometry paired
//! anchor (pair rig, EXPORT ONLY — 2.2#11: no adjudication in-test,
//! every frame rides `.test-logs/diff-reports/v18-*` and the
//! controller's adjudication registry).
//!
//! G-481's face has no standing paired anchor: the statusline's
//! staged group policy (`footerStatuslinePolicy`,
//! packages/tui/src/mini/footer.width.ts) only shows its ladder at
//! narrow widths. Both MINI TUIs (rust `opencode-rust mini` and the
//! ref `opencode mini`) are live-driven at two geometries:
//!  - v18a (40 cols): the wide ladder's settled form — agent · model
//!    (full identity) · menu.
//!  - v18b (18 cols): the narrow ladder — the model clips to 8 cells
//!    and the later stages (menu) stop at the first non-fitting
//!    stage (footer.width.ts:97-118).
//!
//! In-test assertions are rig-health class exclusively (boot settle
//! on both ends); every content judgment is the controller's.

// Test-harness infra lints (see realchain_pair.rs's list for the
// rationale): `{:?}` forensics in panics, wire-shape names in docs.
#![allow(
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    capture_settled, default_pace, export_grid_diff, lane_lock, normalize_grid,
    ref_lane_skip_reason, ref_service_stop, ref_stage, sandbox, service_start, service_stop,
    Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::Duration;

const MODEL_NEEDLE: &str = "claude";

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("VA18 SKIP: {reason}");
            return;
        }
    };
}

struct V18Rig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl V18Rig {
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
        self.rust_tui.write(bytes);
        self.ref_tui.write(bytes);
    }
}

/// Boot BOTH mini TUIs at one geometry. The rust side runs the
/// production binary's `mini` subcommand (`--no-replay` keeps the
/// boot screen free of replayed history); the ref side runs the CLI
/// entry's own `mini` command through the staged bun.
fn v18_rig(tag: &str, cols: u16, rows: u16) -> V18Rig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    // The rust mini: the production binary's `mini` subcommand over
    // the sandbox's own running server.
    let mut rust_cmd = rust.command(&opencode_bin());
    rust_cmd.args(["mini", "--no-replay"]);
    let rust_tui = TuiProcess::spawn_command(rust_cmd, cols, rows);
    // The ref mini: the CLI entry's `mini` command (the same host the
    // TUI uses, driven through the staged bun).
    let mut ref_cmd = refbox.command_ref(&stage, &stage.bun);
    ref_cmd.arg(&stage.entry).args(["mini", "--no-replay"]);
    let ref_tui = TuiProcess::spawn_command(ref_cmd, cols, rows);
    V18Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui }
}

fn opencode_bin() -> std::path::PathBuf {
    // The harness's own binary resolution (`TuiProcess::spawn` execs
    // the same freshness-checked path).
    realchain_harness::opencode_bin_pub()
}

impl Drop for V18Rig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

/// The rig-health boot gate: BOTH minis paint their statusline (the
/// agent + model groups) — no content judgment beyond "alive and
/// booted".
fn boot_both(rig: &mut V18Rig) -> bool {
    let rust_settled = rig
        .rust_tui
        .wait_for(WORDMARK, 0, Duration::from_secs(30))
        || wait_screen(&rig.rust_tui, MODEL_NEEDLE, Duration::from_secs(30));
    let ref_settled =
        rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60))
            || wait_screen(&rig.ref_tui, MODEL_NEEDLE, Duration::from_secs(60));
    if !rust_settled || !ref_settled {
        eprintln!(
            "VA18 boot detail rust={rust_settled} ref={ref_settled}; rust screen:\n{}\nref \
             screen:\n{}",
            rig.rust_tui.screen_text(),
            rig.ref_tui.screen_text()
        );
    }
    std::thread::sleep(Duration::from_secs(3));
    rust_settled && ref_settled
}

fn wait_screen(tui: &TuiProcess, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if tui.screen_text().contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn export_state(rig: &V18Rig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

/// v18a — the WIDE ladder (40 cols): the settled boot statusline
/// (agent · full model · menu) + the idle-with-draft frame.
#[test]
fn v18a_mini_statusline_wide_pair() {
    skip_without_ref_lane!();
    let mut rig = v18_rig("v18a", 40, 24);
    let booted = boot_both(&mut rig);
    export_state(&rig, "v18a", "boot-wide");
    // An idle draft (typed, never submitted) — the statusline under a
    // live composer row.
    rig.write_both(b"hi");
    std::thread::sleep(Duration::from_secs(1));
    export_state(&rig, "v18a", "idle-draft-wide");
    drop(rig);
    assert!(booted, "V18A rig RED: a mini never booted (see the boot detail above)");
}

/// v18b — the NARROW ladder (18 cols): the model group clips to its
/// 8-cell identity stage and the menu stage stops at the first
/// non-fitting stage (footer.width.ts:97-118 — G-481's staged fit).
#[test]
fn v18b_mini_statusline_narrow_pair() {
    skip_without_ref_lane!();
    let mut rig = v18_rig("v18b", 18, 24);
    let booted = boot_both(&mut rig);
    export_state(&rig, "v18b", "boot-narrow");
    rig.write_both(b"hi");
    std::thread::sleep(Duration::from_secs(1));
    export_state(&rig, "v18b", "idle-draft-narrow");
    drop(rig);
    assert!(booted, "V18B rig RED: a mini never booted (see the boot detail above)");
}
