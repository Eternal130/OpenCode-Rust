//! Lane va21f — the G-535..G-539 paired anchors (the 修锚同 lane rule):
//! the R28-audited post-va21e residuals — the mini's `currentAgent()`
//! label face (G-535: the turn summary's agent, the statusline's
//! agent group, the agent-switch notice), the replay feed's active-map
//! status fixup (G-536: children admitted Running by the hydrate
//! settle against the fetched active map — no phantom `N sub`/`bg`
//! hints, no phantom ACTIVE-tab rows on resume), the dialog prompt's
//! up/down line moves + undo/redo (G-537: input.move.up/down and
//! input.undo/input.redo's ctrl+-/ctrl+./super+z arms over the
//! engine's commands + the dialog's snapshot history), the select
//! dialog's paste-into-filter routing (G-538: the focused
//! InputRenderable's ANSI+newline-stripped insert), and the ctx.data
//! held-row shapes (G-539 — the session/pending members ride the
//! plugin faces; the family(unknown) return is exercised through the
//! same plugin seam).
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21f-<anchor>-<state>.{json,txt}` and make
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication registry.
//! The fixtures are self-contained `.js` v2 `{id, setup}` TUI plugins
//! staged into BOTH sandboxes' real drop-in discovery roots (the same
//! staging contract `realchain_v21e.rs` documents).
//!
//! WORKTREE BINARY (va21e lesson): `worktree_root()` redirects
//! `.worktrees/<n>` to the MAIN checkout (fix `61404143`), so without
//! `CARGO_TARGET_DIR` the rig spawns the MAIN checkout's binary —
//! silently testing pre-lane code (the freshness guard walks the main
//! checkout's sources too, so it cannot catch this). Run the anchors
//! from a worktree with the lane's own target dir:
//! `tools/dev-gate.sh 'CARGO_TARGET_DIR=<worktree>/target cargo
//! nextest run -p oc-tui --locked …'` after
//! `cargo build -p oc-cli --bin opencode-rust --locked` in it.
//!
//! PACING: same as v21e/va19 — the two ends boot at different speeds,
//! so every rig-captured transition is toast-marked (the needle waits
//! complete at the SLOWER end) and the settle gaps absorb the boot
//! skew. The resume anchor rides the va19sub turn-scripted gateways
//! (deterministic parent→child→parent) and re-boots the mini with
//! `--continue` after the first pair settles.

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
    capture_settled, default_pace, dump_grid, export_grid_diff, lane_lock, normalize_grid,
    ref_lane_skip_reason, ref_mini_spawn, ref_mini_spawn_with_args, ref_service_stop, ref_stage,
    ref_tui_spawn, sandbox, service_start, service_stop, Gateway, LaneGuard, RefStage, Sandbox,
    TuiProcess, WORDMARK,
};
use std::time::Duration;

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("V21F SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;

/// This WORKTREE's root (no main-checkout redirect: the committed
/// va19sub seed scripts + the runtime record dirs are lane-local —
/// the same shape `realchain_va19.rs` documents).
fn wt_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

struct V21FRig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    _rust_server_url: String,
    _rust_password: Option<String>,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl V21FRig {
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

    /// Rig-health needle wait (the observed needle only settles the
    /// capture; the face's judgment rides the export).
    fn wait_both_screens(&self, needle: &str, secs: u64) -> bool {
        // 2026-11-27 slow-audit fix A: wait BOTH ends CONCURRENTLY (the
        // serial for-loop doubled the wall-clock per state — the ref leg
        // only started after the rust leg's needle landed).
        std::thread::scope(|scope| {
            let legs: Vec<_> = [&self.rust_tui, &self.ref_tui]
                .into_iter()
                .map(|tui| {
                    scope.spawn(move || {
                        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
                        let mut ok = tui.screen_text().contains(needle);
                        while !ok && std::time::Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(100));
                            ok = tui.screen_text().contains(needle);
                        }
                        ok
                    })
                })
                .collect();
            legs.into_iter().all(|leg| leg.join().unwrap_or(false))
        })
    }

    /// The squashed-screen needle (G-540 note): the port's mini starts
    /// every streamed progress chunk on a NEW row where the ref
    /// continues the entry's line — the turn-completion needles match
    /// only with the inter-row whitespace stripped (rig health; the
    /// row-shape difference itself rides the exports for adjudication).
    fn screen_squashed(tui: &TuiProcess) -> String {
        tui.screen_text().chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    fn wait_squashed(tui: &TuiProcess, needle: &str, secs: u64) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        let mut ok = Self::screen_squashed(tui).contains(needle);
        while !ok && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            ok = Self::screen_squashed(tui).contains(needle);
        }
        ok
    }
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// Stage one v21f fixture plugin into a sandbox's config drop-in (the
/// same staging contract as `realchain_v21e.rs` — each side stages into
/// its OWN discovery root; no `@opentui/solid` import needed here, so
/// no host shim).
fn stage_fixture(sandbox: &Sandbox, app_dir: &str, name: &str, tui_js: &str) {
    let plugins = sandbox.root.join("xdg-config").join(app_dir).join("plugins");
    let dir = plugins.join(name);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\n  \"name\": \"{name}\",\
             \n  \"version\": \"1.0.0\",\
             \n  \"main\": \"server.js\",\
             \n  \"type\": \"module\",\
             \n  \"exports\": {{ \"./tui\": \"./tui.js\" }}\n}}\n"
        ),
    )
    .expect("package.json");
    std::fs::write(
        dir.join("server.js"),
        format!(
            "export default {{\n  id: \"{name}\",\n  setup() {{\n    return () => {{}};\n  }},\n}}\n"
        ),
    )
    .expect("server.js");
    std::fs::write(dir.join("tui.js"), tui_js).expect("tui.js");
    let config = sandbox.root.join("xdg-config").join(app_dir).join("opencode.json");
    if !config.is_file() {
        let body = serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "model": "replay/claude-opus-4-7",
            "autoupdate": false,
            "providers": {
                "replay": {
                    "name": "Replay Anthropic",
                    "package": "@ai-sdk/anthropic",
                    "settings": {
                        "baseURL": format!("http://127.0.0.1:{}/v1", sandbox.gateway_port),
                        "apiKey": "replay-sandbox-key"
                    },
                    "models": { "claude-opus-4-7": { "name": "claude-opus-4-7" } }
                }
            }
        });
        std::fs::write(&config, serde_json::to_string_pretty(&body).unwrap())
            .expect("the TUI-side config for plugin discovery");
    }
}

