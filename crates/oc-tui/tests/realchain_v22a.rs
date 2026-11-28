//! Lane va22a — the paired export-only anchors (the 修锚同 lane rule)
//! for the wave's eight register entries:
//!
//! * `v22a-panel-keys` — G-498: the mini footer's searchable-panel
//!   controller key set (pageup/pagedown/home/end/ctrl+n/ctrl+p-while-
//!   open/bare ctrl+u), driven keystroke states on a boot mini pair.
//!   This rig is ALSO the lane's ref-side PROBE: the register called
//!   ctrl+p/ctrl+u "need a ref-side probe" — the exported REF grids
//!   at the post-key states ARE the probe record (the ref's actual
//!   arm behavior, source-predicted: the `command.palette.show`
//!   layer is composer-route-only, footer.view.tsx:561-577, so
//!   ctrl+p-while-open falls to the panel's own `menu.move(-1)`
//!   handler, footer.command.tsx:209-213; the queued panel's
//!   priority-1 layer owns ctrl+u as `queued_prompt.undo`,
//!   footer.command.tsx:975-999).
//! * `v22a-reconnecting` — G-500: both ends' servers stop, the
//!   settled loss box exports (the Yoga half-up centering — the box
//!   one row lower than the pre-fix floor).
//! * `v22a-marquee` / `v22a-marquee-off` — G-501/G-502: a long-titled
//!   tab hovered on the strip (the scroll window with the loop `·`
//!   separator, tinted 0.55 toward the tab background,
//!   session-tabs.tsx:1617-1629), plus the animations-OFF config
//!   state (the leading fade JUMPS instantly, animation.ts:71-74/90).
//! * `v22a-toast-long` — G-517: a long single-token JSON toast (the
//!   word-wrap's punctuation break set — the opentui layout-wrap seam,
//!   text_buffer.rs ascii_break_kind).
//! * `v22a-show-set-large` — G-518: `dialog.show` + `set({size:
//!   "large"})` (the live-children re-flow at the 88-wide box).
//! * `v22a-prompt-preview` — G-504/G-500: the paste-twice dedup
//!   agreement face + the broken-image "No preview" fallback (the
//!   half-up centering leg).
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the
//! settled grids + the paired diff to
//! `.test-logs/diff-reports/v22a-<scenario>-<state>.{json,txt}` and
//! make NO rust-side content assertions (rig health + ref-side
//! sanity only); judgment belongs to the controller's adjudication
//! registry. The fixtures are self-contained `.js` v2 `{id, setup}`
//! TUI plugins staged into BOTH sandboxes' real drop-in discovery
//! roots (the staging contract `realchain_v21b.rs` documents).
//!
//! PACING: the two ends boot at different speeds; chained fixture
//! phases arm off the previous phase's promise/marker with ≥20 s
//! arms (the 2026-11-27 va21g SOP — shorter arms tore the captures).
//!
//! ANCHOR-SCOPE NOTES (the honest reachability ledger):
//! * The G-501 easing curves and the G-503 envelope lifecycle arms
//!   are frame-level animation faces — pinned at unit level with the
//!   fixes themselves (marquee.rs / tab_pulse.rs / lifecycle.rs
//!   tests, the va20 precedent); the realchain anchors capture the
//!   OBSERVABLE settled/structural states of the same surfaces.
//! * The G-504 "same name + DIFFERENT description" identity case has
//!   NO TUI-reachable driver on either end (the wire
//!   `PromptFile.description` enters only through message
//!   projections, never the composer — verified against
//!   prompt/attachment.ts + the ref's paste paths); the port-side
//!   unit test carries the description fixture (store.rs), and this
//!   anchor covers the REACHABLE face (paste-twice: same name +
//!   mention + uri — both engines dedup identically).

