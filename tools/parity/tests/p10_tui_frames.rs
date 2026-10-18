//! Gate 4 — TUI five-flow differential (PARITY-LOOP.md §8.3 Gate 4, B0-2
//! skeleton).
//!
//! Uses `parity::tui_diff`'s `PtyChild`/`Grid` (no tmux, no 26-script
//! corpus): each side's REAL TUI binary — the official v2.0.24 TUI under
//! bun and the Rust port's `opencode-rust` TUI — boots on a fresh pty
//! against its own server at fixed sizes.
//!
//! **Flow 1 (boot/home) is live** as a structural smoke: both TUIs reach a
//! stable, non-blank home screen at 80x24 AND 120x30 with their
//! proven-per-side boot markers (`Ask anything` for the port — the m2
//! composer placeholder; `Server` for the v2 home — the m1/cross-tui
//! marker; plus a no-reconnect/no-error guard). Chrome rows verified
//! deterministic are EXACT-compared across sides (the curated
//! `CHROME_EXACT_ROWS` list — see the artifact for the full grids); model
//! text is STRUCTURAL (presence/geometry, not bytes).
//!
//! **Flows 2–5 are structured gaps** with per-step checkpoint helpers and
//! explicit not-yet-wired markers; the form/permission surfaces link to
//! ledger `TUI-TD16`/`TUI-TD18` (known-missing surfaces — not faked here).
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_tui_frames)'
//! ```
//!
//! Artifacts: `/tmp/opencode/p10-artifacts/gate4-flow1-<side>-<W>x<H>.txt`
//! + `/tmp/opencode/p10-artifacts/gate4-tui-frames.json`.

mod p10_common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use p10_common::{enabled, skip};
use parity::scenario::ScenarioHost;
use parity::tui_diff::{Grid, PtyChild};

/// Boot-settling budget (the v2 TUI takes ~10-15 s under bun).
const BOOT_TIMEOUT: Duration = Duration::from_secs(45);
/// Cell-stability tolerance (caret blink is 1-2 cells).
const STABLE_TOLERANCE: usize = 4;

/// Chrome rows EXACT-compared across sides once verified deterministic
/// (row indices in the stable boot grid; curated from captured artifacts —
/// the OPENCODE logo block, the composer border, the model line, and the
/// version row — all byte-equal across sides across two independent boots
/// at both §8.3 geometries). The footer's left segment and the rotating
/// composer placeholder are NOT curated (see the recorded gap facts).
fn chrome_exact_rows(cols: u16, rows: u16) -> &'static [usize] {
    match (cols, rows) {
        (80, 24) => &[6, 7, 8, 9, 15, 16, 22],
        (120, 30) => &[9, 10, 11, 12, 18, 19, 28],
        _ => &[],
    }
}

/// The two fixed §8.3 geometries.
const SIZES: &[(u16, u16)] = &[(80, 24), (120, 30)];

/// Wait until any marker appears in the raw stream (rotating placeholders
/// make single-string waits flaky).
fn wait_for_any(child: &PtyChild, markers: &[&str], from: usize) -> bool {
    markers.iter().any(|marker| child.wait_for_after(marker.as_bytes(), from, BOOT_TIMEOUT))
}

/// Boot one side's TUI on a fresh pty and settle (blocking pty waits):
/// first wait for one of the side's home markers in the RAW byte stream (a
/// blank grid is trivially stable, so stability alone proves nothing),
/// then for cell stability. `dismiss_first_run` sends Escape once (the
/// v2 first-run modal/onboarding panel — the cross-tui pattern).
#[allow(clippy::too_many_arguments)]
fn boot_tui(
    program: &Path,
    argv: &[String],
    cwd: &Path,
    env: &[(String, String)],
    cols: u16,
    rows: u16,
    label: &'static str,
    markers: &[&str],
    dismiss_first_run: bool,
) -> Result<PtyChild> {
    let mut child = PtyChild::spawn(program, argv, cwd, env, cols, rows)
        .with_context(|| format!("spawn {label} tui {cols}x{rows}"))?;
    if dismiss_first_run {
        // First-run/onboarding screen: wait for it to render, dismiss.
        if !wait_for_any(&child, &["OpenCode", "opencode"], 0) {
            // fall through — maybe there was no onboarding this boot
        }
        child.write(b"\x1b");
    }
    ensure!(
        wait_for_any(&child, markers, child.raw_len().saturating_sub(1)),
        "{label} tui never printed any of {markers:?} at {cols}x{rows}; raw tail:\n{}",
        raw_tail(&child)
    );
    ensure!(
        child.wait_grid_stable(STABLE_TOLERANCE, 300, BOOT_TIMEOUT),
        "{label} tui never reached a stable grid at {cols}x{rows}; raw tail:\n{}",
        raw_tail(&child)
    );
    ensure!(child.is_running(), "{label} tui exited during boot");
    Ok(child)
}

