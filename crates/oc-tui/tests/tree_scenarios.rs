//! Lane TB — the dual-engine tree-scenario gate (oc-tui production
//! tree ⇄ opentui 0.5.14 oracle).
//!
//! Three standing proofs over one fixture table (≥6 representative
//! application states, driven through the same headless assembly
//! seam the golden tests use — `reset_hub` → store mutations →
//! `render_app`):
//!
//! 1. `scenarios_match_committed_captures` — the capturer's output
//!    for every state is byte-identical to the checked-in scenario
//!    under `tools/parity/tree-scenarios/` (regeneration is the
//!    explicit `tools/parity/tree-scenario-capture.sh` script AND the
//!    `OC_TUI_TREE_CAPTURE_UPDATE=1` env var — the PARITY_UPDATE
//!    stop-and-escalate discipline).
//! 2. `capture_is_deterministic` — re-driving the same state on a
//!    fresh hub/store produces byte-identical JSON (no mount-order,
//!    Signal-side-effect, or clock dependence in the walk).
//! 3. `dual_engine_matches_frozen_oracle` — every committed scenario
//!    desugars + replays through opentui's OWN tree rebuilder
//!    (`opentui_diff::tree` / `opentui_diff::scenario`) and the
//!    settle frame compares cell-for-cell IDENTICAL against the
//!    frozen TS-oracle capture (`tools/parity/tree-scenarios/oracle/`,
//!    frozen by `tools/parity/tree-oracle-freeze.sh` — the REAL
//!    @opentui 0.5.14 driven by bun). The frozen capture's scenario
//!    fingerprint must also match the committed scenario file, so a
//!    scenario edit without an oracle re-freeze fails loudly.
//!    Skips (pass) only when the oracle dir is absent — the freeze
//!    artifacts are committed, so in-repo runs always exercise it.

use oc_tui::components::tree_capture::capture_app_state;
use oc_tui::data::{Ingest, SessionSnapshot};
use oc_tui::reactive::Runtime;
use oc_tui::store::Store;

/// The committed scenario directory (repo root / tools/parity).
fn scenario_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/parity/tree-scenarios")
}

/// Boot a session-backed store: one resynced session (s1) with a
/// user + assistant exchange (a CJK run pins the wide-cluster
/// slicing), routed to the session screen.
fn boot_session(store: &Store) {
    use oc_tui::components::message::model::{Message, Part, PartTime};
    store.ingest_all([
        Ingest::Resync {
            sessions: vec![SessionSnapshot {
                id: "s1".to_owned(),
                title: Some("Fix the login bug".to_owned()),
                ..SessionSnapshot::default()
            }],
            unread: Vec::new(),
            messages: Vec::new(),
            cursors: Vec::new(),
            permissions: Vec::new(),
            forms: Vec::new(),
        },
        Ingest::MessagesSnapshot {
            session_id: "s1".to_owned(),
            messages: vec![
                Message::User {
                    id: "u1".to_owned(),
                    text: "fix the login bug — 修复登录页的宽字符换行".to_owned(),
                    created: 1,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "a1".to_owned(),
                    agent: "build".to_owned(),
                    model: "p/m".to_owned(),
                    parts: vec![
                        Part::Reasoning { text: "check the wrap path".to_owned(), time: PartTime::default() },
                        Part::Text { text: "done — the wrap now slices on cluster bounds".to_owned(), time: PartTime::default() },
                    ],
                    finish: Some("stop".to_owned()),
                    error: None,
                    time: PartTime::default(),
                    tokens: Some(0),
                    usage_total: Some(0),
                    cache_read: None,
                    retry: None,
                },
            ],
        },
    ]);
    store.select_tab("s1");
}

/// The fixture table — ≥6 representative application states. Each
/// entry captures through a headless twin at a fixed size; the
/// committed JSON carries the `source` note.
struct StateSpec {
    name: &'static str,
    source: &'static str,
    mount: Mount,
}