#![allow(
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    capture_settled, default_pace, dump_grid, export_grid_diff, lane_lock, normalize_grid,
    ref_lane_skip_reason, ref_mini_spawn, ref_service_stop, ref_stage, ref_tui_spawn, sandbox,
    service_start, service_stop, Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::Duration;

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("V22A SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;
/// The marquee rig's narrow width: one long-titled tab whose title
/// overflows the slot (the hover scroll's gate).
const NARROW_COLS: u16 = 60;

const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";

struct V22ARig {
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

impl V22ARig {
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

/// This WORKTREE's root (no main-checkout redirect: the record seeds
/// are lane-local — the same shape `realchain_v21f.rs` documents).
fn wt_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

/// The full-TUI pair rig at `cols` (the v21b topology: paced replay
/// gateway, both sandboxes). `record` seeds unmatched turn digests
/// with the canned REALCHAIN-REPLAY-OK reply (custom prompts have no
/// corpus cassette — strict mode errors them); `patch` merges into
/// BOTH ends' TUI configs BEFORE the TUIs spawn (the config is read
/// at boot).
fn v22a_rig(
    tag: &str,
    cols: u16,
    record: Option<&std::path::Path>,
    patch: Option<&serde_json::Value>,
) -> V22ARig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, record, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    if let Some(patch) = patch {
        // The TUI-level keys (animations et al.) live in each side's
        // OWN cli.json — the ref reads `global.config/cli.json`
        // (cli/src/config/config.ts:31, XDG_CONFIG_HOME/opencode) and
        // the port reads `roots.config/cli.json` (TuiConfig::load →
        // CliConfig, XDG_CONFIG_HOME/opencode-rust, FILENAME
        // "cli.json"). The `opencode.json` files are the SERVERS'
        // configs (write_config/write_ref_config own them) — the
        // first round patched those and neither TUI saw the key.
        patch_tui_config(&rust.root.join("xdg-config/opencode-rust/cli.json"), patch);
        patch_tui_config(&refbox.root.join("xdg-config/opencode/cli.json"), patch);
    }
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, cols, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, cols, ROWS);
    V22ARig {
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

impl Drop for V22ARig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &V22ARig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V22A precondition RED: rust settled={rust_settled}, ref settled={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

fn capture_pair(rig: &V22ARig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid(&format!("v22a-{scenario}-{state}-ref"), &ref_grid);
    dump_grid(&format!("v22a-{scenario}-{state}-rust"), &rust_grid);
    export_grid_diff(&format!("v22a-{scenario}"), state, &ref_grid, &rust_grid);
}

/// Patch a JSON key into one end's TUI config (read + merge + write).
fn patch_tui_config(path: &std::path::Path, merge: &serde_json::Value) {
    let mut config: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let (Some(base), Some(extra)) = (config.as_object_mut(), merge.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(&config).unwrap()).expect("patch config");
}

/// The `@opentui/solid` host shim staged by the RIG (the same record
/// shapes `realchain_v21b.rs` stages — see its notes on Bun's resolver
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

/// Stage one v22a fixture plugin into a sandbox's config drop-in (the
/// same staging contract as `realchain_v21b.rs` — each side stages
/// into its OWN discovery root). `shims` stages the host shim — the
/// RUST side only: the ref's runtime INJECTS the real
/// `@opentui/solid` (runtime-plugin-support.bun.ts:4-8), so a
/// fixture-local node_modules copy would shadow it there.
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
            "export default {{\n  id: \"{name}\",\n  setup() {{\n    return () => {{}};\n  }},\n}}\n"
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

/// An SGR left-click (press + release) at a grid cell — the strip
/// hover driver (the pointer stays at the cell after the release, so
/// the hovered tab keeps the marquee armed).
fn sgr_click(tui: &mut TuiProcess, x: usize, y: usize) {
    tui.write(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
    std::thread::sleep(Duration::from_millis(120));
    tui.write(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
}

// ===========================================================================
// Anchor 1 — G-498 + the ref-side probe: the mini footer's
// searchable-panel controller key set. The mini pair boots, ctrl+p
// opens the command palette, and the driven states walk the key set:
// down×6 (the selection), End/Home (reveal ∞/0), ctrl+n (move +1),
// pageup (the page reveal), ctrl+p-while-open (the panel's own move
// up — the PROBE arm), a typed query, and bare ctrl+u (the query
// clear — the PROBE arm). The exported REF grids are the probe
// record; the port grids are the fixed faces.
// ===========================================================================
#[test]
fn v22a_panel_keys_pair() {
    skip_without_ref_lane!();
    let tag = "v22a-keys";
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini");
    rust_command.env("PWD", rust.project.display().to_string());
    let rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let ref_tui = ref_mini_spawn(&refbox, &stage, COLS, ROWS);
    let mut rig = V22ARig {
        _lane: lane,
        _gateway: gateway,
        rust,
        refbox,
        stage,
        _rust_server_url: server.url,
        _rust_password: server.password,
        rust_tui,
        ref_tui,
    };
    let rust_settled = rig.rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "V22A panel-keys RED: boot rust={rust_settled} ref={ref_settled}; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
    // ctrl+p opens the command palette (the composer-route layer).
    rig.write_both(b"\x10");
    let open = rig.wait_both_screens("Commands", 20);
    assert!(
        open,
        "V22A panel-keys RED: the palette never opened; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(&rig, "keys", "palette-open");
    // down×6: the selection walks the list.
    for _ in 0..6 {
        rig.write_both(b"\x1b[B");
        settle(1);
    }
    capture_pair(&rig, "keys", "down6");
    // End: reveal(∞) — the LAST row.
    rig.write_both(b"\x1b[F");
    settle(1);
    capture_pair(&rig, "keys", "end");
    // Home: reveal(0) — the first row.
    rig.write_both(b"\x1b[H");
    settle(1);
    capture_pair(&rig, "keys", "home");
    // ctrl+n: move(+1) — the same arm as down.
    rig.write_both(b"\x0e");
    settle(1);
    capture_pair(&rig, "keys", "ctrl-n");
    // pageup: reveal(selected − max(1, limit−1)).
    rig.write_both(b"\x1b[5~");
    settle(1);
    capture_pair(&rig, "keys", "pageup");
    // ctrl+p-while-open: the PROBE arm — the ref's palette layer is
    // composer-route-only (footer.view.tsx:561-577), so the chord
    // falls to the panel's own useKeyboard → menu.move(−1)
    // (footer.command.tsx:209-213). The exported ref grid records
    // the actual behavior.
    rig.write_both(b"\x10");
    settle(1);
    capture_pair(&rig, "keys", "ctrl-p-while-open");
    // A typed query filters the list.
    rig.write_both(b"sett");
    settle(1);
    capture_pair(&rig, "keys", "query");
    // Bare ctrl+u: the PROBE arm — the query clears
    // (footer.command.tsx:254-257) and the menu resets with it.
    rig.write_both(b"\x15");
    settle(1);
    capture_pair(&rig, "keys", "ctrl-u-clear");
    // esc closes the panel.
    rig.write_both(b"\x1b");
    settle(1);
    capture_pair(&rig, "keys", "closed");
    drop(rig);
}

// ===========================================================================
// Anchor 2 — G-500: the reconnecting box's half-up centering. Both
// ends' servers stop; the settled loss overlay exports (the box now
// sits at the Yoga-rounded top — one row lower than the pre-fix
// floor at H=36: top 16, the register's empirical probe).
// ===========================================================================
#[test]
fn v22a_reconnecting_centering_pair() {
    skip_without_ref_lane!();
    // Built inline (the animations-off config must land BEFORE the
    // TUIs spawn — the config is read at boot): animations OFF gives
    // a glyph-stable capture — the spinner cell renders its static
    // fallback (`⋯`, G-430 face 4) instead of the 80 ms braille
    // phase; the first run's only divergence was the mid-animation
    // glyph (⠹ vs ⠇), which says nothing about the box. With the
    // static cell the export isolates the GEOMETRY.
    let key = serde_json::json!({ "animations": false });
    let rig = v22a_rig("v22arec", COLS, None, Some(&key));
    boot_both(&rig);
    // The loss: stop both servers (the TUIs stay alive on their PTYs).
    ref_service_stop(&rig.refbox, &rig.stage);
    service_stop(&rig.rust);
    // Poll each end for its overlay face; capture the grid THE MOMENT
    // it shows (the ref's managed respawn clears it seconds later).
    let overlay_text =
        |screen: &str| screen.contains("Connection lost") || screen.contains("Restarting service");
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
            "V22A reconnecting RED: the loss overlays never settled (rust={rust_seen}, \
             ref={ref_seen}); rust screen:\n{}\nref screen:\n{}",
            rig.rust_tui.screen_text(),
            rig.ref_tui.screen_text()
        );
    };
    let ref_grid = rig.normalize(&ref_grid);
    let rust_grid = rig.normalize(&rust_grid);
    dump_grid("v22a-reconnecting-loss-settled-ref", &ref_grid);
    dump_grid("v22a-reconnecting-loss-settled-rust", &rust_grid);
    drop(rig);
    export_grid_diff("v22a-reconnecting", "loss-settled", &ref_grid, &rust_grid);
}

// ===========================================================================
// The marquee pair helper — a narrow full-TUI pair, one long-titled
// session, the strip tab hovered (the scroll's gate).
// ===========================================================================
fn marquee_pair(tag: &str, animations: Option<bool>) -> V22ARig {
    // PAYLOAD-SEEDED record mode: the long custom prompt has no
    // corpus cassette (the strict digest gateway errors the turn —
    // the rig RED screen in the first run), and the DEFAULT title
    // seed renames the session to the SHORT "Sandbox Replay Session"
    // (which fits the tab — no marquee). The rig seeds BOTH: the
    // canned REALCHAIN-REPLAY-OK reply and a LONG title (the same
    // text as the prompt — the tab's overflow gate).
    let record = wt_root().join(format!(".test-logs/{tag}-seeds"));
    std::fs::create_dir_all(&record).expect("create record dir");
    // The title seed: the CANONICAL default title SSE (the framing the
    // gateway's title-miss seeding already proves on the wire — the
    // earlier default-seeded runs renamed the tab with it) with the
    // title text swapped for the long fixture. (The first round
    // hand-wrote the SSE inside a one-line literal and the source's
    // literal indentation survived into the body — indented `data:`
    // lines are not SSE fields, BOTH clients failed the parse, and
    // the tab stayed "New session": the anchor ran vacuously. A
    // template replace cannot drift.)
    let title_seed = record.join("long-title-sse.txt");
    let template =
        std::fs::read_to_string(wt_root().join("tools/diff/realchain/seeds/title-sse.txt"))
            .expect("the default title seed template");
    std::fs::write(&title_seed, template.replace("Sandbox Replay Session", LONG_TITLE))
        .expect("write the long-title seed");
    let lane = lane_lock();
    let gateway = Gateway::spawn_payload_seeded(
        tag,
        Some(&record),
        &wt_root().join("tools/diff/realchain/seeds/anthropic-text.txt"),
        &title_seed,
    );
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    // G-502② — the animations-off config state (the top-level
    // `animations` key, config/index.tsx:246): the leading fade JUMPS
    // at its timer fires instead of tweening. The key lives in each
    // side's cli.json (see v22a_rig's patch note).
    if let Some(animations) = animations {
        let patch = serde_json::json!({ "animations": animations });
        patch_tui_config(&rust.root.join("xdg-config/opencode-rust/cli.json"), &patch);
        patch_tui_config(&refbox.root.join("xdg-config/opencode/cli.json"), &patch);
    }
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, NARROW_COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, NARROW_COLS, ROWS);
    V22ARig {
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

/// The long session-title fixture (the prompt's own text — the tab's
/// overflow gate; 71 cells overflows the ~50-cell tab at 60 cols).
const LONG_TITLE: &str =
    "a decidedly long session title that overflows the narrow tab slot on the strip";

/// One exchange with a LONG prompt: the session's title (the first
/// prompt's text) overflows the narrow tab slot.
fn long_exchange(rig: &mut V22ARig) {
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(format!("{LONG_TITLE}\r").as_bytes());
    let rust_ok = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(40));
    let ref_ok = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    assert!(
        rust_ok && ref_ok,
        "V22A marquee RED: the long turn never replied (rust={rust_ok}, ref={ref_ok}); \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
}

// ===========================================================================
// Anchor 3 — G-501/G-502①: the hover marquee on the strip (the
// default animations config). The hovered tab's overflowing title
// scrolls; the mid-scroll capture lands with the loop `·` separator
// inside the window (tinted 0.55 toward the tab background — the
// G-502① face), and the late capture lands past the cycle's reset
// tick (window 0, the leading fade settled out — the structural
// face). The scroll timeline is deterministic per end (600 ms still
// + 80 ms/step); the capture instants ride each end's OWN hover
// receipt, so a ±80 ms skew can shift the window by ±1 cell — the
// adjudication's tolerance, noted here per the export discipline.
// ===========================================================================
#[test]
fn v22a_marquee_pair() {
    skip_without_ref_lane!();
    let mut rig = marquee_pair("v22amar", None);
    boot_both(&rig);
    long_exchange(&mut rig);
    // The tab strip shows the long title; give the turn's settle the
    // transcript room (the tab appears with the title at the reply).
    settle(4);
    // The long title must LAND in the tab (the anchor's gate — no
    // overflow, no marquee, no anchor): row 0 shows the title text,
    // not the boot "New session" default (the first round's malformed
    // title seed left the default and the anchor ran vacuously).
    assert!(
        row0_contains(&rig.rust_tui, "decidedly") && row0_contains(&rig.ref_tui, "decidedly"),
        "V22A marquee RED: the long title never landed in the tab; rust row0:\n{}\nref row0:\n{}",
        rig.rust_tui.screen_text().lines().next().unwrap_or_default(),
        rig.ref_tui.screen_text().lines().next().unwrap_or_default()
    );
    capture_pair(&rig, "marquee", "pre-hover");
    // Hover the tab: an SGR click at the strip's first tab (row 0,
    // inside the title's span — the pointer stays there after the
    // release, keeping the marquee armed).
    sgr_click(&mut rig.rust_tui, 10, 0);
    sgr_click(&mut rig.ref_tui, 10, 0);
    // The SEPARATOR-IN-WINDOW state: each end captures its grid the
    // INSTANT row 0's loop `·` sits at the target column (the asserted
    // needle). Both ends' strip geometry agrees at rest (the pre-hover
    // export) and the step cadence is 80 ms on both, so the same `·`
    // column selects the SAME window offset on both ends — a plain
    // needle-then-settle capture can straddle a step boundary and
    // land the ends one cell apart (the first round's single-row
    // divergence: ref cursor 65 vs port cursor 66 — the ref's slice
    // then carries the loop's own leading space and the port's its
    // trailing one). (capture_settled's 10 s stability cap would sail
    // past the cycle's reset — the marquee never settles while
    // scrolling. The `·`'s 0.55 tint and the leading fade are COLOR
    // faces — invisible to the text diff — so this state anchors the
    // window STRUCTURE: the separator cell's position; the tint itself
    // is unit-pinned in marquee.rs.)
    capture_separator_in_window(&rig, "marquee");
    // Past the cycle's reset (600 + (cycle−1)·80 ≈ 5.6 s for a
    // 72-cell title + the 250 ms fade-out): window 0, settled — the
    // stability capture is safe here (the marquee has ended).
    settle(8);
    capture_pair(&rig, "marquee", "scroll-reset-settled");
    drop(rig);
}

/// One-shot row-0 containment check (row 0 = the tab strip — the tab
/// titles).
fn row0_contains(tui: &TuiProcess, needle: &str) -> bool {
    tui.screen_text().lines().next().is_some_and(|row| row.contains(needle))
}

/// The `·` column both ends capture at (the sweep's middle — the
/// separator enters the hovered title box at its right edge and walks
/// left one cell per 80 ms step; 16 sits well inside the pass).
const SEPARATOR_COL: usize = 16;

/// Capture one end's grid the INSTANT row 0's loop `·` sits at
/// [`SEPARATOR_COL`] (each end's own scroll clock). None = the `·`
/// never reached the column inside `secs` (the marquee never armed).
fn capture_at_row0_col(tui: &TuiProcess, secs: u64) -> Option<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        // The strip row is single-width, so the char index is the
        // display column.
        let screen = tui.screen_text();
        let hit = screen.lines().next().is_some_and(|row| {
            row.chars().enumerate().any(|(col, ch)| ch == '\u{b7}' && col == SEPARATOR_COL)
        });
        if hit {
            return Some(screen);
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// The separator-in-window state: both ends capture at the same `·`
/// column (the same window offset — see the call sites' notes), then
/// the paired export. A miss on either end is a rig RED (the anchor
/// would otherwise run vacuously).
fn capture_separator_in_window(rig: &V22ARig, scenario: &str) {
    let (ref_grid, rust_grid) = std::thread::scope(|scope| {
        let legs: Vec<_> = [&rig.ref_tui, &rig.rust_tui]
            .into_iter()
            .map(|tui| scope.spawn(move || capture_at_row0_col(tui, 20)))
            .collect();
        let mut grids = legs.into_iter().map(|leg| leg.join().expect("separator capture leg"));
        (grids.next().flatten(), grids.next().flatten())
    });
    let (Some(ref_grid), Some(rust_grid)) = (ref_grid, rust_grid) else {
        panic!(
            "V22A {scenario} RED: the loop `·` separator never reached column {SEPARATOR_COL} of \
             the strip row (the marquee never armed on the hovered tab); rust screen:\n{}\nref \
             screen:\n{}",
            rig.rust_tui.screen_text(),
            rig.ref_tui.screen_text()
        );
    };
    let ref_grid = rig.normalize(&ref_grid);
    let rust_grid = rig.normalize(&rust_grid);
    dump_grid(&format!("v22a-{scenario}-separator-in-window-ref"), &ref_grid);
    dump_grid(&format!("v22a-{scenario}-separator-in-window-rust"), &rust_grid);
    export_grid_diff(&format!("v22a-{scenario}"), "separator-in-window", &ref_grid, &rust_grid);
}

// ===========================================================================
// Anchor 4 — G-502②: the SAME marquee face with the animations-OFF
// config. The leading fade JUMPS at the delay's end (the ref's
// disabled-arm `jump(next)`, animation.ts:71-74/90) instead of
// tweening — the +730 ms capture sits inside the rise window
// (600..850 ms past each end's own hover), where the jump reads 1
// and a tweening port would read ≈0.2-0.8. A second, later capture
// anchors the off-config separator face (leading 1, held).
// ===========================================================================
#[test]
fn v22a_marquee_off_pair() {
    skip_without_ref_lane!();
    let mut rig = marquee_pair("v22amaroff", Some(false));
    boot_both(&rig);
    long_exchange(&mut rig);
    settle(4);
    // The same title-landed gate as the animations-on anchor.
    assert!(
        row0_contains(&rig.rust_tui, "decidedly") && row0_contains(&rig.ref_tui, "decidedly"),
        "V22A marquee-off RED: the long title never landed in the tab; rust row0:\n{}\nref row0:\n{}",
        rig.rust_tui.screen_text().lines().next().unwrap_or_default(),
        rig.ref_tui.screen_text().lines().next().unwrap_or_default()
    );
    sgr_click(&mut rig.rust_tui, 10, 0);
    sgr_click(&mut rig.ref_tui, 10, 0);
    // The off-config SEPARATOR-IN-WINDOW state (the same asserted
    // column-needle shape as the animations-on anchor): with animations
    // disabled the marquee STILL scrolls (the ref's createMarquee
    // interval is not animations-gated — only the leading fade's tween
    // jumps, session-tabs.tsx:287-303). The jump itself is a COLOR face
    // (invisible to the text diff) and is unit-pinned in marquee.rs
    // (`leading_jumps_instantly_with_animations_off`); this state
    // anchors the off-config window structure.
    capture_separator_in_window(&rig, "marquee-off");
    // The off-config settled face (past the cycle's reset).
    settle(8);
    capture_pair(&rig, "marquee-off", "scroll-reset-settled");
    drop(rig);
}

// ===========================================================================
// The plugin-fixture anchors' fixtures.
// ===========================================================================
const FIXTURES: &[(&str, &str)] = &[
    // 1 — G-517: a long single-token JSON toast (the v21b-router
    // plugin-arm message shape — the punctuation-break wrap face).
    (
        "v22a-toast-long",
        r#"export default {
  id: "v22a.toast.tui",
  setup(ctx) {
    setTimeout(() => {
      ctx.ui.toast.show({
        title: "v22a long toast",
        message: JSON.stringify({
          type: "plugin",
          hasSessionID: false,
          hasParams: false,
          name: "v22a-toast",
          id: "v22a.toast.tui",
          hasData: true,
        }),
        duration: 600000,
      });
    }, 6000);
    return () => {};
  },
};
"#,
    ),
    // 2 — G-518: dialog.show with a space-between header row + a
    // blank row + a long wrapping row; set({size:"large"}) 20 s after
    // the show's open marker (the chained-arm SOP) — the rows
    // re-flow to the 88-wide box on the ref (the live children) and
    // on the port (the re-flattened rows).
    (
        "v22a-show-large",
        r#"import { createElement, insert, setProp } from "@opentui/solid";
export default {
  id: "v22a.show.tui",
  setup(ctx) {
    const report = (tag) =>
      ctx.ui.toast.show({
        title: "v22a show " + tag,
        message: "",
        duration: 600000,
      });
    const body = () => {
      const box = createElement("box");
      const header = createElement("box");
      setProp(header, "flexDirection", "row");
      setProp(header, "justifyContent", "space-between");
      const left = createElement("text");
      insert(left, "v22a left");
      const right = createElement("text");
      insert(right, "right");
      insert(header, left);
      insert(header, right);
      const blank = createElement("text");
      insert(blank, " ");
      const text = createElement("text");
      insert(
        text,
        "v22a long row " + "word ".repeat(28) + "tail",
      );
      insert(box, header);
      insert(box, blank);
      insert(box, text);
      return box;
    };
    setTimeout(() => {
      // The render is a FUNCTION (api.tsx:334's `show(render:
      // () => JSX.Element)` — the host calls it at show time and on
      // every set re-flow).
      ctx.ui.dialog.show(body);
      setTimeout(() => {
        ctx.ui.dialog.set({ size: "large" });
        report("large");
      }, 20000);
    }, 6000);
    return () => {};
  },
};
"#,
    ),
];

/// The plugin-fixture rig (the v21b topology + one staged fixture).
fn v22a_fixture_rig(tag: &str, fixture: &str) -> V22ARig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    let tui_js = FIXTURES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or_else(|| panic!("unknown v22a fixture {fixture}"), |(_, js)| *js);
    let needs_shim = tui_js.contains("@opentui/solid");
    stage_fixture(&rust, "opencode-rust", fixture, tui_js, needs_shim);
    stage_fixture(&refbox, "opencode", fixture, tui_js, false);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V22ARig {
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

/// Needle + capture: the toast marker completes at the SLOWER end;
/// the capture rides ~1 s behind.
fn marker_state(rig: &V22ARig, scenario: &str, state: &str, needle: &str, window: u64) {
    let ok = rig.wait_both_screens(needle, window);
    assert!(
        ok,
        "V22A {scenario} RED: the {state} marker ({needle}) never showed; \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(1);
    capture_pair(rig, scenario, state);
}

// ===========================================================================
// Anchor 5 — G-517: the long single-token JSON toast at t+6 s. The
// toast panel wraps the message at the punctuation break set (the
// ref's opentui word wrap breaks after `:`/`,` — the port's
// toast_break_now carries the same set). The exported rows are the
// wrap faces (the ref's v21b-router rows: the break after
// "hasParams":, the row at "…router.tui", the tail row).
// ===========================================================================
#[test]
fn v22a_toast_long_pair() {
    skip_without_ref_lane!();
    let rig = v22a_fixture_rig("v22atoast", "v22a-toast-long");
    boot_both(&rig);
    marker_state(&rig, "toast-long", "long-open", "v22a long toast", 60);
    drop(rig);
}

// ===========================================================================
// Anchor 6 — G-518: dialog.show at t+6 s (the space-between header +
// the blank row + the long wrapping row — the 60-wide flatten); the
// set({size:"large"}) at +20 s after the open (the chained arm) —
// the ref's live children re-flow to 88 and the port's re-flattened
// rows follow (the header's `right` at the 88-wide right edge).
// ===========================================================================
#[test]
fn v22a_show_set_large_pair() {
    skip_without_ref_lane!();
    let rig = v22a_fixture_rig("v22ashow", "v22a-show-large");
    boot_both(&rig);
    marker_state(&rig, "show-large", "one-open", "v22a left", 60);
    marker_state(&rig, "show-large", "set-large", "v22a show large", 90);
    drop(rig);
}

// ===========================================================================
// Anchor 7 — G-504 + G-500's No-preview leg: `prompt.image_preview`
// on (both ends), a local image pasted (the [Image 1] chip + the
// strip), the SAME path pasted again (the reachable dedup face —
// same name + mention + uri: both engines dedup identically; the
// description-differentiated identity has no TUI-reachable driver on
// either end and lives in the port's unit fixture), and a BROKEN
// png pasted (the decode fails → the "No preview" fallback centered
// by the half-up Yoga rounding — the G-500 leg).
// ===========================================================================
#[test]
fn v22a_prompt_preview_pair() {
    skip_without_ref_lane!();
    let tag = "v22aprev";
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    // The strip's gate, ON for both ends (the `prompt.image_preview`
    // TUI-level key — each side's cli.json; the default is also on,
    // the patch pins it explicitly).
    let preview_key = serde_json::json!({ "prompt": { "image_preview": true } });
    patch_tui_config(&rust.root.join("xdg-config/opencode-rust/cli.json"), &preview_key);
    patch_tui_config(&refbox.root.join("xdg-config/opencode/cli.json"), &preview_key);
    // The pasted local images: the tiny 4x2 PNG fixture + a BROKEN
    // png (garbage bytes — the extension routes the paste to the
    // image arm, the decode fails, the fallback renders).
    let png = include_bytes!("fixtures/r07-tiny-4x2.png");
    for project in [&rust.project, &refbox.project] {
        std::fs::write(project.join("shot.png"), png).expect("stage shot.png");
        std::fs::write(project.join("broken.png"), b"not a png at all").expect("stage broken.png");
    }
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    let mut rig = V22ARig {
        _lane: lane,
        _gateway: gateway,
        rust,
        refbox,
        stage,
        _rust_server_url: server.url,
        _rust_password: server.password,
        rust_tui,
        ref_tui,
    };
    boot_both(&rig);
    // The first paste: the chip + the strip settle.
    let rust_path = rig.rust.project.join("shot.png").to_string_lossy().into_owned();
    let ref_path = rig.refbox.project.join("shot.png").to_string_lossy().into_owned();
    rig.rust_tui.write(format!("\x1b[200~{rust_path}\x1b[201~").as_bytes());
    rig.ref_tui.write(format!("\x1b[200~{ref_path}\x1b[201~").as_bytes());
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
        "V22A preview RED: the image chips never staged (rust={saw_rust}, ref={saw_ref}); \
         rust screen:\n{}\nref screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(2);
    capture_pair(&rig, "preview", "strip-on");
    // The SAME path again: the identity dedup (same name + mention +
    // uri — one thumb on both ends; the description-differentiated
    // arm is the port's unit fixture).
    rig.rust_tui.write(format!("\x1b[200~{rust_path}\x1b[201~").as_bytes());
    rig.ref_tui.write(format!("\x1b[200~{ref_path}\x1b[201~").as_bytes());
    settle(3);
    capture_pair(&rig, "preview", "pasted-twice");
    // The broken png: the decode fails → the "No preview" fallback
    // (the G-500 half-up centering leg — the 11-wide label centered
    // in the thumb box).
    let rust_broken = rig.rust.project.join("broken.png").to_string_lossy().into_owned();
    let ref_broken = rig.refbox.project.join("broken.png").to_string_lossy().into_owned();
    rig.rust_tui.write(format!("\x1b[200~{rust_broken}\x1b[201~").as_bytes());
    rig.ref_tui.write(format!("\x1b[200~{ref_broken}\x1b[201~").as_bytes());
    settle(4);
    capture_pair(&rig, "preview", "no-preview-fallback");
    drop(rig);
}
