//! The OC_TUI_DIFF gated differential test (lane/diff-rt): the same
//! rig as the standalone `tui-diff` binary, runnable under nextest.
//!
//! Opt-in (the OC_TUI_SMOKE pattern): `#[ignore]`d AND requires
//! `OC_TUI_DIFF=1` at runtime:
//!
//! ```text
//! OC_TUI_DIFF=1 cargo nextest run --run-ignored only -E 'test(tui_diff)' \
//!   --no-timeout
//! ```
//!
//! Default: ONE script (`boot-home`) at 80x24 to bound test time.
//! `OC_TUI_DIFF_FULL=1` runs the whole corpus at both sizes; the
//! corpus's content mismatches are the REPORT's subject, so this test
//! asserts only that the rig ran: both sides booted, every step
//! settled, no driver error. The mismatch backlog lives in the
//! artifacts (`/tmp/opencode/tui-diff/report.md`), not in assertions.
//!
//! Skipped without the reference build (bun + ref checkout or its
//! rebuild source), exactly like the other reference-dependent tests.
//! NOTE: bun must be readable by the test user — under the dev gate
//! that means `PARITY_BUN=/tmp/opencode/bun/bin/bun` (see
//! `tools/parity/tui-diff/README.md`).

use std::time::Duration;

use parity::tui_diff::{self, RigConfig};

#[tokio::test]
#[ignore = "differential TUI rig: opt in with OC_TUI_DIFF=1 (needs bun + ref checkout)"]
async fn tui_diff_side_by_side() -> anyhow::Result<()> {
    if std::env::var("OC_TUI_DIFF").ok().as_deref() != Some("1") {
        eprintln!("skipping: OC_TUI_DIFF != 1 (opt-in; see the module docs)");
        return Ok(());
    }
    if tui_diff::find_bun().is_none() {
        eprintln!("skipping: bun not found");
        return Ok(());
    }
    if parity::reference::skip_if_no_bun() {
        eprintln!("skipping: reference checkout missing and not rebuildable");
        return Ok(());
    }

    let mut cfg = RigConfig::default_config();
    if std::env::var("OC_TUI_DIFF_FULL").ok().as_deref() != Some("1") {
        cfg.script_filter = Some("boot-home".to_owned());
        cfg.sizes = vec![(80, 24)];
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    cfg.out_dir = std::path::PathBuf::from("/tmp/opencode/tui-diff").join(format!("test-{stamp}"));

    let report = tui_diff::run_corpus(&cfg).await?;
    let text = report.render();
    std::fs::write(cfg.out_dir.join("report.md"), &text)?;
    eprintln!("[tui-diff-test] report: {}", cfg.out_dir.join("report.md").display());

    // The rig must have actually run: no driver errors, both sides
    // booted on every script run.
    for run in &report.runs {
        anyhow::ensure!(
            run.error.is_none(),
            "script {} @ {}x{} driver error: {}",
            run.script_id,
            run.cols,
            run.rows,
            run.error.clone().unwrap_or_default()
        );
        anyhow::ensure!(run.booted_ref, "ref TUI never painted ({})", run.script_id);
        anyhow::ensure!(run.booted_our, "our TUI never painted ({})", run.script_id);
        anyhow::ensure!(!run.steps.is_empty(), "no steps captured ({})", run.script_id);
    }
    // And it must have observed SOMETHING (grids captured are non-blank).
    for run in &report.runs {
        let non_blank =
            run.steps.iter().any(|step| step.our_text.iter().any(|row| !row.trim().is_empty()));
        anyhow::ensure!(non_blank, "all captured grids blank ({})", run.script_id);
    }
    let _ = Duration::from_secs(0); // keep Duration import for future gates
    Ok(())
}