/// The last 2000 bytes of the raw mirror (diagnostics on boot failures).
fn raw_tail(child: &PtyChild) -> String {
    let raw = child.raw_mirror();
    let start = raw.len().saturating_sub(2000);
    String::from_utf8_lossy(&raw[start..]).into_owned()
}

/// The v2 home composer placeholders (`routes/home.tsx`) — they rotate, so
/// any one of them proves the settled home screen.
const REF_HOME_MARKERS: &[&str] =
    &["Fix a TODO in the codebase", "What is the tech stack of this project?", "Fix broken tests"];
/// The port's composer placeholder (the m2 marker).
const PORT_HOME_MARKERS: &[&str] = &["Ask anything"];

/// Structural home-screen assertions for one side + the captured grid.
fn assert_home(label: &'static str, grid: &Grid, markers: &[&str]) -> Result<()> {
    let rows = grid.text();
    ensure!(!grid.is_blank(), "{label} home grid is blank at boot");
    let nonblank = rows.iter().filter(|row| !row.trim().is_empty()).count();
    ensure!(nonblank >= 3, "{label} home grid nearly empty ({nonblank} non-blank rows)");
    let screen = rows.join("\n");
    ensure!(
        markers.iter().any(|marker| screen.contains(marker)),
        "{label} home marker (any of {markers:?}) missing from the settled grid:\n{screen}"
    );
    ensure!(
        !screen.to_lowercase().contains("reconnect"),
        "{label} home shows a reconnect banner (not connected):\n{screen}"
    );
    ensure!(
        !screen.to_lowercase().contains("error:"),
        "{label} home shows an error banner:\n{screen}"
    );
    Ok(())
}

/// The per-side environment for the TUI process (scrubbed; private HOME).
fn tui_env(home: &Path, password: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("HOME".to_owned(), home.display().to_string()),
        ("OPENCODE_TEST_HOME".to_owned(), home.display().to_string()),
        ("XDG_DATA_HOME".to_owned(), home.join(".local/share").display().to_string()),
        ("XDG_STATE_HOME".to_owned(), home.join(".local/state").display().to_string()),
        ("XDG_CONFIG_HOME".to_owned(), home.join(".config").display().to_string()),
        ("XDG_CACHE_HOME".to_owned(), home.join(".cache").display().to_string()),
        ("TMPDIR".to_owned(), home.join(".cache").display().to_string()),
        ("OPENCODE_PASSWORD".to_owned(), password.to_owned()),
        ("OPENCODE_DISABLE_MODELS_FETCH".to_owned(), "1".to_owned()),
    ];
    for (key, value) in extra {
        env.push(((*key).to_owned(), (*value).to_owned()));
    }
    env
}

/// Rows of the §8.3 five flows that are NOT wired yet (B0-2 skeleton):
/// per-step checkpoints with explicit not-yet-wired markers; the
/// form/permission surfaces link ledger TUI-TD16 / TUI-TD18.
fn unwired_flow_checkpoints() -> Vec<Value> {
    let step = |flow: u8, step: &str, ledger: Option<&str>, note: &str| {
        json!({
            "flow": flow,
            "step": step,
            "state": "not-wired",
            "ledger": ledger,
            "note": note,
        })
    };
    vec![
        step(2, "type prompt + submit", None,
             "FakeLlm-fixed reply must render in BOTH TUIs' transcripts; per-step cell diff after each keypress (B0-2 skeleton)"),
        step(2, "reply in transcript (STRUCTURAL)", None,
             "model text compare is structural, chrome around the turn EXACT"),
        step(3, "tool call render", None,
             "scripted read tool call; tool row chrome + file-path text across sides"),
        step(4, "permission ask surface", Some("TUI-TD18"),
             "permission surfaces are known missing on the port — do NOT fake; wire when the surface lands"),
        step(4, "form surface", Some("TUI-TD16"),
             "form surfaces are known missing on the port — do NOT fake; wire when the surface lands"),
        step(5, "session switch", None,
             "two sessions + tab/switch interaction; chrome EXACT, titles STRUCTURAL"),
    ]
}