/// How a state mounts its shared tree.
enum Mount {
    /// The full headless app assembly (fresh store + hub → mutations →
    /// `render_app`): every family that rides the LAST sync batch
    /// (route skeleton, devtools, dialogs, overlays) settles in the
    /// tree.
    App(fn(&Store)),
    /// The diff route's own headless twin (`render_diff_viewer` =
    /// `reset_hub` → `mount_diff_route` → the composite seam): the
    /// route family mounts + composites INSIDE `render_app`'s route
    /// stage, so the end-of-frame `prune_stale` (a family its lane did
    /// not sync in the final batch unmounts) drops it from the settled
    /// tree even though its ink is in the frame — the twin is the
    /// documented capture seam for this family (diff_viewer_solid's
    /// own golden tests mount the same way).
    DiffTwin,
}

/// The diff twin's fixture (one modified Rust file, unified view).
fn diff_files() -> Vec<oc_tui::components::diff_viewer::DiffFile> {
    use oc_tui::components::diff_viewer::{DiffFile, FileStatus};
    vec![DiffFile {
        file: "src/main.rs".to_owned(),
        additions: 1,
        deletions: 1,
        status: FileStatus::Modified,
        patch: "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"hello\");\n+    println!(\"world\");\n }\n"
            .to_owned(),
        image: false,
        image_data: None,
    }]
}

fn capture_value(spec: &StateSpec) -> Option<serde_json::Value> {
    match spec.mount {
        Mount::App(drive) => {
            capture_app_state(spec.name, spec.source, SIZE.0, SIZE.1, drive)
        }
        Mount::DiffTwin => {
            let theme = oc_tui::theme::ThemeState::v2_dark();
            let files = diff_files();
            let _frame = oc_tui::components::diff_viewer::render_diff_viewer(
                &files,
                &oc_tui::components::diff_viewer::DiffViewerState::default(),
                &oc_tui::components::diff_viewer::DiffViewerOptions {
                    source_label: "Uncommitted",
                    source_detail: "",
                    viewport_height: usize::from(SIZE.1),
                },
                &theme.styles(),
                SIZE.0.into(),
                SIZE.1.into(),
            );
            oc_tui::components::tree_capture::capture_tree_scenario(
                spec.name, spec.source, SIZE.0, SIZE.1,
            )
        }
    }
}

fn states() -> Vec<StateSpec> {
    vec![
        StateSpec {
            name: "tree-home",
            source: "fresh boot — the home route (recents skeleton, no session)",
            mount: Mount::App(|_| {}),
        },
        StateSpec {
            name: "tree-session-transcript",
            source: "session route — transcript (user/assistant + CJK) over the composer",
            mount: Mount::App(boot_session),
        },
        StateSpec {
            name: "tree-dialog",
            source: "session route with the diff base dialog open (portal overlay + scrim)",
            mount: Mount::App(|store| {
                boot_session(store);
                store.open_diff_base_dialog();
            }),
        },
        StateSpec {
            name: "tree-toast-overlay",
            source: "session route with a warning toast + action riding the overlay",
            mount: Mount::App(|store| {
                boot_session(store);
                store.toast_show(oc_tui::components::toast::ToastOptions {
                    title: Some("MCP server failed".to_owned()),
                    message: "context crashed — retry the connection".to_owned(),
                    variant: "warning",
                    duration_ms: 6000,
                    action_label: Some("Retry".to_owned()),
                    action_command: Some("mcp.list".to_owned()),
                    modal: false,
                });
            }),
        },
        StateSpec {
            name: "tree-diff-viewer",
            source:
                "the diff plugin route's headless twin — loaded unified patch on the DiffRenderable",
            mount: Mount::DiffTwin,
        },
        StateSpec {
            name: "tree-stats-page",
            source: "the stats plugin route — the scrollbox-centered poster",
            mount: Mount::App(|store| {
                boot_session(store);
                store.open_stats_page(0);
            }),
        },
        StateSpec {
            name: "tree-chrome-full",
            source: "chrome all on — tabs rail (2 sessions) + transcript + toast + devtools bar",
            mount: Mount::App(|store| {
                boot_session(store);
                store.ingest_all([Ingest::Resync {
                    sessions: vec![
                        SessionSnapshot {
                            id: "s1".to_owned(),
                            title: Some("Fix the login bug".to_owned()),
                            ..SessionSnapshot::default()
                        },
                        SessionSnapshot {
                            id: "s2".to_owned(),
                            title: Some("Review the PR".to_owned()),
                            ..SessionSnapshot::default()
                        },
                    ],
                    unread: vec!["s2".to_owned()],
                    messages: Vec::new(),
                    cursors: Vec::new(),
                    permissions: Vec::new(),
                    forms: Vec::new(),
                }]);
                store.toast_show(oc_tui::components::toast::ToastOptions {
                    title: None,
                    message: "Copied session transcript".to_owned(),
                    variant: "success",
                    duration_ms: 4000,
                    action_label: None,
                    action_command: None,
                    modal: false,
                });
                store.set_devtools_enabled(true);
                store.set_devtools_bar_visible(true);
            }),
        },
    ]
}

