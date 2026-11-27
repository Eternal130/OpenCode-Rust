//! Lane va21b — the G-505..G-513 paired anchors (the 修锚同 lane rule):
//! the R24-flagged un-anchored states of the plugin TUI API surface —
//! dialog.select (promise settle arms, current preselect, actions with
//! bind-less/disabled/hidden, triggering-does-not-close),
//! dialog.show's close/replacement lifecycle (esc / onClose / clear /
//! show#2-while-#1-open), the toast session-title fallback + the
//! same-family plain arm, the tabs member values + move/close return
//! values, the model variant.list/set validation returns, and
//! router.current's ref Route shape (home/session/plugin arms).
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21b-<anchor>-<state>.{json,txt}` and make
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication registry.
//! The fixtures are self-contained `.js` v2 `{id, setup}` TUI plugins
//! staged into BOTH sandboxes' real drop-in discovery roots (the same
//! staging contract `realchain_v21.rs` documents).
//!
//! PACING: the two ends boot at different speeds (the ref's bun boot
//! trails the rust binary by tens of seconds under the gate), and each
//! fixture's timers run from ITS OWN activation — so every state
//! transition that the rig must capture fires a TOAST marker (the
//! needle waits complete at the SLOWER end) and the inter-transition
//! gaps (45 s) absorb the boot skew: the capture window (~2-4 s after
//! the needle) always lands inside the same state on both ends.

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
            eprintln!("V21B SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;
const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";

struct V21BRig {
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

impl V21BRig {
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

/// The `@opentui/solid` host shim staged by the RIG (the same record
/// shapes `realchain_v21.rs` stages — see its notes on Bun's resolver
/// caching; `setProp` joins the pair for the show fixture's layout
/// cues).
fn stage_opentui_shim(fixture: &std::path::Path) {
    let solid = fixture.join("node_modules").join("@opentui").join("solid");
    std::fs::create_dir_all(&solid).expect("shim dir");
    std::fs::write(
        solid.join("package.json"),
        r#"{"name":"@opentui/solid","version":"0.0.0-host-shim","type":"module","main":"index.js"}"#,
    )
    .expect("shim package.json");
    std::fs::write(
        solid.join("index.js"),
        r##"// Host shim (staged by the rig): element records + an EAGER insert.
export function createElement(tag) {
  return { tag, props: {}, children: [] };
}
export function setProp(node, key, value) {
  node.props[key] = value;
}
export function insert(parent, child) {
  const mount = (value) => {
    if (value === null || value === undefined || value === false || value === true) return;
    if (Array.isArray(value)) { for (const item of value) mount(item); return; }
    if (typeof value === "function") { try { mount(value()); } catch { /* contained */ } return; }
    if (typeof value === "string" || typeof value === "number") {
      parent.children.push({ tag: "#text", props: { text: String(value) }, children: [] });
      return;
    }
    if (typeof value === "object" && value.tag !== undefined) { parent.children.push(value); return; }
  };
  mount(child);
}
"##,
    )
    .expect("shim index.js");
}

/// Stage one v21b fixture plugin into a sandbox's config drop-in (the
/// same staging contract as `realchain_v21.rs` — each side stages into
/// its OWN discovery root).
fn stage_fixture(sandbox: &Sandbox, app_dir: &str, name: &str, tui_js: &str, shims: bool) {
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
            "export default {{\n  id: \"{name}\",\n  setup() {{\n    return () => {{}};\n  }},\n}};\n"
        ),
    )
    .expect("server.js");
    std::fs::write(dir.join("tui.js"), tui_js).expect("tui.js");
    if shims {
        stage_opentui_shim(&dir);
    }
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

// The five fixtures. Session-dependent arms poll for the session's
// existence instead of racing fixed timers (the exchange's latency
// varies under the gate); every rig-captured transition fires a toast
// marker and the 45 s inter-transition gaps absorb the boot skew.
const FIXTURES: &[(&str, &str)] = &[
    // 1 — dialog.select: the current preselect ("beta"), a disabled
    // option, and the actions arm (bind-less + bound + hidden). The
    // promise settle rides a toast (submit → the value; any close →
    // undefined). The actions fire guest-side without closing.
    (
        "v21b-select",
        r#"export default {
  id: "v21b.select.tui",
  setup(ctx) {
    const log = [];
    const report = (tag) =>
      ctx.ui.toast.show({
        title: "v21b select " + tag,
        message: log.join(","),
        duration: 600000,
      });
    setTimeout(() => {
      ctx.ui.dialog
        .select({
          title: "v21b select",
          options: [
            { title: "Alpha", value: "alpha" },
            { title: "Beta", value: "beta" },
            { title: "Disabled", value: "disabled", disabled: true },
          ],
          current: "beta",
          actions: [
            {
              title: "BindlessAct",
              onTrigger: () => {
                log.push("bindless-fired");
                report("action");
              },
            },
            {
              title: "HiddenAct",
              bind: "ctrl+g",
              hidden: true,
              onTrigger: () => {
                log.push("hidden-fired");
                report("action");
              },
            },
            {
              title: "BoundAct",
              bind: "ctrl+b",
              onTrigger: (value) => {
                log.push("bound-fired:" + value);
                report("action");
              },
            },
          ],
        })
        .then(
          (value) => {
            log.push("submit:" + value);
            report("settled");
          },
          () => {
            log.push("rejected");
            report("settled");
          },
        );
    }, 6000);
    return () => {};
  },
};
"#,
    ),
    // 2 — dialog.show lifecycle: show#1 (onClose → toast), show#2
    // while #1 is open (the replace fires #1's onClose), set large,
    // set centered-only (the absent size resets to medium), clear
    // (#2's onClose). The body carries the G-510 shapes: a
    // space-between header row, a blank row, and a long wrapping row.
    // Every transition fires its toast marker (the rig's needles).
    (
        "v21b-show",
        r#"import { createElement, insert, setProp } from "@opentui/solid";
export default {
  id: "v21b.show.tui",
  setup(ctx) {
    const log = [];
    const report = (tag) =>
      ctx.ui.toast.show({
        title: "v21b show " + tag,
        message: log.join(","),
        duration: 600000,
      });
    const body = (mark, long) => () => {
      const box = createElement("box");
      const header = createElement("box");
      setProp(header, "flexDirection", "row");
      setProp(header, "justifyContent", "space-between");
      const left = createElement("text");
      insert(left, "v21b " + mark + " left");
      const right = createElement("text");
      insert(right, "right");
      insert(header, left);
      insert(header, right);
      const blank = createElement("text");
      insert(blank, " ");
      const text = createElement("text");
      insert(
        text,
        long
          ? "v21b " + mark + " long row " + "word ".repeat(28) + "tail"
          : "v21b " + mark + " body",
      );
      insert(box, header);
      insert(box, blank);
      insert(box, text);
      return box;
    };
    setTimeout(() => {
      ctx.ui.dialog.show(body("one", true), () => {
        log.push("close1");
        report("close1");
      });
    }, 6000);
    setTimeout(() => {
      ctx.ui.dialog.show(body("two", false), () => {
        log.push("close2");
        report("close2");
      });
    }, 51000);
    setTimeout(() => {
      ctx.ui.dialog.set({ size: "large" });
      report("large");
    }, 96000);
    setTimeout(() => {
      ctx.ui.dialog.set({ centered: true });
      report("centered");
    }, 141000);
    setTimeout(() => {
      ctx.ui.dialog.clear();
    }, 186000);
    return () => {};
  },
};
"#,
    ),
    // 3 — toast arms: the same-family plain toast (no title, the open
    // session's family — polled until the session exists), then the
    // home navigation and the background-family toast (the title
    // falls back to the session's title and the Open action shows).
    (
        "v21b-toast",
        r#"export default {
  id: "v21b.toast.tui",
  async setup(ctx) {
    const waitSessionRoute = async () => {
      // The plain (same-family) arm needs the session ROUTE open —
      // the toast reducer decides plain-vs-action from the live
      // route, and the session's list appearance can beat the
      // navigation by a beat (a race, not a divergence).
      for (let i = 0; i < 60; i++) {
        try {
          const raw =
            ctx.data && ctx.data.session && typeof ctx.data.session.list === "function"
              ? ctx.data.session.list()
              : await ctx.session.list();
          const sessions = Array.isArray(raw)
            ? raw
            : raw && Array.isArray(raw.data)
              ? raw.data
              : [];
          const target = sessions.find((session) => session && typeof session.id === "string");
          const route = ctx.ui.router.current();
          if (target && route && route.type === "session") return target;
        } catch {}
        await new Promise((resolve) => setTimeout(resolve, 1500));
      }
      return null;
    };
    (async () => {
      const target = await waitSessionRoute();
      if (!target) {
        ctx.ui.toast.show({ title: "v21b toast", message: "no sessions", duration: 600000 });
        return;
      }
      ctx.ui.toast.show({
        sessionID: target.id,
        message: "v21b plain same-family",
        duration: 600000,
      });
      await new Promise((resolve) => setTimeout(resolve, 40000));
      try {
        ctx.ui.router.navigate({ type: "home" });
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 5000));
      ctx.ui.toast.show({
        sessionID: target.id,
        message: "v21b background fallback",
        duration: 600000,
      });
    })();
    return () => {};
  },
};
"#,
    ),
    // 4 — tabs + model: after the session exists (polled), the list
    // row's member values (attention busy active promptPulse
    // renaming), the move/close membership returns (unknown id →
    // false), the variant.list/set validation returns; then the home
    // navigation and the close() with NO active tab.
    (
        "v21b-tabs-model",
        r#"export default {
  id: "v21b.tabs.tui",
  async setup(ctx) {
    const waitSession = async () => {
      for (let i = 0; i < 60; i++) {
        try {
          const raw =
            ctx.data && ctx.data.session && typeof ctx.data.session.list === "function"
              ? ctx.data.session.list()
              : await ctx.session.list();
          const sessions = Array.isArray(raw)
            ? raw
            : raw && Array.isArray(raw.data)
              ? raw.data
              : [];
          const target = sessions.find((session) => session && typeof session.id === "string");
          if (target) return target;
        } catch {}
        await new Promise((resolve) => setTimeout(resolve, 1500));
      }
      return null;
    };
    (async () => {
      await waitSession();
      await new Promise((resolve) => setTimeout(resolve, 2000));
      const parts = [];
      try {
        const tabs = ctx.ui.tabs.list() || [];
        const row = tabs[0] || {};
        parts.push(
          "tabs=" + tabs.length +
            " attention=" + JSON.stringify(row.attention) +
            " busy=" + JSON.stringify(row.busy) +
            " active=" + JSON.stringify(row.active) +
            " hasPulse=" + ("promptPulse" in row) +
            " hasRenaming=" + ("renaming" in row),
        );
      } catch {
        parts.push("tabs-err");
      }
      try {
        parts.push("moveUnknown=" + ctx.ui.tabs.move("ses_unknown", 0));
      } catch {
        parts.push("move-err");
      }
      try {
        parts.push("closeUnknown=" + ctx.ui.tabs.close("ses_unknown"));
      } catch {
        parts.push("close-err");
      }
      try {
        const variants = ctx.ui.model.variant.list();
        parts.push("variants=" + JSON.stringify(variants));
        parts.push("setBad=" + ctx.ui.model.variant.set("no-such-variant"));
        parts.push("setNone=" + ctx.ui.model.variant.set(undefined));
      } catch {
        parts.push("model-err");
      }
      ctx.ui.toast.show({
        title: "v21b tabs+model",
        message: parts.join(" ; "),
        duration: 600000,
      });
      await new Promise((resolve) => setTimeout(resolve, 40000));
      try {
        ctx.ui.router.navigate({ type: "home" });
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 5000));
      let closed = "err";
      try {
        closed = String(ctx.ui.tabs.close());
      } catch (error) {
        closed = "err";
      }
      ctx.ui.toast.show({
        title: "v21b close-noactive",
        message: "close=" + closed,
        duration: 600000,
      });
    })();
    return () => {};
  },
};
"#,
    ),
    // 5 — router.current shape: the session arm once the session
    // exists (polled), then the plugin arm after navigating to the
    // plugin's own registered route.
    (
        "v21b-router",
        r#"export default {
  id: "v21b.router.tui",
  async setup(ctx) {
    const shape = (route) =>
      JSON.stringify({
        type: route && route.type,
        hasSessionID: !!(route && "sessionID" in route),
        hasParams: !!(route && "params" in route),
        name: route && route.name,
        id: route && route.id,
        hasData: !!(route && "data" in route),
      });
    const waitSession = async () => {
      for (let i = 0; i < 60; i++) {
        try {
          const raw =
            ctx.data && ctx.data.session && typeof ctx.data.session.list === "function"
              ? ctx.data.session.list()
              : await ctx.session.list();
          const sessions = Array.isArray(raw)
            ? raw
            : raw && Array.isArray(raw.data)
              ? raw.data
              : [];
          const target = sessions.find((session) => session && typeof session.id === "string");
          if (target) return target;
        } catch {}
        await new Promise((resolve) => setTimeout(resolve, 1500));
      }
      return null;
    };
    try {
      const dispose = await ctx.ui.router.register({
        name: "v21b-route",
        render: () => null,
      });
      void dispose;
    } catch {}
    (async () => {
      await waitSession();
      await new Promise((resolve) => setTimeout(resolve, 3000));
      let current = "err";
      try {
        current = shape(ctx.ui.router.current());
      } catch {}
      ctx.ui.toast.show({
        title: "v21b router session",
        message: current,
        duration: 600000,
      });
      await new Promise((resolve) => setTimeout(resolve, 40000));
      try {
        ctx.ui.router.navigate({ type: "plugin", name: "v21b-route", data: { k: 1 } });
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 4000));
      let pluginCurrent = "err";
      try {
        pluginCurrent = shape(ctx.ui.router.current());
      } catch {}
      ctx.ui.toast.show({
        title: "v21b router plugin",
        message: pluginCurrent,
        duration: 600000,
      });
    })();
    return () => {};
  },
};
"#,
    ),
];

/// The rig with ONE fixture staged (the single-plugin confounder).
fn v21b_rig(tag: &str, fixture: &str) -> V21BRig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    let tui_js = FIXTURES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or_else(|| panic!("unknown v21b fixture {fixture}"), |(_, js)| *js);
    let needs_shim = tui_js.contains("\"@opentui/solid\"");
    stage_fixture(&rust, "opencode-rust", fixture, tui_js, needs_shim);
    stage_fixture(&refbox, "opencode", fixture, tui_js, false);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V21BRig {
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

impl Drop for V21BRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V21BRig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21B precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

/// One exchange: submit "hello", wait for the replay needle on both.
fn exchange(rig: &mut V21BRig) {
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_ok = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_ok = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    assert!(rust_ok && ref_ok, "V21B exchange RED: rust={rust_ok} ref={ref_ok}");
}

fn capture_pair(rig: &V21BRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21b-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21b-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21b-{scenario}"), state, &ref_grid, &rust_grid);
}

/// Needle + capture: the toast marker completes at the SLOWER end;
/// the capture rides ~1 s behind (inside the 45 s state window on
/// both ends).
fn marker_state(rig: &V21BRig, scenario: &str, state: &str, needle: &str, window: u64) {
    let ok = rig.wait_both_screens(needle, window);
    assert!(
        ok,
        "V21B {scenario} RED: the {state} marker ({needle}) never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(rig, scenario, state);
}

// ===========================================================================
// Anchor 1 — dialog.select: the t+6 s select opens (current preselect
// "beta", the disabled option, the actions footer); tab moves the
// footer focus; enter with a focused action TRIGGERS it without
// closing; esc closes and the promise settles. The open-state needle
// is rig health; the post-key arms ride the export.
// ===========================================================================
#[test]
fn v21b_select_arms_pair() {
    skip_without_ref_lane!();
    let mut rig = v21b_rig("v21b-select", "v21b-select");
    boot_both(&mut rig);
    let open = rig.wait_both_screens("v21b select", 60);
    assert!(
        open,
        "V21B select RED: the select dialog never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "select", "open");
    // Tab: the footer-action focus cycle (the ref's actionItems skips
    // the bind-less entry; the footer row set rides the export).
    rig.write_both(b"\t");
    settle(1);
    capture_pair(&rig, "select", "tab");
    // Enter with a focused action: the action triggers (the guest
    // toast) and the dialog does NOT close.
    rig.write_both(b"\r");
    settle(2);
    capture_pair(&rig, "select", "action");
    // Escape: the dialog closes; the promise settle rides the export.
    rig.write_both(b"\x1b");
    settle(2);
    capture_pair(&rig, "select", "esc");
    drop(rig);
}

// ===========================================================================
// Anchor 2 — dialog.show lifecycle: show#1 opens at t+6 s (the blank
// row + the space-between header + the wrapping long row — G-510's
// sub-semantics); show#2 replaces it at t+51 s (#1's onClose fires —
// the close1 toast); set({size:"large"}) at t+96 s (the large toast);
// set({centered:true}) at t+141 s (the ABSENT size resets to medium —
// G-508; the centered toast); clear() at t+186 s (#2's onClose — the
// close2 toast). Every transition is toast-marked; the needle waits
// absorb the boot skew.
// ===========================================================================
#[test]
fn v21b_show_lifecycle_pair() {
    skip_without_ref_lane!();
    let mut rig = v21b_rig("v21b-show", "v21b-show");
    boot_both(&mut rig);
    marker_state(&rig, "show", "one-open", "v21b one left", 60);
    marker_state(&rig, "show", "two-replaced", "v21b show close1", 90);
    marker_state(&rig, "show", "set-large", "v21b show large", 90);
    marker_state(&rig, "show", "set-centered-reset", "v21b show centered", 90);
    marker_state(&rig, "show", "cleared", "v21b show close2", 90);
    drop(rig);
}

// ===========================================================================
// Anchor 3 — toast arms: the same-family plain toast (no title —
// polled after the exchange), the home navigation (40 s later), and
// the background-family toast (5 s after that — the title falls back
// to the session's title + the Open action).
// ===========================================================================
#[test]
fn v21b_toast_session_fallback_pair() {
    skip_without_ref_lane!();
    let mut rig = v21b_rig("v21b-toast", "v21b-toast");
    boot_both(&mut rig);
    exchange(&mut rig);
    marker_state(&rig, "toast", "plain-arm", "v21b plain same-family", 150);
    marker_state(&rig, "toast", "fallback-arm", "v21b background fallback", 120);
    drop(rig);
}

// ===========================================================================
// Anchor 4 — tabs + model: after the exchange, the tab row's members +
// the move/close membership returns + the variant validation returns
// (the values toast), then the home navigation (40 s later) and the
// close() with NO active tab (the close-noactive toast).
// ===========================================================================
#[test]
fn v21b_tabs_model_values_pair() {
    skip_without_ref_lane!();
    let mut rig = v21b_rig("v21b-tabs-model", "v21b-tabs-model");
    boot_both(&mut rig);
    exchange(&mut rig);
    marker_state(&rig, "tabs-model", "values", "v21b tabs+model", 150);
    marker_state(&rig, "tabs-model", "close-noactive", "v21b close-noactive", 120);
    drop(rig);
}

// ===========================================================================
// Anchor 5 — router.current shape: the session arm once the session
// exists (after the exchange), then the plugin route navigation
// (40 s later) + the plugin arm's toast (4 s after that).
// ===========================================================================
#[test]
fn v21b_router_current_shape_pair() {
    skip_without_ref_lane!();
    let mut rig = v21b_rig("v21b-router", "v21b-router");
    boot_both(&mut rig);
    exchange(&mut rig);
    marker_state(&rig, "router", "session-arm", "v21b router session", 150);
    marker_state(&rig, "router", "plugin-arm", "v21b router plugin", 120);
    drop(rig);
}
