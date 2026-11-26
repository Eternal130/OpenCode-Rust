//! Lane va21e — the G-528..G-534 paired anchors (the 修锚同 lane rule):
//! the R27-audited post-va21d dialog/plugin residuals — the shared
//! `\b\w` titlecase on the four first-char faces (G-528), the subagent
//! tracker's agent label updates (G-529), the v2 plugin `ctx.data`
//! member (G-530: the session sub-member + the collections as a
//! host-pushed view), `ctx.ui.tabs.open`/`focus` rooting + the
//! append-only open (G-531), the dialog prompt's full edit-binding
//! table (G-532: the line/word deletes, the char moves, the newline
//! arms over the engine's semantics), the paste routing to the
//! focused prompt (G-533), and the plugin select's NO-threshold fuzzy
//! filter (G-534: the weak match renders).
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21e-<anchor>-<state>.{json,txt}` and make
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication registry.
//! The fixtures are self-contained `.js` v2 `{id, setup}` TUI plugins
//! staged into BOTH sandboxes' real drop-in discovery roots (the same
//! staging contract `realchain_v21d.rs` documents).
//!
//! PACING: same as v21d — the two ends boot at different speeds, so
//! every rig-captured transition is toast-marked (the needle waits
//! complete at the SLOWER end) and the settle gaps absorb the boot
//! skew. The tabs anchor rides the va19sub turn-scripted gateway
//! (deterministic parent→child→parent; the plugin POLLS for the child
//! so it fires whenever the turn lands — the poll walks the REF's
//! `ctx.data.session.list()` seam, anchoring G-530's session sub-member
//! face, with the RPC mirror as the fallback).

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
            eprintln!("V21E SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;

struct V21ERig {
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

impl V21ERig {
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
        for tui in [&self.rust_tui, &self.ref_tui] {
            let deadline = std::time::Instant::now() + Duration::from_secs(secs);
            let mut ok = tui.screen_text().contains(needle);
            while !ok && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
                ok = tui.screen_text().contains(needle);
            }
            if !ok {
                return false;
            }
        }
        true
    }
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// Stage one v21e fixture plugin into a sandbox's config drop-in (the
/// same staging contract as `realchain_v21d.rs` — each side stages into
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

// The two digest-rig fixtures (the v21d pacing contract: toast-marked
// transitions, generous inter-transition gaps absorbing the boot skew).
const FIXTURES: &[(&str, &str)] = &[
    // 1 — the prompt edit-table + paste face (G-532/G-533): a prompt
    // with a DESCRIPTION + a LONG seeded value (the R27 un-anchored
    // description+long-value combination) opens at the seeded end
    // caret; the rig's key arms drive the full edit-binding table —
    // typing PAST the width, home + insert (the head window), ctrl+e
    // (the tail), the gradual left-arrow walk across the LEFT MARGIN
    // BAND, ctrl+a/ctrl+e (the line home/end aliases), the newline
    // arm (ctrl+j — the value goes multi-line), ctrl+u/ctrl+k (the
    // line deletes), ctrl+w (the word delete), and the bracketed
    // PASTE while the prompt is open (G-533: the text lands in the
    // prompt at the caret, newlines kept); esc → settle-undefined.
    (
        "v21e-ed",
        r#"export default {
  id: "v21e.ed.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21e ed " + tag, message: "", duration: 600000 });
    setTimeout(() => {
      ctx.ui.dialog
        .prompt({
          title: "v21e edit",
          description: "the description row above the long value",
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
    // 2 — the select faces (G-534): a NO-search categorized select
    // whose category is a GAPPED fuzzy hit for the typed query (a
    // combined score in (0, 0.7) — the pre-fix port's 0.7 threshold
    // dropped the rows; the ref keeps every match); then a SEARCH
    // select with description+footer options (the R27 un-anchored
    // search+description+footer face); esc → settle-undefined on
    // each.
    (
        "v21e-sel",
        r#"export default {
  id: "v21e.sel.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21e sel " + tag, message: "", duration: 600000 });
    setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21e weak",
          options: [
            {
              title: "Alpha",
              value: "alpha",
              description: "first row",
              footer: "f1",
              category: "probe-xray",
            },
            {
              title: "Beta",
              value: "beta",
              description: "second row",
              footer: "f2",
              category: "quiet-zone",
            },
            {
              title: "Gamma",
              value: "gamma",
              description: "third row",
              footer: "f3",
              category: "probe-xray",
            },
          ],
        })
        .then((value) => {
          report("weak " + String(value));
        });
    }, 6000);
    setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21e search",
          search: async (query, options) =>
            options.filter((option) =>
              option.title.toLowerCase().includes(String(query ?? "").toLowerCase()),
            ),
          options: [
            {
              title: "North",
              value: "north",
              description: "the search face",
              footer: "n1",
              category: "Cardinal",
            },
            {
              title: "South",
              value: "south",
              description: "the search face",
              footer: "s2",
              category: "Cardinal",
            },
          ],
        })
        .then((value) => {
          report("search " + String(value));
        });
    }, 96000);
    return () => {};
  },
};
"#,
    ),
];

/// The rig with ONE fixture staged (the single-plugin confounder).
fn v21e_rig(tag: &str, fixture: &str) -> V21ERig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    let tui_js = FIXTURES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or_else(|| panic!("unknown v21e fixture {fixture}"), |(_, js)| *js);
    stage_fixture(&rust, "opencode-rust", fixture, tui_js);
    stage_fixture(&refbox, "opencode", fixture, tui_js);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V21ERig {
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

impl Drop for V21ERig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V21ERig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21E precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

fn capture_pair(rig: &V21ERig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21e-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21e-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21e-{scenario}"), state, &ref_grid, &rust_grid);
}

/// Needle + capture: the toast marker completes at the SLOWER end;
/// the capture rides ~1 s behind (inside the state window on both
/// ends).
fn marker_state(rig: &V21ERig, scenario: &str, state: &str, needle: &str, window: u64) {
    let ok = rig.wait_both_screens(needle, window);
    assert!(
        ok,
        "V21E {scenario} RED: the {state} marker ({needle}) never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(rig, scenario, state);
}

// ===========================================================================
// Anchor 1 — the prompt edit-table + paste faces (G-532/G-533): the
// t+6 s prompt with a DESCRIPTION + a LONG seeded value opens (the
// R27 un-anchored description+long-value combination — the tail window
// at the seeded end caret). The key arms then drive:
//   * typing PAST the width (fresh chars push the viewport's tail),
//   * home + insert (the head window, the R27 typing-past-width state),
//   * ctrl+e (the tail again — the line-end alias),
//   * the gradual LEFT-ARROW walk across the LEFT MARGIN BAND (the
//     stateful band clamp only scrolls when the caret enters the band),
//   * ctrl+a / ctrl+e (the R27 ctrl arms — the line home/end aliases),
//   * ctrl+j (the NEWLINE arm — the value goes multi-line; the height-1
//     viewport follows the caret's line),
//   * ctrl+u / ctrl+k (the line deletes on the multi-line value),
//   * ctrl+w (the word-backward delete),
//   * the BRACKETED PASTE while the prompt is open (G-533: the text
//     lands in the prompt's value at the caret — newlines kept),
//   * esc → settle-undefined (the settled toast).
// The key encodings (ESC[H home / ESC[F end / ESC[D left / 0x01
// ctrl+a / 0x05 ctrl+e / 0x0a ctrl+j / 0x15 ctrl+u / 0x0b ctrl+k /
// 0x17 ctrl+w / 0x7f backspace / the ESC[200~…ESC[201~ paste envelope)
// parse identically on both ends (opentui core's chunk-bun and the
// port's input_term.rs).
// ===========================================================================
#[test]
fn v21e_prompt_edit_table_pair() {
    skip_without_ref_lane!();
    let mut rig = v21e_rig("v21e-ed", "v21e-ed");
    boot_both(&mut rig);
    // The prompt opens with the description + the seeded tail window.
    let open = rig.wait_both_screens("v21e edit", 60);
    assert!(
        open,
        "V21E prompt RED: the prompt dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "ed", "prompt-open");
    // Typing PAST the width: fresh printables at the tail push the
    // value beyond the box width — the viewport's tail follows.
    rig.write_both(b"ABCDEFGH");
    settle(2);
    capture_pair(&rig, "ed", "prompt-typed-past-width");
    // HOME + INSERT: the caret rewinds to the head (the head window)
    // and the printable lands at column 0.
    rig.write_both(b"\x1b[H");
    settle(2);
    capture_pair(&rig, "ed", "prompt-home");
    rig.write_both(b"q");
    settle(2);
    capture_pair(&rig, "ed", "prompt-head-insert");
    // CTRL+e — the tail again (the line-end alias).
    rig.write_both(b"\x05");
    settle(2);
    capture_pair(&rig, "ed", "prompt-ctrl-e");
    // The gradual left-arrow walk: three bursts of 10 — the caret
    // crosses the LEFT MARGIN BAND and the stateful viewport scrolls
    // (a stateless recompute would scroll back earlier).
    for burst in 0..3 {
        let mut walk = Vec::new();
        for _ in 0..10 {
            walk.extend_from_slice(b"\x1b[D");
        }
        rig.write_both(&walk);
        settle(2);
        capture_pair(&rig, "ed", &format!("prompt-walk-{}", burst + 1));
    }
    // CTRL+a — the line-home alias (the head window again).
    rig.write_both(b"\x01");
    settle(2);
    capture_pair(&rig, "ed", "prompt-ctrl-a");
    // CTRL+j — the NEWLINE arm: the value goes multi-line, the caret
    // moves to the new (empty) line, the height-1 viewport follows.
    rig.write_both(b"\x05");
    settle(1);
    rig.write_both(b"\x0a");
    settle(2);
    capture_pair(&rig, "ed", "prompt-newline");
    // Type on the second line, then CTRL+u — delete-to-line-start
    // empties the caret's line.
    rig.write_both(b"second line");
    settle(2);
    capture_pair(&rig, "ed", "prompt-second-line");
    rig.write_both(b"\x15");
    settle(2);
    capture_pair(&rig, "ed", "prompt-ctrl-u");
    // CTRL+a (line 0 home) + CTRL+k — delete-to-line-end trims the
    // first line from the caret.
    rig.write_both(b"\x01");
    settle(1);
    rig.write_both(b"\x1b[D\x1b[D\x1b[D");
    settle(1);
    rig.write_both(b"\x0b");
    settle(2);
    capture_pair(&rig, "ed", "prompt-ctrl-k");
    // Type two words, then CTRL+w — the word-backward delete removes
    // the word before the caret.
    rig.write_both(b"\x05");
    settle(1);
    rig.write_both(b"one two");
    settle(1);
    rig.write_both(b"\x17");
    settle(2);
    capture_pair(&rig, "ed", "prompt-ctrl-w");
    // The BRACKETED PASTE while the prompt is open (G-533): the pasted
    // text (with an embedded newline) lands in the prompt's value at
    // the caret — never in the hidden composer.
    rig.write_both(b"\x1b[200~pasted arm with\x0anewline\x1b[201~");
    settle(2);
    capture_pair(&rig, "ed", "prompt-paste");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    marker_state(&rig, "ed", "prompt-esc", "v21e ed prompt", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 2 — the select faces (G-534): the t+6 s NO-search select
// opens (the grouped shape with the description/footer rows); the
// typed query "pxy" is a GAPPED category-only fuzzy hit — the combined
// score sits in (0, 0.7): the ref (no filterThreshold → fuzzysort's
// default 0 → NEGATIVE_INFINITY) KEEPS the rows, the pre-fix port's
// 0.7 threshold dropped them; esc settles undefined. The t+96 s SEARCH
// select (description+footer options — the R27 un-anchored search face)
// follows; a typed query drives it; esc settles undefined.
// ===========================================================================
#[test]
fn v21e_select_weak_match_pair() {
    skip_without_ref_lane!();
    let mut rig = v21e_rig("v21e-sel", "v21e-sel");
    boot_both(&mut rig);
    // The no-search select opens with the grouped shape.
    let open = rig.wait_both_screens("v21e weak", 60);
    assert!(
        open,
        "V21E select RED: the weak-match dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "sel", "weak-open");
    // The gapped category-only query: "pxy" against "probe-xray" —
    // the combined score is WEAK (in (0, 0.7)) but non-zero, so the
    // ref renders the rows (no threshold) where the pre-fix port
    // filtered them (G-534).
    rig.write_both(b"pxy");
    settle(2);
    capture_pair(&rig, "sel", "weak-typed");
    // Escape: the dialog closes; the promise settles undefined.
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "weak-esc", "v21e sel weak", 30);
    // The search select (description+footer options — the R27
    // un-anchored search+description+footer face).
    marker_state(&rig, "sel", "search-open", "v21e search", 100);
    settle(1);
    capture_pair(&rig, "sel", "search-open");
    // A typed query on the search select.
    rig.write_both(b"sou");
    settle(2);
    capture_pair(&rig, "sel", "search-typed");
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "search-esc", "v21e sel search", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 3 — the R27 un-anchored tabs faces (G-531): `ctx.ui.tabs.
// open(child)` (the child ROOTS to the root's tab — the pre-fix port
// opened a CHILD-keyed tab AND navigated; the ref's open is
// append-only, no navigation) and `ctx.ui.tabs.focus(child)` (the ref
// navigates to the ROOT — the pre-fix port navigated to the child).
// The rig rides the committed va19sub turn scripts (a FOREGROUND
// subagent spawns a real child); the plugin polls for the settled
// child THROUGH `ctx.data.session.list()` (G-530's session sub-member
// face — the ref's synced cache; the RPC mirror only as the fallback),
// then runs the two ops with toast-marked gaps.
// =========================================================================

