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
//! * `--quick` — iteration mode (lane/accel): `--runs 1` + the single
//!   default size 80x24 (per-script `size` overrides still win). FINAL
//!   verification always runs the full matrix (both sizes × `--runs 2`).
//! * `--out DIR`      — artifact root (default `/tmp/opencode/tui-diff`).
//! * `--no-ref`       — driver smoke mode: our side only.
//! * `--no-ref-cache` / `--ref-cache-dir DIR` — the ref-capture cache
//!   (default ON at `tools/parity/tui-diff/cache/ref`, gitignored; the
//!   env kill switch `OC_TUI_DIFF_REF_CACHE=0` also disables it).
//! * `--measure-stability N` — ref-capture determinism measurement
//!   (lane/accel): run the corpus N times as independent live
//!   CAPTURES (no replay), report which (script × size × step) ref
//!   grids are byte-identical across all N runs — the cacheability
//!   table — AND confirm the ref-cache entries (only confirmed-stable
//!   steps ever replay; this is the cache-warming invocation).
//! * `--strict` — exit 1 on any driver error or MISMATCH step (for CI
//!   once the backlog is burned down).
//! * `--probe error-tool [SECS]` — run the corpus3 #39 retry-cadence
//!   probe instead of the corpus: both stacks loop a deterministically
//!   failing tool call for a fixed window (default 10 s); exits 1 when
//!   our request rate exceeds ~2x the ref's.
//! * `--term-profile NAME` — lane/termatrix: run under a different
//!   terminal ENVIRONMENT (TERM/COLORTERM + canned answers to DA1 /
//!   DA2 / XTVERSION / kitty `CSI ? u` / OSC 11 / DECRQM queries, or
//!   inside a real tmux session). Both TUIs see identical answers.
//!   Forces `--no-ref-cache` semantics (baseline entries must never
//!   replay under another environment). `--list-term-profiles` prints
//!   the registry.

use std::collections::BTreeMap;
use std::path::PathBuf;

