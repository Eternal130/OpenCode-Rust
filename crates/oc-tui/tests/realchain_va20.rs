//! va20 (T9) — the fix surfaces' paired anchors (export-only): the
//! scenarios the lane's five fixes own, run on the same pair topology
//! as realchain_pair (the replay gateway + sandboxed BOTH ends). The
//! lane prompt's fallback clause applies to the frame-level animation
//! faces: marquee/grapheme windows, the tab-pulse envelopes, the
//! spinner's static fallback and the reconnecting five-row box are
//! export-anchored at the OBSERVABLE settled states here (or pinned
//! at unit/vt level with the fixes themselves — see marquee.rs,
//! tab_pulse.rs, overlays.rs, composer_solid.rs tests); the exports
//! land in `.test-logs/diff-reports/` for controller adjudication.
//!
//! * `v20-reconnecting5` — G-477: both ends' servers stop, the
//!   settled "Connection lost…" box exports (the five-row face).
//! * `v20-prompt-preview` — G-483: `prompt.image_preview` on + a
//!   pasted local image path; the composer's inline strip exports
//!   (the strip-on face; every other scenario's composer export
//!   already pins the strip-off face — the key defaults off).

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    capture_settled, default_pace, dump_grid, export_grid_diff, lane_lock, normalize_grid,
    ref_lane_skip_reason, ref_service_stop, ref_stage, ref_tui_spawn, sandbox, service_start,
    service_stop, Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::Duration;

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

struct V20Rig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl V20Rig {
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

impl Drop for V20Rig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn v20_rig(tag: &str) -> V20Rig {
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
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V20Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui }
}

fn boot_both(rig: &V20Rig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V20 rig RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    std::thread::sleep(Duration::from_secs(3));
}

/// Patch a JSON key into one end's TUI config (read + merge + write).
fn patch_tui_config(path: &std::path::Path, merge: serde_json::Value) {
    let mut config: serde_json::Value =
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| serde_json::json!({}));
    if let (Some(base), Some(extra)) = (config.as_object_mut(), merge.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(&config).unwrap())
        .unwrap_or_else(|e| panic!("patch config {path:?}: {e}"));
}

// ---------------------------------------------------------------------------
// v20-reconnecting5 — G-477's five-row box, the settled loss face.
// ---------------------------------------------------------------------------

