//! Lane va21d — the G-523..G-527 paired anchors (the 修锚同 lane rule):
//! the R26-audited dialog/select input-model residuals — the
//! no-search plugin select under a TYPED query (G-524: the headers
//! persist, the list stays grouped), the option row's DISTINCT
//! description/footer fields (G-525: the description INLINE after the
//! title, the footer right-aligned), the NO-description prompt box
//! (G-526: the height-6 chrome), the prompt's cursor-aware input
//! (G-527: home/end window moves + mid-text insert + backspace — the
//! ref's `TextareaRenderable` default bindings), the hyphenated
//! confirm-label pills (G-523: `\b\w` titlecase), and the R26
//! un-anchored `ctx.ui.tabs.move/close` with a CHILD session id (the
//! va21c child-id rooting).
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21d-<anchor>-<state>.{json,txt}` and make
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication registry.
//! The fixtures are self-contained `.js` v2 `{id, setup}` TUI plugins
//! staged into BOTH sandboxes' real drop-in discovery roots (the same
//! staging contract `realchain_v21c.rs` documents).
//!
//! PACING: same as v21c — the two ends boot at different speeds and
//! each fixture's timers run from ITS OWN activation, so every
//! rig-captured transition is toast-marked (the needle waits complete
//! at the SLOWER end) and the inter-transition gaps absorb the boot
//! skew. The tabs anchor rides the va19sub turn-scripted gateway
//! (deterministic parent→child→parent sequence; the plugin POLLS for
//! the child so it fires whenever the turn lands).

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
            eprintln!("V21D SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;

struct V21DRig {
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

impl V21DRig {
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
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// Stage one v21d fixture plugin into a sandbox's config drop-in (the
/// same staging contract as `realchain_v21c.rs` — each side stages into
/// its OWN discovery root; no `@opentui/solid` import needed here, so
/// no host shim).
fn stage_fixture(sandbox: &Sandbox, app_dir: &str, name: &str, tui_js: &str) {
    let plugins = sandbox.root.join("xdg-config").join(app_dir).join("plugins");
    let dir = plugins.join(name);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\n  \"name\": \"{name}\",\n  \"version\": \"1.0.0\",\
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

// The two digest-rig fixtures (the v21c pacing contract: toast-marked
// transitions, 45-60 s inter-transition gaps absorbing the boot skew).
const FIXTURES: &[(&str, &str)] = &[
    // 1 — the select faces: a NO-search categorized select whose
    // options carry DISTINCT description+footer+category fields
    // (G-525: the description INLINE after the title, the footer
    // right-aligned); a TYPED query on it (G-524: the grouped shape
    // PERSISTS under the local fuzzy filter — headers stay, the
    // matched row keeps its description+footer); esc →
    // settle-undefined; then the hyphenated-label confirm (G-523:
    // `\b\w` titlecase pills "Merge-Dry-Run"/"Never-Mind"), enter →
    // true.
    (
        "v21d-sel",
        r#"export default {
  id: "v21d.sel.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21d sel " + tag, message: "", duration: 600000 });
    setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21d fields",
          options: [
            {
              title: "Alpha",
              value: "alpha",
              description: "the first option",
              footer: "a1",
              category: "Cat One",
            },
            {
              title: "Beta",
              value: "beta",
              description: "the second option",
              footer: "b2",
              category: "Cat One",
            },
            {
              title: "Gamma",
              value: "gamma",
              description: "the third option",
              footer: "c3",
              category: "Cat Two",
            },
          ],
        })
        .then((value) => {
          report("select " + String(value));
          // va21g (the controller's slow-audit addendum): the confirm
          // phase CHAINS off the select's settle (the v21c-sel
          // template) — the old t+96 s absolute timer was dead dwell.
          setTimeout(() => {
            ctx.ui.dialog
              .confirm({
                title: "v21d confirm",
                message: "run the plan?",
                label: { confirm: "merge-dry-run", cancel: "never-mind" },
              })
              .then((value) => {
                report("confirm " + String(value));
              });
          // 2026-11-27 re-widened 4s->20s: the arm counts from EACH
          // end's own promise-resolve, and the ref's dialog-close lags
          // the rust's by >2.5s — with the marker capture (~1.5s) the
          // 4s gap let the rust leg's next phase open INSIDE the
          // esc/enter state captures (torn exports; v21b's class).
          }, 20000);
        });
    }, 6000);
    return () => {};
  },
};
"#,
    ),
    // 2 — the prompt faces: a prompt with NO description (G-526: the
    // height-6 chrome — no description slot) + a LONG seeded value
    // (the tail window at the seeded end caret); the rig's key arms
    // drive the cursor (G-527): home (the head window + caret at 0),
    // a printable INSERT at the caret, end (back to the tail),
    // left×3 + backspace (the mid-text delete); esc →
    // settle-undefined.
    (
        "v21d-cp",
        r#"export default {
  id: "v21d.cp.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21d cp " + tag, message: "", duration: 600000 });
    setTimeout(() => {
      ctx.ui.dialog
        .prompt({
          title: "v21d edit",
          placeholder: "type here",
          value: "0123456789012345678901234567890123456789012345678901234567890123456789",
        })
        .then((value) => {
          report("prompt " + String(value));
        });
    }, 6000);
    return () => {};
  },
};
"#,
    ),
];

/// The rig with ONE fixture staged (the single-plugin confounder).
fn v21d_rig(tag: &str, fixture: &str) -> V21DRig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    let tui_js = FIXTURES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or_else(|| panic!("unknown v21d fixture {fixture}"), |(_, js)| *js);
    stage_fixture(&rust, "opencode-rust", fixture, tui_js);
    stage_fixture(&refbox, "opencode", fixture, tui_js);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V21DRig {
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

impl Drop for V21DRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V21DRig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21D precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

fn capture_pair(rig: &V21DRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21d-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21d-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21d-{scenario}"), state, &ref_grid, &rust_grid);
}

/// Needle + capture: the toast marker completes at the SLOWER end;
/// the capture rides ~1 s behind (inside the state window on both
/// ends).
fn marker_state(rig: &V21DRig, scenario: &str, state: &str, needle: &str, window: u64) {
    let ok = rig.wait_both_screens(needle, window);
    assert!(
        ok,
        "V21D {scenario} RED: the {state} marker ({needle}) never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(rig, scenario, state);
}

// ===========================================================================
// Anchor 1 — the select faces: the t+6 s NO-search categorized select
// with description+footer+category options opens (the grouped shape
// with the INLINE descriptions + right-aligned footers, G-525); a
// typed query keeps the HEADERS (G-524 — the pre-fix port flattened);
// esc settles undefined; the t+96 s hyphenated-label confirm renders
// the `\b\w` pills (G-523); enter → true (the settled toast).
// ===========================================================================
#[test]
fn v21d_select_fields_grouped_pair() {
    skip_without_ref_lane!();
    let mut rig = v21d_rig("v21d-sel", "v21d-sel");
    boot_both(&mut rig);
    // The select opens with the grouped shape (the open-state needle
    // is rig health; the field layout rides the export).
    let open = rig.wait_both_screens("v21d fields", 60);
    assert!(
        open,
        "V21D select RED: the select dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "sel", "fields-open");
    // A typed query: the local fuzzy filter narrows to Gamma — the
    // CATEGORY HEADER persists (G-524) and the row keeps its
    // description/footer (G-525's filtered shape).
    rig.write_both(b"gam");
    settle(2);
    capture_pair(&rig, "sel", "fields-typed");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "select-esc", "v21d sel select", 30);
    // The hyphenated-label confirm (the G-523 pills).
    marker_state(&rig, "sel", "confirm-open", "v21d confirm", 90);
    settle(1);
    capture_pair(&rig, "sel", "confirm-open");
    // Enter on the confirm pill → resolves true (the settled toast).
    rig.write_both(b"\r");
    marker_state(&rig, "sel", "confirm-enter", "v21d sel confirm", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 2 — the prompt faces: the t+6 s NO-description prompt opens
// (G-526: the height-6 chrome, the LONG seeded value's TAIL window at
// the seeded end caret); the key arms drive the cursor (G-527):
// home → the HEAD window, a printable INSERT at the caret, end → the
// tail again, left×3 + backspace → the mid-text delete; esc settles
// undefined. The key encodings (ESC[H home / ESC[F end / ESC[D left /
// 0x7f backspace) parse identically on both ends (opentui core's
// chunk-bun-sjw2d9bq.js:5203-5204 and the port's input_term.rs:578-579).
// ===========================================================================
#[test]
fn v21d_prompt_cursor_pair() {
    skip_without_ref_lane!();
    let mut rig = v21d_rig("v21d-cp", "v21d-cp");
    boot_both(&mut rig);
    // The prompt opens with the no-description chrome + the seeded
    // tail window.
    let open = rig.wait_both_screens("v21d edit", 60);
    assert!(
        open,
        "V21D prompt RED: the prompt dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "cp", "prompt-open");
    // HOME — the caret (and the viewport with it) rewinds to the
    // value's head.
    rig.write_both(b"\x1b[H");
    settle(2);
    capture_pair(&rig, "cp", "prompt-home");
    // A printable INSERTS at the caret (column 0 — the head).
    rig.write_both(b"q");
    settle(2);
    capture_pair(&rig, "cp", "prompt-insert");
    // END — the caret (and the viewport) return to the tail.
    rig.write_both(b"\x1b[F");
    settle(2);
    capture_pair(&rig, "cp", "prompt-end");
    // left×3 + BACKSPACE — the mid-text delete (the char before the
    // caret, 3 cells in from the end).
    rig.write_both(b"\x1b[D\x1b[D\x1b[D");
    settle(1);
    rig.write_both(b"\x7f");
    settle(2);
    capture_pair(&rig, "cp", "prompt-backspace");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    marker_state(&rig, "cp", "prompt-esc", "v21d cp prompt", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 3 — the R26 un-anchored tabs face: `ctx.ui.tabs.move` with a
// CHILD session id (the va21c child-id rooting — the move resolves the
// child's ROOT tab) and `ctx.ui.tabs.close` with a child id (NOT a
// tab's sessionID → the membership check returns false, no close; the
// ref roots only move, api.tsx:267-281). The rig rides the committed
// va19sub turn scripts (a FOREGROUND subagent spawns a real child);
// the plugin polls for the settled child, opens the root tab, moves
// with the child id, closes with the child id, and toasts the three
// returns — the toast + the rendered tab strip ride the export.
// =========================================================================

const TABS_FIXTURE: &str = r#"export default {
  id: "v21d.tabs.tui",
  async setup(ctx) {
    // The session discovery rides the DUAL path (the ref's synced
    // `ctx.data.session.list()` when present, else the RPC mirror
    // `ctx.session.list()` — the port's ctx has no `data` member):
    // the child's parentID IS the family root for the seeded
    // subagent turn.
    const list = async () => {
      try {
        const raw =
          ctx.data && ctx.data.session && typeof ctx.data.session.list === "function"
            ? ctx.data.session.list()
            : await ctx.session.list();
        return Array.isArray(raw) ? raw : raw && Array.isArray(raw.data) ? raw.data : [];
      } catch {
        return [];
      }
    };
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    (async () => {
      // The child session (a subagent with parentID).
      let family = null;
      for (let i = 0; i < 180 && !family; i++) {
        const sessions = await list();
        const child = sessions.find(
          (session) => session && typeof session.id === "string" && session.parentID,
        );
        if (child) {
          family = { child: child.id, root: String(child.parentID) };
        } else {
          await sleep(1000);
        }
      }
      const parts = [];
      if (!family) {
        parts.push("nochild");
      } else {
        // The dwell: the turn completes (parent→child→parent) before
        // the tab ops run — both ends act on the settled family, and
        // the capture rides the post-turn state (the pace is
        // deterministic per engine).
        await sleep(45000);
        try {
          parts.push("open=" + ctx.ui.tabs.open(family.root));
        } catch {
          parts.push("open-err");
        }
        await sleep(3000);
        // The REGISTERED face: move with the CHILD id (the ref roots
        // it — api.tsx:267-273 `host.data.session.root`; the port's
        // pushed child→root map, G-519-lead) moves the ROOT's tab.
        try {
          parts.push("move=" + ctx.ui.tabs.move(family.child, 0));
        } catch {
          parts.push("move-err");
        }
        // close with the CHILD id: the membership check finds no tab
        // with the child's sessionID → false, no close (both ends —
        // the ref roots only move, api.tsx:274-280).
        try {
          parts.push("close=" + ctx.ui.tabs.close(family.child));
        } catch {
          parts.push("close-err");
        }
      }
      ctx.ui.toast.show({
        title: "v21d tabs " + parts.join(" "),
        message: "",
        duration: 600000,
      });
    })();
    return () => {};
  },
};
"#;

/// The tab pre-authorization block (the subagent round-trip must not
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

#[test]
fn v21d_tabs_child_id_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    // The va19sub turn scripts (deterministic parent→child→parent),
    // one gateway PER engine — the same bootstrap va19's sub rig
    // uses; the record dirs are runtime-only cassettes for the log.
    let seeds = v21d_seeds_root().join("va19sub");
    assert!(
        seeds.join("turn-000.sse").is_file(),
        "V21D tabs RED: missing seed scripts under {}",
        seeds.display()
    );
    let record_rust = wt_root().join(".test-logs/va21d-tabs-seeds-rust");
    let record_ref = wt_root().join(".test-logs/va21d-tabs-seeds-ref");
    std::fs::create_dir_all(&record_rust).expect("create rust record dir");
    std::fs::create_dir_all(&record_ref).expect("create ref record dir");
    let pace = "synthetic:ttft=2500:tps=8";
    let rust_gateway = realchain_harness::Gateway::spawn_turn_paced(
        "v21d-tabs",
        &[],
        Some((&record_rust, &seeds)),
        pace,
    );
    let ref_gateway = realchain_harness::Gateway::spawn_turn_paced(
        "v21d-tabs",
        &[],
        Some((&record_ref, &seeds)),
        pace,
    );
    let rust = sandbox("v21d-tabs", rust_gateway.port);
    let refbox = sandbox("v21d-tabsref", ref_gateway.port);
    stage_fixture(&rust, "opencode-rust", "v21d-tabs", TABS_FIXTURE);
    stage_fixture(&refbox, "opencode", "v21d-tabs", TABS_FIXTURE);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    allow_subagent(&rust, false);
    allow_subagent(&refbox, true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    let rig = V21DRig {
        _lane: lane,
        _gateway: rust_gateway,
        rust,
        refbox,
        stage,
        _rust_server_url: String::new(),
        _rust_password: None,
        rust_tui,
        ref_tui,
    };
    let _ref_gateway = ref_gateway;
    let mut rig = rig;
    boot_both(&mut rig);
    // Drive the seeded subagent turn (the same exchange shape the
    // toolloop/va19 rigs use): the parent spawns the foreground
    // child, the child replies, the parent confirms.
    rig.write_both(b"hello\r");
    // The plugin polls for the child, dwells past the turn, then runs
    // the tab ops — the SUCCESS shape of the toast carries `open=` (a
    // rig-health needle — a replay failure lands `nochild` and never
    // matches).
    let ok = rig.wait_both_screens("v21d tabs open=", 240);
    assert!(
        ok,
        "V21D tabs RED: the plugin never reported the tabs arm; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(2);
    capture_pair(&rig, "tabs", "child-id");
    drop(rig);
}

/// This WORKTREE's root (no main-checkout redirect: the committed
/// va19sub seed scripts + the runtime record dirs are lane-local —
/// the same deliberate choice va19's sub rig makes).
fn wt_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

/// The committed seed-script root (the worktree's realchain seeds).
fn v21d_seeds_root() -> std::path::PathBuf {
    wt_root().join("tools/diff/realchain/seeds")
}