use parity::tui_diff::{self, RigConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Cli { cfg, strict, probe, probe_window, measure_stability } = parse_args()?;
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
    if let Some(reruns) = measure_stability {
        // The measurement IS the cache warmer: N independent live
        // CAPTURE runs (no replay — independence), then each entry is
        // CONFIRMED from the per-step digest agreement across the N
        // runs. Only confirmed-stable steps ever replay.
        let cache_dir = cfg.ref_cache.clone();
        let cfg = RigConfig { runs: reruns, capture_only: true, ..cfg };
        let report = tui_diff::run_corpus(&cfg).await?;
        if let Some(dir) = cache_dir {
            let cache = tui_diff::RefCaptureCache::open_unfingerprinted(&dir);
            // (script, size) → per-run step digests (error-free runs only).
            let mut grouped: BTreeMap<(String, u16, u16), Vec<Vec<u64>>> = BTreeMap::new();
            for run in &report.runs {
                if run.error.is_some() {
                    continue;
                }
                grouped
                    .entry((run.script_id.clone(), run.cols, run.rows))
                    .or_default()
                    .push(run.steps.iter().map(|step| step.ref_digest).collect());
            }
            for ((script, cols, rows), digests) in grouped {
                cache.confirm(&script, cols, rows, &digests);
            }
        }
        let text = render_stability(&report, reruns);
        let path = cfg.out_dir.join("stability.md");
        std::fs::write(&path, &text)?;
        println!("{text}");
        eprintln!("[tui-diff] stability measurement written to {}", path.display());
        return Ok(());
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

/// Parsed CLI knobs (the config plus the non-config flags).
struct Cli {
    cfg: RigConfig,
    strict: bool,
    probe: Option<String>,
    probe_window: u64,
    measure_stability: Option<usize>,
}

/// Parse the CLI.
fn parse_args() -> anyhow::Result<Cli> {
    let mut cfg = RigConfig::default_config();
    let mut strict = false;
    let mut probe: Option<String> = None;
    let mut probe_window = 10_u64;
    let mut measure_stability: Option<usize> = None;
    let mut list_requested = false;
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
            "--quick" => {
                // Iteration protocol (lane/accel): one run, the single
                // default size. Explicit --runs/--sizes given LATER on
                // the command line still override.
                cfg.runs = 1;
                cfg.sizes = vec![(80, 24)];
            }
            "--out" => {
                if let Some(dir) = args.next() {
                    cfg.out_dir = PathBuf::from(dir);
                }
            }
            "--no-ref" => cfg.no_ref = true,
            "--no-ref-cache" => cfg.ref_cache = None,
            "--ref-cache-dir" => {
                let dir = args.next().map(PathBuf::from);
                cfg.ref_cache = dir;
            }
            "--measure-stability" => {
                measure_stability = args.next().and_then(|v| v.parse().ok()).or(Some(5));
            }
            "--strict" => strict = true,
            "--term-profile" => {
                let name = args.next().unwrap_or_default();
                let profile = parity::termatrix::find_term_profile(&name)
                    .ok_or_else(|| anyhow::anyhow!("unknown term profile {name:?}"))?;
                cfg.term_profile = Some(profile);
                // Environment changes invalidate every baseline capture
                // — never replay across environments.
                cfg.ref_cache = None;
            }
            "--list-term-profiles" => {
                list_requested = true;
            }
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
    if list_requested {
        for id in parity::termatrix::term_profile_ids() {
            let profile = parity::termatrix::find_term_profile(id).expect("registry id resolves");
            println!(
                "{:<16} TERM={:<16} COLORTERM={:<10} tmux={:<5} {}",
                profile.id,
                profile.term,
                profile.colorterm.unwrap_or("-"),
                profile.tmux,
                profile.description
            );
        }
        std::process::exit(0);
    }
    Ok(Cli { cfg, strict, probe, probe_window, measure_stability })
}

/// The cacheability measurement: for every (script × size × step),
/// how many of the N reruns produced the SAME ref-grid digest. A step
/// is `stable` iff all N runs agree — exactly the bar the ref-capture
/// cache requires (a step that flips across runs would change cached
/// verdicts vs live ones and must never be replayed).
/// `(script, cols, rows)` → `step_index` → (label, digest → count).
type StabilityTable = BTreeMap<(String, u16, u16), BTreeMap<usize, (String, BTreeMap<u64, usize>)>>;

fn render_stability(report: &tui_diff::CorpusReport, reruns: usize) -> String {
    use std::fmt::Write as _;
    let mut grouped: StabilityTable = BTreeMap::new();
    for run in &report.runs {
        if run.error.is_some() {
            continue;
        }
        for step in &run.steps {
            let entry = grouped
                .entry((run.script_id.clone(), run.cols, run.rows))
                .or_default()
                .entry(step.step_index)
                .or_insert_with(|| (step.label.clone(), BTreeMap::new()));
            *entry.1.entry(step.ref_digest).or_default() += 1;
        }
    }
    let mut out = String::new();
    out.push_str("# Ref-capture stability measurement (lane/accel)\n\n");
    let _ = writeln!(
        out,
        "N = {reruns} independent live runs (cache disabled). A step is **stable** \
         iff its ref grid (char+style) was byte-identical in every run — the \
         cacheability bar. A script is cacheable iff EVERY step is stable.\n"
    );
    out.push_str("| script | size | step | label | distinct ref grids | stable |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    let mut scripts_cacheable = 0usize;
    let mut scripts_total = 0usize;
    for ((script, cols, rows), steps) in &grouped {
        scripts_total += 1;
        let mut all_stable = true;
        for (index, (label, digests)) in steps {
            let distinct = digests.len();
            let observed: usize = digests.values().sum();
            let stable = distinct == 1 && observed == reruns;
            all_stable &= stable;
            let _ = writeln!(
                out,
                "| {script} | {cols}x{rows} | {index} | {} | {distinct} | {} |",
                if label.is_empty() { "-" } else { label },
                if stable { "YES" } else { "no" },
            );
        }
        let _ = writeln!(
            out,
            "| | | | **script cacheable** | | **{}** |",
            if all_stable { "YES" } else { "no" }
        );
        scripts_cacheable += usize::from(all_stable);
    }
    let _ = writeln!(
        out,
        "\n**Summary: {scripts_cacheable}/{scripts_total} scripts fully cacheable.** \
         Scripts marked `no` must keep running the live ref (the bun PTY) — \
         their ref grids flip run-to-run (spinner/blink/placeholder-rotation \
         classes) and a cached verdict would diverge from a live one.\n"
    );
    out
}
