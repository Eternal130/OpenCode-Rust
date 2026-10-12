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
//!   the deterministic-vs-timing split).
//! * `--filter ID`    — run one script only.
//! * `--sizes WxH,…`  — sizes (default `80x24,120x30`).
//! * `--out DIR`      — artifact root (default `/tmp/opencode/tui-diff`).
//! * `--no-ref`       — driver smoke mode: our side only.
//! * `--strict` — exit 1 on any driver error or MISMATCH step (for CI
//!   once the backlog is burned down).
//! * `--probe error-tool [SECS]` — run the corpus3 #39 retry-cadence
//!   probe instead of the corpus: both stacks loop a deterministically
//!   failing tool call for a fixed window (default 10 s); exits 1 when
//!   our request rate exceeds ~2x the ref's.

use std::path::PathBuf;

use parity::tui_diff::{self, RigConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut cfg = RigConfig::default_config();
    let mut strict = false;
    let mut probe: Option<String> = None;
    let mut probe_window = 10_u64;
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
            "--probe" => {
                probe = args.next();
                if probe.is_some() {
                    if let Some(window) = args.next().and_then(|v| v.parse::<u64>().ok()) {
                        probe_window = window;
                    }
                }
            }
            other => anyhow::bail!("unknown flag {other}"),
        }
    }
    std::fs::create_dir_all(&cfg.out_dir)?;
    if let Some(probe) = probe {
        match probe.as_str() {
            "error-tool" => {
                let report = tui_diff::run_error_tool_probe(
                    &cfg,
                    std::time::Duration::from_secs(probe_window),
                )
                .await?;
                let text = report.render();
                let path = cfg.out_dir.join("error-tool-probe.md");
                std::fs::write(&path, &text)?;
                println!("{text}");
                eprintln!("[tui-diff] probe report written to {}", path.display());
                anyhow::ensure!(
                    report.within_tolerance(2.0),
                    "error-tool probe: our request cadence exceeds 2x the reference's"
                );
                return Ok(());
            }
            other => anyhow::bail!("unknown probe {other}"),
        }
    }
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