// The dialog-face fixture (G-537/G-538): a prompt with a seeded
// MULTI-LINE value + a select with fuzzy-friendly rows.
const ED_FIXTURE: &str = r#"export default {
  id: "v21f.ed.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21f ed " + tag, message: "", duration: 600000 });
    // G-539(c) — the family probe (the ref's data.ts:1360
    // `store.session.family[resolveRoot(id)] ?? []`: an UNKNOWN id's
    // family is []; the pre-fix port seeded [id]). No sessions exist
    // yet, so the probe's length is the face: 0 (ref + fixed port) vs
    // 1 (pre-fix port). A PERSISTENT toast — the face rides every
    // export of this scenario as a row both ends must agree on.
    setTimeout(() => {
      const family =
        ctx.data && ctx.data.session && typeof ctx.data.session.family === "function"
          ? ctx.data.session.family("ses_unknown_probe")
          : null;
      ctx.ui.toast.show({
        title: "v21f fam " + (family === null ? "absent" : String(family.length)),
        message: "",
        duration: 600000,
      });
    }, 3000);
    setTimeout(() => {
      ctx.ui.dialog
        .prompt({
          title: "v21f lines",
          description: "the description row above the multi-line value",
          placeholder: "type here",
          value: "alpha line\nbeta line\ngamma tail",
        })
        .then((value) => {
          report("prompt " + String(value));
        });
    }, 6000);
    setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21f paste",
          options: [
            { title: "Theme Light", value: "theme-light", category: "Palette" },
            { title: "Terminal", value: "terminal", category: "Palette" },
            { title: "Thermometer", value: "thermometer", category: "Palette" },
            { title: "Almanac", value: "almanac", category: "Books" },
          ],
        })
        .then((value) => {
          report("sel " + String(value));
        });
    }, 96000);
    return () => {};
  },
};
"#;

