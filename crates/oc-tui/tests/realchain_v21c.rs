//! Lane va21c — the G-519..G-522 paired anchors (the 修锚同 lane rule):
//! the R25-audited un-anchored states of the plugin dialog-member
//! surface — the confirm/prompt member arms (enter → true, esc →
//! settle-undefined, the custom multi-word label pill row), the
//! select-with-search initial flat shape + category-as-footer (G-522),
//! the categorized-option grouping, and the plain-option submit arm
//! (pre-tab enter). The prompt-open state also carries the two va21c
//! lead probes: the long-description wrap (G-237 description leg) and
//! the long seeded-value tail visibility.
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21c-<anchor>-<state>.{json,txt}` and make
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication registry.
//! The fixtures are self-contained `.js` v2 `{id, setup}` TUI plugins
//! staged into BOTH sandboxes' real drop-in discovery roots (the same
//! staging contract `realchain_v21b.rs` documents).
//!
//! PACING: same as v21b — the two ends boot at different speeds and
//! each fixture's timers run from ITS OWN activation, so every
//! rig-captured transition is toast-marked (the needle waits complete
//! at the SLOWER end) and the inter-transition gaps (45 s) absorb the
//! boot skew.

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
            eprintln!("V21C SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;

struct V21CRig {
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

impl V21CRig {
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
        // 2026-11-27 slow-audit fix A (restored after fix C): wait BOTH
        // ends CONCURRENTLY — the fixture's phase timeline is now CHAINED
        // (each phase arms after the previous promise settles, fix C), so
        // the wait no longer carries any implicit alignment duty.
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

/// Stage one v21c fixture plugin into a sandbox's config drop-in (the
/// same staging contract as `realchain_v21b.rs` — each side stages into
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

// The two fixtures. Every rig-captured transition fires a toast
// marker (the needle waits complete at the SLOWER end) and the 45 s
// inter-transition gaps absorb the boot skew.
const FIXTURES: &[(&str, &str)] = &[
    // 1 — the confirm/prompt member arms: a confirm with a custom
    // multi-word label (the G-520 titlecase pills), enter → true
    // (the settled toast); then a prompt with a LONG description
    // (the wrap shape) + a LONG seeded value (the tail-visibility
    // probe) + an explicit placeholder, esc → settle-undefined (the
    // G-519 arm — the toast's String(value) distinguishes undefined
    // from null). va21g (the controller's slow-audit addendum): the
    // prompt phase CHAINS off the confirm's settle (the rig's enter
    // marker) — the v21c-sel chaining template; the old t+96 s
    // absolute timer was dead dwell (any leg speedup raced it).
    (
        "v21c-cp",
        r#"export default {
  id: "v21c.cp.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21c cp " + tag, message: "", duration: 600000 });
    setTimeout(() => {
      ctx.ui.dialog
        .confirm({
          title: "v21c confirm",
          message: "delete everything?",
          label: { confirm: "delete everything", cancel: "never mind" },
        })
        .then((value) => {
          report("confirm " + String(value));
          setTimeout(() => {
            ctx.ui.dialog
              .prompt({
                title: "v21c prompt",
                description:
                  "a fairly long description line that should wrap at the dialog edge when rendered by the generic prompt face of both implementations",
                placeholder: "type here",
                value: "0123456789012345678901234567890123456789012345678901234567890123456789",
              })
              .then((value) => {
                report("prompt " + String(value));
              });
          }, 4000);
        });
    }, 6000);
    return () => {};
  },
};
"#,
    ),
    // 2 — the select arms: a select WITH a search fn + categorized
    // options (the G-522 flat-from-empty-query shape + the
    // category-as-footer), a typed query (the degraded native
    // filter's flat rows), esc → settle-undefined; then a categorized
    // select with NO search (the grouped shape); then a plain select
    // (no categories, no search) whose pre-tab enter submits the
    // selected row (the settled toast). The 135 s gaps give the
    // key-driven states dwell room on BOTH ends (see fixture 1's
    // note).
    (
        "v21c-sel",
        r#"export default {
  id: "v21c.sel.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({ title: "v21c sel " + tag, message: "", duration: 600000 });
    // 2026-11-27 slow-audit fix C: the three select phases used ABSOLUTE
    // wall-clock delays (t+6s / t+141s / t+231s) and only ever passed
    // because the rig's SERIAL wait's ref-leg latency happened to cover
    // the gaps (the marker window of 90s < the 134s gap to t+141s —
    // a flaky landmine: any ref speedup turned it red, and the concurrent
    // wait exposed it immediately). The timeline is now CHAINED: each
    // phase arms 2s after the PREVIOUS phase's promise settles (the esc
    // confirmation the rig's marker already needles) — explicit
    // synchronization, no absolute clock races, ~215s of dead dwell
    // eliminated.
    setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21c search",
          placeholder: "search...",
          options: [
            { title: "Alpha", value: "alpha", category: "Cat One" },
            { title: "Beta", value: "beta", category: "Cat One" },
            { title: "Gamma", value: "gamma", category: "Cat Two" },
          ],
          search: (query, options) =>
            options.filter((option) =>
              option.title.toLowerCase().includes(query.toLowerCase()),
            ),
        })
        .then((value) => {
          report("search " + String(value));
          setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21c grouped",
          options: [
            { title: "Alpha", value: "alpha", category: "Cat One" },
            { title: "Beta", value: "beta", category: "Cat One" },
            { title: "Gamma", value: "gamma", category: "Cat Two" },
          ],
        })
        .then((value) => {
          report("grouped " + String(value));
          setTimeout(() => {
            ctx.ui.dialog
              .select({
                title: "v21c plain",
                options: [
                  { title: "Plain One", value: "plain1" },
                  { title: "Plain Two", value: "plain2" },
                ],
              })
              .then((value) => {
                report("plain " + String(value));
              });
          }, 2000);
        });
    }, 2000);
        });
    }, 6000);
    return () => {};
  },
};
"#,
    ),
];

/// The rig with ONE fixture staged (the single-plugin confounder).
fn v21c_rig(tag: &str, fixture: &str) -> V21CRig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    let tui_js = FIXTURES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or_else(|| panic!("unknown v21c fixture {fixture}"), |(_, js)| *js);
    stage_fixture(&rust, "opencode-rust", fixture, tui_js);
    stage_fixture(&refbox, "opencode", fixture, tui_js);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V21CRig {
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

impl Drop for V21CRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V21CRig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21C precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

fn capture_pair(rig: &V21CRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21c-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21c-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21c-{scenario}"), state, &ref_grid, &rust_grid);
}

/// Needle + capture: the toast marker completes at the SLOWER end;
/// the capture rides ~1 s behind (inside the 45 s state window on
/// both ends).
fn marker_state(rig: &V21CRig, scenario: &str, state: &str, needle: &str, window: u64) {
    let ok = rig.wait_both_screens(needle, window);
    assert!(
        ok,
        "V21C {scenario} RED: the {state} marker ({needle}) never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(rig, scenario, state);
}

// ===========================================================================
// Anchor 1 — the confirm/prompt member arms: the t+6 s confirm opens
// (the custom multi-word label pills — G-520's titlecase); enter →
// true (the settled toast); the t+51 s prompt opens (the long
// description wrap + the long seeded value — the two va21c lead
// probes); esc → settle-undefined (G-519's arm; the toast's
// String(value) tells undefined from null). The open-state needles
// are rig health; the post-key arms ride the export.
// ===========================================================================
#[test]
fn v21c_confirm_prompt_arms_pair() {
    skip_without_ref_lane!();
    let mut rig = v21c_rig("v21c-cp", "v21c-cp");
    boot_both(&mut rig);
    // The confirm opens with the multi-word label pills.
    let open = rig.wait_both_screens("v21c confirm", 60);
    assert!(
        open,
        "V21C confirm RED: the confirm dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "cp", "confirm-open");
    // Enter on the confirm pill → resolves true (the settled toast).
    rig.write_both(b"\r");
    marker_state(&rig, "cp", "confirm-enter", "v21c cp confirm", 30);
    // The prompt opens with the long description + the seeded value.
    marker_state(&rig, "cp", "prompt-open", "v21c prompt", 90);
    // Escape → the promise settles undefined (G-519).
    rig.write_both(b"\x1b");
    marker_state(&rig, "cp", "prompt-esc", "v21c cp prompt", 30);
    drop(rig);
}

// ===========================================================================
// Anchor 2 — the select arms: the t+6 s search select opens (the
// G-522 flat-from-empty-query shape + the category-as-footer); a
// typed query (the degraded native filter's rows); esc →
// settle-undefined; the categorized select with NO search (the
// grouped shape) and the plain select (pre-tab enter submits the
// selected row) CHAIN off the previous phase's settle (the v21c
// slow-audit fix C — no absolute timers).
// ===========================================================================
#[test]
fn v21c_select_search_grouped_pair() {
    skip_without_ref_lane!();
    let mut rig = v21c_rig("v21c-sel", "v21c-sel");
    boot_both(&mut rig);
    // The search select opens flat (no category headers, the category
    // as the row footer).
    let open = rig.wait_both_screens("v21c search", 60);
    assert!(
        open,
        "V21C select RED: the search select never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "sel", "search-open");
    // A typed query: the filter rows (the search arm's degraded
    // native filter on the port vs the closure on the ref).
    rig.write_both(b"gam");
    settle(2);
    capture_pair(&rig, "sel", "search-typed");
    // Escape: the dialog closes; the promise settles undefined (the
    // G-519 select arm).
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "search-esc", "v21c sel search", 30);
    // The categorized select with NO search: the grouped shape.
    marker_state(&rig, "sel", "grouped-open", "v21c grouped", 90);
    // Escape it too (the grouped select's settle).
    rig.write_both(b"\x1b");
    marker_state(&rig, "sel", "grouped-esc", "v21c sel grouped", 30);
    // The plain select: pre-tab enter submits the selected row.
    marker_state(&rig, "sel", "plain-open", "v21c plain", 90);
    rig.write_both(b"\r");
    marker_state(&rig, "sel", "plain-submit", "v21c sel plain", 30);
    drop(rig);
}