const SIZE: (u16, u16) = (100, 30);

fn capture_pretty(spec: &StateSpec) -> String {
    let value = capture_value(spec)
        .unwrap_or_else(|| panic!("{}: no hub tree mounted", spec.name));
    serde_json::to_string_pretty(&value).expect("scenario serialization")
}

/// 1 — the committed captures pin the capturer's output per state.
#[test]
fn scenarios_match_committed_captures() {
    let update = std::env::var("OC_TUI_TREE_CAPTURE_UPDATE").is_ok();
    for spec in states() {
        let json = capture_pretty(&spec);
        let path = scenario_dir().join(format!("{}.json", spec.name));
        if update {
            std::fs::create_dir_all(scenario_dir()).expect("scenario dir");
            std::fs::write(&path, json.as_bytes()).expect("scenario write");
            eprintln!("updated {}", path.display());
            continue;
        }
        let committed =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: read {e}", path.display()));
        assert_eq!(
            committed, json,
            "{}: capture drifted from the committed scenario (regenerate via \
             tools/parity/tree-scenario-capture.sh)",
            spec.name
        );
    }
}

/// The rewrite arm — ONLY reachable through the explicit script (the
/// test asserts the env gate, so a bare `nextest run` cannot rewrite).
#[test]
#[ignore = "explicit regeneration only: tools/parity/tree-scenario-capture.sh"]
fn scenario_capture_rewrite() {
    assert!(
        std::env::var("OC_TUI_TREE_CAPTURE_UPDATE").is_ok(),
        "refusing: OC_TUI_TREE_CAPTURE_UPDATE is not set"
    );
    for spec in states() {
        let json = capture_pretty(&spec);
        let path = scenario_dir().join(format!("{}.json", spec.name));
        std::fs::create_dir_all(scenario_dir()).expect("scenario dir");
        std::fs::write(&path, json.as_bytes()).expect("scenario write");
    }
}

/// 2 — serialization determinism: a fresh twin per capture produces
/// byte-identical JSON, and a THIRD re-render of the same settled
/// store (extra mounts, same state) must not move the walk either.
#[test]
fn capture_is_deterministic() {
    for spec in states() {
        let first = capture_pretty(&spec);
        let second = capture_pretty(&spec);
        assert_eq!(first, second, "{}: fresh-twin capture not reproducible", spec.name);
        assert!(
            first.lines().count() > 40,
            "{}: suspiciously small capture — did anything mount?",
            spec.name
        );
    }
}

