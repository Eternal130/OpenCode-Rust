//! Standalone side-by-side differential runner (lane/diff-rt).
//!
//! Runs the JSON script corpus against BOTH TUIs (v2.0.24 via bun +
//! our `opencode` CLI), same project dir, one shared stub, and prints
//! the human-readable mismatch report. Also writes artifacts under
//! `/tmp/opencode/tui-diff/` (per-step grid dumps of mismatching
//! steps, boot-failure captures).
//!
//! ```text
//! tools/dev-gate.sh 'cargo run -p parity --bin tui-diff -- --runs 2'
//! ```
//!
//! Flags:
//! * `--runs N`       — repeat the corpus N times (default 1; 2 gives
//!                      the deterministic-vs-timing split).
//! * `--filter ID`    — run one script only.
//! * `--sizes WxH,…`  — sizes (default `80x24,120x30`).
//! * `--out DIR`      — artifact root (default `/tmp/opencode/tui-diff`).
//! * `--no-ref`       — driver smoke mode: our side only.
//! * `--strict`       — exit 1 on any driver error or MISMATCH step
//!                      (for CI once the backlog is burned down).

use std::path::PathBuf;

use parity::tui_diff::{self, RigConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut cfg = RigConfig::default_config();
    let mut strict = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--runs" => {
                cfg.runs = args.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            }
            "--filter" => cfg.script_filter = args.next(),
            "--sizes" => {
                if let Some(spec) = args.next() {
                    let sizes: Vec<(u16, u16)> =
                        spec.split(',').filter_map(tui_diff::parse_size_spec).collect();
                    if !sizes.is_empty() {
                        cfg.sizes = sizes;
                    }
                }
            }
            "--out" => {
                if let Some(dir) = args.next() {
                    cfg.out_dir = PathBuf::from(dir);
                }
            }
            "--no-ref" => cfg.no_ref = true,
            "--strict" => strict = true,
            other => anyhow::bail!("unknown flag {other}"),
        }
    }
    // Anchor the out dir to THIS process's cwd: a relative --out would
    // leak relative paths into the spawned stacks (OPENCODE_CONFIG is
    // resolved against the SERVER's cwd, which mirrors the reference
    // checkout — a relative config path silently drops the rig's
    // provider/model pin and the run degrades invisibly).
    if cfg.out_dir.is_relative() {
        let cwd = std::env::current_dir()?;
        cfg.out_dir = cwd.join(&cfg.out_dir);
    }
    std::fs::create_dir_all(&cfg.out_dir)?;
    let report = tui_diff::run_corpus(&cfg).await?;
    let text = report.render();
    let path = cfg.out_dir.join("report.md");
    std::fs::write(&path, &text)?;
    println!("{text}");
    eprintln!("[tui-diff] report written to {}", path.display());
    if strict {
        for run in &report.runs {
            anyhow::ensure!(
                run.error.is_none(),
                "strict: {} @ {}x{} driver error",
                run.script_id,
                run.cols,
                run.rows
            );
            anyhow::ensure!(
                run.mismatch_steps() == 0,
                "strict: {} @ {}x{} has {} MISMATCH steps",
                run.script_id,
                run.cols,
                run.rows,
                run.mismatch_steps()
            );
        }
    }
    Ok(())
}