const TABS_FIXTURE: &str = r#"export default {
  id: "v21e.tabs.tui",
  async setup(ctx) {
    // The session discovery walks the REF's `ctx.data.session.list()`
    // when present (G-530's face), else the RPC mirror — the toast
    // reports WHICH path answered, so the export carries the seam.
    const list = async () => {
      try {
        const hasData = ctx.data && ctx.data.session && typeof ctx.data.session.list === "function";
        const raw = hasData ? ctx.data.session.list() : await ctx.session.list();
        return Array.isArray(raw) ? raw : raw && Array.isArray(raw.data) ? raw.data : [];
      } catch {
        return [];
      }
    };
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    (async () => {
      const parts = ["datalist=" + (ctx.data && ctx.data.session ? "1" : "0")];
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
      if (!family) {
        parts.push("nochild");
      } else {
        // The dwell: the turn completes (parent→child→parent) before
        // the tab ops run — both ends act on the settled family.
        await sleep(45000);
        // G-531 — open(child): the ref ROOTS the child id and the
        // open is APPEND-ONLY (the root's tab exists → early return,
        // NO navigation; the pre-fix port opened a child-keyed tab
        // and navigated to it).
        try {
          parts.push("open-child=" + ctx.ui.tabs.open(family.child));
        } catch {
          parts.push("open-child-err");
        }
      }
      ctx.ui.toast.show({
        title: "v21e tabs " + parts.join(" "),
        message: "",
        duration: 600000,
      });
      if (family) {
        // A settle gap between the two ops so the captures separate
        // the open state from the focus state.
        await sleep(8000);
        const parts2 = [];
        // G-531 — focus(child): the ref navigates to the ROOT
        // (`route.navigate({type: "session", sessionID: root(id)})`,
        // session-tabs.tsx:393-396) — the pre-fix port navigated to
        // the child itself.
        try {
          parts2.push("focus-child=" + ctx.ui.tabs.focus(family.child));
        } catch {
          parts2.push("focus-child-err");
        }
        ctx.ui.toast.show({
          title: "v21e tabs2 " + parts2.join(" "),
          message: "",
          duration: 600000,
        });
      }
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
fn v21e_tabs_open_focus_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    // The va19sub turn scripts (deterministic parent→child→parent),
    // one gateway PER engine — the same bootstrap va19's sub rig
    // uses; the record dirs are runtime-only cassettes for the log.
    let seeds = v21e_seeds_root().join("va19sub");
    assert!(
        seeds.join("turn-000.sse").is_file(),
        "V21E tabs RED: missing seed scripts under {}",
        seeds.display()
    );
    let record_rust = wt_root().join(".test-logs/va21e-tabs-seeds-rust");
    let record_ref = wt_root().join(".test-logs/va21e-tabs-seeds-ref");
    std::fs::create_dir_all(&record_rust).expect("create rust record dir");
    std::fs::create_dir_all(&record_ref).expect("create ref record dir");
    let pace = "synthetic:ttft=2500:tps=8";
    let rust_gateway = realchain_harness::Gateway::spawn_turn_paced(
        "v21e-tabs",
        &[],
        Some((&record_rust, &seeds)),
        pace,
    );
    let ref_gateway = realchain_harness::Gateway::spawn_turn_paced(
        "v21e-tabs",
        &[],
        Some((&record_ref, &seeds)),
        pace,
    );
    let rust = sandbox("v21e-tabs", rust_gateway.port);
    let refbox = sandbox("v21e-tabsref", ref_gateway.port);
    stage_fixture(&rust, "opencode-rust", "v21e-tabs", TABS_FIXTURE);
    stage_fixture(&refbox, "opencode", "v21e-tabs", TABS_FIXTURE);
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
    let rig = V21ERig {
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
    // The plugin polls for the child, dwells past the turn, then
    // opens with the child id — the SUCCESS shape of the toast
    // carries `open-child=` (a rig-health needle — a replay failure
    // lands `nochild` and never matches).
    let ok = rig.wait_both_screens("v21e tabs open-child=", 240);
    assert!(
        ok,
        "V21E tabs RED: the plugin never reported the open arm; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(2);
    capture_pair(&rig, "tabs", "open-child");
    // The focus arm lands ~8 s later (the fixture's gap) — the route
    // moves to the ROOT on the ref (the pre-fix port moved to the
    // child).
    let ok = rig.wait_both_screens("v21e tabs2 focus-child=", 60);
    assert!(
        ok,
        "V21E tabs RED: the plugin never reported the focus arm; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(2);
    capture_pair(&rig, "tabs", "focus-child");
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
fn v21e_seeds_root() -> std::path::PathBuf {
    wt_root().join("tools/diff/realchain/seeds")
}