/// The rig with ONE fixture staged (the single-plugin confounder).
fn v21f_rig(tag: &str, fixture: &str, tui_js: &str) -> V21FRig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    stage_fixture(&rust, "opencode-rust", fixture, tui_js);
    stage_fixture(&refbox, "opencode", fixture, tui_js);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V21FRig {
        _lane: lane,
        _gateway: gateway,
        rust,
        refbox,
        stage,
        _rust_server_url: server.url,
        _rust_password: server.password,
        rust_tui,
        ref_tui,
    }
}

impl Drop for V21FRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V21FRig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21F precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

fn capture_pair(rig: &V21FRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21f-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21f-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21f-{scenario}"), state, &ref_grid, &rust_grid);
}

/// Needle + capture: the toast marker completes at the SLOWER end;
/// the capture rides ~1 s behind (inside the state window on both
/// ends).
fn marker_state(rig: &V21FRig, scenario: &str, state: &str, needle: &str, window: u64) {
    let ok = rig.wait_both_screens(needle, window);
    assert!(
        ok,
        "V21F {scenario} RED: the {state} marker ({needle}) never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(rig, scenario, state);
}

// ===========================================================================
// Anchor 1 — the multi-line prompt's up/down navigation + undo/redo
// (G-537): the t+6 s prompt with a seeded THREE-LINE value opens at
// the end caret (the height-1 viewport shows line 3). The key arms
// then drive:
//   * up (ESC[A → input.move.up): the caret climbs to line 2 — the
//     viewport follows (the pre-fix port no-opped: the stroke fell
//     through to dialog.select.prev on an empty list);
//   * up again → line 1; down (ESC[B → input.move.down) → line 2;
//   * typing + ctrl+u (delete-to-line-start) on line 2, then
//     ctrl+- (the kitty CSI-u encoding `ESC[45;5u`, parsed
//     unconditionally on both ends) → input.undo: the line restores
//     from the dialog's snapshot history;
//   * ctrl+. (`ESC[46;5u`) → input.redo: the delete re-applies;
//   * super+z (`ESC[122;9u`) → the super undo arm (the pre-fix port
//     flattened a super chord onto its plain key — it typed 'z');
//   * the BRACKETED PASTE with an embedded newline at the caret (the
//     multi-line value grows; the height-1 viewport renders the
//     CARET's line);
//   * up after the paste — the caret-line render navigates again;
//   * esc → settle-undefined (the settled toast).
// The key encodings parse identically on both ends (opentui core's
// chunk-bun and the port's input_term.rs — the kitty CSI-u grammar is
// parsed unconditionally on both).
// ===========================================================================
#[test]
fn v21f_prompt_lines_undo_pair() {
    skip_without_ref_lane!();
    let mut rig = v21f_rig("v21f-ed", "v21f-ed", ED_FIXTURE);
    boot_both(&mut rig);
    // The probe toast persists through the scenario (the family face
    // rides every export); the prompt opens at t+6 s regardless.
    let fam = rig.wait_both_screens("v21f fam ", 20);
    assert!(
        fam,
        "V21F family RED: the probe toast never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "ed", "family-probe");
    settle(3);
    // The prompt opens with the description + the seeded tail line.
    let open = rig.wait_both_screens("v21f lines", 60);
    assert!(
        open,
        "V21F prompt RED: the prompt dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "ed", "prompt-open");
    // UP — the caret climbs to line 2 (beta); the viewport follows.
    rig.write_both(b"\x1b[A");
    settle(2);
    capture_pair(&rig, "ed", "prompt-up-1");
    // UP again — line 1 (alpha).
    rig.write_both(b"\x1b[A");
    settle(2);
    capture_pair(&rig, "ed", "prompt-up-2");
    // DOWN — back to line 2.
    rig.write_both(b"\x1b[B");
    settle(2);
    capture_pair(&rig, "ed", "prompt-down");
    // Line-home, then type on line 2 + ctrl+u — the line empties.
    rig.write_both(b"\x01");
    settle(1);
    rig.write_both(b"typed words");
    settle(1);
    capture_pair(&rig, "ed", "prompt-typed");
    rig.write_both(b"\x15");
    settle(2);
    capture_pair(&rig, "ed", "prompt-ctrl-u");
    // ctrl+- (ESC[45;5u) — undo: the line restores.
    rig.write_both(b"\x1b[45;5u");
    settle(2);
    capture_pair(&rig, "ed", "prompt-undo");
    // ctrl+. (ESC[46;5u) — redo: the delete re-applies.
    rig.write_both(b"\x1b[46;5u");
    settle(2);
    capture_pair(&rig, "ed", "prompt-redo");
    // super+z (ESC[122;9u) — the super undo arm.
    rig.write_both(b"\x1b[122;9u");
    settle(2);
    capture_pair(&rig, "ed", "prompt-super-undo");
    // The bracketed paste with an embedded newline at the caret —
    // the multi-line value grows, the viewport renders the caret's
    // line (the pasted text's own line).
    rig.write_both(b"\x1b[200~pasted multi\x0aface text\x1b[201~");
    settle(2);
    capture_pair(&rig, "ed", "prompt-paste-multiline");
    // UP after the paste — the caret-line render navigates.
    rig.write_both(b"\x1b[A");
    settle(2);
    capture_pair(&rig, "ed", "prompt-paste-up");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    marker_state(&rig, "ed", "prompt-esc", "v21f ed prompt", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 2 — the select dialog's paste-into-filter routing (G-538):
// the t+96 s select opens (four rows, two categories); the bracketed
// paste routes to the FOCUSED filter (the InputRenderable's
// handlePaste — ANSI + newlines stripped, index.bun.js:9115-9119):
//   * paste "the" (plain) → the filter narrows to the fuzzy matches;
//   * paste an ANSI-wrapped "me\n" → the filter grows to "theme"
//     (the ANSI run and the newline are stripped before the insert);
//   * esc → settle-undefined.
// The pre-fix port fed the composer BEHIND the modal (the filter
// never saw the text).
// ===========================================================================
#[test]
fn v21f_select_paste_pair() {
    skip_without_ref_lane!();
    let mut rig = v21f_rig("v21f-sel", "v21f-ed", ED_FIXTURE);
    boot_both(&mut rig);
    // The select opens (the prompt leg at t+6 s closes untouched —
    // the rig rides the t+96 s select).
    let prompt_open = rig.wait_both_screens("v21f lines", 60);
    assert!(
        prompt_open,
        "V21F select RED: the prompt leg never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "prompt-esc", "v21f ed prompt", 30);
    let open = rig.wait_both_screens("v21f paste", 110);
    assert!(
        open,
        "V21F select RED: the select dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "sel", "select-open");
    // The plain paste: "the" lands in the filter — the list narrows.
    rig.write_both(b"\x1b[200~the\x1b[201~");
    settle(2);
    capture_pair(&rig, "sel", "select-paste");
    // The ANSI-wrapped + newline-carrying paste: "me" appends (the
    // "\x1b[31m…\x1b[0m" run and the \x0a strip) → the filter reads
    // "theme" — the narrowest window.
    rig.write_both(b"\x1b[200~\x1b[31mme\x1b[0m\x0a\x1b[201~");
    settle(2);
    capture_pair(&rig, "sel", "select-paste-ansi-newline");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "select-esc", "v21f ed sel", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 3 — the non-default agent's turn summary + statusline label
// (G-535): the mini boots with `--agent plan`; one scripted turn
// settles; the turn summary row carries `currentAgent()`'s label
// (footer.ts:455 → turn-summary.ts:11 — the resolved agent NAME, the
// pre-fix port printed the raw id) and the idle statusline's agent
// group carries the same label (footer.view.tsx:242).
// ===========================================================================

struct MiniRig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    _rust_server_url: String,
    _rust_password: Option<String>,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl MiniRig {
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

/// The mini rig (va19's sub_rig spawn shape, argument-carrying): the
/// plain-paced RECORD-MODE gateway (the `--agent` flag changes the
/// turn's request digest — the corpus has no cassette for it, so the
/// record mode seeds the miss with the canned REALCHAIN-REPLAY-OK
/// reply like every bootstrap lane), the rust server, one mini pair.
/// `PWD` is pinned to the sandbox project (the mini's localDirectory
/// chdir prefers `$PWD`) and the cursor probe gets answered.
fn mini_rig(tag: &str, args: &[&str]) -> MiniRig {
    let lane = lane_lock();
    let record = wt_root().join(format!(".test-logs/{tag}-seeds"));
    std::fs::create_dir_all(&record).expect("create record dir");
    let gateway = Gateway::spawn_paced(tag, Some(&record), &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini").args(args);
    rust_command.env("PWD", rust.project.display().to_string());
    let rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let ref_tui = ref_mini_spawn_with_args(&refbox, &stage, COLS, ROWS, args);
    MiniRig {
        _lane: lane,
        _gateway: gateway,
        rust,
        refbox,
        stage,
        _rust_server_url: server.url,
        _rust_password: server.password,
        rust_tui,
        ref_tui,
    }
}

impl Drop for MiniRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both_mini(rig: &mut MiniRig) {
    let rust_settled = rig.rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21F mini RED: boot rust={rust_settled} ref={ref_settled}; rust screen:\n{}\nref \
         screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";

fn capture_mini_pair(rig: &MiniRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21f-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21f-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21f-{scenario}"), state, &ref_grid, &rust_grid);
}

#[test]
fn v21f_agent_summary_pair() {
    skip_without_ref_lane!();
    let mut rig = mini_rig("v21f-ag", &["--agent", "plan"]);
    boot_both_mini(&mut rig);
    // One scripted turn: the reply lands, the turn settles, the
    // summary row renders in the scrollback.
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(60));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(90));
    assert!(
        rust_replied && ref_replied,
        "V21F agent RED: hello turn rust={rust_replied} ref={ref_replied}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    // The turn summary lands after the settle loop's duration arm.
    settle(6);
    capture_mini_pair(&rig, "ag", "turn-summary");
    drop(rig);
}

// ===========================================================================
// Anchor 4 — the resume with historical subagents (G-536): the
// va19sub turn scripts run a parent→child→parent round-trip; the
// first mini pair then EXITS and a second pair boots with
// `--continue` — the replay feed re-hydrates the session's children
// (the mainTool walk + the family index) and the closing pass settles
// every child the active map no longer lists:
//   * the settled resume screen: the statusline carries NO phantom
//     `N sub`/`bg` work hints (the pre-fix port's Running admits
//     never settled);
//   * `down` (session.child.first) opens the subagent menu — the
//     ACTIVE tab lists no settled child;
//   * esc closes the menu; the settled frame exports again.
// ===========================================================================

/// The rig-diagnostic helpers (the session-list probe after the
/// resume pair boots): a URL-safe path encode and a payload preview.
fn urlencoding_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

fn server_url_port(url: &str) -> String {
    url.rsplit_once(':').map_or_else(|| url.to_owned(), |(_, port)| port.to_owned())
}

fn truncate_diag(text: &str) -> String {
    text.chars().take(600).collect()
}

/// The tool pre-authorization block (the subagent round-trip must not
/// stop on a permission dialog) — the same shape va19's sub rig uses.
fn allow_subagent(box_: &Sandbox, ref_side: bool) {
    let path = if ref_side {
        box_.root.join("xdg-config/opencode/opencode.json")
    } else {
        box_.config_path()
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read config {}: {error}", path.display()));
    let mut config: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse config: {error}"));
    config["permission"] = serde_json::json!({
        "bash": "allow",
        "edit": "allow",
        "write": "allow",
        "read": "allow",
        "subagent": "allow",
    });
    std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
        .unwrap_or_else(|error| panic!("write config {}: {error}", path.display()));
}

/// Rig diagnostic (log-only, stderr — never the PTY/exports): the
/// rust server's session list after the resume booted — a SECOND row
/// would mean the resume created a fresh session instead of attaching
/// the first (the rig-health forensics for G-541-class failures).
fn log_post_boot_session_list(server: &realchain_harness::ManagedServer, rust: &Sandbox) {
    let query = format!(
        "http://127.0.0.1:{}/api/session?directory={}&parentID=null&limit=50&order=desc",
        server_url_port(&server.url),
        urlencoding_encode(&rust.project.display().to_string())
    );
    if let Some((status, payload)) = realchain_harness::http_get(&query, server.password.as_deref())
    {
        eprintln!("V21F diag: post-boot session list http {status}: {}", truncate_diag(&payload));
    }
}

#[test]
fn v21f_resume_subagents_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    // The va19sub turn scripts (deterministic parent→child→parent),
    // one gateway PER engine — the same bootstrap va19's sub rig
    // uses; the record dirs are runtime-only cassettes for the log.
    let seeds = wt_root().join("tools/diff/realchain/seeds/va19sub");
    assert!(
        seeds.join("turn-000.sse").is_file(),
        "V21F resume RED: missing seed scripts under {}",
        seeds.display()
    );
    let record_rust = wt_root().join(".test-logs/v21f-seeds-rust");
    let record_ref = wt_root().join(".test-logs/v21f-seeds-ref");
    std::fs::create_dir_all(&record_rust).expect("create rust record dir");
    std::fs::create_dir_all(&record_ref).expect("create ref record dir");
    let pace = "synthetic:ttft=2500:tps=8";
    // Both gateways stay ALIVE through the whole two-phase test (the
    // resume pair re-uses the same servers).
    let rust_gateway =
        Gateway::spawn_turn_paced("v21f-rs", &[], Some((&record_rust, &seeds)), pace);
    let ref_gateway = Gateway::spawn_turn_paced("v21f-rsr", &[], Some((&record_ref, &seeds)), pace);
    let rust = sandbox("v21f-rs", rust_gateway.port);
    let refbox = sandbox("v21f-rsref", ref_gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    allow_subagent(&rust, false);
    allow_subagent(&refbox, true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;

    // Phase 1 — the FIRST mini pair runs the subagent round-trip
    // (turn 0's tool call spawns the child; turn 1 settles it).
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini");
    rust_command.env("PWD", rust.project.display().to_string());
    let mut rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let mut ref_tui = ref_mini_spawn(&refbox, &stage, COLS, ROWS);
    let capture = |rust_tui: &TuiProcess, ref_tui: &TuiProcess, state: &str| {
        capture_pair_state(&rust, &refbox, rust_tui, ref_tui, state);
    };
    let rust_boot = rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_boot = ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    assert!(
        rust_boot && ref_boot,
        "V21F resume RED: first boot rust={rust_boot} ref={ref_boot}; rust screen:\n{}\nref \
         screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(3);
    let rust_from = rust_tui.raw_len();
    let ref_from = ref_tui.raw_len();
    let _ = (rust_from, ref_from);
    rust_tui.write(b"hello\r");
    ref_tui.write(b"hello\r");
    // G-540 note: the streamed reply's chunks land one-per-row on the
    // port (the ref continues the line) — the completion needle rides
    // the SQUASHED screen (rig health; the row shape rides the export).
    let rust_done = V21FRig::wait_squashed(&rust_tui, "round-tripcompleted", 90);
    let ref_done = V21FRig::wait_squashed(&ref_tui, "round-tripcompleted", 120);
    assert!(
        rust_done && ref_done,
        "V21F resume RED: the round-trip never completed rust={rust_done} ref={ref_done}; \
         rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(3);
    capture(&rust_tui, &ref_tui, "first-turn-settled");
    // The first pair EXITS (the two-press ctrl+c chord, then the
    // forced teardown); the SERVERS stay up for the resume pair.
    rust_tui.write(b"\x03\x03");
    ref_tui.write(b"\x03\x03");
    settle(3);
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));

    // Phase 2 — the RESUME pair: `--continue` re-attaches the latest
    // session; the replay feed walks the transcript (the subagent
    // tool parts admit the child; the family index fills it; the
    // closing pass settles it against the fetched active map).
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini").arg("--continue");
    rust_command.env("PWD", rust.project.display().to_string());
    let mut rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let mut ref_tui = ref_mini_spawn_with_args(&refbox, &stage, COLS, ROWS, &["--continue"]);
    // The resume boots: the splash, then the replayed transcript (the
    // replay renders whole projected messages — the needle matches the
    // plain screen; the squashed form stays the robust form).
    let rust_boot = rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_boot = ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    let rust_replayed = V21FRig::wait_squashed(&rust_tui, "round-tripcompleted", 60);
    let ref_replayed = V21FRig::wait_squashed(&ref_tui, "round-tripcompleted", 90);
    log_post_boot_session_list(&server, &rust);
    assert!(
        rust_boot && ref_boot && rust_replayed && ref_replayed,
        "V21F resume RED: boot rust={rust_boot} ref={ref_boot} replayed rust={rust_replayed} \
         ref={ref_replayed}; rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(5);
    resume_menu_states(&rust, &refbox, &mut rust_tui, &mut ref_tui);
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    service_stop(&rust);
    drop(lane);
    drop(rust_gateway);
    drop(ref_gateway);
}

/// The resume pair's state export (the roots-normalized grids, dumped
/// and diff-exported under the v21f-rs scenario).
fn capture_pair_state(
    rust: &Sandbox,
    refbox: &Sandbox,
    rust_tui: &TuiProcess,
    ref_tui: &TuiProcess,
    state: &str,
) {
    let roots =
        [refbox.root.to_string_lossy().into_owned(), rust.root.to_string_lossy().into_owned()];
    let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let normalize = |grid: &str| normalize_grid(grid, &refs);
    let ref_grid = normalize(&capture_settled(ref_tui));
    let rust_grid = normalize(&capture_settled(rust_tui));
    dump_grid(&format!("v21f-rs-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21f-rs-{state}-rust"), &rust_grid);
    export_grid_diff("v21f-rs", state, &ref_grid, &rust_grid);
}

/// The menu face states after the resume settles: `down`
/// (session.child.first) opens the subagent menu — the ACTIVE tab's
/// rows (the phantom listing face); esc closes it and the settled
/// frame exports again.
fn resume_menu_states(
    rust: &Sandbox,
    refbox: &Sandbox,
    rust_tui: &mut TuiProcess,
    ref_tui: &mut TuiProcess,
) {
    rust_tui.write(b"\x1b[B");
    ref_tui.write(b"\x1b[B");
    settle(2);
    capture_pair_state(rust, refbox, rust_tui, ref_tui, "resume-menu");
    rust_tui.write(b"\x1b");
    ref_tui.write(b"\x1b");
    settle(2);
    capture_pair_state(rust, refbox, rust_tui, ref_tui, "resume-menu-esc");
}
