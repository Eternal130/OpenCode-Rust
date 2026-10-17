//! E3 memory-soak canary (SPEC-R6, I8 per-axis tier — R5 §5 "E3 leak canary").
//!
//! Validates the E3 clause METHOD before any 4h soak counts as evidence:
//!   * sampling harness: /proc/PID/status VmRSS + RssAnon + FD count @60s (real clock)
//!   * slope math: least-squares MB/h over the full run
//!   * threshold semantics: leak arm must report slope ~= 600 MB/h +-20% and FLAG;
//!     control arm (allocator disabled) slope ~= 0 within noise.
//!
//! The DEFECT (never-drained 10 MB/min retention = true leak, not GC lag) is
//! injected into a CHILD PROCESS of this runner — the product is never touched.
//! Self-contained: `std` only. Usage:
//!   rustc -O e3_leak_canary.rs -o e3_leak_canary && ./e3_leak_canary --sample 30
//!   `--child leak|control <pid-file>` is internal (spawned by the sampler).

use std::fmt::Write as _;
use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MB: f64 = 1024.0 * 1024.0;
const CHUNK_BYTES: usize = 128 * 1024; // 128 KiB chunks, touched → resident
const RATE_MB_PER_MIN: f64 = 10.0;
const SAMPLE_EVERY: Duration = Duration::from_secs(60);

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

fn rss_mb(pid: u32) -> Option<(f64, f64, usize)> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let mut vmrss = None;
    let mut rssanon = None;
    for line in status.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            vmrss = v.trim().split_whitespace().next().and_then(|s| s.parse::<f64>().ok());
        } else if let Some(v) = line.strip_prefix("RssAnon:") {
            rssanon = v.trim().split_whitespace().next().and_then(|s| s.parse::<f64>().ok());
        }
    }
    let fds = fs::read_dir(format!("/proc/{pid}/fd")).map(|d| d.count()).unwrap_or(0);
    Some((vmrss? / 1024.0, rssanon.unwrap_or(0.0) / 1024.0, fds))
}

fn leak_child() {
    // 10 MB/min in 128 KiB touched chunks, retained forever (never drained,
    // never eligible for GC — a true leak). ~4.7 chunks/min.
    let mut retained: Vec<Vec<u8>> = Vec::new();
    let chunks_per_min = RATE_MB_PER_MIN * MB / CHUNK_BYTES as f64;
    let mut acc = 0.0;
    let mut tick = 0u64;
    let pattern: &[u8] = b"canary-leak-payload-0123456789abcdef"; // defeats page merging
    loop {
        acc += 1.0;
        if acc >= chunks_per_min {
            acc -= chunks_per_min;
            let mut buf = vec![0u8; CHUNK_BYTES];
            for (i, b) in buf.iter_mut().enumerate() {
                if i % 4096 == 0 {
                    *b = pattern[(tick as usize) % pattern.len()];
                    tick = tick.wrapping_add(1);
                }
            }
            retained.push(buf); // never removed: the leak
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}

fn control_child() {
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn spawn_child(mode: &str) -> u32 {
    let exe = std::env::current_exe().unwrap();
    let pid_file = std::env::temp_dir().join(format!("e3canary-{mode}.pid"));
    Command::new(exe)
        .arg("--child")
        .arg(mode)
        .arg(pid_file.to_str().unwrap())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn child")
        .id()
}

fn least_squares_slope_mb_per_hour(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (x, y) in xs.iter().zip(ys) {
        num += (x - mx) * (y - my);
        den += (x - mx) * (x - mx);
    }
    if den <= f64::EPSILON {
        0.0
    } else {
        (num / den) * 3600.0 // per-second -> per-hour
    }
}

fn run_arm(mode: &str, samples: usize, csv: &mut String) -> (f64, f64, usize, usize) {
    let pid = spawn_child(mode);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut max_fds = 0usize;
    let (mut rss0, mut anon0, _) = rss_mb(pid).unwrap();
    let t0 = now();
    let _ = writeln!(csv, "t_s,rss_mb,rssanon_mb,fds");
    for i in 0..=samples {
        std::thread::sleep(if i == 0 { SAMPLE_EVERY } else { SAMPLE_EVERY });
        if let Some((rss, anon, fds)) = rss_mb(pid) {
            let t = now() - t0;
            let _ = writeln!(csv, "{t:.0},{rss:.1},{anon:.1},{fds}");
            xs.push(t);
            ys.push(rss);
            max_fds = max_fds.max(fds);
            rss0 = rss;
            anon0 = anon;
        }
    }
    let slope = least_squares_slope_mb_per_hour(&xs, &ys);
    // kill child
    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    (slope, rss0, anon0 as usize, max_fds)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--child" {
        let _ = fs::write(&args[3], std::process::id().to_string());
        return if args[2] == "leak" { leak_child() } else { control_child() };
    }
    let minutes: usize = args
        .iter()
        .position(|a| a == "--minutes")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let samples = minutes; // one sample per minute
    println!("E3 leak canary: {minutes}-min validation, cadence 60s, rate {RATE_MB_PER_MIN} MB/min (expect ~600 MB/h)");

    let mut leak_csv = String::new();
    let leak = run_arm("leak", samples, &mut leak_csv);
    println!("leak arm  : slope = {:.1} MB/h, final RSS = {:.1} MB, RssAnon = {:.1} MB, maxFDs = {}",
             leak.0, leak.1, leak.2, leak.3);
    let _ = fs::write("/tmp/opencode/spec-r6/e3_leak.csv", &leak_csv);

    let mut ctrl_csv = String::new();
    let ctrl = run_arm("control", samples.min(10), &mut ctrl_csv);
    println!("control arm: slope = {:.1} MB/h (noise floor), final RSS = {:.1} MB", ctrl.0, ctrl.1);
    let _ = fs::write("/tmp/opencode/spec-r6/e3_control.csv", &ctrl_csv);

    // Clause threshold semantics: FLAG = slope exceeds max(2 x ref, ref + 20 MB/h)
    // with ref := 0 (control) → any slope > 20 MB/h flags.
    let leak_flags = leak.0 > 20.0;
    let in_band = (480.0..=720.0).contains(&leak.0);
    let ctrl_ok = ctrl.0 <= 5.0; // ~0 within noise
    println!("VERDICT: leak slope in 600±20% band = {in_band}; leak flags = {leak_flags}; control ≈0 = {ctrl_ok}");
    let pass = in_band && leak_flags && ctrl_ok;
    println!("E3 CANARY: {}", if pass { "PASS (localizes; sampling cadence + slope math validated)" } else { "FAIL" });
    let report = format!(
        "{{\"minutes\": {minutes}, \"leak_slope_mb_per_h\": {:.2}, \"leak_final_rss_mb\": {:.1}, \"control_slope_mb_per_h\": {:.2}, \"leak_in_band\": {in_band}, \"leak_flags\": {leak_flags}, \"control_zero\": {ctrl_ok}, \"pass\": {pass}}}",
        leak.0, leak.1, ctrl.0
    );
    let _ = fs::write("/tmp/opencode/spec-r6/e3_canary.json", report);
    std::process::exit(if pass { 0 } else { 1 });
}