/// The opentui-diff capture fingerprint of a scenario file (FNV-1a,
/// the same word the freeze harness writes — src/main.rs `fnv1a_hex`).
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// PORT-side divergences surfaced by the tree gate and pinned while a
/// fix lands upstream (see git history for the Lane TB finding that
/// used this: opentui-core's nested-Diff row backgrounds, healed by
/// the bump to 2fbe00e — oracle re-frozen, pin dropped). Keep the
/// mechanism: a new mismatch is pinned here with its EXACT signature
/// (mismatched cells / field diffs / first-diff cell) so an opentui
/// rev bump that changes it fails LOUDLY — re-freeze and drop the pin
/// then (stop-and-escalate).
const PINNED_PORT_MISMATCHES: &[(&str, usize, usize, usize, usize, &str)] = &[
    // name, mismatched_cells, field diffs, first-diff x, y, field
];

/// 3 — the dual-engine standing gate: the committed scenario replays
/// through opentui's own rebuilder and matches the frozen oracle
/// frame cell-for-cell.
#[test]
fn dual_engine_matches_frozen_oracle() {
    let oracle_dir = scenario_dir().join("oracle");
    if !oracle_dir.is_dir() {
        eprintln!("SKIP: {} absent — freeze via tools/parity/tree-oracle-freeze.sh", oracle_dir.display());
        return;
    }
    let mut ran = 0usize;
    for spec in states() {
        let scenario_path = scenario_dir().join(format!("{}.json", spec.name));
        let oracle_path = oracle_dir.join(format!("{}.json", spec.name));
        let Some(oracle_json) = std::fs::read_to_string(&oracle_path).ok() else {
            eprintln!("SKIP {}: no frozen oracle ({})", spec.name, oracle_path.display());
            continue;
        };
        ran += 1;
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&scenario_path).expect("scenario read"))
                .expect("scenario parse");
        let desugared = opentui_diff::tree::desugar_scenario(raw).expect("tree desugar");
        let scenario: opentui_diff::scenario::Scenario =
            serde_json::from_value(desugared).expect("scenario load");
        let rust = opentui_diff::scenario::render_rust(&scenario).expect("port replay");
        let oracle: opentui_diff::scenario::Capture =
            serde_json::from_str(&oracle_json).expect("oracle parse");
        // Freeze coupling: the oracle must have been captured from
        // THIS scenario file (the harness's own cache discipline).
        let scenario_hash = fnv1a_hex(&std::fs::read(&scenario_path).expect("scenario bytes"));
        assert_eq!(
            oracle.capture.as_ref().expect("fingerprinted oracle").scenario,
            scenario_hash,
            "{}: frozen oracle predates the committed scenario — re-freeze via \
             tools/parity/tree-oracle-freeze.sh",
            spec.name
        );
        let report = opentui_diff::compare::compare_filtered(&oracle, &rust, false);
        if let Some(&(_, cells, fields, x, y, field)) = PINNED_PORT_MISMATCHES
            .iter()
            .find(|(n, ..)| *n == spec.name)
        {
            assert!(
                !report.identical,
                "{}: pinned port mismatch has HEALED ({} vs {} cells) — re-freeze the oracle \
                 and drop the pin entry",
                spec.name,
                report.mismatched_cells,
                report.total_cells
            );
            assert_eq!(
                (report.mismatched_cells, report.diffs.len()),
                (cells, fields),
                "{}: the pinned divergence signature changed — re-triage before re-pinning",
                spec.name
            );
            let first = &report.diffs[0];
            assert_eq!(
                (first.x, first.y, first.field),
                (x, y, field),
                "{}: the first differing cell moved",
                spec.name
            );
            println!(
                "{}: PINNED PORT MISMATCH (opentui-core nested-diff row bg is \
                 layout-relative, upstream absolute) — {}/{} cells, signature intact",
                spec.name, report.mismatched_cells, report.total_cells
            );
            continue;
        }
        assert!(
            report.identical,
            "{}: DUAL-ENGINE MISMATCH — {} of {} cells differ\n{}\n{}",
            spec.name,
            report.mismatched_cells,
            report.total_cells,
            report.oracle_frame,
            report.rust_frame
        );
        println!("{}: IDENTICAL vs frozen @opentui 0.5.14 oracle ({} cells)", spec.name, report.total_cells);
    }
    assert!(ran > 0, "no frozen oracles found in {}", oracle_dir.display());
}
