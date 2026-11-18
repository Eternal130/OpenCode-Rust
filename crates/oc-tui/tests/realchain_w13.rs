//! W13 — the coverage-ledger execution wave (2.2#13): the ledger's
//! top-priority GAP families become real-chain scenarios under the
//! W12 adjudication architecture (2.2#12 — NO content adjudication
//! in-test; every judgment is an `export_grid_diff`/`export_survey`
//! artifact decided controller-side).
//!
//! Families:
//! * **E (migration)** — v2 data migration on the real boot chain:
//!   preset `auth.json`/db state BEFORE `oc-server` starts, then the
//!   daemon's own bootstrap/migration path runs (concern②'s fixed
//!   behavior: fresh install + existing auth.json IMPORTS credentials).
//! * **B (permission dialog flow)** — the tool loop WITHOUT
//!   pre-authorization: the real ask dialog (once/always/reject), both
//!   engines, keystroke-driven.
//! * **C (omo-slim sections)** — the home footer's width-gated
//!   plugin/MCP/version slots at the trigger width, plugins loaded.
//! * **G (content boundaries)** — transcript-side payload edges
//!   (CJK wrap / emoji ZWJ / no-space long line / empty assistant
//!   reply) through synthetic gateway seeds, both engines.

// Test-harness infra lints (see realchain_pair.rs's list for the
// rationale): `{:?}` forensics in panics, wire-shape names in docs.
#![allow(
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::match_same_arms,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    default_pace, dump_grid, export_grid_diff, export_survey, http_get, http_post, lane_lock,
    normalize_grid, plugin_lane_skip_reason, ref_lane_skip_reason, ref_service_stop, ref_stage,
    ref_tui_spawn, sandbox, service_start, service_stop, stage_plugins, worktree_dir, Gateway,
    LaneGuard, ManagedServer, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const COLS: u16 = 120;
const ROWS: u16 = 36;

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("W13 SKIP: {reason}");
            return;
        }
    };
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

fn wait_screen(tui: &TuiProcess, needle: &str, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if tui.screen_text().contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

// ===========================================================================
// E — the migration family (rust-side real chain: the daemon's own
// boot migration path over preset v2-shape data).
// ===========================================================================

/// The v2-shape auth.json preset (two credentials: one OAuth, one API
/// key — the exact legacy shapes `20260805200742` decodes).
fn preset_auth_json(box_: &Sandbox) {
    std::fs::create_dir_all(box_.legacy_data_root()).expect("legacy data root");
    std::fs::write(
        box_.auth_json(),
        serde_json::json!({
            "github-copilot": {
                "type": "oauth",
                "refresh": "w13-refresh",
                "access": "w13-access",
                "expires": 1_893_456_000_000u64
            },
            "anthropic": { "type": "api", "key": "w13-ant-key" }
        })
        .to_string(),
    )
    .expect("auth.json");
}

fn credential_count(url: &str, password: Option<&str>) -> (u16, usize) {
    let (status, body) =
        http_get(&format!("{url}/api/credential"), password).unwrap_or((0, String::new()));
    let count = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .as_array()
                .map(Vec::len)
                .or_else(|| value.get("data").and_then(|data| data.as_array()).map(Vec::len))
        })
        .unwrap_or(0);
    (status, count)
}

