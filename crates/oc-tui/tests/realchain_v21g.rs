//! Lane va21g — the G-540/G-542..G-546 paired anchors (the 修锚同 lane
//! rule): the mini/scrollback/dialog residuals the R29 audit
//! decomposed out of the va21f first-round corpus —
//!
//! * G-540 — the streamed progress chunks CONTINUE the entry's row
//!   (`entryFlags(commit)` flows into the split-commit newline intent:
//!   a PROGRESS batch carries `start_on_new_line: false` + no trailing
//!   newline, the batch parks at the engine's tail column, and the
//!   rows carry no trailing blank row — the ref's
//!   `createScrollbackWriter` shape, scrollback.writer.tsx:278-295);
//! * G-542+G-546 — the prompt's height-1 VERTICAL band (the mount
//!   paint shows the value's TOP row — the seeded caret's
//!   `gotoLineEnd()` never re-bands it — and the band moves ONLY when
//!   the caret crosses from inside the band's row to outside it);
//! * G-543 — the replay user arm reads the wire's TOP-LEVEL `text`
//!   field and the live user row renders from the SSE delivery (the
//!   POST response's optimistic admit merged into the pending; the
//!   local echo NEVER prints);
//! * G-544 — the resume model seam (`input.model ?? selected?.model`)
//!   + the turn summary's "Default model" fallback;
//! * G-545 — the footer's transient 3 s notice (`setNotice`,
//!   footer.ts:713-722 — the statusline channel, never a scrollback
//!   row).
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21g-<anchor>-<state>.{json,txt}` and make
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication registry.
//!
//! WORKTREE BINARY (va21e lesson, see realchain_v21f.rs' header):
//! run the anchors from a worktree with the lane's own target dir
//! (`CARGO_TARGET_DIR=<worktree>/target`) after
//! `cargo build -p oc-cli --bin opencode-rust --locked` in it.
//!
//! THREADS (controller steering 2026-11-27): the anchor replay runs
//! append `--test-threads 2` — 4 nextest threads × dual-end rigs
//! starve each core (the v21e 685s vs 218s low-concurrency baseline).
//!
//! RIG COMPANIONS (same lane): the ed fixture's select phase CHAINS
//! off the prompt's promise settle (the v21c landmine class — no
//! absolute t+96 s timer racing the scenario legs; the chain lives in
//! `realchain_v21f.rs`'s ED_FIXTURE, mirrored here), and the
//! `replace_version_pill` normalizer gained the mini wordmark's
//! INLINE v-prefixed arm (realchain_harness.rs) so the
//! `vlocal`/`v2.0.24` splash token stops diffing.

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
            eprintln!("V21G SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;

/// This WORKTREE's root (no main-checkout redirect — the same shape
/// `realchain_v21f.rs` documents: the seed scripts + record dirs are
/// lane-local).
fn wt_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

// ===========================================================================
// The shared rig (the v21f V21FRig shape): one plugin-fixture pair —
// both sandboxes stage the same tui.js plugin, the ref side boots the
// staged service, the rust side runs this worktree's server.
// ===========================================================================

struct V21GRig {
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

impl V21GRig {
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

    /// Rig-health needle wait (both ends CONCURRENTLY — the v21f
    /// slow-audit fix A shape).
    fn wait_both_screens(&self, needle: &str, secs: u64) -> bool {
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

    fn capture_pair(&self, scenario: &str, state: &str) {
        let normalize = |grid: &str| self.normalize(grid);
        let ref_grid = normalize(&capture_settled(&self.ref_tui));
        let rust_grid = normalize(&capture_settled(&self.rust_tui));
        dump_grid(&format!("v21g-{scenario}-{state}-ref"), &ref_grid);
        dump_grid(&format!("v21g-{scenario}-{state}-rust"), &rust_grid);
        export_grid_diff(&format!("v21g-{scenario}"), state, &ref_grid, &rust_grid);
    }
}

/// Stage one fixture plugin into a sandbox's config drop-in (the
/// v21f staging contract).
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

/// The dialog-face fixture (the v21f ED_FIXTURE's CHAINED form — the
/// rig companion): a prompt with a seeded MULTI-LINE value; the
/// select phase arms 4 s after the prompt's promise settles (no
/// absolute timers racing the scenario legs).
const ED_FIXTURE: &str = r#"export default {
  id: "v21g.ed.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21f ed " + tag, message: "", duration: 600000 });
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
          // va21g — the select phase CHAINS off the prompt's settle
          // (the v21c landmine class: the old t+96 s ABSOLUTE timer
          // raced the scenario legs).
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
          }, 4000);
        });
    }, 6000);
    return () => {};
  },
};
"#;

/// The plugin-fixture rig (the v21f `v21f_rig` shape): the instant-paced
/// record gateway + both sandboxes staging the SAME fixture (the rust
/// side's config dir is `opencode-rust`; the ref side's is `opencode`)
/// — the dialog fixtures ride the FULL TUI's plugin host.
fn fixture_rig(tag: &str, fixture: &str, tui_js: &str) -> V21GRig {
    let lane = lane_lock();
    let record = wt_root().join(format!(".test-logs/{tag}-seeds"));
    std::fs::create_dir_all(&record).expect("create record dir");
    let gateway = Gateway::spawn_paced(tag, Some(&record), &default_pace());
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
    V21GRig {
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

impl Drop for V21GRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

// ===========================================================================
// Anchor 1 — the prompt's vertical band (G-542 + G-546) + the chained
// select phase (the rig companion): the seeded THREE-line value
// ("alpha line\nbeta line\ngamma tail", the caret seeded at the end).
//   * prompt-open — the MOUNT paint: the input row shows the value's
//     TOP row ("alpha line") — NOT the seeded caret's row (the ref's
//     first paint computes from offset 0; the pre-fix port followed
//     the caret to "gamma tail");
//   * prompt-up-1 — the caret climbs to the middle row: the band
//     STAYS ("alpha line" — the caret was outside and stayed outside;
//     the pre-fix port followed to "beta line");
//   * prompt-up-2 / prompt-down — the caret ENTERS the band / crosses
//     out: both ends EXACT before AND after (the crossing re-bands);
//   * prompt-esc — the chained select arms 4 s later; select-open +
//     the paste legs re-export (the v21f-sel states' chain).
// ===========================================================================

#[test]
fn v21g_prompt_band_pair() {
    skip_without_ref_lane!();
    let mut rig = fixture_rig("v21g-ed", "v21g-ed", ED_FIXTURE);
    let boot_rust = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let boot_ref = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        boot_rust && boot_ref,
        "V21G band RED: boot rust={boot_rust} ref={boot_ref}; rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    // The probe toast rides every export (the family face); the
    // prompt opens at t+6 s.
    let fam = rig.wait_both_screens("v21f fam ", 20);
    assert!(fam, "V21G band RED: the probe toast never showed");
    settle(4);
    let open = rig.wait_both_screens("v21f lines", 60);
    assert!(
        open,
        "V21G band RED: the prompt dialog never opened; rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    rig.capture_pair("ed", "prompt-open");
    // UP — the caret climbs to the middle row ("beta"): the band
    // STAYS at the mount's top row (the caret was outside and stayed
    // outside — no crossing).
    rig.write_both(b"\x1b[A");
    settle(2);
    rig.capture_pair("ed", "prompt-up-1");
    // UP again — the caret lands ON the band's row ("alpha"): it
    // ENTERS the band (outside→inside — no move needed).
    rig.write_both(b"\x1b[A");
    settle(2);
    rig.capture_pair("ed", "prompt-up-2");
    // DOWN — the caret crosses OUT of the band: the crossing re-bands
    // (the band follows the caret to "beta").
    rig.write_both(b"\x1b[B");
    settle(2);
    rig.capture_pair("ed", "prompt-down");
    // Escape: the dialog closes; the promise settles (the toast); the
    // chained select arms 4 s after it.
    rig.write_both(b"\x1b");
    let marker = rig.wait_both_screens("v21f ed prompt", 30);
    assert!(marker, "V21G band RED: the prompt never settled");
    settle(1);
    rig.capture_pair("ed", "prompt-esc");
    // The chained select (the companion's chain — no absolute timer).
    let select = rig.wait_both_screens("v21f paste", 30);
    assert!(
        select,
        "V21G band RED: the chained select never opened; rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    rig.capture_pair("ed", "select-open");
    // The plain paste: "the" lands in the filter — the list narrows.
    rig.write_both(b"\x1b[200~the\x1b[201~");
    settle(2);
    rig.capture_pair("ed", "select-paste");
    // The ANSI-wrapped + newline-carrying paste: "me" appends → the
    // filter reads "theme".
    rig.write_both(b"\x1b[200~\x1b[31mme\x1b[0m\x0a\x1b[201~");
    settle(2);
    rig.capture_pair("ed", "select-paste-ansi-newline");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    let sel_marker = rig.wait_both_screens("v21f ed sel", 30);
    assert!(sel_marker, "V21G band RED: the select never settled");
    drop(rig);
}

// ===========================================================================
// Anchor 2 — the footer notice (G-545): the idle mini pair opens the
// command palette, routes to the agent panel, and picks a DIFFERENT
// agent — the ref shows a TRANSIENT 3 s statusline notice
// (`setNotice("agent …")`, footer.ts:875-880 → :713-722) and NO
// scrollback row; the pre-fix port wrote a permanent `Commit::system`
// row. Two states ride the export: the LIVE notice (captured inside
// the 3 s window) and the CLEARED statusline (captured after it).
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

    fn wait_both_screens(&self, needle: &str, secs: u64) -> bool {
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

    fn capture_pair(&self, scenario: &str, state: &str) {
        let normalize = |grid: &str| self.normalize(grid);
        let ref_grid = normalize(&capture_settled(&self.ref_tui));
        let rust_grid = normalize(&capture_settled(&self.rust_tui));
        dump_grid(&format!("v21g-{scenario}-{state}-ref"), &ref_grid);
        dump_grid(&format!("v21g-{scenario}-{state}-rust"), &rust_grid);
        export_grid_diff(&format!("v21g-{scenario}"), state, &ref_grid, &rust_grid);
    }
}

/// The idle mini rig (the v21f `mini_rig` shape).
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

#[test]
fn v21g_notice_pair() {
    skip_without_ref_lane!();
    let mut rig = mini_rig("v21g-nt", &[]);
    let boot_rust = rig.rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let boot_ref = rig.ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    assert!(
        boot_rust && boot_ref,
        "V21G notice RED: boot rust={boot_rust} ref={boot_ref}; rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
    // The palette (ctrl+p) → four downs to "Switch agent" → enter: the
    // agent panel opens (footer.command.tsx's route; the rows list the
    // catalog's agent IDs — build, plan, …).
    rig.write_both(b"\x10");
    let palette = rig.wait_both_screens("Switch agent", 20);
    assert!(palette, "V21G notice RED: the palette never listed the agent route");
    rig.write_both(b"\x1b[B\x1b[B\x1b[B\x1b[B\r");
    let panel = rig.wait_both_screens("plan", 20);
    assert!(
        panel,
        "V21G notice RED: the agent panel never opened; rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    // Pick the SECOND row (a DIFFERENT agent than the default "Build"
    // — the ref's handleAgentSelect no-ops a same-agent pick,
    // footer.ts:876): down + enter. The pick's notice is a 3 s
    // TRANSIENT — needle it on BOTH ends and capture RAW (the settle
    // machinery's ≥2.5 s stability proof would wait the window out).
    rig.write_both(b"\x1b[B\r");
    let notice = rig.wait_both_screens("agent Plan", 10);
    assert!(
        notice,
        "V21G notice RED: the pick's notice never showed; rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    {
        let normalize = |grid: &str| rig.normalize(grid);
        let ref_grid = normalize(&realchain_harness::capture_transient(&rig.ref_tui));
        let rust_grid = normalize(&realchain_harness::capture_transient(&rig.rust_tui));
        dump_grid("v21g-nt-notice-live-ref", &ref_grid);
        dump_grid("v21g-nt-notice-live-rust", &rust_grid);
        export_grid_diff("v21g-nt", "notice-live", &ref_grid, &rust_grid);
    }
    // Past the window: the notice cleared (the statusline's idle
    // groups return; NO scrollback row ever landed).
    settle(4);
    rig.capture_pair("nt", "notice-cleared");
    drop(rig);
}

// ===========================================================================
// Anchor 3 — the v21f-rs states' re-export (G-540 + G-543 + G-544):
// the va19sub turn-scripted pair runs the subagent round-trip, then
// EXITS and re-boots with `--continue` — the streamed reply's chunks
// CONTINUE one row (G-540), the replayed user echo renders from the
// wire's top-level `text` field (G-543), and the resumed session's
// stored model resolves (G-544). The four v21f-rs states re-export
// under the v21g-rs scenario for the controller's re-adjudication.
// ===========================================================================

/// The tool pre-authorization block (the v21f shape).
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

/// The squashed-screen needle (G-540's rig health: the completion
/// needle matches with the inter-row whitespace stripped — the row
/// shape itself rides the export).
fn screen_squashed(tui: &TuiProcess) -> String {
    tui.screen_text().chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn wait_squashed(tui: &TuiProcess, needle: &str, secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    let mut ok = screen_squashed(tui).contains(needle);
    while !ok && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        ok = screen_squashed(tui).contains(needle);
    }
    ok
}

#[test]
fn v21g_resume_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    // The va19sub turn scripts (deterministic parent→child→parent),
    // one gateway PER engine.
    let seeds = wt_root().join("tools/diff/realchain/seeds/va19sub");
    assert!(
        seeds.join("turn-000.sse").is_file(),
        "V21G resume RED: missing seed scripts under {}",
        seeds.display()
    );
    let record_rust = wt_root().join(".test-logs/v21g-rs-seeds-rust");
    let record_ref = wt_root().join(".test-logs/v21g-rs-seeds-ref");
    std::fs::create_dir_all(&record_rust).expect("create rust record dir");
    std::fs::create_dir_all(&record_ref).expect("create ref record dir");
    let pace = "synthetic:ttft=2500:tps=8";
    let rust_gateway =
        Gateway::spawn_turn_paced("v21g-rs", &[], Some((&record_rust, &seeds)), pace);
    let ref_gateway = Gateway::spawn_turn_paced("v21g-rsr", &[], Some((&record_ref, &seeds)), pace);
    let rust = sandbox("v21g-rs", rust_gateway.port);
    let refbox = sandbox("v21g-rsref", ref_gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    allow_subagent(&rust, false);
    allow_subagent(&refbox, true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;

    let capture = |rust: &Sandbox,
                   refbox: &Sandbox,
                   rust_tui: &TuiProcess,
                   ref_tui: &TuiProcess,
                   state: &str| {
        let roots =
            [refbox.root.to_string_lossy().into_owned(), rust.root.to_string_lossy().into_owned()];
        let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
        let normalize = |grid: &str| normalize_grid(grid, &refs);
        let ref_grid = normalize(&capture_settled(ref_tui));
        let rust_grid = normalize(&capture_settled(rust_tui));
        dump_grid(&format!("v21g-rs-{state}-ref"), &ref_grid);
        dump_grid(&format!("v21g-rs-{state}-rust"), &rust_grid);
        export_grid_diff("v21g-rs", state, &ref_grid, &rust_grid);
    };

    // Phase 1 — the first mini pair runs the subagent round-trip (the
    // streamed reply's chunks continue ONE row — G-540's face).
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini");
    rust_command.env("PWD", rust.project.display().to_string());
    let mut rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let mut ref_tui = ref_mini_spawn(&refbox, &stage, COLS, ROWS);
    let rust_boot = rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_boot = ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    assert!(
        rust_boot && ref_boot,
        "V21G resume RED: first boot rust={rust_boot} ref={ref_boot}; rust screen:\n{}\nref \
         screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(3);
    rust_tui.write(b"hello\r");
    ref_tui.write(b"hello\r");
    let rust_done = wait_squashed(&rust_tui, "round-tripcompleted", 90);
    let ref_done = wait_squashed(&ref_tui, "round-tripcompleted", 120);
    assert!(
        rust_done && ref_done,
        "V21G resume RED: the round-trip never completed rust={rust_done} ref={ref_done}; \
         rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(3);
    capture(&rust, &refbox, &rust_tui, &ref_tui, "first-turn-settled");
    // The first pair EXITS (the two-press chord); the SERVERS stay up.
    rust_tui.write(b"\x03\x03");
    ref_tui.write(b"\x03\x03");
    settle(3);
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));

    // Phase 2 — the RESUME pair: `--continue` re-attaches the session;
    // the replay feed walks the transcript (the user echo from the
    // wire's `text` field — G-543; the stored model resolving —
    // G-544).
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini").arg("--continue");
    rust_command.env("PWD", rust.project.display().to_string());
    let mut rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let mut ref_tui = ref_mini_spawn_with_args(&refbox, &stage, COLS, ROWS, &["--continue"]);
    let rust_boot = rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_boot = ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    let rust_replayed = wait_squashed(&rust_tui, "round-tripcompleted", 60);
    let ref_replayed = wait_squashed(&ref_tui, "round-tripcompleted", 90);
    assert!(
        rust_boot && ref_boot && rust_replayed && ref_replayed,
        "V21G resume RED: boot rust={rust_boot} ref={ref_boot} replayed rust={rust_replayed} \
         ref={ref_replayed}; rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(5);
    capture(&rust, &refbox, &rust_tui, &ref_tui, "resume-settled");
    // The menu face states: `down` opens the subagent menu; esc
    // closes it.
    rust_tui.write(b"\x1b[B");
    ref_tui.write(b"\x1b[B");
    settle(2);
    capture(&rust, &refbox, &rust_tui, &ref_tui, "resume-menu");
    rust_tui.write(b"\x1b");
    ref_tui.write(b"\x1b");
    settle(2);
    capture(&rust, &refbox, &rust_tui, &ref_tui, "resume-menu-esc");
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    service_stop(&rust);
    drop(lane);
    drop(rust_gateway);
    drop(ref_gateway);
}