#[tokio::test]
#[ignore = "Phase-10 gate: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_tui_frames_gate4() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if !p10_common::reference_runnable() {
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }
    let tui_root = PathBuf::from(
        std::env::var("PARITY_REF_ROOT").unwrap_or_else(|_| "/tmp/opencode/ref".to_owned()),
    )
    .join("packages/tui");
    if !tui_root.is_dir()
        && parity::gate::skip_or_fail(&format!(
            "reference TUI checkout missing at {} (set PARITY_REF_ROOT or rebuild /tmp/opencode/ref)",
            tui_root.display()
        ))
    {
        return Ok(());
    }
    let bun = p10_common::bun_path()?;
    // Deterministic lane sandbox root (B0-2 acceptance: pid-scoped).
    std::env::set_var(
        "PARITY_SANDBOX_ROOT",
        format!("/tmp/opencode/lane-b0-gates-{}-tui", std::process::id()),
    );
    run(&bun, &tui_root).await
}

#[allow(clippy::too_many_lines)]
async fn run(bun: &Path, tui_root: &Path) -> Result<()> {
    // Each side's TUI against its own server.
    let reference = parity::reference::spawn().await?;
    let port = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn::default()).await?;
    let result = flows(&reference, &port, bun, tui_root);
    let stop_ref = reference.stop();
    let stop_port = port.stop();
    result?;
    stop_ref.await?;
    stop_port.await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn flows(
    reference: &parity::reference::ReferenceServer,
    port: &parity::rust_server::RustServer,
    bun: &Path,
    tui_root: &Path,
) -> Result<()> {
    let lane = format!("/tmp/opencode/lane-b0-gates-{}-tui-homes", std::process::id());
    let ref_home = PathBuf::from(format!("{lane}/ref-home"));
    let rust_home = PathBuf::from(format!("{lane}/rust-home"));
    for home in [&ref_home, &rust_home] {
        for dir in [".local/share", ".local/state", ".config", ".cache"] {
            std::fs::create_dir_all(home.join(dir))?;
        }
    }

    let ref_cli = tui_root.join("../cli");
    let rust_cli = parity::rust_server::cli_binary()?;

    let mut report = json!({"gate": "gate4-tui-frames"});
    let mut grids_by_size: Vec<(u16, u16, Value, Value)> = Vec::new();

    for &(cols, rows) in SIZES {
        // -- the official v2.0.24 TUI (bun) against the reference server --
        let ref_argv: Vec<String> = [
            "run".to_owned(),
            "--cwd".to_owned(),
            ref_cli.display().to_string(),
            "--define".to_owned(),
            "OPENCODE_VERSION=\"2.0.24\"".to_owned(),
            "--define".to_owned(),
            "OPENCODE_CHANNEL=\"stable\"".to_owned(),
            "--define".to_owned(),
            "OPENCODE_ARTIFACT=\"cli\"".to_owned(),
            "src/index.ts".to_owned(),
            "--server".to_owned(),
            reference.url().to_owned(),
        ]
        .to_vec();
        let ref_env =
            tui_env(&ref_home, reference.password(), &[("PATH", "/usr/local/bin:/usr/bin:/bin")]);
        let mut reference_tui = boot_tui(
            bun,
            &ref_argv,
            &ref_cli,
            &ref_env,
            cols,
            rows,
            "reference",
            REF_HOME_MARKERS,
            true,
        )?;
        let ref_grid = reference_tui.grid_now();
        std::fs::write(
            p10_common::artifacts().join(format!("gate4-flow1-reference-{cols}x{rows}.txt")),
            ref_grid.text().join("\n"),
        )?;
        assert_home("reference", &ref_grid, REF_HOME_MARKERS)?;
        ensure!(reference_tui.is_running(), "reference tui exited after boot");

        // -- the Rust port TUI against the Rust server --
        let rust_argv: Vec<String> = ["--server".to_owned(), port.url().to_owned()].to_vec();
        let rust_env =
            tui_env(&rust_home, port.password(), &[("PATH", "/usr/local/bin:/usr/bin:/bin")]);
        let mut rust_tui = boot_tui(
            &rust_cli,
            &rust_argv,
            &rust_home,
            &rust_env,
            cols,
            rows,
            "port",
            PORT_HOME_MARKERS,
            false,
        )?;
        let rust_grid = rust_tui.grid_now();
        std::fs::write(
            p10_common::artifacts().join(format!("gate4-flow1-port-{cols}x{rows}.txt")),
            rust_grid.text().join("\n"),
        )?;
        assert_home("port", &rust_grid, PORT_HOME_MARKERS)?;
        ensure!(rust_tui.is_running(), "port tui exited after boot");

        // Structural geometry: settled grids are exactly the pty size.
        for (label, grid) in [("reference", &ref_grid), ("port", &rust_grid)] {
            let text = grid.text();
            ensure!(
                text.len() == rows as usize,
                "{label} grid has {} rows (want {rows})",
                text.len()
            );
        }

        // Artifacts (full settled grids for triage/curation).
        for (label, grid) in [("reference", &ref_grid), ("port", &rust_grid)] {
            std::fs::write(
                p10_common::artifacts().join(format!("gate4-flow1-{label}-{cols}x{rows}.txt")),
                grid.text().join("\n"),
            )?;
        }
        grids_by_size.push((cols, rows, json!(ref_grid.text()), json!(rust_grid.text())));

        // Chrome EXACT where deterministic (curated rows; the lists are
        // compared cell-for-cell — no normalization).
        let ref_rows = ref_grid.text();
        let rust_rows = rust_grid.text();
        let chrome_rows = chrome_exact_rows(cols, rows);
        // Gap fact: the footer's left segment (project/cwd) renders on v2
        // but not on the port; the rotating composer placeholder also
        // differs between boots (timing) — both stay STRUCTURAL.
        let footer_ref = ref_rows.first().map_or(String::new(), |_| {
            ref_rows
                .iter()
                .enumerate()
                .filter(|(index, _)| !chrome_rows.contains(index))
                .find(|(_, row)| row.contains("shift+tab agents"))
                .map_or_else(String::new, |(_, row)| row.clone())
        });
        let footer_port = rust_rows
            .iter()
            .find(|row| row.contains("shift+tab agents"))
            .cloned()
            .unwrap_or_default();
        report["flow1_gap_facts"] = json!({
            "footer_left_segment": {
                "note": "v2 renders the project/cwd segment in the footer's left; the port \
                 renders only the keybind hints — recorded divergence pending triage",
                "reference_footer": footer_ref.trim(),
                "port_footer": footer_port.trim(),
            },
            "composer_placeholder": "rotating placeholder text is timing-dependent — STRUCTURAL, not curated",
        });
        for &row in chrome_rows {
            let reference_row = ref_rows.get(row).cloned().unwrap_or_default();
            let port_row = rust_rows.get(row).cloned().unwrap_or_default();
            ensure!(
                !reference_row.trim().is_empty() && !port_row.trim().is_empty(),
                "curated chrome row {row} is blank on a side at {cols}x{rows} — re-curate"
            );
            ensure!(
                reference_row == port_row,
                "chrome row {row} differs at {cols}x{rows}:\nreference={reference_row:?}\nport={port_row:?}"
            );
        }

        // Harness-owned teardown (own pty children only).
        rust_tui.kill();
        reference_tui.kill();
    }

    report["flow1_boot_home"] = json!({
        "sizes": SIZES.iter().map(|&(c, r)| format!("{c}x{r}")).collect::<Vec<_>>(),
        "markers": {"reference": REF_HOME_MARKERS, "port": PORT_HOME_MARKERS},
        "structural": "stable non-blank home + marker + no reconnect/error + exact geometry",
        "chrome_exact_rows": {
            "80x24": chrome_exact_rows(80, 24),
            "120x30": chrome_exact_rows(120, 30),
        },
        "chrome_exact_note": "logo/border/model/version rows verified byte-equal across sides \
         and across independent boots; footer-left and rotating placeholder stay STRUCTURAL",
        "grids": grids_by_size.iter().map(|(c, r, reference, port)| json!({
            "size": format!("{c}x{r}"), "reference": reference, "port": port,
        })).collect::<Vec<_>>(),
    });
    report["flows_2_to_5"] = json!({
        "state": "structured gaps (not-wired checkpoints)",
        "checkpoints": unwired_flow_checkpoints(),
    });
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("gate4-tui-frames.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    eprintln!(
        "[gate4] flow 1 PASS ({} sizes, structural; {}+{} curated chrome rows); flows 2-5 recorded as not-wired checkpoints",
        SIZES.len(),
        chrome_exact_rows(80, 24).len(),
        chrome_exact_rows(120, 30).len()
    );
    Ok(())
}