/// M1 — normal migration: fresh install (no db) + an existing v2
/// `auth.json` → the daemon's bootstrap IMPORTS both credentials
/// (concern②'s fixed behavior), the journal is complete, and a
/// session created over the API is visible.
#[test]
fn w13_m1_fresh_install_imports_auth_json() {
    let (lane, box_, server) = migration_boot_tagged("w13-m1", true);
    let _guard = lane;
    let (status, count) = credential_count(&server.url, server.password.as_deref());
    // A session created over the API is visible (migration didn't wedge
    // the session store).
    let (session_status, session_body) = http_post(
        &format!("{}/api/session", server.url),
        &format!(
            "{{\"location\": {{\"directory\": {}}}}}",
            serde_json::to_string(&box_.project.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "\"/\"".to_owned())
        ),
        server.password.as_deref(),
    )
    .unwrap_or((0, String::new()));
    let (list_status, list_body) =
        http_get(&format!("{}/api/session", server.url), server.password.as_deref())
            .unwrap_or((0, String::new()));
    let sessions = serde_json::from_str::<serde_json::Value>(&list_body)
        .ok()
        .and_then(|value| {
            value
                .as_array()
                .map(Vec::len)
                .or_else(|| value.get("data").and_then(|data| data.as_array()).map(Vec::len))
        })
        .unwrap_or(0);
    // Migration markers: the import marker + the full journal live in
    // the daemon's own db (read after the API probes; WAL allows the
    // concurrent read).
    let (marker_count, journal_count) = journal_facts(&box_.db_path());
    service_stop(&box_);
    assert!(
        status == 200 && count == 2,
        "W13-M1 RED: /api/credential={status} count={count} (expected 200/2)"
    );
    assert!(
        (200..300).contains(&session_status) && list_status == 200 && sessions >= 1,
        "W13-M1 RED: session create={session_status} list={list_status} rows={sessions}; \
         create body: {session_body}"
    );
    assert_eq!((marker_count, journal_count), (1, 48), "journal shape");
}

/// M2 — already-migrated idempotence: restart the daemon over the
/// migrated data (auth.json still present) → no duplicate import, the
/// journal stays complete.
#[test]
fn w13_m2_restarted_daemon_does_not_reimport() {
    let (lane, box_, server) = migration_boot_tagged("w13-m2", true);
    let _guard = lane;
    let (first_status, first_count) = credential_count(&server.url, server.password.as_deref());
    service_stop(&box_);
    // Second boot over the SAME sandbox (db + auth.json both present).
    let server2 = service_start(&box_);
    let (second_status, second_count) = credential_count(&server2.url, server2.password.as_deref());
    let (marker_count, journal_count) = journal_facts(&box_.db_path());
    service_stop(&box_);
    assert!(
        first_status == 200 && second_status == 200 && first_count == 2 && second_count == 2,
        "W13-M2 RED: first=({first_status},{first_count}) second=({second_status},{second_count})"
    );
    assert_eq!((marker_count, journal_count), (1, 48), "journal shape after restart");
}

/// M3 — corrupted credential row: a db whose credential table carries
/// an undecodable value. The ref's semantics (credential.ts:63-67, the
/// G-225 core-store alignment) FAIL the credential read — the scenario
/// pins the blast radius: the daemon boots, sessions list fine, the
/// credential read fails cleanly (observed via survey export, decided
/// controller-side).
#[test]
fn w13_m3_corrupted_credential_row_scopes_the_failure() {
    let (lane, box_, server) = migration_boot_tagged("w13-m3", true);
    let _guard = lane;
    let (good_status, good_count) = credential_count(&server.url, server.password.as_deref());
    service_stop(&box_);
    assert_eq!((good_status, good_count), (200, 2), "pre-corruption sanity");
    // Corrupt one row's value (undecodable JSON).
    {
        let conn = rusqlite::Connection::open(box_.db_path()).expect("open db");
        conn.busy_timeout(Duration::from_secs(5)).ok();
        conn.execute_batch(
            "UPDATE credential SET value = '{\"type\": ' || '\"bogus\"' WHERE integration_id = 'anthropic';",
        )
        .expect("corrupt row");
    }
    let server2 = service_start(&box_);
    let (cred_status, _cred_count) = credential_count(&server2.url, server2.password.as_deref());
    let (session_status, _session_body) =
        http_get(&format!("{}/api/session", server2.url), server2.password.as_deref())
            .unwrap_or((0, String::new()));
    service_stop(&box_);
    // Infra sanity only (2.2#12): the daemon survived and the session
    // store is unaffected. The credential read's exact outcome rides
    // the survey export.
    assert_eq!(session_status, 200, "session list must survive a corrupt credential row");
    export_survey(
        "w13-m3",
        "corrupt-row",
        &[
            ("pre-corruption /api/credential status", 200, u64::from(good_status)),
            (
                "post-corruption /api/credential status (ref fail-the-read)",
                500,
                u64::from(cred_status),
            ),
        ],
        None,
    );
}