/// Both ends' servers stop (each TUI loses ITS OWN managed server —
/// the ref side its self-spawned `serve --service`, the rust side the
/// started service); the managed arm's copy is "Restarting service…"
/// (the unmanaged arm's "Connection lost…" — the G-439 pair), so the
/// settle poll accepts either. The REF's managed client RESPAWNS its
/// server (the overlay window is transient — the bun boot takes
/// seconds), so each side's grid is captured THE MOMENT its overlay
/// shows. The five-row box (pad + headline + gap + copy + pad,
/// reconnecting.tsx:18-37) is the fixed face; the grids land in the
/// diff-reports for adjudication.
#[test]
fn v20_reconnecting_five_row_pair() {
    skip_without_ref_lane!();
    let rig = v20_rig("v20rec5");
    boot_both(&rig);
    // The loss: stop both servers (the TUIs stay alive on their PTYs).
    ref_service_stop(&rig.refbox, &rig.stage);
    service_stop(&rig.rust);
    // Poll each end for its overlay face; capture the grid THE MOMENT
    // it shows (the ref's managed respawn clears it seconds later).
    let overlay_text = |screen: &str| {
        screen.contains("Connection lost") || screen.contains("Restarting service")
    };
    let mut rust_grid: Option<String> = None;
    let mut ref_grid: Option<String> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if rust_grid.is_none() && overlay_text(&rig.rust_tui.screen_text()) {
            std::thread::sleep(Duration::from_millis(400));
            rust_grid = Some(rig.rust_tui.screen_text());
        }
        if ref_grid.is_none() && overlay_text(&rig.ref_tui.screen_text()) {
            std::thread::sleep(Duration::from_millis(400));
            ref_grid = Some(rig.ref_tui.screen_text());
        }
        if rust_grid.is_some() && ref_grid.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let (rust_seen, ref_seen) = (rust_grid.is_some(), ref_grid.is_some());
    let (Some(ref_grid), Some(rust_grid)) = (ref_grid, rust_grid) else {
        panic!(
            "V20 rig RED: the loss overlays never settled (rust={rust_seen}, ref={ref_seen}); \
             rust screen:\n{}\nref screen:\n{}",
            rig.rust_tui.screen_text(),
            rig.ref_tui.screen_text()
        );
    };
    // The five-row box is the face under test: the settled overlay
    // rows around the headline+copy (the gap row between them).
    let ref_grid = rig.normalize(&ref_grid);
    let rust_grid = rig.normalize(&rust_grid);
    dump_grid("v20-reconnecting5-ref", &ref_grid);
    dump_grid("v20-reconnecting5-rust", &rust_grid);
    drop(rig);
    export_grid_diff("v20-reconnecting5", "loss-settled", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// v20-prompt-preview — G-483's inline strip, the strip-on face.
// ---------------------------------------------------------------------------

/// `prompt.image_preview` on (both ends' config documents), then a
/// bracketed paste of a LOCAL image path (each sandbox's own project
/// file — the paste-resolution path both engines share,
/// attachment.rs / prompt/attachment.ts) stages the `[Image 1]` chip
/// and mounts the strip above the composer. The settled grids export
/// (the ref's thumbs ride its own image pipeline — whatever the PTY
/// carries; the port renders the blocks fallback — the diff is the
/// adjudication's).
#[test]
fn v20_prompt_preview_strip_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced("v20prev", None, &default_pace());
    let rust = sandbox("v20prev", gateway.port);
    let refbox = sandbox("v20prevref", gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    // G-483 — the strip's gate, ON for both ends (the ref reads the
    // config document's prompt.image_preview; the port's TuiConfig
    // the same key).
    patch_tui_config(
        &rust.config_path(),
        serde_json::json!({ "prompt": { "image_preview": true } }),
    );
    patch_tui_config(
        &refbox.root.join("xdg-config/opencode/opencode.json"),
        serde_json::json!({ "prompt": { "image_preview": true } }),
    );
    // The pasted local image: the tiny 4x2 PNG fixture in each
    // sandbox's own project.
    let png = include_bytes!("fixtures/r07-tiny-4x2.png");
    for project in [&rust.project, &refbox.project] {
        std::fs::write(project.join("shot.png"), png).expect("stage shot.png");
    }
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    let mut rig =
        V20Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui };
    boot_both(&rig);
    // The bracketed paste of the local image path (each end its own
    // sandbox path — the normalizer redacts both roots).
    let rust_path = rig.rust.project.join("shot.png").to_string_lossy().into_owned();
    let ref_path = rig.refbox.project.join("shot.png").to_string_lossy().into_owned();
    rig.rust_tui.write(format!("\x1b[200~{rust_path}\x1b[201~").as_bytes());
    rig.ref_tui.write(format!("\x1b[200~{ref_path}\x1b[201~").as_bytes());
    // The chip + the strip settle.
    let mut saw_rust = false;
    let mut saw_ref = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if !saw_rust && rig.rust_tui.screen_text().contains("[Image 1]") {
            saw_rust = true;
        }
        if !saw_ref && rig.ref_tui.screen_text().contains("[Image 1]") {
            saw_ref = true;
        }
        if saw_rust && saw_ref {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(
        saw_rust && saw_ref,
        "V20 rig RED: the image chips never staged (rust={saw_rust}, ref={saw_ref}); \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    std::thread::sleep(Duration::from_secs(2));
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("v20-prompt-preview-ref", &ref_grid);
    dump_grid("v20-prompt-preview-rust", &rust_grid);
    drop(rig);
    export_grid_diff("v20-prompt-preview", "strip-on", &ref_grid, &rust_grid);
}
