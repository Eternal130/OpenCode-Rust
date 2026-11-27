//! Lane va21 — the six G-475 paired anchors (the 修锚同 lane rule):
//! setup-shapes / storage-persistence / toast-session-action /
//! dialog-face / router-home-dup / tabs-model. Each = a paired
//! ref↔rust realchain scenario over a v2 `{id, setup}` TUI fixture
//! plugin staged into BOTH sandboxes' `<config>/plugins/` drop-in
//! directories (the REAL discovery path on both ends — ref
//! `localPluginDirectories` + `resolvePlugin`/`Host.resolve().tui`,
//! port `discover_directory_ops` + `resolve_one`/`probe_tui_entry`).
//!
//! W12 discipline: the tests EXPORT the settled grids + the paired
//! diff to `.test-logs/diff-reports/v21-<anchor>-<state>.{json,txt}`
//! and make NO rust-side content assertions (rig health + ref-side
//! sanity only); judgment belongs to the controller's adjudication
//! registry. The fixture plugins are self-contained `.js` (no bare
//! imports beyond `@opentui/solid` — the ref resolves it through its
//! runtime-plugin-support, the port through the host's shims).
//!
//! The fixture shape: `package.json` with `main: "server.js"` +
//! `exports["./tui"]` — the port resolves the server entry (a no-op
//! v2 server plugin) + probes the `./tui` companion (the v2 TUI
//! entry with NO `tui()` face → the G-475 classify arm →
//! `makeCtx` + `setup`); the ref resolves `tui` directly
//! (`Host.resolve({directory}).tui` → its real v2
//! `createPluginContext`).

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
            eprintln!("V21 SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;
const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";

struct V21Rig {
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

impl V21Rig {
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

    /// Rig-health polling screen wait (the same settle class as the
    /// pair lane's helper — the needle is merely observed; content
    /// judgment rides the export).
    fn wait_both_screens(
        &self,
        needle: &str,
        rust_timeout: Duration,
        ref_timeout: Duration,
    ) -> bool {
        // 2026-11-27 slow-audit fix A: wait BOTH ends CONCURRENTLY (the
        // serial legs doubled the wall-clock — the ref leg only started
        // after the rust leg's needle landed).
        std::thread::scope(|scope| {
            let rust_leg = scope.spawn(move || {
                let deadline = std::time::Instant::now() + rust_timeout;
                while std::time::Instant::now() < deadline {
                    if self.rust_tui.screen_text().contains(needle) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                self.rust_tui.screen_text().contains(needle)
            });
            let ref_leg = scope.spawn(move || {
                let deadline = std::time::Instant::now() + ref_timeout;
                while std::time::Instant::now() < deadline {
                    if self.ref_tui.screen_text().contains(needle) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                self.ref_tui.screen_text().contains(needle)
            });
            rust_leg.join().unwrap_or(false) && ref_leg.join().unwrap_or(false)
        })
    }
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// The `@opentui/solid` host-shim staged by the RIG (the same record
/// shapes `ensureOpentuiShims` writes guest-side — copied verbatim so
/// the port's flattener sees the identical element tree). Bun's
/// resolver caches the fixture dir's `node_modules` state at process
/// start: a mid-process shim write (the guest's own ensure pass) is
/// not always visible to the ESM linker when the load.tui frame
/// arrives on the supervisor's request-reply cadence — staging before
/// the TUI boots matches the production corpus shape (omo-slim's
/// payload carries its real deps pre-installed) and the guest-side
/// ensure pass stays as the best-effort fallback.
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

/// Stage one v21 fixture plugin into a sandbox's config drop-in
/// (`<config-dir>/plugins/<name>/`). The two ends' app names differ:
/// the ref's drop-in root is `$XDG_CONFIG_HOME/opencode` (v2's
/// `localPluginDirectories`), the port's is
/// `$XDG_CONFIG_HOME/opencode-rust` (oc-util's `APP_NAME`) — each side
/// stages into its OWN discovery root.
fn stage_fixture(sandbox: &Sandbox, app_dir: &str, name: &str, tui_js: &str, shims: bool) {
    let plugins = sandbox.root.join("xdg-config").join(app_dir).join("plugins");
    let dir = plugins.join(name);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\n  \"name\": \"{name}\",\n  \"version\": \"1.0.0\",\n  \"main\": \"server.js\",\n  \
             \"type\": \"module\",\n  \"exports\": {{ \"./tui\": \"./tui.js\" }}\n}}\n"
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
    // The port's plugin discovery anchors on the CONFIG FILE's parent
    // (`discover_config_files` → the drop-in roots): the config file
    // must EXIST at the TUI's own config root or the drop-in children
    // are never discovered. Mirror the sandbox's provider config.
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

/// The six fixtures. Each `tui.js` is the v2 `{id, setup}` contract —
/// on the ref it runs against the REAL `createPluginContext`; on the
/// port it classifies through the G-475 `load.tui` v2 arm into
/// `makeCtx`. All screen effects (toasts/sections/dialogs) ride the
/// long-duration toast (600s) so the settled capture sees them.
const FIXTURES: &[(&str, &str)] = &[
    // 1 — setup-shapes: the activation lands BOTH the toast and the
    // sidebar slot claim (the append face, api.tsx:298-312).
    (
        "v21-shapes",
        r#"import { createElement, insert } from "@opentui/solid";
export default {
  id: "v21.shapes.tui",
  setup(ctx) {
    ctx.ui.toast.show({
      title: "v21 shapes",
      message: "setup face ran",
      duration: 600000,
    });
    ctx.ui.slot({
      append: "sidebar.content",
      render() {
        const box = createElement("box");
        const text = createElement("text");
        insert(text, "v21 setup slot claim");
        insert(box, text);
        return box;
      },
    });
    return () => {};
  },
};
"#,
    ),
    // 2 — storage-persistence: the store arm's read-modify-write; the
    // restart leg proves the durable round-trip (the boot count
    // increments across TUI restarts on both ends).
    (
        "v21-storage",
        r#"export default {
  id: "v21.storage.tui",
  async setup(ctx) {
    const [state, update] = ctx.storage.store("boots", { initial: { count: 0 } });
    await update((draft) => {
      draft.count += 1;
    });
    ctx.ui.toast.show({
      title: "v21 storage",
      message: "boot count " + state.count,
      duration: 600000,
    });
    return () => {};
  },
};
"#,
    ),
    // 3 — toast-session-action: the sessionID arm — the fixture
    // CHAINS off the session's appearance (the rig's exchange creates
    // it; the v21b-toast waitSessionRoute poll pattern — va21g's
    // slow-audit addendum: the old t+18 s/t+20 s absolute timers were
    // dead dwell AND raced the exchange), then navigates HOME so the
    // toast targets a session whose family is NOT the open route —
    // the `Open` action affordance shows on both ends.
    (
        "v21-toast",
        r#"export default {
  id: "v21.toast.tui",
  async setup(ctx) {
    const listSessions = async () => {
      const raw =
        ctx.data && ctx.data.session && typeof ctx.data.session.list === "function"
          ? ctx.data.session.list()
          : await ctx.session.list();
      return Array.isArray(raw)
        ? raw
        : raw && Array.isArray(raw.data)
          ? raw.data
          : [];
    };
    const waitSession = async () => {
      for (let i = 0; i < 300; i++) {
        try {
          const target = (await listSessions()).find(
            (session) => session && typeof session.id === "string",
          );
          if (target) return target;
        } catch {}
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
      return undefined;
    };
    (async () => {
      const target = await waitSession();
      try {
        ctx.ui.router.navigate({ type: "home" });
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 4000));
      if (target) {
        ctx.ui.toast.show({
          sessionID: target.id,
          title: "v21 background",
          message: "not the open family",
          duration: 600000,
        });
      } else {
        ctx.ui.toast.show({
          title: "v21 toast",
          message: "no sessions",
          duration: 600000,
        });
      }
    })();
    return () => {};
  },
};
"#,
    ),
    // 4 — dialog-face: `dialog.show(render, onClose)` + `dialog.set`
    // (the presentation arm).
    (
        "v21-dialog",
        r#"import { createElement, insert } from "@opentui/solid";
export default {
  id: "v21.dialog.tui",
  setup(ctx) {
    setTimeout(() => {
      ctx.ui.dialog.show(() => {
        const box = createElement("box");
        const text = createElement("text");
        insert(text, "v21 dialog body");
        insert(box, text);
        return box;
      });
      ctx.ui.dialog.set({ size: "large" });
    }, 8000);
    return () => {};
  },
};
"#,
    ),
    // 5 — router-home-dup: the same-plugin duplicate route THROWS
    // (the G-475 bug1 fix — the ref registry semantics), and the
    // home navigation arm (bug2) rides the first message event.
    (
        "v21-router",
        r#"export default {
  id: "v21.router.tui",
  async setup(ctx) {
    let duplicate = "outer";
    try {
      const page = { name: "v21-route", render: () => null };
      const dispose = await ctx.ui.router.register(page);
      try {
        await ctx.ui.router.register(page);
        duplicate = "no-throw";
        dispose();
      } catch (error) {
        duplicate = String(error && error.message ? error.message : error);
      }
    } catch (error) {
      duplicate = "outer:" + String(error);
    }
    ctx.ui.toast.show({
      title: "v21 router dup",
      message: duplicate.slice(0, 44),
      duration: 600000,
    });
    const on = (type, handler) => {
      if (ctx.data && ctx.data.on) return ctx.data.on(type, handler);
      return ctx.event.subscribe(type, handler);
    };
    // bug2's arm — the home navigation rides the FIRST execution event
    // (v2's wire name: `session.execution.started`; `message.updated`
    // is the v1 legacy name that never fires on either end).
    let navigated = false;
    on("session.execution.started", () => {
      if (navigated) return;
      navigated = true;
      setTimeout(() => {
        ctx.ui.router.navigate({ type: "home" });
      }, 1500);
    });
    return () => {};
  },
};
"#,
    ),
    // 6 — tabs-model: the bounded-projection reads (enabled/list +
    // model.current) surface in the toast after the first exchange.
    (
        "v21-tabs-model",
        r#"export default {
  id: "v21.tabs.tui",
  setup(ctx) {
    setTimeout(() => {
      let tabs = [];
      try {
        tabs = ctx.ui.tabs.list() || [];
      } catch {}
      let modelText = "none";
      try {
        const model = ctx.ui.model.current();
        if (model) modelText = model.providerID + "/" + model.modelID;
      } catch (error) {
      }
      ctx.ui.toast.show({
        title: "v21 tabs+model",
        message: "tabs=" + tabs.length + " model=" + modelText,
        duration: 600000,
      });
    }, 9000);
    return () => {};
  },
};
"#,
    ),
];

/// The rig with ONE fixture staged (the single-plugin confounder: a
/// face's diff cannot bleed from another fixture's effects).
fn v21_rig(tag: &str, fixture: &str) -> V21Rig {
    v21_rig_cols(tag, fixture, COLS)
}

/// [`v21_rig`] with an explicit terminal width (the shapes anchor
/// runs 170 cols so the session sidebar docks — the ref's
/// `width - verticalTabsWidth(42) > 120` gate — and the slot claim
/// renders).
fn v21_rig_cols(tag: &str, fixture: &str, cols: u16) -> V21Rig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    let tui_js = FIXTURES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or_else(|| panic!("unknown v21 fixture {fixture}"), |(_, js)| *js);
    // The port side stages the @opentui/solid shim (Bun's resolver
    // caches the node_modules state at process start); the ref
    // resolves @opentui/solid through its runtime-plugin-support.
    stage_fixture(&rust, "opencode-rust", fixture, tui_js, true);
    stage_fixture(&refbox, "opencode", fixture, tui_js, false);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, cols, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, cols, ROWS);
    V21Rig {
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

impl Drop for V21Rig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V21Rig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V21 precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

/// One exchange: submit "hello", wait for the replay needle on both.
fn exchange(rig: &mut V21Rig) {
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_ok = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_ok = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    assert!(rust_ok && ref_ok, "V21 exchange RED: rust={rust_ok} ref={ref_ok}");
}

fn capture_pair(rig: &V21Rig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v21-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v21-{scenario}"), state, &ref_grid, &rust_grid);
}

// ===========================================================================
// Anchor 1 — setup-shapes: the v2 `{id, setup}` TUI plugin activates
// on BOTH ends (the port's load.tui classify arm, the ref's own
// resolvePlugin) and its setup lands the toast + the sidebar slot
// claim. The 170-col rig docks the session sidebar (the ref's
// `width - verticalTabsWidth > 120` gate) so the `sidebar.content`
// slot claim renders; the exchange enters the session route that
// mounts it. Export-only: the settled grids + diff carry the faces.
// ===========================================================================
#[test]
fn v21_setup_shapes_pair() {
    skip_without_ref_lane!();
    let mut rig = v21_rig_cols("v21-shapes", "v21-shapes", 170);
    boot_both(&mut rig);
    // The toast rides a 600 s duration: it stays on screen through
    // the settled capture.
    settle(6);
    exchange(&mut rig);
    settle(6);
    capture_pair(&rig, "shapes", "booted");
    drop(rig);
}

// ===========================================================================
// Anchor 2 — storage-persistence: the `ctx.storage.store` arm's
// read-modify-write (boot count 1), then the TUIs RESTART and the
// second boot reads the durable value (boot count 2). The paired
// export carries both states.
// ===========================================================================
#[test]
fn v21_storage_persistence_pair() {
    skip_without_ref_lane!();
    let mut rig = v21_rig("v21-storage", "v21-storage");
    boot_both(&mut rig);
    settle(8);
    capture_pair(&rig, "storage", "first-boot");
    // Restart the TUIs (the servers stay; the state roots persist).
    rig.rust_tui.shutdown(Duration::from_secs(10));
    rig.ref_tui.shutdown(Duration::from_secs(10));
    let stage = ref_stage(&rig.refbox);
    let rust_tui = TuiProcess::spawn(&rig.rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&rig.refbox, &stage, COLS, ROWS);
    rig.rust_tui = rust_tui;
    rig.ref_tui = ref_tui;
    boot_both(&mut rig);
    settle(8);
    capture_pair(&rig, "storage", "restart");
    drop(rig);
}

// ===========================================================================
// Anchor 3 — toast-session-action: one exchange creates a session; the
// fixture navigates home (t+18 s, the router home arm) and toasts that
// session's id (t+20 s) while home holds — the sessionID arm's `Open`
// action affordance renders on both ends. Rig health: condition-wait
// the toast's face on each end (the affordance judgment rides the
// export).
// ===========================================================================
#[test]
fn v21_toast_session_action_pair() {
    skip_without_ref_lane!();
    let mut rig = v21_rig("v21-toast", "v21-toast");
    boot_both(&mut rig);
    exchange(&mut rig);
    let toasts = rig.wait_both_screens(
        "v21 background",
        // Rig health windows are generous: under the full-crate gate
        // the rigs share the box with the plugin-host bun-spawning
        // families, and a starved guest's crash/restart reload lands
        // the plugin (and its t+20 s timer) well after the exchange.
        Duration::from_secs(120),
        Duration::from_secs(120),
    );
    assert!(
        toasts,
        "V21 toast RED: the sessionID toast never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(2);
    capture_pair(&rig, "toast", "session-action");
    drop(rig);
}

// ===========================================================================
// Anchor 4 — dialog-face: the fixture's timer opens
// `dialog.show(render, onClose)` + `dialog.set({size})` at t+8 s; the
// settled capture carries the open dialog on both ends.
// ===========================================================================
#[test]
fn v21_dialog_face_pair() {
    skip_without_ref_lane!();
    let mut rig = v21_rig("v21-dialog", "v21-dialog");
    boot_both(&mut rig);
    settle(12);
    capture_pair(&rig, "dialog", "open");
    drop(rig);
}

// ===========================================================================
// Anchor 5 — router-home-dup: the same-plugin duplicate registration
// THROWS on both ends (G-475 bug1 — the port silently overwrote
// before; the ref's `Route already registered` rides the fixture's
// toast), and the home navigation (bug2) returns both TUIs home
// after the first exchange.
// ===========================================================================
#[test]
fn v21_router_home_dup_pair() {
    skip_without_ref_lane!();
    let mut rig = v21_rig("v21-router", "v21-router");
    boot_both(&mut rig);
    settle(8);
    capture_pair(&rig, "router", "dup-toast");
    // The exchange enters the session route; the fixture's
    // message.updated handler navigates home 1.5 s later (bug2's arm).
    exchange(&mut rig);
    settle(8);
    capture_pair(&rig, "router", "navigated-home");
    drop(rig);
}

// ===========================================================================
// Anchor 6 — tabs-model: the bounded-projection reads
// (`ctx.ui.tabs.list()` + `ctx.ui.model.current()`) surface in the
// fixture's t+9 s toast after the first exchange opened a tab.
// ===========================================================================
#[test]
fn v21_tabs_model_pair() {
    skip_without_ref_lane!();
    let mut rig = v21_rig("v21-tabs-model", "v21-tabs-model");
    boot_both(&mut rig);
    exchange(&mut rig);
    settle(10);
    capture_pair(&rig, "tabs-model", "after-exchange");
    drop(rig);
}