/// The migration rig with the auth.json preset toggle.
fn migration_boot_tagged(tag: &str, preset: bool) -> (LaneGuard, Sandbox, ManagedServer) {
    let lane = lane_lock();
    let gateway = Gateway::spawn(tag, None);
    let box_ = sandbox(tag, gateway.port);
    box_.write_config(&[], "replay/claude-opus-4-7");
    if preset {
        preset_auth_json(&box_);
    }
    let server = service_start(&box_);
    drop(gateway);
    (lane, box_, server)
}

/// (import-marker rows, total journal rows) from the daemon's db.
fn journal_facts(db: &Path) -> (u64, u64) {
    let conn = rusqlite::Connection::open(db).expect("open db (journal)");
    let import_marker: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM migration WHERE id = '20260805200742_import_legacy_credentials'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(u64::MAX);
    let journal: u64 =
        conn.query_row("SELECT COUNT(*) FROM migration", [], |row| row.get(0)).unwrap_or(u64::MAX);
    (import_marker, journal)
}

// ===========================================================================
// B — the permission dialog flow (pair, real ask, keystroke-driven).
// ===========================================================================

const CONFIRM_NEEDLE: &str = "TOOLLOOP-CONFIRMED";
const README_BEFORE: &str = "# t9 sandbox project\n";
const README_AFTER: &str = "# t9 sandbox project (tests green)\n";

fn corpus_tools_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-tools")
}
fn corpus_tools_ref_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-tools-ref")
}
fn toolloop_seeds_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/seeds/toolloop")
}

fn stage_shims(rust: &Sandbox) -> PathBuf {
    let out = rust.root.join("shimstage");
    let status = std::process::Command::new("python3")
        .arg(worktree_dir("tools/diff/realchain/shim_extract.py"))
        .arg("--spec")
        .arg(toolloop_seeds_dir().join("shims.json"))
        .arg("--out")
        .arg(&out)
        .output()
        .expect("spawn shim_extract.py");
    assert!(
        status.status.success(),
        "shim_extract.py failed: {}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    out
}

/// The tool-loop pair rig WITHOUT the permission pre-authorization —
/// every tool call raises the real ask dialog on both engines.
struct PermRig {
    _lane: LaneGuard,
    rust_gateway: Gateway,
    ref_gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl PermRig {
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

fn perm_rig(tag: &str) -> PermRig {
    let lane = lane_lock();
    let corpus = [corpus_tools_dir(), corpus_tools_ref_dir()];
    let rust_gateway = Gateway::spawn_turn(tag, &corpus, None);
    let ref_gateway = Gateway::spawn_turn(tag, &corpus, None);
    let mut rust = sandbox(tag, rust_gateway.port);
    let mut refbox = sandbox(&format!("{tag}ref"), ref_gateway.port);
    let shim_dir = stage_shims(&rust);
    rust.shim_dir = Some(shim_dir);
    refbox.shim_dir = Some(rust.root.join("shimstage"));
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    // NO preauthorize_tools: the ask dialog is the scenario's surface —
    // the explicit "ask" policy raises it on every bash/edit call.
    rust.ask_tools(false);
    refbox.ask_tools(true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    std::fs::write(rust.project.join("README.md"), README_BEFORE).expect("rust README");
    std::fs::write(refbox.project.join("README.md"), README_BEFORE).expect("ref README");
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    PermRig { _lane: lane, rust_gateway, ref_gateway, rust, refbox, stage, rust_tui, ref_tui }
}

impl Drop for PermRig {
    fn drop(&mut self) {
        let _ = (&self.rust_gateway, &self.ref_gateway);
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn perm_boot(rig: &mut PermRig) {
    let rust_booted = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_booted = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_booted && ref_booted,
        "PERM precondition RED: booted rust={rust_booted} ref={ref_booted}"
    );
    settle(2);
}

/// Wait for the ask dialog on BOTH ends (the header is the shared
/// anchor: permission.tsx "Permission required").
fn wait_dialog(rig: &PermRig, secs: u64) -> (bool, bool) {
    (
        wait_screen(&rig.rust_tui, "Permission required", secs),
        wait_screen(&rig.ref_tui, "Permission required", secs),
    )
}

/// B-once — the default selection ("Allow once") runs the tool; the
/// SECOND ask (the edit turn) answers once again; the loop completes
/// and the settled grids export.
#[test]
fn w13_perm_once_pair() {
    skip_without_ref_lane!();
    let mut rig = perm_rig("w13-po");
    perm_boot(&mut rig);
    rig.write_both(b"hello\r");
    let (rust_ask, ref_ask) = wait_dialog(&rig, 40);
    let dialog_ref = rig.normalize(&rig.ref_tui.screen_text());
    let dialog_rust = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("w13-po-dialog-ref", &dialog_ref);
    dump_grid("w13-po-dialog-rust", &dialog_rust);
    assert!(
        rust_ask && ref_ask,
        "PERM-once RED: ask dialog rust={rust_ask} ref={ref_ask}; rust screen:\n{}",
        rig.rust_tui.screen_text()
    );
    export_grid_diff("w13-perm-once", "dialog", &dialog_ref, &dialog_rust);
    // "Allow once" is the default selection — plain Enter.
    rig.write_both(b"\r");
    let (rust_out, ref_out) = (
        wait_screen(&rig.rust_tui, "test result: ok", 30),
        wait_screen(&rig.ref_tui, "test result: ok", 45),
    );
    // Second ask: the edit turn.
    let (rust_ask2, ref_ask2) = wait_dialog(&rig, 40);
    assert!(
        rust_out && ref_out && rust_ask2 && ref_ask2,
        "PERM-once RED: turn-0 output rust={rust_out} ref={ref_out}; edit ask \
         rust={rust_ask2} ref={ref_ask2}"
    );
    rig.write_both(b"\r");
    let (rust_done, ref_done) = (
        wait_screen(&rig.rust_tui, CONFIRM_NEEDLE, 40),
        wait_screen(&rig.ref_tui, CONFIRM_NEEDLE, 60),
    );
    settle(4);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    let rust_readme =
        std::fs::read_to_string(rig.rust.project.join("README.md")).unwrap_or_default();
    let ref_readme =
        std::fs::read_to_string(rig.refbox.project.join("README.md")).unwrap_or_default();
    drop(rig);
    assert!(rust_done && ref_done, "PERM-once RED: confirm rust={rust_done} ref={ref_done}");
    assert_eq!(
        (rust_readme.as_str(), ref_readme.as_str()),
        (README_AFTER, README_AFTER),
        "PERM-once RED: the once-allowed edit must land on both ends"
    );
    export_grid_diff("w13-perm-once", "settled", &ref_grid, &rust_grid);
}

/// B-always — "Always allow" persists the grant: the same-action
/// follow-ups in the loop never re-ask (observed via export), the
/// settled grids export.
#[test]
fn w13_perm_always_pair() {
    skip_without_ref_lane!();
    let mut rig = perm_rig("w13-pa");
    perm_boot(&mut rig);
    rig.write_both(b"hello\r");
    let (rust_ask, ref_ask) = wait_dialog(&rig, 40);
    assert!(rust_ask && ref_ask, "PERM-always RED: no ask rust={rust_ask} ref={ref_ask}");
    // Shift once (once → always) and run. G-451: the ref dialog binds
    // left/h + right/l (not tab), so drive the shift with right-arrow —
    // both engines move once → always.
    rig.write_both(b"\x1b[C\r");
    let (rust_out, ref_out) = (
        wait_screen(&rig.rust_tui, "test result: ok", 30),
        wait_screen(&rig.ref_tui, "test result: ok", 45),
    );
    assert!(rust_out && ref_out, "PERM-always RED: turn-0 output rust={rust_out} ref={ref_out}");
    // The edit ask is a DIFFERENT action — always again.
    let (rust_ask2, ref_ask2) = wait_dialog(&rig, 40);
    assert!(rust_ask2 && ref_ask2, "PERM-always RED: edit ask rust={rust_ask2} ref={ref_ask2}");
    rig.write_both(b"\x1b[C\r");
    let (rust_done, ref_done) = (
        wait_screen(&rig.rust_tui, CONFIRM_NEEDLE, 40),
        wait_screen(&rig.ref_tui, CONFIRM_NEEDLE, 60),
    );
    settle(4);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    drop(rig);
    assert!(rust_done && ref_done, "PERM-always RED: confirm rust={rust_done} ref={ref_done}");
    export_grid_diff("w13-perm-always", "settled", &ref_grid, &rust_grid);
}

/// B-reject — "Reject" denies the tool call: the rejected row replaces
/// the tool output; the post-rejection grids export (the model turn
/// after a rejection is NOT in the recorded corpus — the gateway miss
/// is expected and the scenario exports whatever each engine makes of
/// it; that asymmetry is exactly the adjudication surface).
#[test]
fn w13_perm_reject_pair() {
    skip_without_ref_lane!();
    let mut rig = perm_rig("w13-pr");
    perm_boot(&mut rig);
    rig.write_both(b"hello\r");
    let (rust_ask, ref_ask) = wait_dialog(&rig, 40);
    assert!(rust_ask && ref_ask, "PERM-reject RED: no ask rust={rust_ask} ref={ref_ask}");
    // G-451: reject via ESCAPE. The ref dialog (permission.tsx
    // SessionQuestion) binds left/h + right/l — NOT tab — so the old
    // `\t\t\r` chord silently selected "Allow once" on the ref end
    // (the W13 export compared a rejected rust turn against a
    // fully-allowed ref loop). Both engines reply reject immediately
    // on escape for a top-level session (rust permission_key /
    // ref escapeKey="reject" → dismiss → onSelect("reject")).
    rig.write_both(b"\x1b");
    // The reply lands as a rejected tool state; give both engines time
    // to settle their post-rejection surfaces, then export.
    settle(8);
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    dump_grid("w13-pr-reject-ref", &ref_grid);
    dump_grid("w13-pr-reject-rust", &rust_grid);
    drop(rig);
    export_grid_diff("w13-perm-reject", "rejected", &ref_grid, &rust_grid);
}

// ===========================================================================
// C — omo-slim sections at the trigger width (home footer 64/80 gates).
// ===========================================================================

/// The plugin rig at 96 cols (≥80: the plugin command slot's gate;
// <120: no docked sidebar — the footer is the surface). Both states
/// (home + a live session) export.
#[test]
fn w13_omo_sections_at_trigger_width_pair() {
    skip_without_ref_lane!();
    if let Some(reason) = plugin_lane_skip_reason() {
        eprintln!("W13-OMO SKIP: {reason}");
        return;
    }
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced("w13-omo", None, &default_pace());
    let rust = sandbox("w13-omo", gateway.port);
    let refbox = sandbox("w13-omoref", gateway.port);
    let rust_specs = stage_plugins(&rust, false);
    let ref_specs = stage_plugins(&refbox, true);
    assert_eq!(rust_specs, ref_specs, "both ends must carry the SAME plugin specs");
    let specs: Vec<&str> = rust_specs.iter().map(String::as_str).collect();
    rust.write_config_ext(&[], "replay/claude-opus-4-7", &specs);
    refbox.write_ref_config_ext(&[], "replay/claude-opus-4-7", &specs);
    // G-455③ — pin the omo plugin's builtin REMOTE MCP set to the
    // deterministic half: `context7` (mcp.context7.com, anonymous
    // access flaps 401/ok between runs and engines) is disabled on
    // BOTH ends through the plugin's own `omo.disabled_mcps` config;
    // `gh_grep` stays (anonymous-stable). Without this the system
    // prompt's Code Mode catalog section set is network-nondeterministic
    // and the request digest can never converge.
    for path in [rust.config_path(), refbox.root.join("xdg-config/opencode/opencode.json")] {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read config {}: {e}", path.display()));
        let mut config: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse config: {e}"));
        config["omo"] = serde_json::json!({ "disabled_mcps": ["context7"] });
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
            .unwrap_or_else(|e| panic!("write config {}: {e}", path.display()));
    }
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let mut rust_tui = TuiProcess::spawn(&rust, 96, ROWS);
    let mut ref_tui = ref_tui_spawn(&refbox, &stage, 96, ROWS);
    let roots =
        [refbox.root.to_string_lossy().into_owned(), rust.root.to_string_lossy().into_owned()];
    let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let normalize = |grid: &str| normalize_grid(grid, &refs);

    let rust_booted = rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_booted = ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_booted && ref_booted,
        "OMO precondition RED: boot rust/rust={rust_booted}/{ref_booted}"
    );
    // Plugin load is async — settle past the wordmark.
    settle(8);
    let home_ref = normalize(&ref_tui.screen_text());
    let home_rust = normalize(&rust_tui.screen_text());
    dump_grid("w13-omo-home-ref", &home_ref);
    dump_grid("w13-omo-home-rust", &home_rust);
    export_grid_diff("w13-omo", "home", &home_ref, &home_rust);

    // Session state: one reply, then export the footer again.
    // W13 finding (exported, not asserted): with the real plugins
    // loaded, the RUST engine's main turn never reaches the provider
    // (gateway requests stay at the title call) while the ref replies
    // — the divergence rides the exported diff + survey.
    let rust_from = rust_tui.raw_len();
    let ref_from = ref_tui.raw_len();
    rust_tui.write(b"hello\r");
    ref_tui.write(b"hello\r");
    let ref_replied = ref_tui.wait_for(b"REALCHAIN-REPLAY-OK", ref_from, Duration::from_secs(60));
    let rust_replied =
        rust_tui.wait_for(b"REALCHAIN-REPLAY-OK", rust_from, Duration::from_secs(30));
    settle(4);
    let session_ref = normalize(&ref_tui.screen_text());
    let session_rust = normalize(&rust_tui.screen_text());
    dump_grid("w13-omo-session-ref", &session_ref);
    dump_grid("w13-omo-session-rust", &session_rust);
    let stats = gateway.stats();
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    service_stop(&rust);
    let _ = &server.url;
    drop(gateway);
    drop(lane);
    assert!(ref_replied, "OMO RED (ref-side rig sanity): the ref reply never rendered; {stats}");
    export_grid_diff("w13-omo", "session", &session_ref, &session_rust);
    export_survey(
        "w13-omo",
        "session-reply",
        &[("rust reply rendered with plugins loaded (ref=1)", 1, u64::from(rust_replied))],
        None,
    );
}

// ===========================================================================
// G — content boundaries (transcript-side payload edges, synthetic
// gateway seeds, both engines).
// ===========================================================================

const G_PAYLOADS: &[(&str, &str)] = &[
    // (scenario suffix, prompt) — the reply seed bodies live in
    // seeds/w13/<suffix>.txt.
    ("cjk", "cjk"),
    ("emoji", "emoji"),
    ("longline", "longline"),
    ("empty", "empty"),
];

fn w13_corpus_dir(ref_side: bool) -> PathBuf {
    worktree_dir(if ref_side {
        "tools/diff/realchain/corpus-w13-ref"
    } else {
        "tools/diff/realchain/corpus-w13"
    })
}

fn w13_seed(suffix: &str) -> PathBuf {
    worktree_dir(&format!("tools/diff/realchain/seeds/w13/{suffix}.txt"))
}

/// One payload scenario: both engines send the prompt, the seeded
/// reply renders (or errors — exported either way), settled grids
/// export.
fn g_scenario(suffix: &str, prompt: &str) {
    let lane = lane_lock();
    let tag = format!("w13-g-{suffix}");
    let gateway = Gateway::spawn(&tag, None);
    let rust = sandbox(&tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let _server = service_start(&rust);
    let mut rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let mut ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    let roots =
        [refbox.root.to_string_lossy().into_owned(), rust.root.to_string_lossy().into_owned()];
    let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let normalize = |grid: &str| normalize_grid(grid, &refs);

    let rust_booted = rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_booted = ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_booted && ref_booted,
        "G-{suffix} precondition RED: boot {rust_booted}/{ref_booted}"
    );
    settle(2);
    rust_tui.write(format!("{prompt}\r").as_bytes());
    ref_tui.write(format!("{prompt}\r").as_bytes());
    // The reply's shape is payload-specific (the empty seed renders
    // nothing) — settle on TIME, not a needle.
    settle(12);
    let ref_grid = normalize(&ref_tui.screen_text());
    let rust_grid = normalize(&rust_tui.screen_text());
    dump_grid(&format!("w13-g-{suffix}-ref"), &ref_grid);
    dump_grid(&format!("w13-g-{suffix}-rust"), &rust_grid);
    let stats = gateway.stats();
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    service_stop(&rust);
    drop(gateway);
    drop(lane);
    eprintln!("G-{suffix} gateway stats: {stats}");
    export_grid_diff(&format!("w13-g-{suffix}"), "settled", &ref_grid, &rust_grid);
}

#[test]
fn w13_g_cjk_wrap_pair() {
    skip_without_ref_lane!();
    g_scenario("cjk", "cjk");
}

#[test]
fn w13_g_emoji_zwj_pair() {
    skip_without_ref_lane!();
    g_scenario("emoji", "emoji");
}

#[test]
fn w13_g_longline_pair() {
    skip_without_ref_lane!();
    g_scenario("longline", "longline");
}

#[test]
fn w13_g_empty_reply_pair() {
    skip_without_ref_lane!();
    g_scenario("empty", "empty");
}

// ===========================================================================
// Bootstrap seeds — record each engine's payload-turn request shapes
// into the committed corpus-w13{,-ref} dirs:
// `W13_RECORD=1 cargo nextest run -p oc-tui --test realchain_w13 --locked --no-fail-fast`
// ===========================================================================

fn g_bootstrap_side(ref_side: bool) {
    let suffix_tag = if ref_side { "ref" } else { "rust" };
    for (suffix, prompt) in G_PAYLOADS {
        let tag = format!("w13-g-{suffix}-{suffix_tag}");
        let lane = lane_lock();
        let record_dir = w13_corpus_dir(ref_side);
        std::fs::create_dir_all(&record_dir).expect("record dir");
        let gateway = Gateway::spawn_payload_seeded(
            &tag,
            Some(&record_dir),
            &w13_seed(suffix),
            &worktree_dir("tools/diff/realchain/seeds/title-sse.txt"),
        );
        let box_ = sandbox(&tag, gateway.port);
        if ref_side {
            box_.write_ref_config(&[], "replay/claude-opus-4-7");
            let stage = ref_stage(&box_);
            ref_service_stop(&box_, &stage);
            let mut tui = ref_tui_spawn(&box_, &stage, COLS, ROWS);
            assert!(tui.wait_for(WORDMARK, 0, Duration::from_secs(60)), "ref bootstrap settle");
            settle(2);
            let from = tui.raw_len();
            tui.write(format!("{prompt}\r").as_bytes());
            // The payload renders (or errors) — wait out the turn.
            let _ = from;
            settle(12);
            let screen = tui.screen_text();
            let stats = gateway.stats();
            tui.shutdown(Duration::from_secs(10));
            ref_service_stop(&box_, &stage);
            eprintln!("--- ref w13-g-{suffix} screen ---\n{screen}\n--- gateway {stats}");
        } else {
            box_.write_config(&[], "replay/claude-opus-4-7");
            let _server = service_start(&box_);
            let mut tui = TuiProcess::spawn(&box_, COLS, ROWS);
            assert!(tui.wait_for(WORDMARK, 0, Duration::from_secs(30)), "rust bootstrap settle");
            settle(2);
            tui.write(format!("{prompt}\r").as_bytes());
            settle(12);
            let screen = tui.screen_text();
            let stats = gateway.stats();
            tui.shutdown(Duration::from_secs(10));
            service_stop(&box_);
            eprintln!("--- rust w13-g-{suffix} screen ---\n{screen}\n--- gateway {stats}");
        }
        drop(gateway);
        drop(lane);
    }
}

#[test]
fn w13x_rust_bootstrap_seed() {
    if std::env::var("W13_RECORD").is_err() {
        eprintln!("w13x_rust_bootstrap_seed: skipped (W13_RECORD unset)");
        return;
    }
    g_bootstrap_side(false);
}

#[test]
fn w13x_ref_bootstrap_seed() {
    skip_without_ref_lane!();
    if std::env::var("W13_RECORD").is_err() {
        eprintln!("w13x_ref_bootstrap_seed: skipped (W13_RECORD unset)");
        return;
    }
    g_bootstrap_side(true);
}

/// The plugin-loaded "hello" cassettes for the omo session state: the
/// staged plugins change the system prompt, so the plain hello digests
/// strict-miss — record both engines' plugin-shape requests once.
#[test]
fn w13x_omo_bootstrap_seed() {
    if std::env::var("W13_RECORD").is_err() {
        eprintln!("w13x_omo_bootstrap_seed: skipped (W13_RECORD unset)");
        return;
    }
    skip_without_ref_lane!();
    if let Some(reason) = plugin_lane_skip_reason() {
        eprintln!("w13x_omo_bootstrap_seed SKIP: {reason}");
        return;
    }
    for ref_side in [true, false] {
        let lane = lane_lock();
        let record_dir = w13_corpus_dir(ref_side);
        std::fs::create_dir_all(&record_dir).expect("record dir");
        let gateway = Gateway::spawn_payload_seeded(
            "w13-omo",
            Some(&record_dir),
            &worktree_dir("tools/diff/realchain/seeds/anthropic-text.txt"),
            &worktree_dir("tools/diff/realchain/seeds/title-sse.txt"),
        );
        let box_ = sandbox(if ref_side { "w13-omoref" } else { "w13-omo" }, gateway.port);
        let specs = stage_plugins(&box_, ref_side);
        let spec_refs: Vec<&str> = specs.iter().map(String::as_str).collect();
        if ref_side {
            box_.write_ref_config_ext(&[], "replay/claude-opus-4-7", &spec_refs);
            let stage = ref_stage(&box_);
            ref_service_stop(&box_, &stage);
            let mut tui = ref_tui_spawn(&box_, &stage, COLS, ROWS);
            assert!(tui.wait_for(WORDMARK, 0, Duration::from_secs(60)), "ref omo bootstrap settle");
            settle(4);
            tui.write(b"hello\r");
            let ok = tui.wait_for(b"REALCHAIN-REPLAY-OK", 0, Duration::from_secs(60));
            let stats = gateway.stats();
            tui.shutdown(Duration::from_secs(10));
            ref_service_stop(&box_, &stage);
            eprintln!("--- ref omo bootstrap --- gateway {stats}");
            assert!(ok, "ref omo bootstrap: reply never rendered; stats {stats}");
        } else {
            box_.write_config_ext(&[], "replay/claude-opus-4-7", &spec_refs);
            let _server = service_start(&box_);
            let mut tui = TuiProcess::spawn(&box_, COLS, ROWS);
            assert!(
                tui.wait_for(WORDMARK, 0, Duration::from_secs(30)),
                "rust omo bootstrap settle"
            );
            settle(6);
            tui.write(b"hello\r");
            let ok = tui.wait_for(b"REALCHAIN-REPLAY-OK", 0, Duration::from_secs(60));
            let stats = gateway.stats();
            let screen = tui.screen_text();
            tui.shutdown(Duration::from_secs(10));
            service_stop(&box_);
            eprintln!("--- rust omo bootstrap --- gateway {stats}");
            eprintln!("--- rust omo screen ---\n{screen}");
            if !ok {
                // W13 finding: the plugin-loaded rust main turn never
                // reaches the provider — recorded as an exported
                // divergence by the scenario test, not a record-lane
                // failure (there is no request to record).
                eprintln!("w13x_omo_bootstrap_seed: rust leg did not reply (known W13 export)");
            }
        }
        drop(gateway);
        drop(lane);
    }
}

/// Corpus layout sanity (no TUI): the seed bodies exist and the
/// recorded corpora carry the four payload cassettes per engine.
#[test]
fn w13x_corpus_layout() {
    if std::env::var("W13_RECORD").is_ok() {
        // The record run itself populates the corpora — layout is a
        // POST-record sanity.
        eprintln!("w13x_corpus_layout: skipped during record");
        return;
    }
    for (suffix, _prompt) in G_PAYLOADS {
        let seed = std::fs::read_to_string(w13_seed(suffix))
            .unwrap_or_else(|e| panic!("read seed {suffix}: {e}"));
        assert!(seed.contains("message_stop"), "seed {suffix} is not an SSE body");
    }
    for dir in [w13_corpus_dir(false), w13_corpus_dir(true)] {
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("corpus dir {}: {e}", dir.display()));
        let count = entries.count();
        assert!(
            count >= 4,
            "corpus dir {} must carry the 4 payload cassettes (has {count})",
            dir.display()
        );
    }
}
